use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use std::net::{IpAddr, UdpSocket};
use std::time::{Duration, Instant};

use crate::colors::{
    get_scene_name, hsv_to_rgb, parse_brightness, parse_color, parse_kelvin, parse_scene,
    rgb_to_hex, SCENES,
};
use crate::state::{validate_ip, State};

pub const WIZ_PORT: u16 = 38899;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(600);

/// WiZ bulbs become unreliable when a UI sends a burst of UDP commands.  This
/// is deliberately shared by every worker packet, including polls.
pub const MAX_PACKETS_PER_SECOND: u32 = 12;

#[derive(Debug)]
pub enum BulbError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Message(String),
    StateConflict {
        expected: String,
        current: Box<PilotResult>,
    },
}

impl fmt::Display for BulbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "{e}"),
            Self::Message(message) => f.write_str(message),
            Self::StateConflict { expected, current } => write!(
                f,
                "bulb state changed since it was last read (expected {expected}, current {}); refresh before applying the change",
                current.state_token()
            ),
        }
    }
}

impl std::error::Error for BulbError {}
impl From<std::io::Error> for BulbError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<serde_json::Error> for BulbError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

/// Spacing limiter for the single worker's UDP traffic.  The first packet is
/// immediate; all following packets are at least 1/12 second apart.
pub struct PacketRateLimiter {
    next_packet: Instant,
}

impl PacketRateLimiter {
    pub fn new() -> Self {
        Self {
            next_packet: Instant::now(),
        }
    }
    pub fn wait(&mut self) {
        let now = Instant::now();
        if self.next_packet > now {
            std::thread::sleep(self.next_packet - now);
        }
        self.next_packet =
            Instant::now() + Duration::from_secs_f64(1.0 / MAX_PACKETS_PER_SECOND as f64);
    }
}

impl Default for PacketRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PilotResult {
    pub mac: Option<String>,
    pub rssi: Option<i32>,
    pub state: Option<bool>,
    #[serde(rename = "sceneId")]
    pub scene_id: Option<u32>,
    pub dimming: Option<u8>,
    pub temp: Option<u16>,
    pub r: Option<u8>,
    pub g: Option<u8>,
    pub b: Option<u8>,
    /// Warm-white channel; carries the desaturated part of an RGB pick.
    pub w: Option<u8>,
    /// Cold-white channel.
    pub c: Option<u8>,
    /// WiZ firmware sends this as either a string or a number.
    pub src: Option<serde_json::Value>,
    #[serde(rename = "speed")]
    pub speed: Option<u8>,
    #[serde(rename = "schdPsetId")]
    pub rhythm_id: Option<u32>,
    pub ratio: Option<u8>,
}

impl PilotResult {
    pub fn is_on(&self) -> bool {
        self.state.unwrap_or(false)
    }

    pub fn brightness_255(&self) -> Option<u8> {
        self.dimming
            .map(|d| ((d as f64 * 255.0 / 100.0).round().clamp(0.0, 255.0)) as u8)
    }

    /// The bulb's displayed color, folding the warm-white channel back in.
    ///
    /// `setPilot` splits a color across the RGB LEDs and the white LED (see
    /// [`rgb_to_rgbcw`]), so the raw `r`/`g`/`b` read back from the bulb is the
    /// saturated remainder, not the color the user picked. Without this inverse
    /// a warm off-white reads back as a dark orange.
    pub fn rgb(&self) -> Option<(u8, u8, u8)> {
        match (self.r, self.g, self.b) {
            // All channels dark means the bulb is in white/scene mode and is
            // reporting no color at all, not the color black.
            (Some(0), Some(0), Some(0)) if self.w.unwrap_or(0) == 0 => None,
            (Some(r), Some(g), Some(b)) => Some(rgbcw_to_rgb(r, g, b, self.w.unwrap_or(0))),
            _ => None,
        }
    }

    /// Whether the bulb's raw channels are exactly what `rgb` would have sent.
    ///
    /// The RGB/white split is lossy in brightness, so a readback of a color we
    /// just set does not reproduce the original hex. Comparing in channel space
    /// instead keeps the UI from drifting the user's pick after the confirm poll.
    pub fn matches_rgb(&self, rgb: [u8; 3]) -> bool {
        let ((r, g, b), w) = rgb_to_rgbcw(rgb[0], rgb[1], rgb[2]);
        self.r == Some(r) && self.g == Some(g) && self.b == Some(b) && self.w.unwrap_or(0) == w
    }

    pub fn scene_name(&self) -> Option<&'static str> {
        self.scene_id
            .and_then(|id| if id != 0 { get_scene_name(id) } else { None })
    }

    /// Stable token for the controllable state. Telemetry (MAC/RSSI) is
    /// intentionally absent. `src` is retained as typed status metadata but
    /// excluded because it describes the controller, not the light state.
    pub fn state_token(&self) -> String {
        let snapshot = json!({
            "state": self.state, "dimming": self.dimming, "sceneId": self.scene_id,
            "r": self.r, "g": self.g, "b": self.b, "c": self.c, "w": self.w,
            "temp": self.temp, "speed": self.speed, "schdPsetId": self.rhythm_id,
            "ratio": self.ratio,
        });
        // A small deterministic FNV-1a fingerprint avoids a dependency while
        // retaining a stable, opaque token across processes.
        let mut hash = 0xcbf29ce484222325_u64;
        for byte in serde_json::to_vec(&snapshot).expect("JSON values serialize") {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{hash:016x}")
    }
}

#[derive(Debug, Clone)]
pub struct WiZclickFavorite {
    pub mode: u32,
    pub scene_id: u32,
    pub scene_name: String,
}

pub fn send_udp_json(
    ip: &str,
    payload: &serde_json::Value,
    timeout: Duration,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error>> {
    Ok(send_udp_json_limited(ip, payload, timeout, None)?)
}

pub fn send_udp_json_limited(
    ip: &str,
    payload: &serde_json::Value,
    timeout: Duration,
    limiter: Option<&mut PacketRateLimiter>,
) -> Result<Option<serde_json::Value>, BulbError> {
    let clean_ip = validate_ip(ip).map_err(|error| BulbError::Message(error.to_string()))?;
    let expected_ip: IpAddr = clean_ip
        .parse()
        .map_err(|e| BulbError::Message(format!("invalid WiZ IP {clean_ip}: {e}")))?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;

    let msg = serde_json::to_vec(payload)?;
    if let Some(limiter) = limiter {
        limiter.wait();
    }
    socket.send_to(&msg, format!("{clean_ip}:{WIZ_PORT}"))?;

    let mut buf = [0u8; 2048];
    match socket.recv_from(&mut buf) {
        Ok((amt, sender)) => {
            if sender.ip() != expected_ip || sender.port() != WIZ_PORT {
                return Err(BulbError::Message(format!(
                    "ignored WiZ response from unexpected sender {sender}"
                )));
            }
            let res: serde_json::Value = serde_json::from_slice(&buf[..amt])?;
            Ok(Some(res))
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::TimedOut
                || e.kind() == std::io::ErrorKind::WouldBlock
            {
                Ok(None)
            } else {
                Err(BulbError::Io(e))
            }
        }
    }
}

pub fn send_pilot(ip: &str, params: serde_json::Value) -> Result<(), BulbError> {
    send_pilot_limited(ip, params, None)
}

pub fn send_pilot_limited(
    ip: &str,
    params: serde_json::Value,
    limiter: Option<&mut PacketRateLimiter>,
) -> Result<(), BulbError> {
    let payload = json!({
        "method": "setPilot",
        "params": params
    });
    let response = send_udp_json_limited(ip, &payload, DEFAULT_TIMEOUT, limiter)?
        .ok_or_else(|| BulbError::Message(format!("setPilot request to bulb at {ip} timed out")))?;
    if let Some(error) = response.get("error") {
        return Err(BulbError::Message(format!("Bulb error: {error}")));
    }
    if let Some(method) = response.get("method") {
        if method.as_str() != Some("setPilot") {
            return Err(BulbError::Message(
                "setPilot response has a mismatched method".to_string(),
            ));
        }
    }
    if response
        .pointer("/result/success")
        .and_then(|value| value.as_bool())
        != Some(true)
    {
        return Err(BulbError::Message(
            "setPilot response did not acknowledge success".to_string(),
        ));
    }
    Ok(())
}

pub fn get_pilot(ip: &str) -> Result<PilotResult, BulbError> {
    get_pilot_limited(ip, None)
}

pub fn get_pilot_limited(
    ip: &str,
    limiter: Option<&mut PacketRateLimiter>,
) -> Result<PilotResult, BulbError> {
    let payload = json!({ "method": "getPilot" });
    let resp = send_udp_json_limited(ip, &payload, Duration::from_millis(800), limiter)?
        .ok_or_else(|| {
            BulbError::Message(format!(
                "request to bulb at {ip} timed out (check power and network connection)"
            ))
        })?;

    if let Some(err) = resp.get("error") {
        return Err(BulbError::Message(format!("Bulb error: {err}")));
    }
    if let Some(method) = resp.get("method") {
        if method.as_str() != Some("getPilot") {
            return Err(BulbError::Message(
                "getPilot response has a mismatched method".to_string(),
            ));
        }
    }

    let result = resp
        .get("result")
        .ok_or_else(|| BulbError::Message("Missing 'result' in response".to_string()))?;
    if result
        .get("state")
        .and_then(|value| value.as_bool())
        .is_none()
    {
        return Err(BulbError::Message(
            "getPilot response is missing boolean 'state'".to_string(),
        ));
    }
    let pilot: PilotResult = serde_json::from_value(result.clone())?;
    Ok(pilot)
}

/// Apply an optimistic, delta-only update. A fresh read happens before every
/// write and another read confirms the actual outcome. WiZ has no atomic CAS,
/// so the confirmation intentionally does not retry a competing write.
pub fn apply_update(
    ip: &str,
    expected_state_token: Option<&str>,
    params: serde_json::Value,
) -> Result<PilotResult, BulbError> {
    apply_update_limited(ip, expected_state_token, params, None)
}

pub fn apply_update_limited(
    ip: &str,
    expected_state_token: Option<&str>,
    params: serde_json::Value,
    mut limiter: Option<&mut PacketRateLimiter>,
) -> Result<PilotResult, BulbError> {
    let object = params
        .as_object()
        .ok_or_else(|| BulbError::Message("update parameters must be an object".to_string()))?;
    if object.is_empty() {
        return Err(BulbError::Message(
            "apply_update requires at least one change".to_string(),
        ));
    }
    const ALLOWED: &[&str] = &[
        "state", "dimming", "sceneId", "r", "g", "b", "c", "w", "temp", "speed",
    ];
    if let Some(unknown) = object.keys().find(|key| !ALLOWED.contains(&key.as_str())) {
        return Err(BulbError::Message(format!(
            "unsupported update field '{unknown}'"
        )));
    }
    if object.get("state").is_some_and(|value| !value.is_boolean()) {
        return Err(BulbError::Message("state must be a boolean".to_string()));
    }
    for field in ["dimming", "r", "g", "b", "c", "w"] {
        let maximum = if field == "dimming" { 100 } else { 255 };
        if let Some(value) = object.get(field) {
            if value.as_u64().is_none_or(|number| number > maximum) {
                return Err(BulbError::Message(format!(
                    "{field} must be an integer from 0 to {maximum}"
                )));
            }
        }
    }
    if let Some(value) = object.get("temp") {
        if value
            .as_u64()
            .is_none_or(|number| !(1000..=10000).contains(&number))
        {
            return Err(BulbError::Message(
                "temp must be an integer from 1000 to 10000".to_string(),
            ));
        }
    }
    if let Some(value) = object.get("sceneId") {
        let valid = value
            .as_u64()
            .is_some_and(|id| SCENES.iter().any(|(scene_id, _)| *scene_id as u64 == id));
        if !valid {
            return Err(BulbError::Message(
                "sceneId must name a supported WiZ scene".to_string(),
            ));
        }
    }
    if let Some(value) = object.get("speed") {
        if value
            .as_u64()
            .is_none_or(|number| !(10..=200).contains(&number))
        {
            return Err(BulbError::Message(
                "speed must be an integer from 10 to 200".to_string(),
            ));
        }
    }
    let is_off = object.get("state").and_then(|v| v.as_bool()) == Some(false);
    if is_off && object.len() != 1 {
        return Err(BulbError::Message(
            "a turn-off update cannot also contain light-mode changes".to_string(),
        ));
    }
    let mode_count = [
        object.contains_key("sceneId"),
        object.contains_key("temp"),
        ["r", "g", "b", "c", "w"]
            .iter()
            .any(|key| object.contains_key(*key)),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if mode_count > 1 {
        return Err(BulbError::Message(
            "an update may contain only one light mode".to_string(),
        ));
    }

    let current = get_pilot_limited(ip, limiter.as_deref_mut())?;
    if let Some(expected) = expected_state_token {
        if expected != current.state_token() {
            return Err(BulbError::StateConflict {
                expected: expected.to_string(),
                current: Box::new(current),
            });
        }
    }
    // A duplicate `state: true` can reassert rhythm behaviour. Preserve a
    // matching observation without emitting a packet or a needless readback.
    if object.len() == 1
        && object.get("state").and_then(|v| v.as_bool()) == Some(true)
        && current.is_on()
    {
        return Ok(current);
    }
    send_pilot_limited(ip, params, limiter.as_deref_mut())?;
    get_pilot_limited(ip, limiter)
}

pub fn get_favorites(ip: &str) -> Result<Vec<WiZclickFavorite>, Box<dyn std::error::Error>> {
    let payload = json!({ "method": "getFavs", "params": {} });
    let response = send_udp_json(ip, &payload, DEFAULT_TIMEOUT)?
        .ok_or_else(|| "getFavs request timed out".to_string())?;
    if let Some(error) = response.get("error") {
        return Err(format!("Bulb error: {error}").into());
    }
    if let Some(method) = response.get("method") {
        if method.as_str() != Some("getFavs") {
            return Err("getFavs response has a mismatched method"
                .to_string()
                .into());
        }
    }
    let favorites = response
        .get("result")
        .and_then(|result| result.get("favs"))
        .and_then(|favorites| favorites.as_array())
        .ok_or_else(|| "getFavs response is missing an array 'result.favs'".to_string())?;

    let mut favs = Vec::with_capacity(favorites.len());
    for (idx, item) in favorites.iter().enumerate() {
        let raw_id = if let Some(array) = item.as_array() {
            array.first().and_then(|value| value.as_u64())
        } else {
            item.as_u64()
        }
        .ok_or_else(|| format!("getFavs entry {} has no numeric scene id", idx + 1))?;
        let scene_id = u32::try_from(raw_id)
            .map_err(|_| format!("getFavs entry {} scene id is out of range", idx + 1))?;
        if scene_id == 0 {
            return Err(format!("getFavs entry {} has invalid scene id 0", idx + 1).into());
        }
        favs.push(WiZclickFavorite {
            mode: (idx + 1) as u32,
            scene_id,
            scene_name: get_scene_name(scene_id).unwrap_or("Unknown").to_string(),
        });
    }
    Ok(favs)
}

pub fn set_power(ip: &str, state: bool) -> Result<(), Box<dyn std::error::Error>> {
    apply_update(ip, None, json!({ "state": state }))
        .map(|_| ())
        .map_err(Into::into)
}

/// Convert the CLI's 0..255 brightness into a safe WiZ delta.
///
/// WiZ represents an ON dimming value only from 10 through 100 percent. Zero
/// is therefore an explicit OFF command; values 1..24 cannot be represented
/// faithfully and must not be silently promoted to the bulb's 10% minimum.
pub fn brightness_pilot_params(brightness: u8) -> Result<serde_json::Value, BulbError> {
    match brightness {
        0 => Ok(json!({ "state": false })),
        1..=24 => Err(BulbError::Message(
            "brightness 1..24 is below the WiZ 10% minimum; use 0 to turn off or at least 25"
                .to_string(),
        )),
        25..=255 => {
            let dimming = (brightness as f64 * 100.0 / 255.0).round() as u8;
            Ok(json!({ "state": true, "dimming": dimming }))
        }
    }
}

pub fn set_brightness(ip: &str, brightness: u8) -> Result<(), Box<dyn std::error::Error>> {
    let params = brightness_pilot_params(brightness)?;
    apply_update(ip, None, params)
        .map(|_| ())
        .map_err(Into::into)
}

pub fn set_temperature(ip: &str, kelvin: u16) -> Result<(), Box<dyn std::error::Error>> {
    // The CLI accepts 1000..10000K. Do not silently narrow it to the GUI
    // slider's practical range of 2200..6500K.
    apply_update(ip, None, json!({ "state": true, "temp": kelvin }))
        .map(|_| ())
        .map_err(Into::into)
}

/// Convert an sRGB triple into the WiZ five-channel mix (`r`,`g`,`b` + warm white).
///
/// A WiZ RGBTW bulb renders desaturated colors through its dedicated white LED,
/// not by driving the RGB LEDs toward white. Sending raw sRGB leaves the white
/// channels at whatever the previous command set, so a warm off-white lands as
/// a cold bluish wash. This is a direct port of `pywizlight`'s
/// `rgbcw.rgb2rgbcw`, which is the conversion the Python CLI/GUI has always used.
pub fn rgb_to_rgbcw(r: u8, g: u8, b: u8) -> ((u8, u8, u8), u8) {
    const EPSILON: f64 = 1.0e-5;
    /// Highest value the warm-white channel is driven to.
    const CW_MAX: f64 = 128.0;
    /// Unit vectors 120° apart, one per RGB primary.
    const BASIS: [(f64, f64); 3] = [
        (1.0, 0.0),
        (-0.5, 0.866_025_403_784_438_6),
        (-0.5, -0.866_025_403_784_438_6),
    ];

    // Black is an actual RGB choice. The generic zero-saturation path uses a
    // fully-on white channel, which would incorrectly turn black into white.
    if r == 0 && g == 0 && b == 0 {
        return ((0, 0, 0), 0);
    }

    let dot = |a: (f64, f64), b: (f64, f64)| a.0 * b.0 + a.1 * b.1;

    // Project the color onto the hue plane; the projection's length is saturation.
    let (rf, gf, bf) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let mut hue = (
        BASIS[0].0 * rf + BASIS[1].0 * gf + BASIS[2].0 * bf,
        BASIS[0].1 * rf + BASIS[1].1 * gf + BASIS[2].1 * bf,
    );
    let len_sq = dot(hue, hue);
    let saturation = if len_sq > EPSILON { len_sq.sqrt() } else { 0.0 };
    if saturation > EPSILON {
        hue = (hue.0 / saturation, hue.1 / saturation);
    }

    let mut rgb = [0.0_f64; 3];
    if saturation > EPSILON {
        // Pick the one or two primaries that can reach this hue.
        let max_angle = (std::f64::consts::TAU / 3.0 - EPSILON).cos();
        let mask = [
            dot(hue, BASIS[0]) > max_angle,
            dot(hue, BASIS[1]) > max_angle,
            dot(hue, BASIS[2]) > max_angle,
        ];
        let picked: Vec<usize> = (0..3).filter(|&i| mask[i]).collect();

        if picked.len() == 1 {
            rgb[picked[0]] = 1.0;
        } else if picked.len() == 2 {
            let (first, second) = (BASIS[picked[0]], BASIS[picked[1]]);
            // Ray/line intersection against the line through `second`.
            let ab = (second.1, -second.0);
            let coeff0 = dot(hue, ab) / dot(first, ab);
            let intersection = (first.0 * -coeff0 + hue.0, first.1 * -coeff0 + hue.1);
            let coeff1 = dot(intersection, second);
            // Colors outside the basis hexagon are unreachable; rescale into gamut.
            let max_coeff = coeff0.max(coeff1);
            rgb[picked[0]] = (coeff0 / max_coeff).min(1.0);
            rgb[picked[1]] = (coeff1 / max_coeff).min(1.0);
        }
    }

    // Discontinuous split: above half saturation the RGB LEDs stay saturated and
    // the white channel fades out; below it the white channel saturates instead.
    let cw = if saturation >= 0.5 {
        1.0 - (saturation - 0.5) * 2.0
    } else {
        for channel in &mut rgb {
            *channel *= saturation * 2.0;
        }
        1.0
    };

    (
        (
            (rgb[0] * 255.0) as u8,
            (rgb[1] * 255.0) as u8,
            (rgb[2] * 255.0) as u8,
        ),
        (cw * CW_MAX).max(0.0) as u8,
    )
}

/// Inverse of [`rgb_to_rgbcw`]: recover the displayed color from the bulb's
/// five channels. Port of `pywizlight`'s `rgbcw.rgbcw2hs`, then HSV at full value.
pub fn rgbcw_to_rgb(r: u8, g: u8, b: u8, w: u8) -> (u8, u8, u8) {
    const EPSILON: f64 = 1.0e-5;
    const CW_MAX: f64 = 128.0;
    const BASIS: [(f64, f64); 3] = [
        (1.0, 0.0),
        (-0.5, 0.866_025_403_784_438_6),
        (-0.5, -0.866_025_403_784_438_6),
    ];

    let (rf, gf, bf) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let cw = (w as f64).min(CW_MAX) / CW_MAX;
    let hue_vec = (
        BASIS[0].0 * rf + BASIS[1].0 * gf + BASIS[2].0 * bf,
        BASIS[0].1 * rf + BASIS[1].1 * gf + BASIS[2].1 * bf,
    );

    // Mirror of the forward split: a saturated white channel means the RGB LEDs
    // encode the lower half of the saturation range, otherwise they are maxed
    // out and the white channel encodes the upper half.
    let len_sq = hue_vec.0 * hue_vec.0 + hue_vec.1 * hue_vec.1;
    let saturation = if cw >= 1.0 {
        let len = if len_sq > EPSILON { len_sq.sqrt() } else { 0.0 };
        len * 0.5
    } else {
        1.0 - cw / 2.0
    };

    let hue = hue_vec.1.atan2(hue_vec.0).to_degrees().rem_euclid(360.0);
    hsv_to_rgb(hue as f32, saturation as f32, 1.0)
}

/// `setPilot` parameters for an RGB pick, white channel included.
pub fn rgb_pilot_params(r: u8, g: u8, b: u8) -> serde_json::Value {
    let ((pr, pg, pb), w) = rgb_to_rgbcw(r, g, b);
    json!({ "state": true, "r": pr, "g": pg, "b": pb, "w": w })
}

pub fn set_rgb(ip: &str, r: u8, g: u8, b: u8) -> Result<(), Box<dyn std::error::Error>> {
    apply_update(ip, None, rgb_pilot_params(r, g, b))
        .map(|_| ())
        .map_err(Into::into)
}

pub fn set_scene(ip: &str, scene_id: u32) -> Result<(), Box<dyn std::error::Error>> {
    apply_update(ip, None, json!({ "state": true, "sceneId": scene_id }))
        .map(|_| ())
        .map_err(Into::into)
}

#[allow(dead_code)]
pub fn apply_saved_state(ip: &str, state: &State) -> Result<(), Box<dyn std::error::Error>> {
    if !state.power {
        return set_power(ip, false);
    }

    let dim = ((state.brightness as f64 * 100.0 / 255.0).round() as u8).clamp(10, 100);
    if state.mode == "scene" && state.scene_id > 0 {
        apply_update(
            ip,
            None,
            json!({ "state": true, "dimming": dim, "sceneId": state.scene_id }),
        )?;
    } else if state.mode == "kelvin" && state.kelvin > 0 {
        let temp = state.kelvin;
        apply_update(
            ip,
            None,
            json!({ "state": true, "dimming": dim, "temp": temp }),
        )?;
    } else {
        let ((pr, pg, pb), w) = rgb_to_rgbcw(state.rgb[0], state.rgb[1], state.rgb[2]);
        apply_update(
            ip,
            None,
            json!({ "state": true, "dimming": dim, "r": pr, "g": pg, "b": pb, "w": w }),
        )?;
    }
    Ok(())
}

pub fn command_on(ip: &str) -> Result<(), Box<dyn std::error::Error>> {
    set_power(ip, true)?;
    println!("✓ ON");
    Ok(())
}

pub fn command_off(ip: &str) -> Result<(), Box<dyn std::error::Error>> {
    set_power(ip, false)?;
    println!("✓ OFF");
    Ok(())
}

pub fn command_toggle(ip: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let pilot = get_pilot(ip)?;
    let new_state = !pilot.is_on();
    apply_update(
        ip,
        Some(&pilot.state_token()),
        json!({ "state": new_state }),
    )?;
    if new_state {
        println!("✓ ON (toggled)");
    } else {
        println!("✓ OFF (toggled)");
    }
    Ok(new_state)
}

pub fn command_status(ip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let pilot = get_pilot(ip)?;
    let favs = get_favorites(ip).unwrap_or_default();

    println!("Bulb:       {ip}");
    if let Some(ref mac) = pilot.mac {
        println!("MAC:        {mac}");
    }
    if let Some(rssi) = pilot.rssi {
        println!("Signal:     {rssi} dBm");
    }
    println!("Power:      {}", if pilot.is_on() { "ON" } else { "OFF" });

    if let Some(b) = pilot.brightness_255() {
        let pct = (b as f64 * 100.0 / 255.0).round() as u8;
        println!("Brightness: {b}/255 ({pct}%)");
    }

    if let Some(sname) = pilot.scene_name() {
        println!(
            "Scene:      {} (ID: {})",
            sname,
            pilot.scene_id.unwrap_or(0)
        );
    } else if let Some(sid) = pilot.scene_id {
        if sid != 0 {
            println!("Scene:      ID {sid}");
        } else {
            println!("Scene:      None");
        }
    } else {
        println!("Scene:      None");
    }

    if let Some((r, g, b)) = pilot.rgb() {
        let hex = rgb_to_hex(r, g, b);
        println!("RGB:        {r},{g},{b} ({hex})");
    }

    if let Some(k) = pilot.temp {
        println!("Kelvin:     {k}K");
    }

    if let Some(ref src) = pilot.src {
        println!("Source:     {src}");
    }

    if !favs.is_empty() {
        let fav_strs: Vec<String> = favs
            .iter()
            .map(|f| format!("Mode {}: {}", f.mode, f.scene_name))
            .collect();
        println!("WiZclick:   {}", fav_strs.join(" | "));
    }

    Ok(())
}

pub fn command_color(ip: &str, value: &str) -> Result<(u8, u8, u8), Box<dyn std::error::Error>> {
    let (r, g, b) = parse_color(value)?;
    set_rgb(ip, r, g, b)?;
    let hex = rgb_to_hex(r, g, b);
    println!("✓ RGB({r}, {g}, {b}) ({hex})");
    Ok((r, g, b))
}

pub fn command_brightness(ip: &str, value: &str) -> Result<u8, Box<dyn std::error::Error>> {
    let b = parse_brightness(value)?;
    set_brightness(ip, b)?;
    if b == 0 {
        println!("✓ OFF (brightness 0)");
        return Ok(b);
    }
    let pct = (b as f64 * 100.0 / 255.0).round() as u8;
    println!("✓ Brightness {b}/255 ({pct}%)");
    Ok(b)
}

pub fn command_kelvin(ip: &str, value: &str) -> Result<u16, Box<dyn std::error::Error>> {
    let k = parse_kelvin(value)?;
    set_temperature(ip, k)?;
    println!("✓ {k}K");
    Ok(k)
}

pub fn command_scene(
    ip: &str,
    value: &str,
) -> Result<(u32, &'static str), Box<dyn std::error::Error>> {
    let (sid, sname) = parse_scene(value)?;
    set_scene(ip, sid)?;
    println!("✓ Scene {sid} ({sname})");
    Ok((sid, sname))
}

pub fn command_scenes() {
    println!("Available WiZ Scenes:");
    println!("-----------------------------------");
    let mut sorted_scenes = SCENES.to_vec();
    sorted_scenes.sort_by_key(|&(id, _)| id);
    for (sid, sname) in sorted_scenes {
        if sid <= 40 {
            println!("  {sid:2}: {sname}");
        }
    }
    println!("-----------------------------------");
    println!("Usage: wizctl scene <id|name>");
}

pub fn command_wizclick(ip: &str, mode: Option<u32>) -> Result<(), Box<dyn std::error::Error>> {
    let mut favorites = get_favorites(ip)?;
    if favorites.is_empty() {
        favorites = vec![
            WiZclickFavorite {
                mode: 1,
                scene_id: 6,
                scene_name: "Cozy".to_string(),
            },
            WiZclickFavorite {
                mode: 2,
                scene_id: 14,
                scene_name: "Night light".to_string(),
            },
        ];
    }

    if let Some(m) = mode {
        let matched = favorites.iter().find(|f| f.mode == m).ok_or_else(|| {
            format!(
                "WiZclick mode must be between 1 and {}, got {m}",
                favorites.len()
            )
        })?;
        set_scene(ip, matched.scene_id)?;
        println!("✓ WiZclick Mode {m} ({})", matched.scene_name);
        return Ok(());
    }

    println!("WiZclick Settings (Wall Switch Modes):");
    println!("----------------------------------------");
    for fav in &favorites {
        println!(
            "  Mode {} (Click {}): {} (Scene ID: {})",
            fav.mode, fav.mode, fav.scene_name, fav.scene_id
        );
    }
    println!("----------------------------------------");
    println!("Toggle physical wall switch once for Mode 1, twice quickly for Mode 2.");
    println!("Usage: wizctl wizclick [1|2]");
    Ok(())
}

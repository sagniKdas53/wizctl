use std::net::UdpSocket;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::colors::{
    get_scene_name, hsv_to_rgb, parse_brightness, parse_color, parse_kelvin, parse_scene,
    rgb_to_hex, SCENES,
};
use crate::state::{validate_ip, State};

pub const WIZ_PORT: u16 = 38899;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(600);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    pub src: Option<String>,
}

impl PilotResult {
    pub fn is_on(&self) -> bool {
        self.state.unwrap_or(false)
    }

    pub fn brightness_255(&self) -> Option<u8> {
        self.dimming.map(|d| {
            ((d as f64 * 255.0 / 100.0).round().clamp(0.0, 255.0)) as u8
        })
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
        self.scene_id.and_then(|id| if id != 0 { get_scene_name(id) } else { None })
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
    let clean_ip = validate_ip(ip)?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;

    let msg = serde_json::to_vec(payload)?;
    socket.send_to(&msg, format!("{clean_ip}:{WIZ_PORT}"))?;

    let mut buf = [0u8; 2048];
    match socket.recv_from(&mut buf) {
        Ok((amt, _)) => {
            let res: serde_json::Value = serde_json::from_slice(&buf[..amt])?;
            Ok(Some(res))
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock {
                Ok(None)
            } else {
                Err(Box::new(e))
            }
        }
    }
}

pub fn send_pilot(ip: &str, params: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let payload = json!({
        "method": "setPilot",
        "params": params
    });
    // Send and attempt to read reply with 600ms timeout
    let _ = send_udp_json(ip, &payload, DEFAULT_TIMEOUT)?;
    Ok(())
}

pub fn get_pilot(ip: &str) -> Result<PilotResult, Box<dyn std::error::Error>> {
    let payload = json!({ "method": "getPilot" });
    let resp = send_udp_json(ip, &payload, Duration::from_millis(800))?
        .ok_or_else(|| format!("request to bulb at {ip} timed out (check power and network connection)"))?;

    if let Some(err) = resp.get("error") {
        return Err(format!("Bulb error: {err}").into());
    }

    let result = resp.get("result").ok_or_else(|| "Missing 'result' in response")?;
    let pilot: PilotResult = serde_json::from_value(result.clone())?;
    Ok(pilot)
}

pub fn get_favorites(ip: &str) -> Result<Vec<WiZclickFavorite>, Box<dyn std::error::Error>> {
    let payload = json!({ "method": "getFavs", "params": {} });
    let resp = send_udp_json(ip, &payload, DEFAULT_TIMEOUT)?;
    let mut favs = Vec::new();

    if let Some(res) = resp.and_then(|r| r.get("result").cloned()) {
        if let Some(favs_arr) = res.get("favs").and_then(|f| f.as_array()) {
            for (idx, item) in favs_arr.iter().enumerate() {
                let sid = if let Some(arr) = item.as_array() {
                    arr.first().and_then(|v| v.as_u64()).unwrap_or(0) as u32
                } else if let Some(num) = item.as_u64() {
                    num as u32
                } else {
                    0
                };
                if sid != 0 {
                    let name = get_scene_name(sid).unwrap_or("Unknown").to_string();
                    favs.push(WiZclickFavorite {
                        mode: (idx + 1) as u32,
                        scene_id: sid,
                        scene_name: name,
                    });
                }
            }
        }
    }
    Ok(favs)
}

pub fn set_power(ip: &str, state: bool) -> Result<(), Box<dyn std::error::Error>> {
    send_pilot(ip, json!({ "state": state }))
}

pub fn set_brightness(ip: &str, brightness: u8) -> Result<(), Box<dyn std::error::Error>> {
    let dim = ((brightness as f64 * 100.0 / 255.0).round() as u8).clamp(10, 100);
    send_pilot(ip, json!({ "state": true, "dimming": dim }))
}

pub fn set_temperature(ip: &str, kelvin: u16) -> Result<(), Box<dyn std::error::Error>> {
    let temp = kelvin.clamp(2200, 6500);
    send_pilot(ip, json!({ "state": true, "temp": temp }))
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
    send_pilot(ip, rgb_pilot_params(r, g, b))
}

pub fn set_scene(ip: &str, scene_id: u32) -> Result<(), Box<dyn std::error::Error>> {
    send_pilot(ip, json!({ "state": true, "sceneId": scene_id }))
}

#[allow(dead_code)]
pub fn apply_saved_state(ip: &str, state: &State) -> Result<(), Box<dyn std::error::Error>> {
    if !state.power {
        return set_power(ip, false);
    }

    let dim = ((state.brightness as f64 * 100.0 / 255.0).round() as u8).clamp(10, 100);
    if state.mode == "scene" && state.scene_id > 0 {
        send_pilot(ip, json!({ "state": true, "dimming": dim, "sceneId": state.scene_id }))?;
    } else if state.mode == "kelvin" && state.kelvin > 0 {
        let temp = state.kelvin.clamp(2200, 6500);
        send_pilot(ip, json!({ "state": true, "dimming": dim, "temp": temp }))?;
    } else {
        let ((pr, pg, pb), w) = rgb_to_rgbcw(state.rgb[0], state.rgb[1], state.rgb[2]);
        send_pilot(
            ip,
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
    set_power(ip, new_state)?;
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
        println!("Scene:      {} (ID: {})", sname, pilot.scene_id.unwrap_or(0));
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
    let (r, g, b) = parse_color(value).map_err(|e| e)?;
    set_rgb(ip, r, g, b)?;
    let hex = rgb_to_hex(r, g, b);
    println!("✓ RGB({r}, {g}, {b}) ({hex})");
    Ok((r, g, b))
}

pub fn command_brightness(ip: &str, value: &str) -> Result<u8, Box<dyn std::error::Error>> {
    let b = parse_brightness(value).map_err(|e| e)?;
    set_brightness(ip, b)?;
    let pct = (b as f64 * 100.0 / 255.0).round() as u8;
    println!("✓ Brightness {b}/255 ({pct}%)");
    Ok(b)
}

pub fn command_kelvin(ip: &str, value: &str) -> Result<u16, Box<dyn std::error::Error>> {
    let k = parse_kelvin(value).map_err(|e| e)?;
    set_temperature(ip, k)?;
    println!("✓ {k}K");
    Ok(k)
}

pub fn command_scene(ip: &str, value: &str) -> Result<(u32, &'static str), Box<dyn std::error::Error>> {
    let (sid, sname) = parse_scene(value).map_err(|e| e)?;
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
    let mut favorites = get_favorites(ip).unwrap_or_default();
    if favorites.is_empty() {
        favorites = vec![
            WiZclickFavorite { mode: 1, scene_id: 6, scene_name: "Cozy".to_string() },
            WiZclickFavorite { mode: 2, scene_id: 14, scene_name: "Night light".to_string() },
        ];
    }

    if let Some(m) = mode {
        let matched = favorites.iter().find(|f| f.mode == m)
            .ok_or_else(|| format!("WiZclick mode must be between 1 and {}, got {m}", favorites.len()))?;
        set_scene(ip, matched.scene_id)?;
        println!("✓ WiZclick Mode {m} ({})", matched.scene_name);
        return Ok(());
    }

    println!("WiZclick Settings (Wall Switch Modes):");
    println!("----------------------------------------");
    for fav in &favorites {
        println!("  Mode {} (Click {}): {} (Scene ID: {})", fav.mode, fav.mode, fav.scene_name, fav.scene_id);
    }
    println!("----------------------------------------");
    println!("Toggle physical wall switch once for Mode 1, twice quickly for Mode 2.");
    println!("Usage: wizctl wizclick [1|2]");
    Ok(())
}

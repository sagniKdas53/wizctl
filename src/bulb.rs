use std::net::UdpSocket;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::colors::{get_scene_name, parse_brightness, parse_color, parse_kelvin, parse_scene, rgb_to_hex, SCENES};
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

    pub fn rgb(&self) -> Option<(u8, u8, u8)> {
        match (self.r, self.g, self.b) {
            (Some(r), Some(g), Some(b)) => Some((r, g, b)),
            _ => None,
        }
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
    let temp = kelvin.clamp(2700, 6500);
    send_pilot(ip, json!({ "state": true, "temp": temp }))
}

pub fn set_rgb(ip: &str, r: u8, g: u8, b: u8) -> Result<(), Box<dyn std::error::Error>> {
    send_pilot(ip, json!({ "state": true, "r": r, "g": g, "b": b }))
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
        let temp = state.kelvin.clamp(2700, 6500);
        send_pilot(ip, json!({ "state": true, "dimming": dim, "temp": temp }))?;
    } else {
        send_pilot(ip, json!({ "state": true, "dimming": dim, "r": state.rgb[0], "g": state.rgb[1], "b": state.rgb[2] }))?;
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

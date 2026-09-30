use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::colors::{parse_brightness, parse_color, parse_kelvin, DEFAULT_PRESET_COLORS};

pub const DEFAULT_BULB_IP: &str = "192.168.0.102";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub ip: String,
    pub power: bool,
    pub brightness: u8,
    pub rgb: [u8; 3],
    pub hex: String,
    pub kelvin: u16,
    pub scene_id: u32,
    pub mode: String,
    pub recent_colors: Vec<String>,
    pub restore_on_reconnect: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            ip: DEFAULT_BULB_IP.to_string(),
            power: true,
            brightness: 255,
            rgb: [255, 140, 0],
            hex: "#ff8c00".to_string(),
            kelvin: 2700,
            scene_id: 6,
            mode: "color".to_string(),
            recent_colors: DEFAULT_PRESET_COLORS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            restore_on_reconnect: false,
        }
    }
}

pub fn validate_ip(ip: &str) -> Result<String, String> {
    let val = ip.trim();
    if val.is_empty() {
        return Err("IP address cannot be empty".to_string());
    }
    let addr = Ipv4Addr::from_str(val).map_err(|_| format!("Invalid IP address format: '{ip}'"))?;
    if addr.is_multicast() {
        return Err(format!("Multicast IP addresses are not permitted: '{ip}'"));
    }
    if addr == Ipv4Addr::new(255, 255, 255, 255) {
        return Err(format!(
            "Global broadcast IP address is not permitted: '{ip}'"
        ));
    }
    Ok(val.to_string())
}

fn config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(appdata) = env::var("APPDATA") {
            return PathBuf::from(appdata).join("wizctl");
        }
        if let Ok(profile) = env::var("USERPROFILE") {
            return PathBuf::from(profile)
                .join("AppData")
                .join("Roaming")
                .join("wizctl");
        }
    }
    #[cfg(not(windows))]
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("wizctl");
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".config").join("wizctl");
    }
    PathBuf::from(".").join(".config").join("wizctl")
}

pub fn get_state_file_path() -> PathBuf {
    let config_dir = config_dir();
    if fs::create_dir_all(&config_dir).is_ok() {
        return config_dir.join("state.json");
    }
    if let Ok(home) = env::var("HOME") {
        let fallback = PathBuf::from(home).join(".wizctl");
        let _ = fs::create_dir_all(&fallback);
        return fallback.join("state.json");
    }
    PathBuf::from("state.json")
}

fn set_rgb(state: &mut State, rgb: (u8, u8, u8)) {
    state.rgb = [rgb.0, rgb.1, rgb.2];
    state.hex = format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2);
}

/// Match the Python schema's field-by-field recovery: a malformed field falls
/// back to its default while valid neighbours remain intact.
pub fn sanitize_json_state(raw: &Value) -> State {
    let mut clean = State::default();
    let Some(raw) = raw.as_object() else {
        return clean;
    };
    if let Some(ip) = raw.get("ip").and_then(Value::as_str) {
        if let Ok(ip) = validate_ip(ip) {
            clean.ip = ip;
        }
    }
    if let Some(power) = raw.get("power").and_then(Value::as_bool) {
        clean.power = power;
    }
    if let Some(brightness) = raw.get("brightness") {
        let parsed = brightness
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .or_else(|| {
                brightness
                    .as_str()
                    .and_then(|value| parse_brightness(value).ok())
            });
        if let Some(brightness) = parsed {
            clean.brightness = brightness.max(25);
        }
    }
    if let Some(rgb) = raw.get("rgb").and_then(Value::as_array) {
        if rgb.len() == 3 {
            let channels: Option<Vec<u8>> = rgb
                .iter()
                .map(|value| value.as_i64().map(|channel| channel.clamp(0, 255) as u8))
                .collect();
            if let Some(channels) = channels {
                set_rgb(&mut clean, (channels[0], channels[1], channels[2]));
            }
        }
    }
    // The Python implementation deliberately lets a valid hex value override RGB.
    if let Some(hex) = raw.get("hex").and_then(Value::as_str) {
        if let Ok(rgb) = parse_color(hex) {
            set_rgb(&mut clean, rgb);
        }
    }
    if let Some(kelvin) = raw.get("kelvin") {
        let parsed = kelvin
            .as_u64()
            .and_then(|value| u16::try_from(value).ok())
            .filter(|value| (1000..=10_000).contains(value))
            .or_else(|| kelvin.as_str().and_then(|value| parse_kelvin(value).ok()));
        if let Some(kelvin) = parsed {
            clean.kelvin = kelvin;
        }
    }
    if let Some(scene_id) = raw.get("scene_id").and_then(Value::as_u64) {
        if (1..=40).contains(&scene_id) {
            clean.scene_id = scene_id as u32;
        }
    }
    if let Some(mode) = raw.get("mode").and_then(Value::as_str) {
        if matches!(mode, "color" | "kelvin" | "scene") {
            clean.mode = mode.to_string();
        }
    }
    if let Some(recent_colors) = raw.get("recent_colors").and_then(Value::as_array) {
        let valid_recents: Vec<String> = recent_colors
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|color| parse_color(color).ok())
            .map(|(r, g, b)| format!("#{r:02x}{g:02x}{b:02x}"))
            .take(16)
            .collect();
        if !valid_recents.is_empty() {
            clean.recent_colors = valid_recents;
        }
    }
    if let Some(restore) = raw.get("restore_on_reconnect").and_then(Value::as_bool) {
        clean.restore_on_reconnect = restore;
    }
    clean
}

pub fn sanitize_state(state: State) -> State {
    match serde_json::to_value(state) {
        Ok(value) => sanitize_json_state(&value),
        Err(_) => State::default(),
    }
}

pub fn load_state() -> State {
    let path = get_state_file_path();
    let mut state = fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .map(|value| sanitize_json_state(&value))
        .unwrap_or_default();
    if let Ok(env_ip) = env::var("WIZ_IP").or_else(|_| env::var("BULB_IP")) {
        if let Ok(clean) = validate_ip(&env_ip) {
            state.ip = clean;
        }
    }
    state
}

pub fn save_state_at(
    path: &std::path::Path,
    state: &State,
) -> Result<(), Box<dyn std::error::Error>> {
    let clean = sanitize_state(state.clone());
    let json_bytes = serde_json::to_vec_pretty(&clean)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let sequence = SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp_path =
        path.with_extension(format!("{}.{}.{}.tmp", std::process::id(), nonce, sequence));
    let mut temp = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)?;
    temp.write_all(&json_bytes)?;
    temp.sync_all()?;
    drop(temp);
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    Ok(())
}

pub fn save_state(state: &State) -> Result<(), Box<dyn std::error::Error>> {
    save_state_at(&get_state_file_path(), state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_ip() {
        assert!(validate_ip("192.168.0.102").is_ok());
        assert!(validate_ip("10.0.0.1").is_ok());
        assert!(validate_ip("").is_err());
        assert!(validate_ip("not_an_ip").is_err());
        assert!(validate_ip("255.255.255.255").is_err());
        assert!(validate_ip("224.0.0.1").is_err());
    }

    #[test]
    fn retains_valid_fields_when_neighbours_are_corrupt() {
        let state = sanitize_json_state(&json!({
            "ip": "10.0.0.5", "brightness": "not a brightness", "hex": "#123456",
            "kelvin": 50, "mode": "invalid", "restore_on_reconnect": true
        }));
        assert_eq!(state.ip, "10.0.0.5");
        assert_eq!(state.brightness, 255);
        assert_eq!(state.rgb, [18, 52, 86]);
        assert_eq!(state.kelvin, 2700);
        assert_eq!(state.mode, "color");
        assert!(state.restore_on_reconnect);
    }

    #[test]
    fn concurrent_saves_use_separate_temporary_files() {
        let directory = std::env::temp_dir().join(format!("wizctl-state-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("state.json");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let writers: Vec<_> = (0..8)
            .map(|brightness| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let state = State {
                        brightness,
                        ..State::default()
                    };
                    barrier.wait();
                    save_state_at(&path, &state).unwrap();
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        let saved: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(saved["brightness"].as_u64().is_some());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_commit_keeps_existing_destination() {
        let directory =
            std::env::temp_dir().join(format!("wizctl-state-failure-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("state.json");
        fs::create_dir(&path).unwrap();
        let marker = path.join("existing-state");
        fs::write(&marker, "keep me").unwrap();
        assert!(save_state_at(&path, &State::default()).is_err());
        assert_eq!(fs::read_to_string(marker).unwrap(), "keep me");
        fs::remove_dir_all(directory).unwrap();
    }
}

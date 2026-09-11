use std::env;
use std::fs;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::colors::{parse_color, DEFAULT_PRESET_COLORS};

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
        let env_ip = env::var("WIZ_IP")
            .or_else(|_| env::var("BULB_IP"))
            .ok()
            .and_then(|ip| validate_ip(&ip).ok());

        Self {
            ip: env_ip.unwrap_or_else(|| DEFAULT_BULB_IP.to_string()),
            power: true,
            brightness: 255,
            rgb: [255, 140, 0],
            hex: "#ff8c00".to_string(),
            kelvin: 2700,
            scene_id: 6,
            mode: "color".to_string(),
            recent_colors: DEFAULT_PRESET_COLORS.iter().map(|s| s.to_string()).collect(),
            restore_on_reconnect: false,
        }
    }
}

pub fn validate_ip(ip: &str) -> Result<String, String> {
    let val = ip.trim();
    if val.is_empty() {
        return Err("IP address cannot be empty".to_string());
    }

    let addr = Ipv4Addr::from_str(val)
        .map_err(|_| format!("Invalid IP address format: '{ip}'"))?;

    if addr.is_multicast() {
        return Err(format!("Multicast IP addresses are not permitted: '{ip}'"));
    }

    if addr == Ipv4Addr::new(255, 255, 255, 255) {
        return Err(format!("Global broadcast IP address is not permitted: '{ip}'"));
    }

    Ok(val.to_string())
}

pub fn get_state_file_path() -> PathBuf {
    let config_dir = if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("wizctl")
    } else if let Ok(home) = env::var("HOME") {
        PathBuf::from(home).join(".config").join("wizctl")
    } else {
        PathBuf::from(".").join(".config").join("wizctl")
    };

    if fs::create_dir_all(&config_dir).is_ok() {
        config_dir.join("state.json")
    } else if let Ok(home) = env::var("HOME") {
        let fallback = PathBuf::from(home).join(".wizctl");
        let _ = fs::create_dir_all(&fallback);
        fallback.join("state.json")
    } else {
        PathBuf::from("state.json")
    }
}

pub fn sanitize_state(mut state: State) -> State {
    if validate_ip(&state.ip).is_err() {
        state.ip = DEFAULT_BULB_IP.to_string();
    }

    state.brightness = state.brightness.max(1);

    if !(1000..=10000).contains(&state.kelvin) {
        state.kelvin = 2700;
    }

    if state.scene_id == 0 || state.scene_id > 40 {
        state.scene_id = 6;
    }

    // Ensure hex matches rgb
    state.hex = format!(
        "#{:02x}{:02x}{:02x}",
        state.rgb[0], state.rgb[1], state.rgb[2]
    );

    // Filter recent colors
    let mut valid_recents = Vec::new();
    for color in &state.recent_colors {
        if let Ok((r, g, b)) = parse_color(color) {
            let hex = format!("#{r:02x}{g:02x}{b:02x}");
            if !valid_recents.contains(&hex) {
                valid_recents.push(hex);
            }
        }
    }
    if valid_recents.is_empty() {
        state.recent_colors = DEFAULT_PRESET_COLORS.iter().map(|s| s.to_string()).collect();
    } else {
        valid_recents.truncate(16);
        state.recent_colors = valid_recents;
    }

    state
}

pub fn load_state() -> State {
    let path = get_state_file_path();
    let mut state = if path.is_file() {
        match fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<State>(&content) {
                Ok(loaded) => sanitize_state(loaded),
                Err(_) => State::default(),
            },
            Err(_) => State::default(),
        }
    } else {
        State::default()
    };

    // Environment override has highest precedence
    if let Ok(env_ip) = env::var("WIZ_IP").or_else(|_| env::var("BULB_IP")) {
        if let Ok(clean) = validate_ip(&env_ip) {
            state.ip = clean;
        }
    }

    state
}

pub fn save_state(state: &State) -> Result<(), Box<dyn std::error::Error>> {
    let clean = sanitize_state(state.clone());
    let path = get_state_file_path();
    let temp_path = path.with_extension("tmp");

    let json_bytes = serde_json::to_vec_pretty(&clean)?;
    fs::write(&temp_path, json_bytes)?;
    fs::rename(&temp_path, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_ip() {
        assert!(validate_ip("192.168.0.102").is_ok());
        assert!(validate_ip("10.0.0.1").is_ok());
        assert!(validate_ip("").is_err());
        assert!(validate_ip("not_an_ip").is_err());
        assert!(validate_ip("255.255.255.255").is_err());
        assert!(validate_ip("224.0.0.1").is_err()); // Multicast
    }

    #[test]
    fn test_sanitize_state() {
        let mut state = State::default();
        state.brightness = 0;
        state.kelvin = 200;
        let sanitized = sanitize_state(state);
        assert_eq!(sanitized.brightness, 1);
        assert_eq!(sanitized.kelvin, 2700);
    }
}

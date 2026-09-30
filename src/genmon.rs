use std::env;
use std::fs;
use std::path::PathBuf;

use crate::bulb::{get_pilot, PilotResult};
use crate::colors::get_scene_name;
use crate::state::load_state;
use crate::state::State;

const ICON_ON_BYTES: &[u8] = include_bytes!("../assets/panel_bulb_on.png");
const ICON_OFF_BYTES: &[u8] = include_bytes!("../assets/panel_bulb_off.png");
const ICON_OFFLINE_BYTES: &[u8] = include_bytes!("../assets/panel_bulb_offline.png");

pub fn get_panel_asset_dir() -> PathBuf {
    let base_dir = if let Ok(data_home) = env::var("XDG_DATA_HOME") {
        PathBuf::from(data_home).join("wizctl").join("assets")
    } else if let Ok(home) = env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("wizctl")
            .join("assets")
    } else {
        PathBuf::from("/tmp/wizctl/assets")
    };

    let _ = fs::create_dir_all(&base_dir);

    // Auto-extract embedded icons if missing or empty
    let on_path = base_dir.join("panel_bulb_on.png");
    if !on_path.exists() || fs::metadata(&on_path).map(|m| m.len()).unwrap_or(0) == 0 {
        let _ = fs::write(&on_path, ICON_ON_BYTES);
    }

    let off_path = base_dir.join("panel_bulb_off.png");
    if !off_path.exists() || fs::metadata(&off_path).map(|m| m.len()).unwrap_or(0) == 0 {
        let _ = fs::write(&off_path, ICON_OFF_BYTES);
    }

    let offline_path = base_dir.join("panel_bulb_offline.png");
    if !offline_path.exists() || fs::metadata(&offline_path).map(|m| m.len()).unwrap_or(0) == 0 {
        let _ = fs::write(&offline_path, ICON_OFFLINE_BYTES);
    }

    base_dir
}

pub fn generate_genmon_xml(target_ip: Option<&str>) -> String {
    let state = load_state();
    let ip = target_ip.unwrap_or(&state.ip);
    render_genmon_xml(ip, &state, None)
}

pub fn render_genmon_xml(ip: &str, state: &State, pilot: Option<&PilotResult>) -> String {
    let assets_dir = get_panel_asset_dir();

    let pct = pilot
        .and_then(|p| p.dimming)
        .unwrap_or_else(|| (state.brightness as f64 * 100.0 / 255.0).round() as u8);
    let power = pilot.and_then(|p| p.state).unwrap_or(state.power);
    let power_str = if power { "ON" } else { "OFF" };
    let scene_id = pilot
        .map(|p| p.scene_id.unwrap_or(0))
        .unwrap_or(if state.mode == "scene" {
            state.scene_id
        } else {
            0
        });
    let kelvin = pilot
        .map(|p| p.temp.unwrap_or(0))
        .unwrap_or(if state.mode == "kelvin" {
            state.kelvin
        } else {
            0
        });

    let (icon_path, status_text) = if pilot.is_none() {
        (
            assets_dir.join("panel_bulb_offline.png"),
            "OFFLINE".to_string(),
        )
    } else if power {
        let text = if scene_id > 0 {
            let sname = get_scene_name(scene_id).unwrap_or("Scene");
            format!("{sname} ({pct}%)")
        } else {
            format!("{pct}%")
        };
        (assets_dir.join("panel_bulb_on.png"), text)
    } else {
        (assets_dir.join("panel_bulb_off.png"), "OFF".to_string())
    };

    let mut tooltip_lines = vec![
        format!("WiZ Smart Light ({ip})"),
        format!("Status: {power_str}"),
        format!("Brightness: {pct}%"),
    ];

    if pilot.is_none() {
        tooltip_lines.push("Bulb unreachable; showing last known values".to_string());
    }
    if scene_id > 0 {
        let sname = get_scene_name(scene_id).unwrap_or("Unknown");
        tooltip_lines.push(format!("Scene: {sname} (ID {scene_id})"));
    } else if kelvin > 0 {
        tooltip_lines.push(format!("White Temp: {kelvin}K"));
    } else {
        tooltip_lines.push(format!("Color: {}", state.hex));
    }

    tooltip_lines.push("Left-click: Quick Widget | Double-click: Toggle".to_string());
    let tooltip = tooltip_lines.join("\n");

    let click_cmd = "wizctl widget --click";

    format!(
        "<img>{}</img>\n<txt> {} </txt>\n<tool>{}</tool>\n<click>{}</click>\n",
        icon_path.display(),
        status_text,
        tooltip,
        click_cmd
    )
}

pub fn run_genmon(target_ip: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let state = load_state();
    let ip = target_ip.unwrap_or(&state.ip);
    let pilot = get_pilot(ip).ok();
    print!("{}", render_genmon_xml(ip, &state, pilot.as_ref()));
    Ok(())
}

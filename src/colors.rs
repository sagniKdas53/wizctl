pub const DEFAULT_PRESET_COLORS: [&str; 10] = [
    "#ff453a", // Red
    "#ff9f0a", // Orange
    "#ffd60a", // Yellow
    "#32d74b", // Green
    "#64d2ff", // Cyan
    "#0a84ff", // Blue
    "#bf5af2", // Purple
    "#ff375f", // Pink
    "#ffd1a9", // Warm White
    "#f0f4f8", // Cool White
];

pub const SCENES: &[(u32, &str)] = &[
    (1, "Ocean"),
    (2, "Romance"),
    (3, "Sunset"),
    (4, "Party"),
    (5, "Fireplace"),
    (6, "Cozy"),
    (7, "Forest"),
    (8, "Pastel colors"),
    (9, "Wake-up"),
    (10, "Bedtime"),
    (11, "Warm white"),
    (12, "Daylight"),
    (13, "Cool white"),
    (14, "Night light"),
    (15, "Focus"),
    (16, "Relax"),
    (17, "True colors"),
    (18, "TV time"),
    (19, "Plantgrowth"),
    (20, "Spring"),
    (21, "Summer"),
    (22, "Fall"),
    (23, "Deep dive"),
    (24, "Jungle"),
    (25, "Mojito"),
    (26, "Club"),
    (27, "Christmas"),
    (28, "Halloween"),
    (29, "Candlelight"),
    (30, "Golden white"),
    (31, "Pulse"),
    (32, "Steampunk"),
    (33, "Diwali"),
    (34, "White"),
    (35, "Alarm"),
    (36, "Snowy sky"),
    (40, "Dim-to-warm"),
];

pub const NAMED_COLORS: &[(&str, (u8, u8, u8))] = &[
    ("red", (255, 0, 0)),
    ("green", (0, 255, 0)),
    ("blue", (0, 0, 255)),
    ("white", (255, 255, 255)),
    ("yellow", (255, 255, 0)),
    ("cyan", (0, 255, 255)),
    ("magenta", (255, 0, 255)),
    ("orange", (255, 128, 0)),
    ("purple", (128, 0, 255)),
    ("pink", (255, 80, 160)),
    ("warmwhite", (255, 214, 170)),
    ("coolwhite", (212, 235, 255)),
    ("daylight", (255, 255, 251)),
    ("gold", (255, 215, 0)),
    ("lime", (50, 205, 50)),
    ("teal", (0, 128, 128)),
    ("violet", (238, 130, 238)),
    ("indigo", (75, 0, 130)),
    ("amber", (255, 191, 0)),
    ("crimson", (220, 20, 60)),
    ("turquoise", (64, 224, 208)),
    ("coral", (255, 127, 80)),
];

pub fn get_scene_name(scene_id: u32) -> Option<&'static str> {
    for &(id, name) in SCENES {
        if id == scene_id {
            return Some(name);
        }
    }
    None
}

pub fn rgb_to_hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub fn kelvin_to_rgb(kelvin: u16) -> (u8, u8, u8) {
    let temp = (kelvin.clamp(1000, 40000) as f64) / 100.0;

    let red = if temp <= 66.0 {
        255.0
    } else {
        let r = temp - 60.0;
        329.698727446 * r.powf(-0.1332047592)
    }
    .clamp(0.0, 255.0);

    let green = if temp <= 66.0 {
        99.4708025861 * temp.max(1.0).ln() - 161.1195681661
    } else {
        let g = (temp - 60.0).max(1.0);
        288.1221695283 * g.powf(-0.0755148492)
    }
    .clamp(0.0, 255.0);

    let blue = if temp >= 66.0 {
        255.0
    } else if temp <= 19.0 {
        0.0
    } else {
        let b = (temp - 10.0).max(1.0);
        138.5177312231 * b.ln() - 305.0447927307
    }
    .clamp(0.0, 255.0);

    (red.round() as u8, green.round() as u8, blue.round() as u8)
}

/// Convert sRGB bytes to HSV with hue in degrees `[0, 360)` and s/v in `[0, 1]`.
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;

    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;

    let hue = if delta <= f32::EPSILON {
        0.0
    } else if max == rf {
        60.0 * (((gf - bf) / delta) % 6.0)
    } else if max == gf {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };

    let hue = if hue < 0.0 { hue + 360.0 } else { hue };
    let sat = if max <= f32::EPSILON { 0.0 } else { delta / max };
    (hue, sat, max)
}

/// Convert HSV (hue in degrees, s/v in `[0, 1]`) to sRGB bytes.
pub fn hsv_to_rgb(hue: f32, sat: f32, val: f32) -> (u8, u8, u8) {
    let h = hue.rem_euclid(360.0);
    let s = sat.clamp(0.0, 1.0);
    let v = val.clamp(0.0, 1.0);

    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (rf, gf, bf) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    (
        ((rf + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((gf + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((bf + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

pub fn parse_color(value: &str) -> Result<(u8, u8, u8), String> {
    let val = value.trim().to_lowercase();
    if val.is_empty() {
        return Err("Color value cannot be empty".to_string());
    }

    // Direct named match
    for &(name, rgb) in NAMED_COLORS {
        if val == name {
            return Ok(rgb);
        }
    }

    // Normalized named match (strip -, _, spaces)
    let norm_val: String = val.chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '_').collect();
    for &(name, rgb) in NAMED_COLORS {
        if norm_val == name {
            return Ok(rgb);
        }
    }

    // #RRGGBB or RRGGBB
    let hex_val = val.strip_prefix('#').unwrap_or(&val);
    if hex_val.len() == 6 && hex_val.chars().all(|c| c.is_ascii_hexdigit()) {
        let r = u8::from_str_radix(&hex_val[0..2], 16).map_err(|e| e.to_string())?;
        let g = u8::from_str_radix(&hex_val[2..4], 16).map_err(|e| e.to_string())?;
        let b = u8::from_str_radix(&hex_val[4..6], 16).map_err(|e| e.to_string())?;
        return Ok((r, g, b));
    }

    // #RGB or RGB
    if hex_val.len() == 3 && hex_val.chars().all(|c| c.is_ascii_hexdigit()) {
        let r_char = hex_val.chars().nth(0).unwrap();
        let g_char = hex_val.chars().nth(1).unwrap();
        let b_char = hex_val.chars().nth(2).unwrap();
        let r = u8::from_str_radix(&format!("{r_char}{r_char}"), 16).map_err(|e| e.to_string())?;
        let g = u8::from_str_radix(&format!("{g_char}{g_char}"), 16).map_err(|e| e.to_string())?;
        let b = u8::from_str_radix(&format!("{b_char}{b_char}"), 16).map_err(|e| e.to_string())?;
        return Ok((r, g, b));
    }

    // R, G, B
    let parts: Vec<&str> = val.split(',').map(|s| s.trim()).collect();
    if parts.len() == 3 {
        if let (Ok(r), Ok(g), Ok(b)) = (parts[0].parse::<u8>(), parts[1].parse::<u8>(), parts[2].parse::<u8>()) {
            return Ok((r, g, b));
        }
    }

    Err(format!(
        "Invalid color '{value}'. Use a name (e.g. red, warmwhite), #RRGGBB (#ff5500), #RGB (#f50), or R,G,B (255,128,0)"
    ))
}

pub fn parse_brightness(value: &str) -> Result<u8, String> {
    let val = value.trim();
    if val.is_empty() {
        return Err("Brightness value cannot be empty".to_string());
    }

    if let Some(pct_str) = val.strip_suffix('%') {
        let pct = pct_str
            .parse::<f64>()
            .map_err(|_| format!("Invalid brightness percentage: '{val}'"))?;
        if !(0.0..=100.0).contains(&pct) || pct.is_nan() || pct.is_infinite() {
            return Err("Brightness percentage must be between 0% and 100%".to_string());
        }
        return Ok((pct * 255.0 / 100.0).round().clamp(0.0, 255.0) as u8);
    }

    let b = val
        .parse::<i32>()
        .map_err(|_| format!("Invalid brightness value '{val}'. Use 0-255 or 0%-100%"))?;
    if !(0..=255).contains(&b) {
        return Err("Brightness must be between 0 and 255".to_string());
    }
    Ok(b as u8)
}

pub fn parse_kelvin(value: &str) -> Result<u16, String> {
    let val = value.trim();
    if val.is_empty() {
        return Err("Kelvin value cannot be empty".to_string());
    }

    let cleaned = val.trim_end_matches(|c| c == 'k' || c == 'K').trim();
    let k = cleaned
        .parse::<u32>()
        .map_err(|_| format!("Invalid Kelvin temperature: '{value}'. Expected integer e.g. 2700 or 4000K"))?;

    if !(1000..=10000).contains(&k) {
        return Err(format!(
            "Kelvin temperature {k}K is outside safe hardware range (1000K - 10000K, typical: 2200K - 6500K)"
        ));
    }
    Ok(k as u16)
}

pub fn parse_scene(value: &str) -> Result<(u32, &'static str), String> {
    let val = value.trim();
    if val.is_empty() {
        return Err("Scene value cannot be empty".to_string());
    }

    // Try parsing as numeric ID
    if let Ok(id) = val.parse::<u32>() {
        for &(sid, sname) in SCENES {
            if sid == id {
                return Ok((sid, sname));
            }
        }
        return Err(format!("Unknown WiZ scene ID: {id}"));
    }

    // Try parsing as scene name
    let norm_val: String = val.to_lowercase().chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '_').collect();
    for &(sid, sname) in SCENES {
        let norm_sname: String = sname.to_lowercase().chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '_').collect();
        if norm_val == norm_sname {
            return Ok((sid, sname));
        }
    }

    Err(format!("Unknown WiZ scene '{value}'. Run 'wizctl scenes' to list available scenes."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_color() {
        assert_eq!(parse_color("red").unwrap(), (255, 0, 0));
        assert_eq!(parse_color("warm-white").unwrap(), (255, 214, 170));
        assert_eq!(parse_color("#ff0000").unwrap(), (255, 0, 0));
        assert_eq!(parse_color("#00f").unwrap(), (0, 0, 255));
        assert_eq!(parse_color("255, 128, 0").unwrap(), (255, 128, 0));
        assert!(parse_color("invalid_color").is_err());
    }

    #[test]
    fn test_parse_brightness() {
        assert_eq!(parse_brightness("128").unwrap(), 128);
        assert_eq!(parse_brightness("50%").unwrap(), 128);
        assert_eq!(parse_brightness("100%").unwrap(), 255);
        assert_eq!(parse_brightness("0%").unwrap(), 0);
        assert!(parse_brightness("300").is_err());
        assert!(parse_brightness("150%").is_err());
    }

    #[test]
    fn test_parse_kelvin() {
        assert_eq!(parse_kelvin("2700").unwrap(), 2700);
        assert_eq!(parse_kelvin("4000K").unwrap(), 4000);
        assert_eq!(parse_kelvin("6500k").unwrap(), 6500);
        assert!(parse_kelvin("500").is_err());
        assert!(parse_kelvin("15000").is_err());
    }

    #[test]
    fn test_parse_scene() {
        assert_eq!(parse_scene("6").unwrap(), (6, "Cozy"));
        assert_eq!(parse_scene("cozy").unwrap(), (6, "Cozy"));
        assert_eq!(parse_scene("Sunset").unwrap(), (3, "Sunset"));
        assert_eq!(parse_scene("night-light").unwrap(), (14, "Night light"));
        assert!(parse_scene("999").is_err());
        assert!(parse_scene("not_a_scene").is_err());
    }

    #[test]
    fn test_kelvin_to_rgb() {
        let (r, g, b) = kelvin_to_rgb(2700);
        assert_eq!(r, 255);
        assert!(g > 150 && g < 185);
        assert!(b > 70 && b < 100);
    }

    #[test]
    fn test_hsv_roundtrip_preserves_color() {
        for &(r, g, b) in &[
            (255u8, 0u8, 0u8),
            (0, 255, 0),
            (0, 0, 255),
            (18, 200, 137),
            (240, 240, 240),
            (0, 0, 0),
        ] {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let (r2, g2, b2) = hsv_to_rgb(h, s, v);
            assert!(
                (r as i32 - r2 as i32).abs() <= 1
                    && (g as i32 - g2 as i32).abs() <= 1
                    && (b as i32 - b2 as i32).abs() <= 1,
                "roundtrip drifted: {r},{g},{b} -> {r2},{g2},{b2}"
            );
        }
    }

    #[test]
    fn test_hsv_hue_wraps_at_360() {
        assert_eq!(hsv_to_rgb(360.0, 1.0, 1.0), hsv_to_rgb(0.0, 1.0, 1.0));
        assert_eq!(hsv_to_rgb(-60.0, 1.0, 1.0), hsv_to_rgb(300.0, 1.0, 1.0));
    }
}

use wizctl::colors::*;
use wizctl::state::*;

#[test]
fn test_named_colors() {
    assert_eq!(parse_color("red").unwrap(), (255, 0, 0));
    assert_eq!(parse_color("blue").unwrap(), (0, 0, 255));
    assert_eq!(parse_color("green").unwrap(), (0, 255, 0));
    assert_eq!(parse_color("warmwhite").unwrap(), (255, 214, 170));
    assert_eq!(parse_color("cool-white").unwrap(), (212, 235, 255));
}

#[test]
fn test_hex_colors() {
    assert_eq!(parse_color("#ff5500").unwrap(), (255, 85, 0));
    assert_eq!(parse_color("ff5500").unwrap(), (255, 85, 0));
    assert_eq!(parse_color("#f50").unwrap(), (255, 85, 0));
    assert_eq!(parse_color("f50").unwrap(), (255, 85, 0));
}

#[test]
fn test_rgb_parsing() {
    assert_eq!(parse_color("255,128,0").unwrap(), (255, 128, 0));
    assert_eq!(parse_color("  255 , 128 , 0  ").unwrap(), (255, 128, 0));
}

#[test]
fn test_brightness_parsing() {
    assert_eq!(parse_brightness("0").unwrap(), 0);
    assert_eq!(parse_brightness("255").unwrap(), 255);
    assert_eq!(parse_brightness("128").unwrap(), 128);
    assert_eq!(parse_brightness("50%").unwrap(), 128);
    assert_eq!(parse_brightness("100%").unwrap(), 255);
    assert_eq!(parse_brightness("0%").unwrap(), 0);
    assert!(parse_brightness("256").is_err());
    assert!(parse_brightness("101%").is_err());
}

#[test]
fn test_kelvin_parsing() {
    assert_eq!(parse_kelvin("2700").unwrap(), 2700);
    assert_eq!(parse_kelvin("2700K").unwrap(), 2700);
    assert_eq!(parse_kelvin("6500k").unwrap(), 6500);
    assert!(parse_kelvin("999").is_err());
    assert!(parse_kelvin("10001").is_err());
}

#[test]
fn test_scene_parsing() {
    assert_eq!(parse_scene("1").unwrap(), (1, "Ocean"));
    assert_eq!(parse_scene("ocean").unwrap(), (1, "Ocean"));
    assert_eq!(parse_scene("Cozy").unwrap(), (6, "Cozy"));
    assert_eq!(parse_scene("candlelight").unwrap(), (29, "Candlelight"));
    assert_eq!(parse_scene("night light").unwrap(), (14, "Night light"));
    assert_eq!(parse_scene("night-light").unwrap(), (14, "Night light"));
}

#[test]
fn test_kelvin_to_rgb_curve() {
    let (r2700, g2700, b2700) = kelvin_to_rgb(2700);
    assert_eq!(r2700, 255);
    assert!(g2700 > 150 && g2700 < 185);
    assert!(b2700 > 70 && b2700 < 100);

    let (r6500, g6500, b6500) = kelvin_to_rgb(6500);
    assert_eq!(r6500, 255);
    assert!(b6500 >= 245);
    assert!(g6500 > 240);
}

#[test]
fn test_state_defaults_and_sanitization() {
    let mut state = State::default();
    assert_eq!(state.power, true);
    assert_eq!(state.brightness, 255);
    assert_eq!(state.kelvin, 2700);
    assert_eq!(state.scene_id, 6);

    state.brightness = 0;
    state.kelvin = 50000;
    let sanitized = sanitize_state(state);
    assert_eq!(sanitized.brightness, 1);
    assert_eq!(sanitized.kelvin, 2700);
}

//! A WiZ RGBTW bulb renders desaturated colors on its dedicated white LED.
//! Sending raw sRGB leaves the white channels wherever the previous command put
//! them, so a warm off-white came out as a cold bluish wash. These tests pin the
//! `pywizlight`-compatible channel split that fixes it.

use wizctl::bulb::{rgb_pilot_params, rgb_to_rgbcw, rgbcw_to_rgb, PilotResult};
use wizctl::colors::rgb_to_hsv;

/// Warm off-white must drive the white LED, not the RGB LEDs.
#[test]
fn near_white_lands_on_the_white_channel() {
    let ((r, g, b), w) = rgb_to_rgbcw(0xef, 0xdc, 0xd8);
    assert_eq!(w, 128, "white channel should be saturated, got {w}");
    assert!(
        u16::from(r) + u16::from(g) + u16::from(b) < 80,
        "RGB LEDs should stay dim for a near-white pick, got ({r}, {g}, {b})"
    );
    assert!(r > g && g >= b, "warm tint should lead on red: ({r}, {g}, {b})");
}

#[test]
fn fully_saturated_colors_keep_the_rgb_leds_and_drop_the_white() {
    assert_eq!(rgb_to_rgbcw(255, 0, 0), ((255, 0, 0), 0));
    assert_eq!(rgb_to_rgbcw(0, 255, 0), ((0, 255, 0), 0));
    assert_eq!(rgb_to_rgbcw(0, 0, 255), ((0, 0, 255), 0));
}

#[test]
fn pure_white_is_all_white_channel() {
    assert_eq!(rgb_to_rgbcw(255, 255, 255), ((0, 0, 0), 128));
    assert_eq!(rgbcw_to_rgb(0, 0, 0, 128), (255, 255, 255));
}

#[test]
fn pilot_params_carry_the_white_channel() {
    let params = rgb_pilot_params(0xef, 0xdc, 0xd8);
    assert_eq!(params["state"], true);
    assert_eq!(params["w"], 128);
    assert!(params["r"].is_number() && params["g"].is_number() && params["b"].is_number());
}

/// Hue survives the channel split; brightness and some saturation do not.
///
/// The bulb's RGB gamut is the hexagon spanned by three unit basis vectors, so a
/// fully saturated mid-hue like `#0080ff` legitimately reads back a little less
/// saturated. Hue is the part that must not move.
#[test]
fn roundtrip_preserves_hue() {
    for &(r, g, b) in &[
        (0xffu8, 0x00u8, 0x00u8),
        (0x00, 0x80, 0xff),
        (0x7c, 0x46, 0x55),
        (0xca, 0x99, 0xc0),
        (0x0d, 0xa7, 0x8d),
        (0xff, 0xff, 0x00),
    ] {
        let ((pr, pg, pb), w) = rgb_to_rgbcw(r, g, b);
        let (br, bg, bb) = rgbcw_to_rgb(pr, pg, pb, w);

        let (want_h, want_s, _) = rgb_to_hsv(r, g, b);
        let (got_h, _, _) = rgb_to_hsv(br, bg, bb);
        assert!(want_s >= 0.15, "fixture must be chromatic enough to have a hue");

        let delta = (want_h - got_h).abs();
        let delta = delta.min(360.0 - delta);
        assert!(
            delta <= 4.0,
            "hue drifted for #{r:02x}{g:02x}{b:02x}: want {want_h:.1}, got {got_h:.1}"
        );
    }
}

/// The white channel is what carries desaturation, so it must fall as the pick
/// gets more colorful. This is the ordering the bluish-wash bug violated.
///
/// Off-primary hues never reach the hexagon corner, so a nominally full
/// saturation at 30° keeps a little white rather than dropping to zero — only
/// the primaries and secondaries bottom out (covered above).
#[test]
fn white_channel_falls_as_saturation_rises() {
    let mut previous = u16::MAX;
    for step in 0..=10 {
        let sat = step as f32 / 10.0;
        let (r, g, b) = wizctl::colors::hsv_to_rgb(30.0, sat, 1.0);
        let (_, w) = rgb_to_rgbcw(r, g, b);
        assert!(
            u16::from(w) <= previous,
            "white channel rose at saturation {sat}: {w} > {previous}"
        );
        if step == 0 {
            assert_eq!(w, 128, "a gray pick must run entirely on the white LED");
        }
        previous = u16::from(w);
    }
    assert!(
        previous < 40,
        "a saturated pick should barely use the white LED, got {previous}"
    );
}

#[test]
fn readback_recognizes_the_color_it_was_given() {
    let pick = [0xef, 0xdc, 0xd8];
    let ((r, g, b), w) = rgb_to_rgbcw(pick[0], pick[1], pick[2]);
    let pilot = PilotResult {
        r: Some(r),
        g: Some(g),
        b: Some(b),
        w: Some(w),
        ..Default::default()
    };

    assert!(pilot.matches_rgb(pick), "confirm poll should match the pick");
    assert!(!pilot.matches_rgb([0x00, 0x1a, 0xc9]));
}

#[test]
fn all_channels_dark_reports_no_color() {
    let dark = PilotResult {
        r: Some(0),
        g: Some(0),
        b: Some(0),
        w: Some(0),
        temp: Some(2700),
        ..Default::default()
    };
    assert_eq!(dark.rgb(), None, "white/scene mode must not read back as black");
}

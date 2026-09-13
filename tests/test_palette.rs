//! Integration coverage for palette extraction against a real photo.
//!
//! Sample photos are not part of this checkout, so the assertions are skipped
//! when none are found. Set `WIZCTL_SAMPLE_IMAGE` to point at a specific file.

use std::path::{Path, PathBuf};

use wizctl::palette::{extract_palette, is_supported_image, load_image_info};

fn is_lowercase_hex(hex: &str) -> bool {
    let bytes = hex.as_bytes();
    bytes.len() == 7
        && bytes[0] == b'#'
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

/// Smallest sample image available, so an unoptimized `cargo test` stays quick.
fn sample_image() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("WIZCTL_SAMPLE_IMAGE") {
        let path = PathBuf::from(explicit);
        return path.is_file().then_some(path);
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [manifest.join("samples"), manifest.join("../../samples")];

    candidates
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_supported_image(path))
        .min_by_key(|path| std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX))
}

#[test]
fn rejects_non_image_extensions() {
    assert!(!is_supported_image(Path::new("foo.txt")));
    assert!(!is_supported_image(Path::new("foo")));
    assert!(is_supported_image(Path::new("foo.JPG")));
    assert!(is_supported_image(Path::new("foo.webp")));
}

#[test]
fn extracts_palette_from_sample_photo() {
    let Some(path) = sample_image() else {
        eprintln!("skipping: no sample image found");
        return;
    };

    assert!(is_supported_image(&path));

    let palette = extract_palette(&path, 8).expect("palette from sample photo");
    assert!(
        (1..=8).contains(&palette.len()),
        "expected 1..=8 colors, got {}",
        palette.len()
    );

    let mut sum = 0.0_f32;
    let mut previous = u64::MAX;
    for color in &palette {
        assert!(color.percentage > 0.0, "zero-weight color {color:?}");
        assert!(color.pixels > 0);
        assert!(
            color.pixels <= previous,
            "palette not sorted descending: {palette:?}"
        );
        previous = color.pixels;

        let hex = color.hex();
        assert!(is_lowercase_hex(&hex), "bad hex {hex}");
        sum += color.percentage;
    }
    assert!(
        (sum - 100.0).abs() <= 1.0,
        "percentages summed to {sum}, expected ~100"
    );
}

#[test]
fn thumbnail_preserves_original_dimensions_and_buffer_size() {
    let Some(path) = sample_image() else {
        eprintln!("skipping: no sample image found");
        return;
    };

    let info = load_image_info(&path, 64).expect("image info from sample photo");
    assert!(info.width > 0 && info.height > 0);
    assert!(
        info.thumb_width <= 64 && info.thumb_height <= 64,
        "thumbnail {}x{} exceeds the 64px box",
        info.thumb_width,
        info.thumb_height
    );
    assert_eq!(
        info.thumb_rgba.len(),
        info.thumb_width * info.thumb_height * 4
    );
}

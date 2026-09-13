//! Dominant-color extraction from images, for feeding colors to a WiZ bulb.
//!
//! Port of the Python `wizctl.palette` module. Pillow's `MEDIANCUT` quantizer is
//! replaced by a self-contained median-cut implementation because the `image`
//! crate's `color_quant` (NeuQuant) backend is not part of our feature set.

use std::path::Path;

use image::{DynamicImage, GenericImageView, RgbaImage};

/// Longest edge (in pixels) the image is reduced to before quantizing.
const MAX_EDGE: u32 = 512;

/// Upper bound on the number of pixels fed into median cut.
const MAX_SAMPLES: usize = 200_000;

/// A dominant RGB color and its share of the sampled pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaletteColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub pixels: u64,
    pub percentage: f32,
}

impl PaletteColor {
    /// The color in wizctl's accepted hexadecimal form (`#rrggbb`, lowercase).
    pub fn hex(&self) -> String {
        crate::colors::rgb_to_hex(self.r, self.g, self.b)
    }
}

/// Original image dimensions plus a decoded RGBA thumbnail for previewing.
#[derive(Debug, Clone)]
pub struct ImageInfo {
    /// Original pixel width.
    pub width: u32,
    /// Original pixel height.
    pub height: u32,
    pub thumb_width: usize,
    pub thumb_height: usize,
    /// `thumb_width * thumb_height * 4` bytes, row-major RGBA.
    pub thumb_rgba: Vec<u8>,
}

/// Return up to `colors` dominant colors, most frequent first.
pub fn extract_palette(path: &Path, colors: usize) -> Result<Vec<PaletteColor>, String> {
    if !(1..=16).contains(&colors) {
        return Err("palette size must be between 1 and 16".to_string());
    }
    let image = decode(path)?;

    // Shrink first, expand to RGBA second: a 66 MP photo never materializes as a
    // 265 MB RGBA buffer this way.
    let small = shrink_to_rgba(image, MAX_EDGE);

    // Drop fully transparent pixels, stride-subsampling anything oversized.
    let raw = small.as_raw();
    let total = raw.len() / 4;
    let stride = if total > MAX_SAMPLES {
        total.div_ceil(MAX_SAMPLES)
    } else {
        1
    };
    let mut samples: Vec<[u8; 3]> = Vec::with_capacity(total / stride + 1);
    for pixel in raw.chunks_exact(4).step_by(stride) {
        if pixel[3] > 0 {
            samples.push([pixel[0], pixel[1], pixel[2]]);
        }
    }
    if samples.is_empty() {
        return Err(format!("image has no visible pixels: {}", path.display()));
    }

    let visible = samples.len() as f64;
    let mut palette: Vec<PaletteColor> = median_cut(&mut samples, colors)
        .into_iter()
        .map(|(rgb, count)| PaletteColor {
            r: rgb[0],
            g: rgb[1],
            b: rgb[2],
            pixels: count as u64,
            percentage: (count as f64 * 100.0 / visible) as f32,
        })
        .collect();
    // Most dominant first; hex keeps equal-weight buckets deterministically ordered.
    palette.sort_by(|a, b| b.pixels.cmp(&a.pixels).then_with(|| a.hex().cmp(&b.hex())));
    Ok(palette)
}

/// Decode `path`, reporting the original size plus an RGBA thumbnail whose
/// longest edge is `thumb_max`.
pub fn load_image_info(path: &Path, thumb_max: u32) -> Result<ImageInfo, String> {
    let image = decode(path)?;
    let (width, height) = image.dimensions();
    let thumb = shrink_to_rgba(image, thumb_max.max(1));
    let (thumb_width, thumb_height) = (thumb.width(), thumb.height());
    Ok(ImageInfo {
        width,
        height,
        thumb_width: thumb_width as usize,
        thumb_height: thumb_height as usize,
        thumb_rgba: thumb.into_raw(),
    })
}

/// True when the file extension is one the `image` feature set can decode.
pub fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| {
            matches!(
                ext.as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff"
            )
        })
}

fn decode(path: &Path) -> Result<DynamicImage, String> {
    if !path.is_file() {
        return Err(format!("image not found: {}", path.display()));
    }
    image::open(path).map_err(|e| format!("cannot read image '{}': {e}", path.display()))
}

/// Reduce `image` so its longest edge is at most `max`, then expand to RGBA8.
/// Resampling the still-narrow source representation (usually RGB8) first keeps
/// the peak allocation and the filter cost proportional to the original decode
/// rather than to a 4-channel copy of it.
fn shrink_to_rgba(image: DynamicImage, max: u32) -> RgbaImage {
    let (width, height) = image.dimensions();
    let (thumb_width, thumb_height) = fit(width, height, max);
    if (thumb_width, thumb_height) == (width, height) {
        image.into_rgba8()
    } else {
        image.thumbnail(thumb_width, thumb_height).into_rgba8()
    }
}

/// Largest box that fits `max` on its longest edge, preserving aspect ratio.
/// Never upscales.
fn fit(width: u32, height: u32, max: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest == 0 || longest <= max {
        return (width, height);
    }
    let scale = f64::from(max) / f64::from(longest);
    let scaled = |edge: u32| ((f64::from(edge) * scale).round() as u32).max(1);
    (scaled(width), scaled(height))
}

/// One contiguous slice of `samples`, with its widest channel cached.
struct Bucket {
    start: usize,
    end: usize,
    channel: usize,
    spread: u8,
}

impl Bucket {
    fn new(samples: &[[u8; 3]], start: usize, end: usize) -> Self {
        let mut channel = 0;
        let mut spread = 0;
        for axis in 0..3 {
            let mut lo = u8::MAX;
            let mut hi = u8::MIN;
            for pixel in &samples[start..end] {
                lo = lo.min(pixel[axis]);
                hi = hi.max(pixel[axis]);
            }
            let range = hi.saturating_sub(lo);
            if range > spread {
                spread = range;
                channel = axis;
            }
        }
        Bucket {
            start,
            end,
            channel,
            spread,
        }
    }

    fn splittable(&self) -> bool {
        self.end - self.start > 1 && self.spread > 0
    }
}

/// Median cut: repeatedly split the widest bucket along its widest channel at
/// the median until `target` buckets exist or no bucket can be split further.
/// The cut is snapped to the nearest run boundary so pixels sharing a value on
/// the split channel always land in the same bucket (this is what keeps a
/// two-tone image at two colors instead of shredding the larger tone).
/// Returns `(mean color, pixel count)` per bucket.
fn median_cut(samples: &mut [[u8; 3]], target: usize) -> Vec<([u8; 3], usize)> {
    let len = samples.len();
    let mut buckets = vec![Bucket::new(samples, 0, len)];

    while buckets.len() < target {
        let Some(index) = buckets
            .iter()
            .enumerate()
            .filter(|(_, bucket)| bucket.splittable())
            .max_by_key(|(_, bucket)| bucket.spread)
            .map(|(index, _)| index)
        else {
            break;
        };
        let Bucket {
            start, end, channel, ..
        } = buckets[index];
        let slice = &mut samples[start..end];
        slice.sort_unstable_by_key(|pixel| pixel[channel]);
        let middle = start + boundary_near_median(slice, channel);
        buckets[index] = Bucket::new(samples, start, middle);
        buckets.push(Bucket::new(samples, middle, end));
    }

    buckets
        .iter()
        .map(|bucket| {
            let pixels = &samples[bucket.start..bucket.end];
            let count = pixels.len();
            let mut sums = [0u64; 3];
            for pixel in pixels {
                for axis in 0..3 {
                    sums[axis] += u64::from(pixel[axis]);
                }
            }
            let mean = |axis: usize| {
                let total = count as u64;
                (((sums[axis] * 2 + total) / (total * 2)).min(255)) as u8
            };
            ([mean(0), mean(1), mean(2)], count)
        })
        .collect()
}

/// Index (relative to `sorted`) of the value-change boundary closest to the
/// median. `sorted` must be sorted by `channel` and span at least two distinct
/// values on it, which guarantees a boundary in `1..sorted.len()` exists.
fn boundary_near_median(sorted: &[[u8; 3]], channel: usize) -> usize {
    let len = sorted.len();
    let median = len / 2;
    let is_boundary = |index: usize| sorted[index - 1][channel] != sorted[index][channel];
    for offset in 0..len {
        if median + offset < len && is_boundary(median + offset) {
            return median + offset;
        }
        if median >= offset + 1 && is_boundary(median - offset) {
            return median - offset;
        }
    }
    median
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Writes a synthetic PNG to the temp dir and removes it on drop.
    struct TempImage {
        path: PathBuf,
    }

    impl TempImage {
        fn new(name: &str, width: u32, height: u32, fill: impl Fn(u32, u32) -> [u8; 4]) -> Self {
            let image =
                image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(fill(x, y)));
            let path = std::env::temp_dir().join(format!(
                "wizctl_palette_test_{}_{}.png",
                name,
                std::process::id()
            ));
            image.save(&path).expect("write synthetic png");
            TempImage { path }
        }
    }

    impl Drop for TempImage {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn rejects_palette_size_out_of_range() {
        let image = TempImage::new("bounds", 8, 8, |_, _| [10, 20, 30, 255]);
        for colors in [0, 17, 64] {
            assert_eq!(
                extract_palette(&image.path, colors),
                Err("palette size must be between 1 and 16".to_string()),
                "colors={colors} should be rejected"
            );
        }
        assert!(extract_palette(&image.path, 1).is_ok());
        assert!(extract_palette(&image.path, 16).is_ok());
    }

    #[test]
    fn reports_missing_file() {
        let missing = std::env::temp_dir().join("wizctl_palette_definitely_absent.png");
        assert_eq!(
            extract_palette(&missing, 4),
            Err(format!("image not found: {}", missing.display()))
        );
        assert_eq!(
            load_image_info(&missing, 64).unwrap_err(),
            format!("image not found: {}", missing.display())
        );
    }

    #[test]
    fn solid_color_yields_one_dominant_color() {
        let image = TempImage::new("solid", 40, 40, |_, _| [12, 200, 90, 255]);
        let palette = extract_palette(&image.path, 6).expect("palette");
        assert_eq!(palette[0].hex(), "#0cc85a");
        assert!(
            (palette[0].percentage - 100.0).abs() < 0.01,
            "expected ~100%, got {}",
            palette[0].percentage
        );
        assert_eq!(palette[0].pixels, 1600);
        // Median cut cannot split a zero-spread bucket, so only one color exists.
        assert_eq!(palette.len(), 1);
    }

    #[test]
    fn three_quarters_red_one_quarter_blue() {
        // 64 wide: first 48 columns red, last 16 blue -> 75% / 25%.
        let image = TempImage::new("split", 64, 16, |x, _| {
            if x < 48 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        });
        let palette = extract_palette(&image.path, 4).expect("palette");
        assert_eq!(palette.len(), 2);
        assert_eq!(palette[0].hex(), "#ff0000");
        assert_eq!(palette[1].hex(), "#0000ff");
        assert!(
            (palette[0].percentage - 75.0).abs() < 1.0,
            "red was {}",
            palette[0].percentage
        );
        assert!(
            (palette[1].percentage - 25.0).abs() < 1.0,
            "blue was {}",
            palette[1].percentage
        );
    }

    #[test]
    fn transparent_pixels_are_ignored() {
        // Top half fully transparent black, bottom half opaque green.
        let image = TempImage::new("alpha", 32, 32, |_, y| {
            if y < 16 {
                [0, 0, 0, 0]
            } else {
                [0, 128, 0, 255]
            }
        });
        let palette = extract_palette(&image.path, 5).expect("palette");
        assert_eq!(palette.len(), 1);
        assert_eq!(palette[0].hex(), "#008000");
        assert_eq!(palette[0].pixels, 32 * 16);
        assert!((palette[0].percentage - 100.0).abs() < 0.01);
    }

    #[test]
    fn fully_transparent_image_is_rejected() {
        let image = TempImage::new("empty", 12, 12, |_, _| [40, 40, 40, 0]);
        assert_eq!(
            extract_palette(&image.path, 3),
            Err(format!("image has no visible pixels: {}", image.path.display()))
        );
    }

    #[test]
    fn image_info_keeps_original_size_and_scales_thumbnail() {
        let image = TempImage::new("info", 200, 100, |x, _| {
            [(x % 256) as u8, 64, 128, 255]
        });
        let info = load_image_info(&image.path, 50).expect("info");
        assert_eq!((info.width, info.height), (200, 100));
        assert_eq!((info.thumb_width, info.thumb_height), (50, 25));
        assert_eq!(info.thumb_rgba.len(), 50 * 25 * 4);

        // Smaller than thumb_max: never upscaled.
        let info = load_image_info(&image.path, 4096).expect("info");
        assert_eq!((info.thumb_width, info.thumb_height), (200, 100));
        assert_eq!(info.thumb_rgba.len(), 200 * 100 * 4);
    }

    #[test]
    fn supported_extensions() {
        for name in [
            "a.png", "b.JPG", "c.jpeg", "d.webp", "e.gif", "f.bmp", "g.tif", "h.TIFF",
        ] {
            assert!(is_supported_image(Path::new(name)), "{name}");
        }
        for name in ["a.txt", "b.svg", "c", "d.png.gz", "pngfile"] {
            assert!(!is_supported_image(Path::new(name)), "{name}");
        }
    }
}

//! Black-box coverage for the Rust palette subcommand's non-interactive modes.

use std::path::PathBuf;
use std::process::Command;

struct TempImage {
    path: PathBuf,
}

impl TempImage {
    fn two_colors() -> Self {
        let path = std::env::temp_dir().join(format!(
            "wizctl_palette_cli_{}_{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let image = image::RgbaImage::from_fn(2, 2, |x, y| {
            if x == 1 && y == 1 {
                image::Rgba([0, 0, 255, 255])
            } else {
                image::Rgba([255, 0, 0, 255])
            }
        });
        image.save(&path).expect("write image fixture");
        Self { path }
    }
}

impl Drop for TempImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn wizctl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wizctl"))
}

#[test]
fn plain_palette_is_numbered_deterministically_and_accepts_global_ip() {
    let image = TempImage::two_colors();
    let output = wizctl()
        .args([
            "--ip",
            "192.168.0.50",
            "palette",
            image.path.to_str().expect("utf-8 test path"),
            "--colors=2",
            "--plain",
        ])
        .output()
        .expect("run wizctl");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 output"),
        format!(
            "Palette from {}:\n  1. #ff0000   75.0%  wizctl color '#ff0000'\n  2. #0000ff   25.0%  wizctl color '#0000ff'\n",
            image.path.display()
        )
    );
}

#[test]
fn apply_rejects_out_of_range_choice_before_contacting_bulb() {
    let image = TempImage::two_colors();
    let output = wizctl()
        .args([
            "palette",
            image.path.to_str().expect("utf-8 test path"),
            "--colors",
            "2",
            "--apply",
            "3",
        ])
        .output()
        .expect("run wizctl");

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "wizctl: palette choice must be between 1 and 2\n"
    );
}

#[test]
fn tui_flag_reports_interactive_terminal_requirement_when_piped() {
    let image = TempImage::two_colors();
    let output = wizctl()
        .args([
            "palette",
            image.path.to_str().expect("utf-8 test path"),
            "--tui",
        ])
        .output()
        .expect("run wizctl");

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "wizctl: palette picker needs an interactive ANSI terminal\n"
    );
}

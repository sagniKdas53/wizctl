use wizctl::{bulb, genmon, state, studio, ui};

use std::env;
use std::fs;
use std::path::Path;
use std::process;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!(
        r#"wizctl {VERSION} - Control WiZ smart light bulbs over the local LAN

USAGE:
    wizctl [OPTIONS] [SUBCOMMAND]

OPTIONS:
    -h, --help           Print help information
    -v, --version        Print version information
    -i, --ip <IP>        IP address of the WiZ bulb (default: 192.168.0.102 or $WIZ_IP)

SUBCOMMANDS:
    widget               Open the compact panel popover (default)
    gui, studio          Open the full WiZ Controller studio window
    on                   Turn the bulb on
    off                  Turn the bulb off
    toggle               Toggle bulb power state
    status               Show bulb status
    color <VALUE>        Set RGB color (name, hex #ff5500, or R,G,B)
    brightness <VALUE>   Set brightness (0-255 or 0%-100%)
    kelvin <VALUE>       Set color temperature in Kelvin (1000K-10000K)
    scene <VALUE>        Set WiZ scene by ID or name
    scenes               List all available WiZ scenes
    wizclick [MODE]      View or activate WiZclick wall switch modes
    genmon               Output XML status block for xfce4-genmon-plugin
"#
    );
}

fn handle_panel_click(target_ip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let stamp_file = Path::new("/tmp/wizctl_panel_click_stamp");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    let mut is_double_click = false;
    if stamp_file.exists() {
        if let Ok(content) = fs::read_to_string(stamp_file) {
            if let Ok(prev) = content.trim().parse::<f64>() {
                if now - prev <= 0.35 {
                    is_double_click = true;
                }
            }
        }
    }

    if is_double_click {
        let _ = fs::remove_file(stamp_file);
        bulb::command_toggle(target_ip)?;
        Ok(())
    } else {
        let _ = fs::write(stamp_file, format!("{now}"));
        thread::sleep(Duration::from_millis(350));
        if stamp_file.exists() {
            if let Ok(content) = fs::read_to_string(stamp_file) {
                if let Ok(cur) = content.trim().parse::<f64>() {
                    if (cur - now).abs() < 0.001 {
                        let _ = fs::remove_file(stamp_file);
                        ui::run_widget(Some(target_ip.to_string()))
                            .map_err(|e| format!("Widget error: {e}"))?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let mut target_ip = state::load_state().ip;
    let mut click_mode = false;
    let mut positional = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                return;
            }
            "-v" | "--version" => {
                println!("wizctl {VERSION}");
                return;
            }
            "-i" | "--ip" => {
                if i + 1 < args.len() {
                    i += 1;
                    match state::validate_ip(&args[i]) {
                        Ok(ip) => target_ip = ip,
                        Err(e) => {
                            eprintln!("wizctl: {e}");
                            process::exit(1);
                        }
                    }
                } else {
                    eprintln!("wizctl: option '--ip' requires an argument");
                    process::exit(1);
                }
            }
            "--click" => {
                click_mode = true;
            }
            arg if arg.starts_with("--ip=") => {
                let ip = &arg[5..];
                match state::validate_ip(ip) {
                    Ok(clean) => target_ip = clean,
                    Err(e) => {
                        eprintln!("wizctl: {e}");
                        process::exit(1);
                    }
                }
            }
            other => {
                positional.push(other.to_string());
            }
        }
        i += 1;
    }

    let command = positional.first().map(|s| s.as_str()).unwrap_or("widget");

    let result: Result<(), Box<dyn std::error::Error>> = match command {
        "widget" => {
            if click_mode {
                handle_panel_click(&target_ip)
            } else {
                ui::run_widget(Some(target_ip)).map_err(|e| format!("Widget error: {e}").into())
            }
        }
        "gui" | "studio" => {
            studio::run_studio(Some(target_ip)).map_err(|e| format!("Studio error: {e}").into())
        }
        "on" => bulb::command_on(&target_ip),
        "off" => bulb::command_off(&target_ip),
        "toggle" => {
            bulb::command_toggle(&target_ip).map(|_| ())
        }
        "status" => bulb::command_status(&target_ip),
        "color" => {
            if positional.len() > 1 {
                bulb::command_color(&target_ip, &positional[1]).map(|_| ())
            } else {
                eprintln!("wizctl: color command requires a value (e.g. red, #ff5500, or 255,128,0)");
                process::exit(1);
            }
        }
        "brightness" => {
            if positional.len() > 1 {
                bulb::command_brightness(&target_ip, &positional[1]).map(|_| ())
            } else {
                eprintln!("wizctl: brightness command requires a value (0-255 or 0%-100%)");
                process::exit(1);
            }
        }
        "kelvin" => {
            if positional.len() > 1 {
                bulb::command_kelvin(&target_ip, &positional[1]).map(|_| ())
            } else {
                eprintln!("wizctl: kelvin command requires a temperature (e.g. 2700, 4000K)");
                process::exit(1);
            }
        }
        "scene" => {
            if positional.len() > 1 {
                bulb::command_scene(&target_ip, &positional[1]).map(|_| ())
            } else {
                eprintln!("wizctl: scene command requires a scene name or ID (e.g. cozy, sunset)");
                process::exit(1);
            }
        }
        "scenes" => {
            bulb::command_scenes();
            Ok(())
        }
        "wizclick" => {
            let mode = if positional.len() > 1 {
                match positional[1].parse::<u32>() {
                    Ok(m) => Some(m),
                    Err(_) => {
                        eprintln!("wizctl: invalid WiZclick mode '{}'", positional[1]);
                        process::exit(1);
                    }
                }
            } else {
                None
            };
            bulb::command_wizclick(&target_ip, mode)
        }
        "genmon" => genmon::run_genmon(Some(&target_ip)),
        unknown => {
            eprintln!("wizctl: unknown command '{unknown}'. Run 'wizctl --help' for usage.");
            process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("wizctl: {e}");
        process::exit(1);
    }
}

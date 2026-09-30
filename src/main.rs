use wizctl::{bulb, genmon, palette, state, studio, ui};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
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
    brightness <VALUE>   Set brightness (0 = OFF; 25-255 or 10%-100%)
    kelvin <VALUE>       Set color temperature in Kelvin (1000K-10000K)
    scene <VALUE>        Set WiZ scene by ID or name
    scenes               List all available WiZ scenes
    palette <IMAGE>      Extract dominant image colors (see `palette --help`)
    wizclick [MODE]      View or activate WiZclick wall switch modes
    genmon               Output XML status block for xfce4-genmon-plugin
"#
    );
}

fn print_palette_help() {
    println!(
        r#"USAGE:
    wizctl [--ip <IP>] palette <IMAGE> [--colors <N>] [--apply <N>] [--plain] [--tui]

OPTIONS:
    --colors <N>         Number of colors to extract, from 1 to 16 (default: 6)
    --apply <N>          Set the numbered palette color on the bulb
    --plain              Print the palette instead of opening the interactive picker
    --tui                Open the interactive palette picker
"#
    );
}

fn format_palette(image: &str, colors: &[palette::PaletteColor]) -> String {
    let mut lines = vec![format!("Palette from {image}:")];
    for (index, color) in colors.iter().enumerate() {
        let hex = color.hex();
        lines.push(format!(
            "  {}. {hex}  {:5.1}%  wizctl color '{hex}'",
            index + 1,
            color.percentage
        ));
    }
    lines.join("\n")
}

#[cfg(windows)]
fn supports_tui() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

#[cfg(not(windows))]
fn supports_tui() -> bool {
    io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && env::var("TERM").map(|term| term != "dumb").unwrap_or(false)
}

struct TerminalMode;

impl Drop for TerminalMode {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, crossterm::style::ResetColor);
        let _ = stdout.write_all(b"\x1b[?25h\n");
    }
}

/// Pick a color with Crossterm's cross-platform raw-mode and event APIs.
fn choose_palette_color(
    image: &str,
    colors: &[palette::PaletteColor],
    ip: &str,
) -> Result<Option<palette::PaletteColor>, String> {
    if !supports_tui() {
        return Err("palette picker needs an interactive ANSI terminal".to_string());
    }
    enable_raw_mode().map_err(|e| format!("palette picker could not configure terminal: {e}"))?;
    let _mode = TerminalMode;

    let mut stdout = io::stdout();
    let mut selected = 0usize;
    let result = loop {
        let mut frame = format!("\x1b[2J\x1b[H\x1b[1mImage palette\x1b[0m  {image}\n");
        frame.push_str(&format!(
            "Use Up/Down then Enter to apply to {ip}; q or Esc cancels.\n\n"
        ));
        for (index, color) in colors.iter().enumerate() {
            let marker = if index == selected { ">" } else { " " };
            let hex = color.hex();
            frame.push_str(&format!(
                "{marker} {}. \x1b[48;2;{};{};{}m      \x1b[0m {hex}  {:5.1}%\n",
                index + 1,
                color.r,
                color.g,
                color.b,
                color.percentage
            ));
        }
        if stdout
            .write_all(frame.as_bytes())
            .and_then(|_| stdout.flush())
            .is_err()
        {
            break Err("palette picker could not write to terminal".to_string());
        }
        match event::read()
            .map_err(|e| format!("palette picker could not read from terminal: {e}"))?
        {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Enter => break Ok(Some(colors[selected])),
                KeyCode::Char('q') | KeyCode::Esc => break Ok(None),
                KeyCode::Char('c')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    break Ok(None)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = (selected + colors.len() - 1) % colors.len()
                }
                KeyCode::Down | KeyCode::Char('j') => selected = (selected + 1) % colors.len(),
                _ => {}
            },
            _ => {}
        }
    };
    result
}

fn run_palette(target_ip: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args
        .first()
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        print_palette_help();
        return Ok(());
    }
    let Some(image) = args.first() else {
        return Err("palette command requires an image path".into());
    };
    let mut count = 6usize;
    let mut apply = None;
    let mut plain = false;
    let mut tui = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--colors" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or("option '--colors' requires an argument")?;
                count = value
                    .parse()
                    .map_err(|_| "--colors must be a number from 1 to 16")?;
            }
            "--apply" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or("option '--apply' requires an argument")?;
                apply = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| "--apply must be a palette number")?,
                );
            }
            "--plain" => plain = true,
            "--tui" => tui = true,
            "-h" | "--help" => {
                print_palette_help();
                return Ok(());
            }
            option if option.starts_with("--colors=") => {
                count = option[9..]
                    .parse()
                    .map_err(|_| "--colors must be a number from 1 to 16")?;
            }
            option if option.starts_with("--apply=") => {
                apply = Some(
                    option[8..]
                        .parse::<usize>()
                        .map_err(|_| "--apply must be a palette number")?,
                );
            }
            unknown => return Err(format!("unknown palette option '{unknown}'").into()),
        }
        index += 1;
    }
    if plain && tui {
        return Err("--plain and --tui cannot be used together".into());
    }
    let colors = palette::extract_palette(Path::new(image), count)?;
    if let Some(choice) = apply {
        if !(1..=colors.len()).contains(&choice) {
            return Err(format!("palette choice must be between 1 and {}", colors.len()).into());
        }
        println!("{}", format_palette(image, &colors));
        bulb::command_color(target_ip, &colors[choice - 1].hex())?;
    } else if tui || (!plain && supports_tui()) {
        if let Some(color) = choose_palette_color(image, &colors, target_ip)? {
            bulb::command_color(target_ip, &color.hex())?;
        }
    } else {
        println!("{}", format_palette(image, &colors));
    }
    Ok(())
}

fn handle_panel_click(target_ip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let stamp_file = ui::runtime_dir().join("panel_click_stamp");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    let mut is_double_click = false;
    if stamp_file.exists() {
        if let Ok(content) = fs::read_to_string(&stamp_file) {
            if let Ok(prev) = content.trim().parse::<f64>() {
                if now - prev <= 0.35 {
                    is_double_click = true;
                }
            }
        }
    }

    if is_double_click {
        let _ = fs::remove_file(&stamp_file);
        bulb::command_toggle(target_ip)?;
        Ok(())
    } else {
        let _ = fs::write(&stamp_file, format!("{now}"));
        thread::sleep(Duration::from_millis(350));
        if stamp_file.exists() {
            if let Ok(content) = fs::read_to_string(&stamp_file) {
                if let Ok(cur) = content.trim().parse::<f64>() {
                    if (cur - now).abs() < 0.001 {
                        let _ = fs::remove_file(&stamp_file);
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
            "-h" | "--help" if positional.is_empty() => {
                print_help();
                return;
            }
            "-v" | "--version" if positional.is_empty() => {
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
        "toggle" => bulb::command_toggle(&target_ip).map(|_| ()),
        "status" => bulb::command_status(&target_ip),
        "color" => {
            if positional.len() > 1 {
                bulb::command_color(&target_ip, &positional[1]).map(|_| ())
            } else {
                eprintln!(
                    "wizctl: color command requires a value (e.g. red, #ff5500, or 255,128,0)"
                );
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
        "palette" => run_palette(&target_ip, &positional[1..]),
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

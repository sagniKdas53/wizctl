#[cfg(target_os = "linux")]
#[test]
fn explicit_binary_wins_over_path_binary() {
    use std::{env, fs, process::Command};
    let root = env::temp_dir().join(format!("wizctl-installer-{}", std::process::id()));
    let selected = root.join("selected/wizctl");
    let other = root.join("path/wizctl");
    fs::create_dir_all(selected.parent().unwrap()).unwrap();
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&selected, "selected binary").unwrap();
    fs::write(&other, "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&other, fs::Permissions::from_mode(0o755)).unwrap();
    let config = root.join("config");
    let panel = config.join("xfce4/panel/launcher-1");
    fs::create_dir_all(&panel).unwrap();
    fs::write(
        panel.join("wizctl.desktop"),
        "[Desktop Entry]\nExec=wizctl widget\n",
    )
    .unwrap();
    let output = Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/setup_panel_widget.sh"
        ))
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", &config)
        .env("WIZCTL_BIN_PATH", &selected)
        .env("WIZCTL_SKIP_PANEL_RELOAD", "1")
        .env(
            "PATH",
            format!(
                "{}:{}",
                other.parent().unwrap().display(),
                env::var("PATH").unwrap()
            ),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let desktop =
        fs::read_to_string(root.join("home/.local/share/applications/wizctl.desktop")).unwrap();
    assert!(desktop.contains(&format!("Exec={} gui", selected.display())));
    let launcher = fs::read_to_string(panel.join("wizctl.desktop")).unwrap();
    assert!(launcher.contains(&format!("Exec={} widget --click", selected.display())));
    assert_eq!(fs::read_to_string(&selected).unwrap(), "selected binary");
    fs::remove_dir_all(root).unwrap();
}

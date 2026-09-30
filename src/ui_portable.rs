//! Desktop entry point on platforms without the XFCE/X11 panel integration.

use std::path::PathBuf;

pub fn runtime_dir() -> PathBuf {
    let path = crate::state::get_state_file_path();
    let dir = path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("runtime");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn run_widget(target_ip: Option<String>) -> Result<(), eframe::Error> {
    crate::studio::run_studio(target_ip)
}

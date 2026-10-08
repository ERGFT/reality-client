#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> Result<(), slint::PlatformError> {
    reality_client_rs::run_ui()
}

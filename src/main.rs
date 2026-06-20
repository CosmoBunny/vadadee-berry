#![cfg_attr(
    all(not(debug_assertions), not(target_os = "android")),
    windows_subsystem = "windows"
)]

fn main() -> eframe::Result<()> {
    vadadee_berry::run_desktop()
}
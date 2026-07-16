#![cfg_attr(
    all(not(debug_assertions), not(target_os = "android")),
    windows_subsystem = "windows"
)]

#[cfg(not(target_os = "android"))]
fn main() -> eframe::Result<()> {
    vadadee_berry::run_desktop()
}

#[cfg(target_os = "android")]
fn main() {
    // Android uses `android_main` in the cdylib; this bin is host-only.
    eprintln!("vadadee-berry: desktop binary — build the Android library target instead");
    std::process::exit(1);
}
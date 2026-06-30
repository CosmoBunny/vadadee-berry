#![warn(clippy::all, rust_2018_idioms)]

pub mod animation;
pub mod blend;
pub mod app;
pub mod canvas;
pub mod document;
pub mod fonts;
pub mod gradient_ui;
pub mod history;
pub mod icons;
pub mod io;
pub mod render;
pub mod text_glyph;
pub mod theme;
pub mod tools;
pub mod ui;
pub mod video_decode;
pub mod export_worker;
pub mod audio_extract;

use app::VadadeeBerryApp;

fn native_options() -> eframe::NativeOptions {
    #[cfg(target_os = "android")]
    {
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Vadadee Berry")
                .with_fullscreen(true),
            multisampling: 4,
            ..Default::default()
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1400.0, 900.0])
                .with_min_inner_size([800.0, 500.0])
                .with_title("Vadadee Berry — vector editor"),
            multisampling: 4,
            ..Default::default()
        }
    }
}

fn init_logging() {
    #[cfg(target_os = "android")]
    {
        android_logger::init_once(
            android_logger::Config::default().with_max_level(log::LevelFilter::Info),
        );
    }
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    {
        let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .filter_module("egui_winit::clipboard", log::LevelFilter::Off)
            .filter_module("symphonia_format_isomp4", log::LevelFilter::Off)
            .filter_module("symphonia_bundle", log::LevelFilter::Off)
            .try_init();
    }
}

/// Desktop entry (Linux, Windows, macOS).
pub fn run_desktop() -> eframe::Result<()> {
    init_logging();
    eframe::run_native(
        "vadadee-berry",
        native_options(),
        Box::new(|cc| Ok(Box::new(VadadeeBerryApp::new(cc)))),
    )
}

#[cfg(target_os = "android")]
pub static ANDROID_APP: std::sync::OnceLock<winit::platform::android::activity::AndroidApp> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    init_logging();
    ANDROID_APP.set(app.clone()).ok();
    let mut options = native_options();
    options.android_app = Some(app);
    if let Err(err) = eframe::run_native(
        "vadadee-berry",
        options,
        Box::new(|cc| Ok(Box::new(VadadeeBerryApp::new(cc)))),
    ) {
        log::error!("eframe exited with error: {err}");
    }
}
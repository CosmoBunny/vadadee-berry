#![warn(clippy::all, rust_2018_idioms)]

pub mod av_ui;
pub mod node_editor_ui;
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
pub mod layer_cache;
pub mod perf;
pub mod shading;
pub mod spatial_index;
pub mod left_dock;
pub mod render;
pub mod text_glyph;
pub mod theme;
pub mod tools;
pub mod raster;
pub mod path_physics;
pub mod ui;
pub mod video_decode;
pub mod export_worker;
pub mod export_audio;
pub mod recorder;
pub mod audio_extract;
#[cfg(not(target_os = "android"))]
pub mod screen_capture;
pub mod collab;
pub mod sys_stats;
#[cfg(not(target_os = "android"))]
pub mod mcp;

use app::VadadeeBerryApp;

/// Must match `eframe::NativeOptions::multisampling` — shading WGSL pipelines use the same MSAA count as egui.
pub const VIEWPORT_MSAA_SAMPLES: u32 = 4;

fn native_options() -> eframe::NativeOptions {
    #[cfg(target_os = "android")]
    {
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Vadadee Berry")
                .with_fullscreen(true),
            multisampling: VIEWPORT_MSAA_SAMPLES as u16,
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
            multisampling: VIEWPORT_MSAA_SAMPLES as u16,
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
            // probe reach EOF spam on tiny/non-audio files (e.g. still images on Video tracks)
            .filter_module("symphonia_core", log::LevelFilter::Off)
            .filter_module("symphonia_core::probe", log::LevelFilter::Off)
            .filter_module("symphonia", log::LevelFilter::Off)
            // usvg spams WARN for every SVG text element using a font not in its DB;
            // our canvas handles font loading separately so this is always safe to mute.
            .filter_module("usvg::parser::text", log::LevelFilter::Off)
            // Portal ScreenCast / zbus Request noise during capture (Linux).
            // zbus INFO/WARN about proxy cache + GetAll is noise, not a real failure.
            .filter_module("zbus", log::LevelFilter::Error)
            .filter_module("zbus::proxy", log::LevelFilter::Error)
            .filter_module("zbus::connection", log::LevelFilter::Error)
            .filter_module("ashpd", log::LevelFilter::Warn)
            // rodio underrun spam while UI is busy
            .filter_module("rodio", log::LevelFilter::Error)
            .filter_module("cpal", log::LevelFilter::Error)
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
#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod animation;
mod app;
mod canvas;
mod document;
mod fonts;
mod gradient_ui;
mod history;
mod icons;
mod io;
mod render;
mod text_glyph;
mod theme;
mod tools;
mod ui;

use app::VadadeeBerryApp;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .filter_module("egui_winit::clipboard", log::LevelFilter::Off)
        .init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("Vadadee Berry — vector editor"),
        multisampling: 4,
        ..Default::default()
    };

    eframe::run_native(
        "vadadee-berry",
        native_options,
        Box::new(|cc| Ok(Box::new(VadadeeBerryApp::new(cc)))),
    )
}
//! Screen dimmer overlay — main entry point.
//!
//! Creates the window, configures the event loop, and connects egui with winit
//! via eframe. The event loop runs until the user closes the window.

mod app;
mod settings;
mod storage;

use app::DimmerApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    // NativeOptions configures the window before the event loop starts.
    // ViewportBuilder uses the builder pattern for window properties.
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_transparent(true)      // Required for overlay: see-through background
            .with_decorations(false)    // Borderless: no title bar or frame
            .with_always_on_top()      // Overlay stays above other windows
            .with_fullscreen(true),     // Cover entire screen
        ..Default::default()
    };

    eframe::run_native(
        "Screen Dimmer",
        native_options,
        Box::new(|_cc| Ok(Box::new(DimmerApp::new()))),
    )
}

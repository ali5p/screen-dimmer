//! App module: UI, opacity logic, and keyboard input handling.
//!
//! The App::update method receives &mut self, allowing direct mutation of opacity
//! when handling keyboard input (ownership: we own the state, borrow mutably each frame).

use crate::settings::UsageData;
use crate::storage;
use chrono::Timelike;
use eframe::egui;
use std::path::PathBuf;

/// Opacity step when pressing Up/Down arrows.
const STEP: f32 = 0.05;
/// Maximum opacity (nearly opaque black).
const MAX_OPACITY: f32 = 0.95;
/// Minimum opacity (nearly invisible).
const MIN_OPACITY: f32 = 0.05;

/// Main application state for the screen dimmer overlay.
pub struct DimmerApp {
    /// Current overlay opacity (0.0 = invisible, 1.0 = fully black).
    opacity: f32,
    /// Path to usage.json for persistence.
    usage_path: PathBuf,
    /// Cached usage data; updated and saved when opacity changes.
    usage_data: UsageData,
}

impl DimmerApp {
    /// Create app, loading opacity for the current hour from usage.json if present.
    pub fn new() -> Self {
        let usage_path = PathBuf::from(storage::USAGE_FILE);
        let usage_data = storage::load(&usage_path);
        let hour = current_hour();
        let opacity = usage_data
            .get(hour)
            .map(|v| v.clamp(MIN_OPACITY, MAX_OPACITY))
            .unwrap_or(0.5);
        Self {
            opacity,
            usage_path,
            usage_data,
        }
    }

    /// Increase opacity by STEP, clamped to MAX_OPACITY.
    fn increase_opacity(&mut self) {
        self.opacity = (self.opacity + STEP).min(MAX_OPACITY);
        self.save_opacity_for_current_hour();
    }

    /// Decrease opacity by STEP, clamped to MIN_OPACITY.
    fn decrease_opacity(&mut self) {
        self.opacity = (self.opacity - STEP).max(MIN_OPACITY);
        self.save_opacity_for_current_hour();
    }

    /// Persist current opacity for the current hour to usage.json.
    fn save_opacity_for_current_hour(&mut self) {
        self.usage_data.set(current_hour(), self.opacity);
        storage::save(&self.usage_path, &self.usage_data);
    }
}

/// Current hour (0–23) in local time.
fn current_hour() -> u8 {
    chrono::Local::now().hour() as u8
}

impl eframe::App for DimmerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle keyboard input. ctx.input() borrows input state via closure;
        // key_pressed() returns true if the key was pressed this frame.
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            self.increase_opacity();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            self.decrease_opacity();
        }

        // Request repaint so opacity changes are reflected immediately.
        ctx.request_repaint();

        // Draw full-screen black overlay with current opacity.
        // The window is transparent; we paint a semi-transparent black rect
        // over the entire screen to create the dimming effect.
        let opacity = self.opacity;
        let screen_rect = ctx.screen_rect();
        let dim_color = egui::Color32::from_rgba_unmultiplied(0, 0, 0, (opacity * 255.0) as u8);
        let painter = ctx.layer_painter(egui::LayerId::background());
        painter.rect_filled(screen_rect, egui::Rounding::ZERO, dim_color);
    }

    /// Clear color: fully transparent so our black overlay blends correctly.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }
}

//! Run: `cargo run --features gamma_exp --bin gamma_test`
//!
//! Chord prefix: **Alt+S** + **↓** dimmer, **↑** brighter, **A** stop & restore.
//! Brightness 5–90%, step 5%. Quit f.lux while testing.

use std::io::{self, Write};
use std::thread::sleep;
use std::time::Duration;

use screen_dimmer::gamma::{
    install_gamma_safety_hooks, GammaController, LinearDimResult,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_A, VK_DOWN, VK_MENU, VK_S, VK_UP,
};

/// Same step/range semantics as overlay opacity (5% steps; here = brightness factor).
const BRIGHTNESS_STEP: f32 = 0.05;
const BRIGHTNESS_MIN: f32 = 0.05;
const BRIGHTNESS_MAX: f32 = 0.90;

const INITIAL_BRIGHTNESS: f32 = 0.60;

fn key_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

fn chord_base() -> bool {
    key_down(VK_MENU) && key_down(VK_S)
}

#[derive(Default)]
struct ChordEdges {
    /// Alt+S+↓ — more dim 
    dimmer: bool,
    /// Alt+S+↑ — brighter
    brighter: bool,
    /// Alt+S+A — stop
    stop: bool,
}

enum ChordAction {
    Dimmer,
    Brighter,
    Stop,
}

impl ChordEdges {
    fn poll(&mut self) -> Option<ChordAction> {
        let b = chord_base();
        let dim = b && key_down(VK_DOWN);
        let br = b && key_down(VK_UP);
        let stop = b && key_down(VK_A);

        let action = if stop && !self.stop {
            Some(ChordAction::Stop)
        } else if dim && !self.dimmer {
            Some(ChordAction::Dimmer)
        } else if br && !self.brighter {
            Some(ChordAction::Brighter)
        } else {
            None
        };

        self.dimmer = dim;
        self.brighter = br;
        self.stop = stop;
        action
    }
}

fn wait_for_enter(msg: &str) {
    eprintln!("{msg}");
    let _ = io::stderr().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}

fn fmt_path(p: LinearDimResult) -> &'static str {
    match p {
        LinearDimResult::ScaledCaptured => "scaled",
        LinearDimResult::SyntheticLinear => "linear fallback",
        LinearDimResult::Failed => "failed",
    }
}

fn main() {
    println!("screen-dimmer gamma_test — experimental gamma dim");
    println!("Chords: Alt+S+↓ dimmer  Alt+S+↑ brighter  Alt+S+A stop");
    println!("Brightness {BRIGHTNESS_MIN:.0}%–{BRIGHTNESS_MAX:.0}% in {BRIGHTNESS_STEP:.0}% steps (relative to captured ramp).");
    println!("Tip: exit f.lux. If HDR is on, turn it off.\n");

    let gamma = match GammaController::new_primary() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error: {e}");
            wait_for_enter("Press Enter to close.");
            std::process::exit(1);
        }
    };

    install_gamma_safety_hooks(gamma.restore_snapshot());

    let mut factor = INITIAL_BRIGHTNESS;
    let mut edges = ChordEdges::default();

    unsafe {
        let path = gamma.apply_linear_dim(factor);
        println!(
            "Start {:.0}% brightness — {} ({})",
            factor * 100.0,
            fmt_path(path),
            path
        );
        if path == LinearDimResult::Failed {
            eprintln!("Initial ramp failed; chord adjustments may not change the screen.");
        }
        io::stdout().flush().ok();

        loop {
            if let Some(act) = edges.poll() {
                match act {
                    ChordAction::Stop => break,
                    ChordAction::Dimmer => {
                        factor = (factor - BRIGHTNESS_STEP).max(BRIGHTNESS_MIN);
                    }
                    ChordAction::Brighter => {
                        factor = (factor + BRIGHTNESS_STEP).min(BRIGHTNESS_MAX);
                    }
                }
                let path = gamma.apply_linear_dim(factor);
                println!(
                    "{:.0}% — {} ({})",
                    factor * 100.0,
                    fmt_path(path),
                    path
                );
                io::stdout().flush().ok();
            }
            sleep(Duration::from_millis(16));
        }

        gamma.restore();
    }

    println!("Restored. Done.");
    wait_for_enter("Press Enter to close.");
}

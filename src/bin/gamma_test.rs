//! Run from a terminal: `cargo run --features gamma_exp --bin gamma_test`
//!
//! Uses **primary display only** (`GammaController::new_primary`). Quit f.lux (and similar) while testing.

use std::io::{self, Write};
use std::thread::sleep;
use std::time::Duration;

use screen_dimmer::gamma::{
    install_gamma_safety_hooks, GammaController, LinearDimResult,
};

fn wait_for_enter(msg: &str) {
    eprintln!("{msg}");
    let _ = io::stderr().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}

fn main() {
    println!("screen-dimmer gamma_test — experimental **gamma ramp** dim");
    println!("The whole screen should look obviously darker for ~10 s, then normalize.");
    println!("Tip: exit f.lux. If HDR is on, turn it off — legacy gamma often does not apply.\n");

    let gamma = match GammaController::new_primary() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error: {e}");
            wait_for_enter("Press Enter to close.");
            std::process::exit(1);
        }
    };

    install_gamma_safety_hooks(gamma.restore_snapshot());

    unsafe {
        // Scale the *captured* ramp first; 0.55–0.65 is usually a clear dim.
        match gamma.apply_linear_dim(0.6) {
            LinearDimResult::ScaledCaptured => {
                println!("Applied: scaled captured ramp (~60%). Waiting 10 seconds...");
            }
            LinearDimResult::SyntheticLinear => {
                println!("Applied: synthetic linear fallback (scale refused). Waiting 10 seconds...");
            }
            LinearDimResult::Failed => {
                eprintln!("Warning: both scaled and linear Set failed — no dimming.");
            }
        }
        io::stdout().flush().ok();
        sleep(Duration::from_secs(10));
        gamma.restore();
    }

    println!("Restored. Done.");
    wait_for_enter("Press Enter to close.");
}

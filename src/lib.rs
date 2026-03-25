//! Library API for the `screen-dimmer` crate.
//!
//! Enable **`gamma_exp`** for experimental Windows gamma ramp helpers (multi-monitor, panic/Ctrl+C restore).

#[cfg(all(feature = "gamma_exp", not(windows)))]
compile_error!("The `gamma_exp` feature requires Windows.");

#[cfg(all(windows, feature = "gamma_exp"))]
pub mod gamma;

//! Screen dimmer: Windows layered overlay (`UpdateLayeredWindow` + premultiplied BGRA).
//! **Release**: no console; fatal errors use a message box. **Debug**: console + `eprintln!`.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_layered;

mod settings;
mod storage;

#[cfg(all(windows, not(debug_assertions)))]
fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(all(windows, not(debug_assertions)))]
fn show_error_dialog(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_OK,
    };
    let text = wide_z(message);
    let caption = wide_z("Screen Dimmer");
    unsafe {
        MessageBoxW(0, text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

#[cfg(windows)]
fn main() {
    if let Err(e) = windows_layered::run() {
        let msg = format!("screen-dimmer: {e}");
        #[cfg(debug_assertions)]
        eprintln!("{msg}");
        #[cfg(not(debug_assertions))]
        show_error_dialog(&msg);
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("screen-dimmer requires Windows (layered window overlay).");
    std::process::exit(1);
}

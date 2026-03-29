//! Screen dimmer: Windows layered overlay (`UpdateLayeredWindow` + premultiplied BGRA).
//! No console window when run as an `.exe`. Errors use a message box.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_layered;

mod settings;
mod storage;

#[cfg(windows)]
fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
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
        show_error_dialog(&msg);
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("screen-dimmer requires Windows (layered window overlay).");
    std::process::exit(1);
}

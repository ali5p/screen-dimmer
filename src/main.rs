//! Screen dimmer: Windows layered overlay (`UpdateLayeredWindow` + premultiplied BGRA).

#[cfg(windows)]
mod windows_layered;

mod settings;
mod storage;

#[cfg(windows)]
fn main() {
    if let Err(e) = windows_layered::run() {
        eprintln!("screen-dimmer: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("screen-dimmer requires Windows (layered window overlay).");
    std::process::exit(1);
}

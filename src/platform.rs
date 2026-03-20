//! Windows-specific platform code for click-through overlay.
//!
//! Uses raw_window_handle to get the Win32 HWND, then modifies extended
//! window styles (WS_EX_TRANSPARENT) so mouse events pass through.
//! Unsafe: Win32 APIs require valid window handles; we assume eframe provides one.

#[cfg(windows)]
pub use windows_impl::set_click_through;

#[cfg(not(windows))]
pub fn set_click_through(_handle: raw_window_handle::RawWindowHandle, _enabled: bool) {
    // No-op on non-Windows; click-through is Windows-only per spec.
}

#[cfg(windows)]
mod windows_impl {
    use raw_window_handle::{RawWindowHandle, Win32WindowHandle};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOP,
        SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_TRANSPARENT,
    };

    /// Enable or disable click-through for the given window.
    /// When enabled, mouse events pass through the overlay to windows below.
    ///
    /// # Safety
    /// Requires a valid Win32 HWND from the current process. eframe's Frame
    /// provides this via HasWindowHandle. The handle must not be destroyed
    /// during the call.
    pub fn set_click_through(handle: RawWindowHandle, enabled: bool) {
        let hwnd = match handle {
            RawWindowHandle::Win32(Win32WindowHandle { hwnd, .. }) => hwnd.get() as HWND,
            _ => return,
        };

        // SAFETY: hwnd is from eframe's native window; we assume it is valid.
        // GetWindowLongPtrW/SetWindowLongPtrW are safe for valid HWNDs.
        unsafe {
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let new_style = if enabled {
                ex_style | WS_EX_TRANSPARENT
            } else {
                ex_style & !WS_EX_TRANSPARENT
            };
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style as isize);
            // Apply the style change; SWP_* flags avoid moving/resizing.
            SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
            );
        }
    }
}

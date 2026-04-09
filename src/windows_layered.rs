//! Single HWND layered overlay: `UpdateLayeredWindow` + premultiplied BGRA buffer.
//!
//! - `WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TRANSPARENT` — always click-through.
//! - **Alt+S+↑/↓** adjusts opacity; **Alt+S+A** quits. Chords use `GetAsyncKeyState` (edge-triggered).
//! - **Control panel**: **▼** darker, **▲** brighter, **×** quit, **−** collapse (minimize panel only).
//! - Overlay uses `WS_EX_TOOLWINDOW` so it does **not** get a taskbar button (only the control panel does).
//! - Buffer + `UpdateLayeredWindow` only when opacity changes.
//! - Window spans the **virtual screen** (all monitors) via `GetSystemMetrics`.

use crate::settings::UsageData;
use crate::storage;
use chrono::Timelike;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, TRUE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetStockObject, ReleaseDC,
    SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    DIB_RGB_COLORS, HBITMAP, HBRUSH, HDC, HGDIOBJ, RGBQUAD, WHITE_BRUSH,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_S, VK_DOWN, VK_MENU, VK_A, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetSystemMetrics,
    GetWindowLongPtrW, LoadCursorW, PeekMessageW, PostQuitMessage, RegisterClassW, SetWindowLongPtrW,
    ShowWindow, TranslateMessage, UnregisterClassW, BN_CLICKED, BS_PUSHBUTTON, CREATESTRUCTW, CS_HREDRAW,
    CS_VREDRAW, GWLP_USERDATA, HMENU, IDC_ARROW, MSG, PM_REMOVE, SM_CXSCREEN, SM_CXVIRTUALSCREEN,
    SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_MINIMIZE, SW_SHOW,
    ULW_ALPHA, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_NCDESTROY, WM_QUIT, WNDCLASSW, WS_BORDER,
    WS_CAPTION, WS_CHILD, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_POPUP,
    WS_SYSMENU, WS_VISIBLE,
};

const STEP: f32 = 0.05;
const MAX_OPACITY: f32 = 0.95;
const MIN_OPACITY: f32 = 0.05;

const IDC_BTN_DARKER: isize = 1001;
const IDC_BTN_BRIGHTER: isize = 1002;
const IDC_BTN_QUIT: isize = 1003;
const IDC_BTN_COLLAPSE: isize = 1004;

const PANEL_W: i32 = 248;
const PANEL_H: i32 = 96;

/// `(overlay HWND, control panel HWND)` for coordinated shutdown.
static APP_WINDOWS: Mutex<Option<(HWND, HWND)>> = Mutex::new(None);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Virtual desktop bounds: `(x, y, width, height)` in screen coordinates (all monitors).
fn virtual_screen_bounds() -> (i32, i32, i32, i32) {
    unsafe {
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if w > 0 && h > 0 {
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            (x, y, w, h)
        } else {
            // Single-monitor fallback
            (
                0,
                0,
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
            )
        }
    }
}

/// True if this virtual key is currently held (high bit of GetAsyncKeyState).
fn key_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

/// Alt+S held together — prefix for dimmer-only shortcuts.
fn dimmer_chord_down() -> bool {
    key_down(VK_MENU) && key_down(VK_S)
}

/// Rising edge: combo was false last frame, true this frame.
#[derive(Default)]
struct ChordEdges {
    up: bool,
    down: bool,
    quit: bool,
}

impl ChordEdges {
    fn poll(&mut self, hwnd: HWND) {
        let base = dimmer_chord_down();
        let up = base && key_down(VK_DOWN);
        let down = base && key_down(VK_UP);
        let quit_key = base && key_down(VK_A);

        let Some(st) = state_mut(hwnd) else {
            self.up = up;
            self.down = down;
            self.quit = quit_key;
            return;
        };

        unsafe {
            let state = &mut *st;
            if up && !self.up {
                let prev = state.alpha_byte;
                state.set_opacity_f32(state.opacity_f32() + STEP);
                if state.alpha_byte != prev {
                    state.fill_bits();
                    let _ = state.update_layered_window(hwnd);
                    state.save_usage();
                }
            }
            if down && !self.down {
                let prev = state.alpha_byte;
                state.set_opacity_f32(state.opacity_f32() - STEP);
                if state.alpha_byte != prev {
                    state.fill_bits();
                    let _ = state.update_layered_window(hwnd);
                    state.save_usage();
                }
            }
            if quit_key && !self.quit {
                quit_app_shared();
            }
        }

        self.up = up;
        self.down = down;
        self.quit = quit_key;
    }
}

struct OverlayState {
    width: i32,
    height: i32,
    alpha_byte: u8,
    usage_path: PathBuf,
    usage_data: UsageData,
    hdc_mem: HDC,
    hbitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut u8,
    bits_len: usize,
}

impl OverlayState {
    fn opacity_f32(&self) -> f32 {
        self.alpha_byte as f32 / 255.0
    }

    fn set_opacity_f32(&mut self, o: f32) {
        let o = o.clamp(MIN_OPACITY, MAX_OPACITY);
        self.alpha_byte = (o * 255.0).round() as u8;
    }

    /// Premultiplied BGRA: black → premul RGB = 0; A = opacity.
    fn fill_bits(&mut self) {
        let a = self.alpha_byte;
        let slice = unsafe { std::slice::from_raw_parts_mut(self.bits, self.bits_len) };
        for chunk in slice.chunks_exact_mut(4) {
            chunk[0] = 0;
            chunk[1] = 0;
            chunk[2] = 0;
            chunk[3] = a;
        }
    }

    unsafe fn update_layered_window(&self, hwnd: HWND) -> bool {
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let src = POINT { x: 0, y: 0 };
        let size = SIZE {
            cx: self.width,
            cy: self.height,
        };
        windows_sys::Win32::UI::WindowsAndMessaging::UpdateLayeredWindow(
            hwnd,
            0,
            std::ptr::null(),
            &size,
            self.hdc_mem,
            &src,
            0 as COLORREF,
            &blend,
            ULW_ALPHA,
        ) == TRUE
    }

    fn save_usage(&mut self) {
        let hour = chrono::Local::now().hour() as u8;
        self.usage_data.set(hour, self.opacity_f32());
        storage::save(&self.usage_path, &self.usage_data);
    }

    fn new(hwnd: HWND, width: i32, height: i32) -> Result<Self, String> {
        let usage_path = PathBuf::from(storage::USAGE_FILE);
        let usage_data = storage::load(&usage_path);
        let hour = chrono::Local::now().hour() as u8;
        let opacity = usage_data
            .get(hour)
            .map(|v| v.clamp(MIN_OPACITY, MAX_OPACITY))
            .unwrap_or(0.5);
        let alpha_byte = (opacity * 255.0).round() as u8;

        if width <= 0 || height <= 0 {
            return Err("invalid overlay size (virtual screen metrics)".into());
        }
        let bits_len = (width * height * 4) as usize;

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbReserved: 0,
            }],
        };

        let hdc_screen = unsafe { GetDC(hwnd) };
        if hdc_screen == 0 {
            return Err("GetDC failed".into());
        }
        let mut bits_raw: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbitmap = unsafe {
            CreateDIBSection(
                hdc_screen,
                &bmi,
                DIB_RGB_COLORS,
                &mut bits_raw,
                0,
                0,
            )
        };
        unsafe { ReleaseDC(hwnd, hdc_screen) };
        if hbitmap == 0 || bits_raw.is_null() {
            return Err("CreateDIBSection failed".into());
        }

        let hdc_mem = unsafe { CreateCompatibleDC(0) };
        if hdc_mem == 0 {
            unsafe {
                DeleteObject(hbitmap as _);
            }
            return Err("CreateCompatibleDC failed".into());
        }
        let old_bitmap = unsafe { SelectObject(hdc_mem, hbitmap as HGDIOBJ) };

        Ok(Self {
            width,
            height,
            alpha_byte,
            usage_path,
            usage_data,
            hdc_mem,
            hbitmap,
            old_bitmap,
            bits: bits_raw as *mut u8,
            bits_len,
        })
    }

    unsafe fn destroy_gdi(&mut self) {
        if self.hdc_mem != 0 {
            if self.old_bitmap != 0 {
                SelectObject(self.hdc_mem, self.old_bitmap);
            }
            DeleteDC(self.hdc_mem);
            self.hdc_mem = 0;
        }
        if self.hbitmap != 0 {
            DeleteObject(self.hbitmap as _);
            self.hbitmap = 0;
        }
        self.bits = std::ptr::null_mut();
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        WM_NCDESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayState;
            if !ptr.is_null() {
                let mut state = Box::from_raw(ptr);
                state.destroy_gdi();
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn state_mut(hwnd: HWND) -> Option<*mut OverlayState> {
    let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut OverlayState;
    if p.is_null() {
        None
    } else {
        Some(p)
    }
}

unsafe fn adjust_opacity_delta(overlay_hwnd: HWND, delta: f32) {
    let Some(ptr) = state_mut(overlay_hwnd) else {
        return;
    };
    let state = &mut *ptr;
    let prev = state.alpha_byte;
    state.set_opacity_f32(state.opacity_f32() + delta);
    if state.alpha_byte != prev {
        state.fill_bits();
        let _ = state.update_layered_window(overlay_hwnd);
        state.save_usage();
    }
}

unsafe fn quit_app_shared() {
    let pair = APP_WINDOWS.lock().unwrap().take();
    if let Some((overlay, panel)) = pair {
        if overlay != 0 {
            DestroyWindow(overlay);
        }
        if panel != 0 {
            DestroyWindow(panel);
        }
    }
    PostQuitMessage(0);
}

unsafe extern "system" fn panel_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = lparam as *const CREATESTRUCTW;
            let overlay = (*cs).lpCreateParams as HWND;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, overlay as isize);
            let inst = (*cs).hInstance;
            let btn = wide("Button\0");
            let darker = wide("▼\0");
            let brighter = wide("▲\0");
            let quit_lbl = wide("×\0");
            let collapse = wide("−\0");
            let style = WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32;
            let bw = 52i32;
            let gap = 8i32;
            let mut x = 8i32;
            let y = 10i32;
            let h = 28i32;
            CreateWindowExW(
                0,
                btn.as_ptr(),
                darker.as_ptr(),
                style,
                x,
                y,
                bw,
                h,
                hwnd,
                IDC_BTN_DARKER as HMENU,
                inst,
                std::ptr::null_mut(),
            );
            x += bw + gap;
            CreateWindowExW(
                0,
                btn.as_ptr(),
                brighter.as_ptr(),
                style,
                x,
                y,
                bw,
                h,
                hwnd,
                IDC_BTN_BRIGHTER as HMENU,
                inst,
                std::ptr::null_mut(),
            );
            x += bw + gap;
            CreateWindowExW(
                0,
                btn.as_ptr(),
                quit_lbl.as_ptr(),
                style,
                x,
                y,
                bw,
                h,
                hwnd,
                IDC_BTN_QUIT as HMENU,
                inst,
                std::ptr::null_mut(),
            );
            x += bw + gap;
            CreateWindowExW(
                0,
                btn.as_ptr(),
                collapse.as_ptr(),
                style,
                x,
                y,
                bw,
                h,
                hwnd,
                IDC_BTN_COLLAPSE as HMENU,
                inst,
                std::ptr::null_mut(),
            );
            0
        }
        WM_COMMAND => {
            let id = (wparam as u32) & 0xffff;
            let code = ((wparam as u32) >> 16) & 0xffff;
            if code == BN_CLICKED {
                let overlay = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as HWND;
                match id as isize {
                    IDC_BTN_DARKER if overlay != 0 => adjust_opacity_delta(overlay, STEP),
                    IDC_BTN_BRIGHTER if overlay != 0 => adjust_opacity_delta(overlay, -STEP),
                    IDC_BTN_QUIT => quit_app_shared(),
                    IDC_BTN_COLLAPSE => {
                        ShowWindow(hwnd, SW_MINIMIZE);
                    }
                    _ => {}
                }
            }
            0
        }
        WM_CLOSE => {
            quit_app_shared();
            0
        }
        WM_DESTROY => DefWindowProcW(hwnd, msg, wparam, lparam),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn run() -> Result<(), String> {
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    if instance == 0 {
        return Err("GetModuleHandleW failed".into());
    }

    let class_name = wide("ScreenDimmerLayeredOverlay\0");
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: 0,
        hCursor: unsafe { LoadCursorW(0, IDC_ARROW) },
        hbrBackground: 0,
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
    };

    if unsafe { RegisterClassW(&wc) } == 0 {
        return Err("RegisterClassW failed".into());
    }

    let (vx, vy, vw, vh) = virtual_screen_bounds();

    let title = wide("Screen Dimmer (layered)\0");
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            vx,
            vy,
            vw,
            vh,
            0,
            0,
            instance,
            std::ptr::null_mut(),
        )
    };
    if hwnd == 0 {
        unsafe {
            UnregisterClassW(class_name.as_ptr(), instance);
        }
        return Err("CreateWindowExW failed".into());
    }

    let mut state = OverlayState::new(hwnd, vw, vh)?;
    state.fill_bits();
    unsafe {
        if !state.update_layered_window(hwnd) {
            state.destroy_gdi();
            DestroyWindow(hwnd);
            UnregisterClassW(class_name.as_ptr(), instance);
            return Err("UpdateLayeredWindow failed".to_string());
        }
        let raw = Box::into_raw(Box::new(state));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw as isize);
        ShowWindow(hwnd, SW_SHOW);
    }

    let panel_class = wide("ScreenDimmerPanel\0");
    let panel_title = wide("Screen Dimmer\0");
    let pwc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(panel_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: 0,
        hCursor: unsafe { LoadCursorW(0, IDC_ARROW) },
        hbrBackground: unsafe { GetStockObject(WHITE_BRUSH) as HBRUSH },
        lpszMenuName: std::ptr::null(),
        lpszClassName: panel_class.as_ptr(),
    };

    if unsafe { RegisterClassW(&pwc) } == 0 {
        unsafe {
            DestroyWindow(hwnd);
            UnregisterClassW(class_name.as_ptr(), instance);
        }
        return Err("RegisterClassW (panel) failed".into());
    }

    let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let sh = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let px = sw - PANEL_W - 24;
    let py = sh - PANEL_H - 72;

    let panel_hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            panel_class.as_ptr(),
            panel_title.as_ptr(),
            WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_BORDER,
            px,
            py,
            PANEL_W,
            PANEL_H,
            0,
            0,
            instance,
            hwnd as *mut std::ffi::c_void,
        )
    };
    if panel_hwnd == 0 {
        unsafe {
            DestroyWindow(hwnd);
            UnregisterClassW(panel_class.as_ptr(), instance);
            UnregisterClassW(class_name.as_ptr(), instance);
        }
        return Err("CreateWindowExW (panel) failed".into());
    }

    unsafe {
        *APP_WINDOWS.lock().unwrap() = Some((hwnd, panel_hwnd));
        ShowWindow(panel_hwnd, SW_MINIMIZE);
    }

    let mut msg = unsafe { std::mem::zeroed::<MSG>() };
    let mut chord = ChordEdges::default();

    loop {
        chord.poll(hwnd);

        unsafe {
            while PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    let _ = APP_WINDOWS.lock().unwrap().take();
                    UnregisterClassW(panel_class.as_ptr(), instance);
                    UnregisterClassW(class_name.as_ptr(), instance);
                    return Ok(());
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        std::thread::sleep(Duration::from_millis(16));
    }
}

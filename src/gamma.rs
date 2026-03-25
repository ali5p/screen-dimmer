//! Experimental per-display gamma ramps via `GetDeviceGammaRamp` / `SetDeviceGammaRamp`.
//!
//! - **[`GammaController::new_primary`]** — primary display: desktop `GetDC`, then `CreateDCW` per monitor (primary first),
//!   then `EnumDisplayDevicesW` for the primary adapter. Apply/restore reuse the same DC path.
//! - **[`GammaController::new`]** — all monitors via `EnumDisplayMonitors`.
//!
//! Use [`install_gamma_safety_hooks`] after [`GammaController::restore_snapshot`]. Task Manager / `TerminateProcess`
//! cannot be intercepted.
//!
//! **Note:** Other apps (e.g. f.lux) also change gamma ramps; quit them while testing or ramps will fight each other.

use std::collections::HashMap;
use std::ptr;
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, TRUE};
use windows_sys::Win32::Graphics::Gdi::{
    CreateDCW, DeleteDC, EnumDisplayDevicesW, EnumDisplayMonitors, GetDC, GetMonitorInfoW, HDC,
    HMONITOR, MONITORINFO, MONITORINFOEXW, ReleaseDC, DISPLAY_DEVICEW,
    DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_PRIMARY_DEVICE,
};
use windows_sys::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};
use windows_sys::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

/// 256 × 3 channels × 16-bit (Windows gamma ramp layout).
pub const RAMP_WORDS: usize = 256 * 3;
pub type GammaRamp = [u16; RAMP_WORDS];

static HOOK_STORAGE: Mutex<Option<GammaRestoreState>> = Mutex::new(None);

/// How the primary-display ramp was opened (apply/restore must use the same path).
#[derive(Clone, Copy, Debug)]
enum PrimaryBinding {
    Desktop,
    NamedDevice([u16; 32]),
}

#[derive(Clone, Debug)]
enum Source {
    Primary {
        ramp: GammaRamp,
        binding: PrimaryBinding,
    },
    PerMonitor(HashMap<isize, GammaRamp>),
}

#[derive(Clone, Debug)]
pub struct GammaRestoreState {
    source: Source,
}

impl GammaRestoreState {
    pub unsafe fn restore_all(&self) {
        match &self.source {
            Source::Primary { ramp, binding } => {
                restore_primary_ramp(ramp, *binding);
            }
            Source::PerMonitor(map) => {
                struct Ctx<'a> {
                    map: &'a HashMap<isize, GammaRamp>,
                }

                unsafe extern "system" fn enum_proc(
                    hmon: HMONITOR,
                    hdc: HDC,
                    _: *mut RECT,
                    lparam: LPARAM,
                ) -> BOOL {
                    let ctx = &*(lparam as *const Ctx<'_>);
                    if let Some(ramp) = ctx.map.get(&hmon) {
                        let _ = try_set_gamma_ramp(hmon, hdc, ramp.as_ptr());
                    }
                    TRUE
                }

                let ctx = Ctx { map };
                let _ = EnumDisplayMonitors(
                    0,
                    ptr::null(),
                    Some(enum_proc),
                    &ctx as *const _ as LPARAM,
                );
            }
        }
    }
}

/// Snapshot for hooks (multi-monitor path). Prefer [`GammaController::restore_snapshot`].
pub fn capture_restore_state() -> Result<GammaRestoreState, GammaError> {
    Ok(GammaRestoreState {
        source: Source::PerMonitor(unsafe { capture_all_monitors()? }),
    })
}

/// Registers Ctrl+C / console close (via `ctrlc`) and a panic hook that both call [`GammaRestoreState::restore_all`].
pub fn install_gamma_safety_hooks(state: GammaRestoreState) {
    if let Ok(mut g) = HOOK_STORAGE.lock() {
        *g = Some(state);
    }

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_from_hook_storage();
        previous(info);
    }));

    let _ = ctrlc::set_handler(|| {
        restore_from_hook_storage();
        std::process::exit(130);
    });
}

fn restore_from_hook_storage() {
    let taken = HOOK_STORAGE
        .try_lock()
        .ok()
        .and_then(|mut g| g.take());
    if let Some(s) = taken {
        unsafe {
            s.restore_all();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GammaError {
    /// `GetDC` / enumeration failed.
    DisplayDcFailed,
    /// Could not read a gamma ramp (driver, remote desktop, HDR-only path, or another app owns ramps).
    NoWritableRamp,
}

impl std::fmt::Display for GammaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GammaError::DisplayDcFailed => write!(f, "could not open a display device context"),
            GammaError::NoWritableRamp => write!(
                f,
                "gamma ramp not available (disable Windows HDR for the display if on, quit f.lux, update GPU driver)"
            ),
        }
    }
}

impl std::error::Error for GammaError {}

fn display_driver_w() -> *const u16 {
    const S: [u16; 8] = [
        b'D' as u16,
        b'I' as u16,
        b'S' as u16,
        b'P' as u16,
        b'L' as u16,
        b'A' as u16,
        b'Y' as u16,
        0,
    ];
    S.as_ptr()
}

unsafe fn read_ramp_on_dc(hdc: HDC) -> Option<GammaRamp> {
    let mut ramp = [0u16; RAMP_WORDS];
    if GetDeviceGammaRamp(hdc, ramp.as_mut_ptr() as *mut _) != 0 {
        Some(ramp)
    } else {
        None
    }
}

/// `CreateDCW` with `NULL` then `DISPLAY` driver — some GPUs only accept one of them.
unsafe fn try_read_ramp_create_dc(device: &[u16; 32]) -> Option<GammaRamp> {
    let drivers: [*const u16; 2] = [ptr::null(), display_driver_w()];
    for &drv in &drivers {
        let hdc = CreateDCW(drv, device.as_ptr(), ptr::null(), ptr::null());
        if hdc == 0 {
            continue;
        }
        let out = read_ramp_on_dc(hdc);
        DeleteDC(hdc);
        if out.is_some() {
            return out;
        }
    }
    None
}

unsafe fn set_ramp_primary(ramp: &GammaRamp, binding: PrimaryBinding) {
    match binding {
        PrimaryBinding::Desktop => {
            let hdc = GetDC(0 as HWND);
            if hdc != 0 {
                let _ = SetDeviceGammaRamp(hdc, ramp.as_ptr() as *const _);
                ReleaseDC(0 as HWND, hdc);
            }
        }
        PrimaryBinding::NamedDevice(device) => {
            let drivers: [*const u16; 2] = [ptr::null(), display_driver_w()];
            for &drv in &drivers {
                let hdc = CreateDCW(drv, device.as_ptr(), ptr::null(), ptr::null());
                if hdc == 0 {
                    continue;
                }
                let ok = SetDeviceGammaRamp(hdc, ramp.as_ptr() as *const _) != 0;
                DeleteDC(hdc);
                if ok {
                    break;
                }
            }
        }
    }
}

unsafe fn restore_primary_ramp(ramp: &GammaRamp, binding: PrimaryBinding) {
    set_ramp_primary(ramp, binding);
}

struct CollectMonitorsCtx {
    entries: Vec<(bool, [u16; 32])>,
}

unsafe extern "system" fn collect_monitors_enum(
    hmon: HMONITOR,
    _hdc: HDC,
    _: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let ctx = &mut *(lparam as *mut CollectMonitorsCtx);
    let mut miex: MONITORINFOEXW = std::mem::zeroed();
    miex.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmon, &mut miex.monitorInfo as *mut MONITORINFO) != 0 {
        let primary = (miex.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;
        ctx.entries.push((primary, miex.szDevice));
    }
    TRUE
}

unsafe fn try_capture_via_display_devices() -> Option<(GammaRamp, PrimaryBinding)> {
    let mut i = 0u32;
    loop {
        let mut dd: DISPLAY_DEVICEW = std::mem::zeroed();
        dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        if EnumDisplayDevicesW(ptr::null(), i, &mut dd, 0) == 0 {
            break;
        }
        let flags = dd.StateFlags;
        if (flags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP) != 0
            && (flags & DISPLAY_DEVICE_PRIMARY_DEVICE) != 0
        {
            if let Some(ramp) = try_read_ramp_create_dc(&dd.DeviceName) {
                return Some((ramp, PrimaryBinding::NamedDevice(dd.DeviceName)));
            }
        }
        i += 1;
        if i > 64 {
            break;
        }
    }
    None
}

unsafe fn capture_primary_ramp() -> Result<(GammaRamp, PrimaryBinding), GammaError> {
    let hdc = GetDC(0 as HWND);
    if hdc != 0 {
        if let Some(ramp) = read_ramp_on_dc(hdc) {
            ReleaseDC(0 as HWND, hdc);
            return Ok((ramp, PrimaryBinding::Desktop));
        }
        ReleaseDC(0 as HWND, hdc);
    }

    let mut collect = CollectMonitorsCtx {
        entries: Vec::new(),
    };
    if EnumDisplayMonitors(
        0,
        ptr::null(),
        Some(collect_monitors_enum),
        &mut collect as *mut _ as LPARAM,
    ) != 0
    {
        collect.entries.sort_by_key(|(primary, _)| !primary);
        for (_, device) in &collect.entries {
            if let Some(ramp) = try_read_ramp_create_dc(device) {
                return Ok((ramp, PrimaryBinding::NamedDevice(*device)));
            }
        }
    }

    if let Some(pair) = try_capture_via_display_devices() {
        return Ok(pair);
    }

    Err(GammaError::NoWritableRamp)
}

unsafe fn try_get_gamma_ramp(hmon: HMONITOR, hdc_enum: HDC) -> Option<GammaRamp> {
    let mut ramp = [0u16; RAMP_WORDS];
    if GetDeviceGammaRamp(hdc_enum, ramp.as_mut_ptr() as *mut _) != 0 {
        return Some(ramp);
    }
    let mut miex: MONITORINFOEXW = std::mem::zeroed();
    miex.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmon, &mut miex.monitorInfo as *mut MONITORINFO) == 0 {
        return None;
    }
    let hdc = CreateDCW(
        ptr::null(),
        miex.szDevice.as_ptr(),
        ptr::null(),
        ptr::null(),
    );
    if hdc == 0 {
        return None;
    }
    let ok = GetDeviceGammaRamp(hdc, ramp.as_mut_ptr() as *mut _) != 0;
    DeleteDC(hdc);
    if ok {
        Some(ramp)
    } else {
        None
    }
}

unsafe fn try_set_gamma_ramp(hmon: HMONITOR, hdc_enum: HDC, ramp: *const u16) -> bool {
    if SetDeviceGammaRamp(hdc_enum, ramp as *const _) != 0 {
        return true;
    }
    let mut miex: MONITORINFOEXW = std::mem::zeroed();
    miex.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmon, &mut miex.monitorInfo as *mut MONITORINFO) == 0 {
        return false;
    }
    let hdc = CreateDCW(
        ptr::null(),
        miex.szDevice.as_ptr(),
        ptr::null(),
        ptr::null(),
    );
    if hdc == 0 {
        return false;
    }
    let ok = SetDeviceGammaRamp(hdc, ramp as *const _) != 0;
    DeleteDC(hdc);
    ok
}

struct CaptureCtx {
    map: HashMap<isize, GammaRamp>,
}

unsafe extern "system" fn capture_enum(
    hmon: HMONITOR,
    hdc: HDC,
    _: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let ctx = &mut *(lparam as *mut CaptureCtx);
    if let Some(ramp) = try_get_gamma_ramp(hmon, hdc) {
        ctx.map.insert(hmon, ramp);
    }
    TRUE
}

unsafe fn capture_all_monitors() -> Result<HashMap<isize, GammaRamp>, GammaError> {
    let mut ctx = CaptureCtx {
        map: HashMap::new(),
    };
    if EnumDisplayMonitors(
        0,
        ptr::null(),
        Some(capture_enum),
        &mut ctx as *mut _ as LPARAM,
    ) == 0
    {
        return Err(GammaError::DisplayDcFailed);
    }
    if ctx.map.is_empty() {
        return Err(GammaError::NoWritableRamp);
    }
    Ok(ctx.map)
}

struct ApplyCtx {
    gamma: f32,
    scale: f32,
    red_bias: f32,
}

unsafe extern "system" fn apply_enum(
    hmon: HMONITOR,
    hdc: HDC,
    _: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let ctx = &*(lparam as *const ApplyCtx);
    let mut ramp = [0u16; RAMP_WORDS];
    build_ramp(ctx, &mut ramp);
    let _ = try_set_gamma_ramp(hmon, hdc, ramp.as_ptr());
    TRUE
}

fn build_ramp(ctx: &ApplyCtx, ramp: &mut GammaRamp) {
    for i in 0..256 {
        let x = i as f32 / 255.0;
        let base = (x.powf(ctx.gamma) * ctx.scale).clamp(0.0, 1.0);
        let r = (base * ctx.red_bias).clamp(0.0, 1.0);
        let g = base * 0.6;
        let b = base * 0.3;
        ramp[i] = (r * 65535.0) as u16;
        ramp[256 + i] = (g * 65535.0) as u16;
        ramp[512 + i] = (b * 65535.0) as u16;
    }
}

pub struct GammaController {
    source: Source,
}

impl GammaController {
    /// All monitors (enumerate + per-monitor DC fallback).
    pub fn new() -> Result<Self, GammaError> {
        let map = unsafe { capture_all_monitors()? };
        Ok(Self {
            source: Source::PerMonitor(map),
        })
    }

    /// Primary display only — tries desktop `GetDC`, then per-monitor `CreateDCW`, then `EnumDisplayDevicesW`.
    pub fn new_primary() -> Result<Self, GammaError> {
        let (ramp, binding) = unsafe { capture_primary_ramp()? };
        Ok(Self {
            source: Source::Primary { ramp, binding },
        })
    }

    pub fn restore_snapshot(&self) -> GammaRestoreState {
        GammaRestoreState {
            source: self.source.clone(),
        }
    }

    pub unsafe fn apply(&self, gamma: f32, scale: f32, red_bias: f32) {
        let ctx = ApplyCtx {
            gamma,
            scale,
            red_bias,
        };
        match &self.source {
            Source::Primary { binding, .. } => {
                let mut ramp = [0u16; RAMP_WORDS];
                build_ramp(&ctx, &mut ramp);
                set_ramp_primary(&ramp, *binding);
            }
            Source::PerMonitor(_) => {
                let _ = EnumDisplayMonitors(
                    0,
                    ptr::null(),
                    Some(apply_enum),
                    &ctx as *const _ as LPARAM,
                );
            }
        }
    }

    pub unsafe fn restore(&self) {
        let state = GammaRestoreState {
            source: self.source.clone(),
        };
        state.restore_all();
    }
}

//! Finds where the text cursor (caret) is on screen, in whatever app has focus.
//!
//! No single Windows API works everywhere, so three sources are tried in order
//! and the first usable answer wins:
//!
//! 1. The classic Win32 caret, for plain edit controls (Notepad, dialogs).
//! 2. UI Automation, for modern apps, WPF and some browsers.
//! 3. MSAA, which Chrome, Edge and Electron apps often answer only through.
//!
//! When none answer, the caller falls back to the search palette.

use std::ffi::c_void;
use std::mem::size_of;
use std::thread;
use std::time::Duration;

use ::windows::Win32::Foundation::{HWND, POINT, RECT};
use ::windows::Win32::Graphics::Gdi::ClientToScreen;
use ::windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use ::windows::Win32::System::Ole::{SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound};
use ::windows::Win32::System::Variant::VARIANT;
use ::windows::Win32::UI::Accessibility::{
    AccessibleObjectFromWindow, CUIAutomation, IAccessible, IUIAutomation,
    IUIAutomationTextPattern2, IUIAutomationTextRange, TextUnit_Character, UIA_TextPattern2Id,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetSystemMetrics,
    GetWindowThreadProcessId, OBJID_CARET, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};
use ::windows::core::Interface;

/// Which source reported the caret position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretSource {
    Win32,
    UiAutomation,
    Msaa,
}

/// The caret's rectangle in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaretPosition {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub source: CaretSource,
}

/// Where the caret is, or `None` if the focused app won't say.
pub fn caret_position() -> Option<CaretPosition> {
    // UI Automation and MSAA need COM on the calling thread. Ignore the
    // result: an already-initialised thread reports it harmlessly.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    let (focus, thread_info) = focus_window()?;
    let found = win32_caret(&thread_info)
        .map(|r| (r, CaretSource::Win32))
        .or_else(|| uia_caret().map(|r| (r, CaretSource::UiAutomation)))
        .or_else(|| msaa_caret(focus).map(|r| (r, CaretSource::Msaa)));

    found.and_then(|(rect, source)| {
        usable(&rect).then_some(CaretPosition {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
            source,
        })
    })
}

/// Rejects empty rectangles and ones that sit outside every monitor, which
/// some apps report when they have no real caret.
fn usable(rect: &RECT) -> bool {
    if rect.left == 0 && rect.top == 0 && rect.right == 0 && rect.bottom == 0 {
        return false;
    }
    if rect.right < rect.left || rect.bottom < rect.top {
        return false;
    }
    let (sx, sy, sw, sh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    rect.right > sx && rect.left < sx + sw && rect.bottom > sy && rect.top < sy + sh
}

/// The window that has keyboard focus, plus the foreground thread's GUI info.
fn focus_window() -> Option<(HWND, GUITHREADINFO)> {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0.is_null() {
            return None;
        }
        let thread_id = GetWindowThreadProcessId(foreground, None);
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(thread_id, &mut info).ok()?;
        let focus = if info.hwndFocus.0.is_null() {
            foreground
        } else {
            info.hwndFocus
        };
        Some((focus, info))
    }
}

/// Source 1: the classic caret, converted from window to screen coordinates.
fn win32_caret(info: &GUITHREADINFO) -> Option<RECT> {
    if info.hwndCaret.0.is_null() {
        return None;
    }
    let mut top_left = POINT {
        x: info.rcCaret.left,
        y: info.rcCaret.top,
    };
    let mut bottom_right = POINT {
        x: info.rcCaret.right,
        y: info.rcCaret.bottom,
    };
    unsafe {
        if !ClientToScreen(info.hwndCaret, &mut top_left).as_bool()
            || !ClientToScreen(info.hwndCaret, &mut bottom_right).as_bool()
        {
            return None;
        }
    }
    Some(RECT {
        left: top_left.x,
        top: top_left.y,
        right: bottom_right.x,
        bottom: bottom_right.y,
    })
}

/// Source 2: the focused element's text caret through UI Automation.
fn uia_caret() -> Option<RECT> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let element = automation.GetFocusedElement().ok()?;
        let pattern: IUIAutomationTextPattern2 =
            element.GetCurrentPatternAs(UIA_TextPattern2Id).ok()?;
        let mut active = Default::default();
        let range = pattern.GetCaretRange(&mut active).ok()?;
        range_bounds(&range).or_else(|| {
            // A collapsed range has no size in many apps; widening it to the
            // character it sits on gives something to measure.
            let wider = range.Clone().ok()?;
            wider.ExpandToEnclosingUnit(TextUnit_Character).ok()?;
            range_bounds(&wider)
        })
    }
}

/// The first rectangle of a UI Automation text range, as screen pixels.
fn range_bounds(range: &IUIAutomationTextRange) -> Option<RECT> {
    unsafe {
        let array = range.GetBoundingRectangles().ok()?;
        if array.is_null() {
            return None;
        }
        let lower = SafeArrayGetLBound(array, 1).ok()?;
        let upper = SafeArrayGetUBound(array, 1).ok()?;
        // Values come in fours: left, top, width, height.
        if upper - lower + 1 < 4 {
            return None;
        }
        let mut values = [0f64; 4];
        for (offset, value) in values.iter_mut().enumerate() {
            let index = lower + offset as i32;
            SafeArrayGetElement(array, &index, (value as *mut f64).cast::<c_void>()).ok()?;
        }
        Some(RECT {
            left: values[0] as i32,
            top: values[1] as i32,
            right: (values[0] + values[2]) as i32,
            bottom: (values[1] + values[3]) as i32,
        })
    }
}

/// Source 3: the caret object that Chrome, Edge and Electron apps expose
/// through MSAA.
fn msaa_caret(focus: HWND) -> Option<RECT> {
    unsafe {
        let mut raw: *mut c_void = std::ptr::null_mut();
        AccessibleObjectFromWindow(focus, OBJID_CARET.0 as u32, &IAccessible::IID, &mut raw)
            .ok()?;
        if raw.is_null() {
            return None;
        }
        let accessible = IAccessible::from_raw(raw);
        let (mut left, mut top, mut width, mut height) = (0, 0, 0, 0);
        accessible
            .accLocation(
                &mut left,
                &mut top,
                &mut width,
                &mut height,
                &VARIANT::from(0i32),
            )
            .ok()?;
        Some(RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        })
    }
}

/// Diagnostic for checking the lookup in real apps: when `BLOOM_CARET_PROBE`
/// is set, logs which source answered each time the answer changes.
pub fn start_caret_probe() {
    if std::env::var_os("BLOOM_CARET_PROBE").is_none() {
        return;
    }
    let spawned = thread::Builder::new()
        .name("bloom-caret-probe".into())
        .spawn(|| {
            let mut last: Option<Option<CaretPosition>> = None;
            loop {
                thread::sleep(Duration::from_millis(500));
                let now = caret_position();
                if last != Some(now) {
                    match now {
                        Some(p) => eprintln!(
                            "bloom: caret via {:?} at ({}, {}) {}x{}",
                            p.source,
                            p.left,
                            p.top,
                            p.right - p.left,
                            p.bottom - p.top
                        ),
                        None => eprintln!("bloom: caret not found"),
                    }
                    last = Some(now);
                }
            }
        });
    if spawned.is_err() {
        eprintln!("bloom: couldn't start the caret probe");
    }
}

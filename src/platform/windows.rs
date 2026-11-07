use crate::matcher::KeyEvent;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

static EVENTS: OnceLock<Sender<KeyEvent>> = OnceLock::new();

/// Starts the low-level keyboard and mouse hooks on their own thread.
/// Bloom only reads keys here; it never blocks or changes them.
pub fn start_hook(tx: Sender<KeyEvent>) {
    let _ = EVENTS.set(tx);
    thread::Builder::new()
        .name("bloom-hook".into())
        .spawn(|| unsafe {
            let module = GetModuleHandleW(null());
            let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
            if kb.is_null() || mouse.is_null() {
                eprintln!("bloom: could not install keyboard/mouse hooks");
            }
            let mut msg: MSG = zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn hook thread");
}

fn emit(ev: KeyEvent) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.send(ev);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code == HC_ACTION as i32 {
            let k = &*(lparam as *const KBDLLHOOKSTRUCT);
            let msg = wparam as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            // Skip synthetic input, including Bloom's own typing.
            if down
                && (k.flags & LLKHF_INJECTED) == 0
                && let Some(ev) = translate(k)
            {
                emit(ev);
            }
        }
        CallNextHookEx(null_mut(), code, wparam, lparam)
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code == HC_ACTION as i32 {
            match wparam as u32 {
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => emit(KeyEvent::Reset),
                _ => {}
            }
        }
        CallNextHookEx(null_mut(), code, wparam, lparam)
    }
}

fn key_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

unsafe fn translate(k: &KBDLLHOOKSTRUCT) -> Option<KeyEvent> {
    let vk = k.vkCode as VIRTUAL_KEY;
    match vk {
        VK_BACK => return Some(KeyEvent::Backspace),
        VK_RETURN => return Some(KeyEvent::Char('\n')),
        VK_TAB => return Some(KeyEvent::Char('\t')),
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT | VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_MENU
        | VK_LMENU | VK_RMENU | VK_CAPITAL | VK_NUMLOCK | VK_SCROLL => return None,
        VK_LWIN | VK_RWIN => return Some(KeyEvent::Reset),
        _ => {}
    }

    let ctrl = key_down(VK_CONTROL);
    let alt = key_down(VK_MENU);
    // Ctrl+key or Alt+key is a shortcut, not typing. Ctrl+Alt together is AltGr.
    if ctrl != alt {
        return Some(KeyEvent::Reset);
    }

    let mut state = [0u8; 256];
    if key_down(VK_SHIFT) {
        state[VK_SHIFT as usize] = 0x80;
    }
    if ctrl && alt {
        state[VK_CONTROL as usize] = 0x80;
        state[VK_MENU as usize] = 0x80;
    }
    unsafe {
        if GetKeyState(VK_CAPITAL as i32) & 1 != 0 {
            state[VK_CAPITAL as usize] = 0x01;
        }
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), null_mut());
        let layout = GetKeyboardLayout(thread);
        let mut buf = [0u16; 8];
        // Flag 0x4: don't change the keyboard state (keeps dead keys working in the target app).
        let n = ToUnicodeEx(
            k.vkCode,
            k.scanCode,
            state.as_ptr(),
            buf.as_mut_ptr(),
            buf.len() as i32,
            0x4,
            layout,
        );
        match n {
            n if n < 0 => None,         // dead key, wait for the next one
            0 => Some(KeyEvent::Reset), // arrows, Home, F-keys, Esc…
            n => {
                let s = String::from_utf16_lossy(&buf[..n as usize]);
                match s.chars().last() {
                    Some(c) if !c.is_control() => Some(KeyEvent::Char(c)),
                    _ => Some(KeyEvent::Reset),
                }
            }
        }
    }
}

// ---------- Typing ----------

fn key_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    for chunk in inputs.chunks(64) {
        unsafe {
            SendInput(
                chunk.len() as u32,
                chunk.as_ptr(),
                size_of::<INPUT>() as i32,
            );
        }
        if inputs.len() > 64 {
            thread::sleep(Duration::from_millis(4));
        }
    }
}

fn tap(vk: VIRTUAL_KEY, times: usize) {
    let mut v = Vec::with_capacity(times * 2);
    for _ in 0..times {
        v.push(key_input(vk, 0, 0));
        v.push(key_input(vk, 0, KEYEVENTF_KEYUP));
    }
    send(&v);
}

/// Waits (up to half a second) until Shift/Ctrl/Alt/Win are released,
/// so they don't combine with the keys Bloom sends.
pub fn wait_for_modifiers_released() {
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(500)
        && [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .any(|&k| key_down(k))
    {
        thread::sleep(Duration::from_millis(10));
    }
}

pub fn backspace(n: usize) {
    if n > 0 {
        tap(VK_BACK, n);
    }
}

pub fn cursor_left(n: usize) {
    if n > 0 {
        tap(VK_LEFT, n);
    }
}

/// Types text as Unicode keystrokes. Newlines and tabs are sent as real keys.
pub fn type_text(text: &str) {
    let mut v = Vec::with_capacity(text.len() * 2);
    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' => {
                v.push(key_input(VK_RETURN, 0, 0));
                v.push(key_input(VK_RETURN, 0, KEYEVENTF_KEYUP));
            }
            '\t' => {
                v.push(key_input(VK_TAB, 0, 0));
                v.push(key_input(VK_TAB, 0, KEYEVENTF_KEYUP));
            }
            c => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    v.push(key_input(0, *u, KEYEVENTF_UNICODE));
                    v.push(key_input(0, *u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            }
        }
    }
    send(&v);
}

/// Pastes text through the clipboard, then puts the previous clipboard text back.
pub fn paste_text(text: &str) {
    let mut clip = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(_) => return type_text(text),
    };
    let previous = clip.get_text().ok();
    let pasteable = text.replace("\r\n", "\n").replace('\n', "\r\n");
    if clip.set_text(pasteable).is_err() {
        return type_text(text);
    }
    thread::sleep(Duration::from_millis(30));
    send(&[
        key_input(VK_CONTROL, 0, 0),
        key_input(b'V' as u16, 0, 0),
        key_input(b'V' as u16, 0, KEYEVENTF_KEYUP),
        key_input(VK_CONTROL, 0, KEYEVENTF_KEYUP),
    ]);
    // Give the target app time to read the clipboard before restoring it.
    thread::sleep(Duration::from_millis(250));
    if let Some(prev) = previous {
        let _ = clip.set_text(prev);
    }
}

// ---------- Windows and focus ----------

pub fn foreground_window() -> isize {
    unsafe { GetForegroundWindow() as isize }
}

/// True when the window belongs to Bloom itself (search or settings).
pub fn is_own_window(hwnd: isize) -> bool {
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd as HWND, &mut pid);
        pid == GetCurrentProcessId()
    }
}

/// Brings a window to the front, even when Bloom isn't the active app.
pub fn focus_window(hwnd: isize) {
    unsafe {
        let target = hwnd as HWND;
        if IsWindow(target) == 0 {
            return;
        }
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, null_mut());
        let me = GetCurrentThreadId();
        let attach = fg_thread != 0 && fg_thread != me;
        if attach {
            AttachThreadInput(me, fg_thread, 1);
        }
        if IsIconic(target) != 0 {
            ShowWindow(target, SW_RESTORE);
        }
        BringWindowToTop(target);
        SetForegroundWindow(target);
        if attach {
            AttachThreadInput(me, fg_thread, 0);
        }
    }
}

/// Windows 11 rounded corners for a borderless window.
pub fn round_corners(hwnd: isize) {
    let pref = DWMWCP_ROUND;
    unsafe {
        DwmSetWindowAttribute(
            hwnd as HWND,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &pref as *const _ as *const _,
            size_of::<i32>() as u32,
        );
    }
}

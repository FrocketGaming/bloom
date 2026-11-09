use crate::matcher::{KeyEvent, Nav};
use std::borrow::Cow;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// Stamped on every key Bloom sends ("BLOM"), so the listener skips Bloom's own
/// typing while still seeing keys simulated by other tools, such as Mouse
/// Without Borders forwarding a keyboard from another PC.
const BLOOM_TAG: usize = 0x424C_4F4D;

/// Set when Windows refuses to accept Bloom's keys. In practice that means the
/// focused window belongs to an elevated process (UIPI). Cleared when read.
static INPUT_BLOCKED: AtomicBool = AtomicBool::new(false);

/// Keys you're holding (not Bloom's own), by virtual-key code, and when your
/// last key event arrived. Bloom waits for your typing to settle before it
/// sends anything; see `wait_for_typing_to_settle`.
static HELD: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
static LAST_INPUT_MS: AtomicU64 = AtomicU64::new(0);

/// Until when (in `clock` milliseconds) the suggestion list owns the arrow keys,
/// Enter, Tab and Esc. It expires by itself so that a stuck list can never
/// leave those keys dead.
static LIST_KEYS_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// Until when (in `clock` milliseconds) the characters you type are held back
/// instead of reaching the app. Bloom holds them while it erases a trigger and
/// types the expansion, so that nothing you type lands in the middle of it, and
/// types them afterwards. Like the list keys, it expires by itself so a stuck
/// hold can never leave the keyboard dead.
static HOLD_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// The characters held back so far, in the order they were typed.
static HELD_BACK: std::sync::Mutex<Vec<char>> = std::sync::Mutex::new(Vec::new());

/// How long a hold lasts without being ended. Longer than any expansion should take.
const HOLD_LEASE_MS: u64 = 5_000;

/// The most characters one hold keeps; a key held down repeats quickly.
const HOLD_LIMIT: usize = 64;

/// Starts holding back the characters you type. Pair with `finish_hold` or
/// `discard_hold`.
pub fn begin_hold() {
    if let Ok(mut held) = HELD_BACK.lock() {
        held.clear();
        HOLD_UNTIL_MS.store(clock() + HOLD_LEASE_MS, Ordering::Relaxed);
    }
}

/// Ends the hold and types what was held back, now that Bloom is done typing.
/// Characters that arrive while those are typed are held and typed too, so none
/// can slip in between. Returns what was typed.
pub fn finish_hold() -> String {
    let mut typed = String::new();
    loop {
        let held: String = {
            let Ok(mut held) = HELD_BACK.lock() else {
                return typed;
            };
            if held.is_empty() {
                HOLD_UNTIL_MS.store(0, Ordering::Relaxed);
                return typed;
            }
            held.drain(..).collect()
        };
        type_text(&held);
        typed.push_str(&held);
    }
}

/// Ends the hold and forgets what was held back.
pub fn discard_hold() {
    if let Ok(mut held) = HELD_BACK.lock() {
        held.clear();
        HOLD_UNTIL_MS.store(0, Ordering::Relaxed);
    }
}

/// Keeps a typed character back while a hold is on. True if the key was kept
/// and must not reach the app.
unsafe fn hold_back(k: &KBDLLHOOKSTRUCT) -> bool {
    if clock() >= HOLD_UNTIL_MS.load(Ordering::Relaxed) {
        return false;
    }
    let Some(KeyEvent::Char(c)) = (unsafe { translate(k) }) else {
        return false;
    };
    let Ok(mut held) = HELD_BACK.lock() else {
        return false;
    };
    // Checked again under the lock: `finish_hold` ends the hold while holding it.
    if clock() >= HOLD_UNTIL_MS.load(Ordering::Relaxed) || held.len() >= HOLD_LIMIT {
        return false;
    }
    held.push(c);
    true
}

/// Keys whose press Bloom swallowed, so their release is swallowed too.
static SWALLOWED: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];

/// How long the list keeps the keys without being refreshed.
const LIST_KEYS_LEASE_MS: u64 = 30_000;

/// While `on`, arrows, Enter, Tab and Esc go to the suggestion list instead of
/// the app being typed into. Call again on each change to renew the lease.
pub fn set_list_keys(on: bool) {
    let until = if on { clock() + LIST_KEYS_LEASE_MS } else { 0 };
    LIST_KEYS_UNTIL_MS.store(until, Ordering::Relaxed);
}

fn clock() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

// ---------- Listening (low-level hook, checked against the hook chain) ----------
//
// Bloom must only react to keys that actually reach an app on this PC. When
// Mouse Without Borders sends the keyboard to another PC, its own low-level
// hook swallows every key here by returning non-zero. Raw Input still reports
// those swallowed keys, so it can't tell the difference.
//
// A hook can: Bloom passes each key along with CallNextHookEx first, which
// returns what the rest of the chain decided. Zero means the key went on to the
// focused app; anything else means a later hook (Mouse Without Borders) blocked
// it, and Bloom ignores it. If Mouse Without Borders hooked in after Bloom, it
// runs first and Bloom never sees the swallowed key at all. Either way, Bloom
// only counts keys that reached this PC, whichever program started first.
//
// On the receiving PC, Mouse Without Borders replays the keys with SendInput.
// Those count as typing: Bloom filters out only its own keys, by tag.

/// Starts listening to the keyboard and mouse on a dedicated thread.
/// Bloom passes every event along unchanged, except the few keys the open
/// suggestion list takes (see `set_list_keys`).
pub fn start_listener(tx: Sender<KeyEvent>) {
    let _ = EVENTS.set(tx);
    thread::Builder::new()
        .name("bloom-input".into())
        .spawn(|| unsafe {
            let module = GetModuleHandleW(null());
            let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
            if kb.is_null() || mouse.is_null() {
                eprintln!("bloom: could not start listening to the keyboard");
            }
            // Low-level hooks are called on this thread, so it must keep pumping messages.
            let mut msg: MSG = zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn input thread");
}

fn emit(ev: KeyEvent) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.send(ev);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        // Hand the key to the rest of the chain first, and keep its verdict.
        let verdict = CallNextHookEx(null_mut(), code, wparam, lparam);
        if code == HC_ACTION as i32 && verdict == 0 {
            let k = &*(lparam as *const KBDLLHOOKSTRUCT);
            let msg = wparam as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            if k.dwExtraInfo != BLOOM_TAG {
                let index = (k.vkCode & 0xFF) as usize;
                // While Bloom is typing, your characters wait their turn. They
                // are not "input" for the settle check, as they never reach the app.
                if down && hold_back(k) {
                    SWALLOWED[index].store(true, Ordering::Relaxed);
                    return 1;
                }
                HELD[index].store(down, Ordering::Relaxed);
                LAST_INPUT_MS.store(clock(), Ordering::Relaxed);
                if !down && SWALLOWED[index].swap(false, Ordering::Relaxed) {
                    return 1;
                }
                if down && let Some(nav) = list_key(k.vkCode as VIRTUAL_KEY) {
                    SWALLOWED[index].store(true, Ordering::Relaxed);
                    emit(KeyEvent::Nav(nav));
                    return 1;
                }
                if down && let Some(ev) = translate(k) {
                    emit(ev);
                }
            }
        }
        verdict
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let verdict = CallNextHookEx(null_mut(), code, wparam, lparam);
        if code == HC_ACTION as i32 && verdict == 0 {
            match wparam as u32 {
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => emit(KeyEvent::Reset),
                _ => {}
            }
        }
        verdict
    }
}

/// The suggestion-list action for a key, while the list owns the keys. Keys
/// with Ctrl, Alt or Shift held stay with the app.
fn list_key(vk: VIRTUAL_KEY) -> Option<Nav> {
    if clock() >= LIST_KEYS_UNTIL_MS.load(Ordering::Relaxed) {
        return None;
    }
    if key_down(VK_CONTROL) || key_down(VK_MENU) || key_down(VK_SHIFT) {
        return None;
    }
    match vk {
        VK_UP => Some(Nav::Up),
        VK_DOWN => Some(Nav::Down),
        VK_RETURN | VK_TAB => Some(Nav::Accept),
        VK_ESCAPE => Some(Nav::Close),
        _ => None,
    }
}

/// Inside a low-level hook, the async key state is the system's view just
/// before this key, which is what the focused app will apply to it. Mirroring
/// it means Bloom reads the same character the app receives.
fn key_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

unsafe fn translate(k: &KBDLLHOOKSTRUCT) -> Option<KeyEvent> {
    let vk = k.vkCode as VIRTUAL_KEY;
    match vk {
        // A character sent directly as Unicode (on-screen keyboards, remote tools).
        VK_PACKET => {
            return match char::from_u32(k.scanCode) {
                Some(c) if !c.is_control() => Some(KeyEvent::Char(c)),
                _ => Some(KeyEvent::Reset),
            };
        }
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
                // Decoded in place: this runs inside the keyboard hook, which
                // Windows cuts off if it is slow, so it must not allocate.
                match char::decode_utf16(buf[..n as usize].iter().copied()).last() {
                    Some(Ok(c)) if !c.is_control() => Some(KeyEvent::Char(c)),
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
                dwExtraInfo: BLOOM_TAG,
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    let mut refused = false;
    for chunk in inputs.chunks(64) {
        let sent = unsafe {
            SendInput(
                chunk.len() as u32,
                chunk.as_ptr(),
                size_of::<INPUT>() as i32,
            )
        };
        refused |= sent as usize != chunk.len();
        if inputs.len() > 64 {
            thread::sleep(Duration::from_millis(4));
        }
    }
    if refused {
        INPUT_BLOCKED.store(true, Ordering::Relaxed);
    }
}

/// True if Windows refused Bloom's keys since the last call. The usual cause is
/// the focused window running as administrator, which Bloom can only report,
/// not work around.
pub fn take_input_blocked() -> bool {
    INPUT_BLOCKED.swap(false, Ordering::Relaxed)
}

/// Presses a key `times` times, one press per call with a short gap, the same
/// pacing as typed characters. Sent as one burst, some apps dropped a press:
/// `;fu` left its `;` behind when one of three backspaces went missing.
fn tap(vk: VIRTUAL_KEY, times: usize) {
    for i in 0..times {
        if i > 0 {
            thread::sleep(CHAR_GAP);
        }
        send(&[key_input(vk, 0, 0), key_input(vk, 0, KEYEVENTF_KEYUP)]);
    }
}

/// How long your keyboard must be quiet before Bloom types.
const SETTLE_MS: u64 = 60;
/// Never wait longer than this, so a stuck key can't block expansions.
const SETTLE_LIMIT: Duration = Duration::from_millis(600);

/// Waits until you've released every key and nothing new has arrived for a
/// moment, so Bloom's keys don't interleave with yours.
///
/// This matters most on a PC receiving a keyboard through Mouse Without
/// Borders: the end of your typing (the trigger's last key-up) is still being
/// replayed there when the trigger fires, and keys mixed into that stream lost
/// their Shift or Ctrl (`12:53` came out as `12;53`, Ctrl+V as `v`).
pub fn wait_for_typing_to_settle() {
    let start = Instant::now();
    while start.elapsed() < SETTLE_LIMIT {
        let mut any_held = false;
        for (vk, held) in HELD.iter().enumerate() {
            if held.load(Ordering::Relaxed) {
                // A key-up can go missing (swallowed while crossing to the
                // other PC). Trust Windows' own key state over a stale flag.
                if key_down(vk as VIRTUAL_KEY) {
                    any_held = true;
                } else {
                    held.store(false, Ordering::Relaxed);
                }
            }
        }
        let any_held = any_held
            || [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
                .iter()
                .any(|&k| key_down(k));
        let quiet = clock().saturating_sub(LAST_INPUT_MS.load(Ordering::Relaxed)) >= SETTLE_MS;
        if !any_held && quiet {
            return;
        }
        thread::sleep(Duration::from_millis(5));
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

/// Pause between typed characters, so apps and other keyboard hooks see them
/// as ordinary, separate key presses.
const CHAR_GAP: Duration = Duration::from_millis(2);

fn foreground_layout() -> HKL {
    unsafe { GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), null_mut())) }
}

/// The key, and whether Shift is needed, that types `ch` on this layout.
/// None for characters that need AltGr or have no key at all.
fn key_for(ch: char, layout: HKL) -> Option<(VIRTUAL_KEY, bool)> {
    let unit = u16::try_from(ch as u32).ok()?;
    let scan = unsafe { VkKeyScanExW(unit, layout) };
    if scan == -1 {
        return None;
    }
    let vk = (scan & 0xFF) as VIRTUAL_KEY;
    let mods = (scan >> 8) & 0xFF;
    // Bit 1 is Shift. Ctrl, Alt and the rarer layout states aren't pressed,
    // because they can trigger shortcuts in the target app.
    match mods {
        0 => Some((vk, false)),
        1 => Some((vk, true)),
        _ => None,
    }
}

/// True when every character can be typed as ordinary key presses on the
/// focused app's keyboard layout.
pub fn can_type_as_keys(text: &str) -> bool {
    let layout = foreground_layout();
    text.chars()
        .all(|c| matches!(c, '\n' | '\t' | '\r') || key_for(c, layout).is_some())
}

/// Types text the way a keyboard would: each character as its ordinary key,
/// with Shift where needed. Only characters with no key on the layout are sent
/// as Unicode keystrokes. Some keyboard hooks (Mouse Without Borders) mangle
/// Unicode keystrokes, repeating one character for the rest (`12:44 PM` came
/// out as `MMMMMMMM`), while ordinary key presses always come through intact.
pub fn type_text(text: &str) {
    let layout = foreground_layout();
    // Caps Lock flips letters typed as keys, so flip Shift back for them.
    let caps = unsafe { GetKeyState(VK_CAPITAL as i32) & 1 != 0 };
    let mut first = true;
    let mut v = Vec::with_capacity(6);
    for ch in text.chars() {
        v.clear();
        match ch {
            '\r' => continue,
            '\n' => {
                v.push(key_input(VK_RETURN, 0, 0));
                v.push(key_input(VK_RETURN, 0, KEYEVENTF_KEYUP));
            }
            '\t' => {
                v.push(key_input(VK_TAB, 0, 0));
                v.push(key_input(VK_TAB, 0, KEYEVENTF_KEYUP));
            }
            c => match key_for(c, layout) {
                Some((vk, shift)) => {
                    let shift = shift != (caps && c.is_alphabetic());
                    if shift {
                        v.push(key_input(VK_SHIFT, 0, 0));
                    }
                    v.push(key_input(vk, 0, 0));
                    v.push(key_input(vk, 0, KEYEVENTF_KEYUP));
                    if shift {
                        v.push(key_input(VK_SHIFT, 0, KEYEVENTF_KEYUP));
                    }
                }
                None => {
                    let mut units = [0u16; 2];
                    for u in c.encode_utf16(&mut units) {
                        v.push(key_input(0, *u, KEYEVENTF_UNICODE));
                        v.push(key_input(0, *u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                    }
                }
            },
        }
        if !first {
            thread::sleep(CHAR_GAP);
        }
        first = false;
        send(&v);
    }
}

/// Clipboard sync tools (Mouse Without Borders' Share clipboard, cloud
/// clipboards) open the clipboard every time it changes, and while they hold
/// it nobody else can read or write it. Retry instead of failing.
const CLIPBOARD_TRIES: u32 = 20;
const CLIPBOARD_RETRY_GAP: Duration = Duration::from_millis(25);

fn retry<T>(mut attempt: impl FnMut() -> Option<T>) -> Option<T> {
    for i in 0..CLIPBOARD_TRIES {
        if i > 0 {
            thread::sleep(CLIPBOARD_RETRY_GAP);
        }
        if let Some(v) = attempt() {
            return Some(v);
        }
    }
    None
}

/// Clipboard text compared without line-ending differences.
fn same_text(a: &str, b: &str) -> bool {
    a.chars()
        .filter(|&c| c != '\r')
        .eq(b.chars().filter(|&c| c != '\r'))
}

/// What was on the clipboard before Bloom pasted, waiting to be put back.
enum Previous {
    Text(String),
    Image(arboard::ImageData<'static>),
    Nothing,
}

/// The restore waiting to happen, tagged with the paste that scheduled it.
/// A newer paste takes it over, so your original clipboard is restored once,
/// after the last of several quick expansions.
static PENDING_RESTORE: std::sync::Mutex<Option<(u64, Previous)>> = std::sync::Mutex::new(None);
static PASTE_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Mouse Without Borders' Share clipboard ignores a clipboard change that
/// comes less than a second after the previous one. Restoring sooner would
/// leave the snippet, not your clipboard, as what the other PC receives.
const RESTORE_AFTER: Duration = Duration::from_millis(1100);

/// Pastes text through the clipboard, then puts the previous clipboard back.
pub fn paste_text(text: &str) {
    let pasteable = text.replace("\r\n", "\n").replace('\n', "\r\n");
    let Some(mut clip) = retry(|| arboard::Clipboard::new().ok()) else {
        return type_text(text);
    };

    // Restore whatever was there, not just text. Replacing a screenshot with
    // snippet text and never putting it back is the kind of loss people only
    // notice later, and images are the one non-text form worth decoding for.
    // If an earlier paste is still waiting to restore, the clipboard holds its
    // snippet; the clipboard to restore is the one that paste saved.
    let generation = PASTE_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    {
        let mut pending = PENDING_RESTORE.lock().unwrap();
        let previous = match pending.take() {
            Some((_, earlier)) => earlier,
            None => match clip.get().text() {
                Ok(t) => Previous::Text(t),
                Err(_) => clip
                    .get()
                    .image()
                    .map(Previous::Image)
                    .unwrap_or(Previous::Nothing),
            },
        };
        *pending = Some((generation, previous));
    }

    // Only press Ctrl+V once the clipboard really holds the snippet; a sync
    // tool can grab the clipboard between our write and the paste.
    let ready = retry(|| {
        clip.set_text(pasteable.as_str()).ok()?;
        let now = clip.get_text().ok()?;
        same_text(&now, &pasteable).then_some(())
    });
    if ready.is_none() {
        // Last resort, so the expansion isn't lost. Nothing to restore.
        take_restore(generation);
        return type_text(text);
    }

    thread::sleep(Duration::from_millis(30));
    send(&[
        key_input(VK_CONTROL, 0, 0),
        key_input(b'V' as u16, 0, 0),
        key_input(b'V' as u16, 0, KEYEVENTF_KEYUP),
        key_input(VK_CONTROL, 0, KEYEVENTF_KEYUP),
    ]);
    // Let the target app read the clipboard before Bloom moves on.
    thread::sleep(Duration::from_millis(150));

    // Restore later, in the background, so the next trigger isn't held up.
    thread::spawn(move || {
        thread::sleep(RESTORE_AFTER);
        let Some(previous) = take_restore(generation) else {
            return; // a newer paste took over the restore
        };
        let Some(mut clip) = retry(|| arboard::Clipboard::new().ok()) else {
            return;
        };
        // Put the old clipboard back only if it still holds the snippet. If
        // something newer is there (you copied, or a synced clipboard
        // arrived), leave it alone.
        let still_ours = retry(|| clip.get_text().ok()).is_some_and(|t| same_text(&t, &pasteable));
        if !still_ours {
            return;
        }
        match previous {
            Previous::Text(t) => {
                retry(|| clip.set_text(t.as_str()).ok());
            }
            Previous::Image(img) => {
                // Borrowed on each attempt: a screenshot can be tens of
                // megabytes, and a retry must not copy it again.
                retry(|| {
                    clip.set_image(arboard::ImageData {
                        width: img.width,
                        height: img.height,
                        bytes: Cow::Borrowed(&img.bytes),
                    })
                    .ok()
                });
            }
            // The clipboard was empty or held something Bloom can't copy
            // (files, rich formats). Leave the snippet rather than wipe it.
            Previous::Nothing => {}
        }
    });
}

/// Claims the pending restore if it still belongs to this paste.
fn take_restore(generation: u64) -> Option<Previous> {
    let mut pending = PENDING_RESTORE.lock().unwrap();
    match pending.as_ref() {
        Some((g, _)) if *g == generation => pending.take().map(|(_, p)| p),
        _ => None,
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

/// Marks a window as one that never takes focus and stays above others, so
/// the app being typed into keeps its text cursor.
pub fn make_non_activating(hwnd: isize) {
    unsafe {
        let target = hwnd as HWND;
        let style = GetWindowLongPtrW(target, GWL_EXSTYLE);
        let wanted = (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST) as isize;
        SetWindowLongPtrW(target, GWL_EXSTYLE, style | wanted);
    }
}

/// Moves, resizes and shows a window in screen pixels without activating it.
pub fn place_without_activating(hwnd: isize, x: i32, y: i32, width: i32, height: i32) {
    unsafe {
        SetWindowPos(
            hwnd as HWND,
            HWND_TOPMOST,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

/// Hides a window shown with `place_without_activating`. Asking the window
/// toolkit to hide it does nothing, because it never learned the window was shown.
pub fn hide_window(hwnd: isize) {
    unsafe {
        ShowWindow(hwnd as HWND, SW_HIDE);
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

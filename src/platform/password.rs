//! Knows whether the focused field is a password box, so Bloom can stop
//! paying attention to typing there.
//!
//! Windows' accessibility system (UI Automation) reports every focus change
//! across apps, and every field it reports says whether it's a password field.
//! That covers browsers, modern apps and classic Windows dialogs alike.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use ::windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use ::windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationFocusChangedEventHandler,
    IUIAutomationFocusChangedEventHandler_Impl,
};
use ::windows::core::{Ref, Result, implement};

static IN_PASSWORD_FIELD: AtomicBool = AtomicBool::new(false);

/// True while a password field has keyboard focus.
pub fn password_field_focused() -> bool {
    IN_PASSWORD_FIELD.load(Ordering::Relaxed)
}

fn is_password(element: &IUIAutomationElement) -> bool {
    // If Windows can't say, assume it isn't: refusing to expand anywhere the
    // answer is unclear would make Bloom stop working in odd apps.
    unsafe { element.CurrentIsPassword() }
        .map(|b| b.as_bool())
        .unwrap_or(false)
}

#[implement(IUIAutomationFocusChangedEventHandler)]
struct FocusWatcher;

impl IUIAutomationFocusChangedEventHandler_Impl for FocusWatcher_Impl {
    fn HandleFocusChangedEvent(&self, sender: Ref<IUIAutomationElement>) -> Result<()> {
        let password = sender.as_ref().is_some_and(is_password);
        IN_PASSWORD_FIELD.store(password, Ordering::Relaxed);
        Ok(())
    }
}

/// Starts watching focus changes on a background thread. If UI Automation is
/// unavailable, Bloom keeps working and simply can't detect password fields.
pub fn start_password_watch() {
    let spawned = thread::Builder::new().name("bloom-focus".into()).spawn(|| {
        if let Err(e) = watch() {
            eprintln!("bloom: can't detect password fields: {e}");
        }
    });
    if spawned.is_err() {
        eprintln!("bloom: can't detect password fields");
    }
}

fn watch() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        if let Ok(focused) = automation.GetFocusedElement() {
            IN_PASSWORD_FIELD.store(is_password(&focused), Ordering::Relaxed);
        }
        let watcher: IUIAutomationFocusChangedEventHandler = FocusWatcher.into();
        automation.AddFocusChangedEventHandler(None, &watcher)?;
        // UI Automation calls the watcher on its own threads; this thread only
        // has to keep the subscription alive for as long as Bloom runs.
        loop {
            thread::park();
        }
    }
}

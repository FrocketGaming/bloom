use crate::config::{Config, Paths};
use crate::snippets::Store;
use crate::undo::Undo;
use crate::usage::Usage;
use crate::variables::Variables;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, RwLock};

/// A trigger-started snippet waiting for its form to be filled in.
pub struct PendingForm {
    pub id: String,
    /// Characters typed right after the trigger, re-typed after the expansion.
    pub trailing: String,
    /// Exactly what Bloom deleted when the trigger fired, so cancelling the
    /// form can put it back where it was.
    pub restore: String,
}

pub struct AppState {
    pub paths: Paths,
    pub config: RwLock<Config>,
    pub config_error: RwLock<Option<String>>,
    pub store: RwLock<Store>,
    pub variables: RwLock<Variables>,
    pub paused: AtomicBool,
    /// The window that was active before the palette opened; text goes back there.
    pub prev_window: Mutex<Option<isize>>,
    pub pending_form: Mutex<Option<PendingForm>>,
    /// How to take back the last expansion, until the next key press.
    pub undo: Mutex<Option<Undo>>,
    /// How often and how recently each snippet is used.
    pub usage: Mutex<Usage>,
}

impl AppState {
    pub fn reload_snippets(&self) {
        let store = Store::load(&self.paths.snippets_dir);
        *self.store.write().unwrap() = store;
    }

    pub fn reload_variables(&self) {
        let variables = Variables::load(&self.paths.variables_file);
        *self.variables.write().unwrap() = variables;
    }
}

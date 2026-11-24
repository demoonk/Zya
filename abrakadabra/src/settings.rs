//! Settings compatibility layer: the real window is implemented in `settings_ui`.
//! Only the entry point the tray and the CLI actually use is re-exported.

pub use crate::settings_ui::open_blocking;

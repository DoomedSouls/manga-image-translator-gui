// manga-translator-gtk/src/ui/mod.rs
//
// UI module — re-exports all UI components.

mod css;
mod dialogs;
mod file_browser;
mod main_window;
mod preview;
mod settings_panel;

pub use main_window::build_main_window;

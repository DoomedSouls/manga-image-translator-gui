// manga-translator-gtk/src/lib.rs
//
// Library root — exposes public modules so that integration tests
// (under tests/) and external code can import types from this crate.
//
// The binary entry point lives in main.rs and re-uses these modules
// through `use manga_translator_gtk::…`.

pub mod config;
pub mod i18n;
pub mod ipc_bridge;
pub mod ui;

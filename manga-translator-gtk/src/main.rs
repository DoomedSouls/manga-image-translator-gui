// manga-translator-gtk/src/main.rs
//
// Manga Image Translator — GTK4 / libadwaita GUI (Rust Prototype)
//
// Build:  cargo build
// Run:    cargo run

use manga_translator_gtk::{i18n, ui};

use adw::prelude::*;
use gtk::gdk::Display;
use gtk::gio;
use gtk::glib;
use std::fs::File;
use std::io::Write;

const APP_ID: &str = "com.manga-translator.gui";

/// Custom CSS for the application (accent colors, animations, widgets)
const APP_CSS: &str = include_str!("../resources/style.css");

/// Global log file handle — shared with the Log Viewer dialog.
/// Initialized in `init_logging()`, written to on every log message.
static LOG_FILE: std::sync::Mutex<Option<File>> = std::sync::Mutex::new(None);

fn main() -> glib::ExitCode {
    // Initialize logger that writes to both stderr AND a log file.
    // The log file can be viewed from the Log Viewer dialog.
    init_logging();

    // Initialize gettext/i18n
    i18n::init();

    // Apply saved language (if any)
    {
        let config = manga_translator_gtk::config::ConfigManager::new();
        if !config.settings.ui_language.is_empty() {
            manga_translator_gtk::i18n::set_language(&config.settings.ui_language);
        }
    }

    // Create the Adwaita application
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    // Connect startup — load custom CSS
    app.connect_startup(|_app| {
        load_global_css();
    });

    // Connect activate — build and show the main window
    app.connect_activate(|app| {
        let window = ui::build_main_window(app);
        window.present();
    });

    // Handle command-line files (open with…)
    app.connect_open(|app, files, _hint| {
        // Activate first (ensures a window exists), then open files
        app.activate();
        if let Some(raw_window) = app.active_window() {
            if let Some(file) = files.first() {
                if let Some(path) = file.path() {
                    // Fire the "open-directory" action installed by build_main_window
                    let action_name = format!("win.open-directory");
                    if let Some(path_str) = path.to_str() {
                        let variant = path_str.to_variant();
                        let _ = raw_window.activate_action(&action_name, Some(&variant));
                    }
                }
            }
        }
    });

    // Run the application
    app.run()
}

/// Initialize dual logging: stderr (for terminal) + log file.
///
/// The log file is created at `~/.config/manga-translator-gtk/manga-translator.log`.
/// Each session starts fresh (old log is rotated to `.log.old`).
fn init_logging() {
    let log_path = manga_translator_gtk::config::ConfigManager::log_file_path();

    // Ensure config directory exists
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Rotate old log: .log → .log.old
    let old_path = log_path.with_extension("log.old");
    let _ = std::fs::rename(&log_path, &old_path);

    // Open log file for this session
    if let Ok(file) = File::create(&log_path) {
        *LOG_FILE.lock().unwrap() = Some(file);
    }

    // Build env_logger that tees to both stderr and the log file.
    // RUST_LOG controls the level; defaults to "info" if unset.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(|buf, record| {
            let timestamp = buf.timestamp();
            let line = format!("[{} {:5}] {}\n", timestamp, record.level(), record.args());

            // Write to log file (if available)
            if let Ok(mut guard) = LOG_FILE.lock() {
                if let Some(ref mut file) = *guard {
                    let _ = file.write_all(line.as_bytes());
                    let _ = file.flush();
                }
            }

            // Also write to stderr (for terminal / RUST_LOG)
            buf.write_all(line.as_bytes())
        })
        .init();

    log::info!("Log file: {}", log_path.display());
}

/// Load global application CSS provider
fn load_global_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(APP_CSS);

    gtk::style_context_add_provider_for_display(
        &Display::default().expect("Could not connect to a display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

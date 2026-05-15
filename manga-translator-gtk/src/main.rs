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
    // ── Windows: show panic as message box instead of vanishing console ──
    #[cfg(target_os = "windows")]
    setup_windows_panic_hook();

    // Initialize hotpath profiler (zero-cost when feature is disabled).
    // Drops + prints report when _guard goes out of scope at process exit.
    let _hotpath_guard = hotpath_init();

    // Initialize logger that writes to both stderr AND a log file.
    // The log file can be viewed from the Log Viewer dialog.
    init_logging();

    // Free the console window on Windows — we have logging to file now.
    #[cfg(target_os = "windows")]
    unsafe {
        winapi_free_console();
    }

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

// ── Windows helpers ──────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn setup_windows_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{}", info);
        eprintln!("PANIC: {}", msg);

        // Try to flush the log file so the panic is recorded
        if let Ok(mut guard) = LOG_FILE.lock() {
            if let Some(ref mut file) = *guard {
                let _ = file.write_all(format!("PANIC: {}\n", msg).as_bytes());
                let _ = file.flush();
            }
        }

        // Show a message box so the user can see what went wrong.
        // Without this, a panic just flashes a CMD window and disappears.
        unsafe {
            let caption = b"Manga Translator - Error\0";
            let full_msg = format!("{}\0", msg);
            winapi_message_box_a(full_msg.as_ptr(), caption.as_ptr());
        }
    }));
}

#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    fn FreeConsole() -> i32;
    fn MessageBoxA(hwnd: usize, text: *const u8, caption: *const u8, flags: u32) -> i32;
}

#[cfg(target_os = "windows")]
unsafe fn winapi_free_console() {
    unsafe { FreeConsole() };
}

#[cfg(target_os = "windows")]
unsafe fn winapi_message_box_a(text: *const u8, caption: *const u8) {
    unsafe { MessageBoxA(0, text, caption, 0x10) }; // MB_ICONERROR
}

// ── Logging ──────────────────────────────────────────────────────────

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

/// Initialize hotpath profiler when the `hotpath` feature is enabled.
/// Returns an RAII guard that prints the performance report on drop.
/// When the feature is disabled, this is a no-op returning unit.
#[cfg(feature = "hotpath")]
fn hotpath_init() -> hotpath::HotpathGuard {
    use hotpath::{Format, HotpathGuardBuilder, Section};

    HotpathGuardBuilder::new("main")
        .percentiles(&[50, 95, 99])
        .with_functions_limit(30)
        .with_sections(vec![
            Section::FunctionsTiming,
            Section::FunctionsAlloc,
            Section::Threads,
        ])
        .format(Format::Table)
        .build()
}

/// No-op when hotpath feature is disabled.
#[cfg(not(feature = "hotpath"))]
fn hotpath_init() {}

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

#![allow(dead_code)]
// manga-translator-gtk/src/ui/dialogs.rs
//
// Dialog builders for:
//   - Keyboard shortcuts
//   - Open directory
//   - Log viewer
//   - API keys
//   - Accent color selection (swatches + custom hex)
//   - About dialog
//
// Helper functions:
//   - check_api_key_status
//   - service_display_name
//   - load_log_content
//   - apply_accent / is_valid_hex_color
//
// All dialogs use Adw.Dialog (bottom-sheet on mobile, center on desktop)
// consistent with the Material 3 / libadwaita design language.

use adw::prelude::*;
use gtk::gdk::Display;
use gtk::gio;
use gtk::glib;
use gtk::glib::clone;
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::config::{self, AccentColor, ConfigManager};
use crate::i18n;
use crate::ipc_bridge::IpcBridge;
use crate::ui::css;
use crate::ui::main_window::{Widgets, WindowState};

// ---------------------------------------------------------------------------
// Keyboard Shortcuts Dialog
// ---------------------------------------------------------------------------

/// Show a dialog listing all keyboard shortcuts.
pub fn show_shortcuts_dialog(window: &adw::ApplicationWindow) {
    let dialog = adw::Dialog::builder()
        .title(&i18n::t("Tastenkürzel"))
        .content_width(480)
        .content_height(520)
        .build();

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    let main_box = gtk::Box::new(gtk::Orientation::Vertical, 18);
    main_box.set_margin_start(18);
    main_box.set_margin_end(18);
    main_box.set_margin_top(18);
    main_box.set_margin_bottom(18);

    // ── Helper: add a shortcut group ────────────────────────────────────
    fn add_group(parent: &gtk::Box, title: &str, shortcuts: &[(&str, &str)]) {
        let heading = gtk::Label::new(Some(title));
        heading.set_halign(gtk::Align::Start);
        heading.add_css_class("heading");
        parent.append(&heading);

        let grid = gtk::Grid::new();
        grid.set_column_spacing(24);
        grid.set_row_spacing(4);
        grid.set_margin_top(6);
        grid.set_margin_bottom(12);

        for (i, (accel, desc)) in shortcuts.iter().enumerate() {
            let key_label = gtk::Label::new(Some(accel));
            key_label.set_halign(gtk::Align::Start);
            key_label.add_css_class("monospace");
            key_label.add_css_class("caption");
            grid.attach(&key_label, 0, i as i32, 1, 1);

            let desc_label = gtk::Label::new(Some(desc));
            desc_label.set_halign(gtk::Align::Start);
            desc_label.add_css_class("caption");
            grid.attach(&desc_label, 1, i as i32, 1, 1);
        }

        parent.append(&grid);
    }

    // ── General shortcuts ───────────────────────────────────────────────
    add_group(
        &main_box,
        &i18n::t("Allgemein"),
        &[
            ("Ctrl+O", &i18n::t("Verzeichnis öffnen")),
            ("Ctrl+R / F5", &i18n::t("Aktualisieren")),
            ("Alt+←", &i18n::t("Zurück")),
            ("Ctrl+T", &i18n::t("Übersetzen")),
            ("Escape", &i18n::t("Übersetzung abbrechen")),
            ("Ctrl+K", &i18n::t("API Schlüssel")),
            ("F9", &i18n::t("Einstellungen ein-/ausblenden")),
        ],
    );

    // ── File selection shortcuts ────────────────────────────────────────
    add_group(
        &main_box,
        &i18n::t("Dateiauswahl"),
        &[
            ("Ctrl+A", &i18n::t("Alle auswählen")),
            ("Ctrl+Shift+A", &i18n::t("Auswahl aufheben")),
            ("Ctrl+L", &i18n::t("Suche fokussieren")),
        ],
    );

    // ── View shortcuts ──────────────────────────────────────────────────
    add_group(
        &main_box,
        &i18n::t("Ansicht"),
        &[
            ("Ctrl+1", &i18n::t("Rasteransicht")),
            ("Ctrl+2", &i18n::t("Listenansicht")),
        ],
    );

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scrolled.set_child(Some(&main_box));

    toolbar.set_content(Some(&scrolled));

    dialog.set_child(Some(&toolbar));
    dialog.present(Some(window));
}

// ---------------------------------------------------------------------------
// Open Directory Dialog
// ---------------------------------------------------------------------------

/// Show a folder-chooser dialog.
///
/// * `current_dir` — starting folder for the file chooser.
/// * `on_selected` — callback invoked with the chosen path.
pub fn show_open_directory_dialog(
    window: &adw::ApplicationWindow,
    current_dir: PathBuf,
    on_selected: Box<dyn Fn(&Path)>,
) {
    let dialog = gtk::FileDialog::new();
    dialog.set_title(&i18n::t("Verzeichnis öffnen"));

    if current_dir.exists() {
        dialog.set_initial_folder(Some(&gio::File::for_path(&current_dir)));
    }

    dialog.select_folder(Some(window), gio::Cancellable::NONE, move |result| {
        if let Ok(file) = result {
            if let Some(path) = file.path() {
                on_selected(&path);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// API Key Pre-Flight Check
// ---------------------------------------------------------------------------

/// Check if the currently selected translator requires an API key and
/// update the start button accordingly (red pulsing warning vs green).
///
/// Also checks if the Python backend is available at all — if virtual
/// environment and manga-image-translator paths are not configured, the
/// button shows a "Backend nicht konfiguriert" warning instead.
pub fn check_api_key_status(widgets: &Rc<RefCell<Widgets>>, state: &Rc<RefCell<WindowState>>) {
    // ── Backend availability check ──────────────────────────────────────
    // If no virtual env / manga-translator paths are configured AND
    // auto-discovery can't find the backend, show a dedicated warning.
    {
        let state_ref = state.borrow();
        let cfg = state_ref.config.borrow();
        let venv_empty = cfg.settings.virtual_env_path.is_empty();
        let mt_empty = cfg.settings.manga_translator_path.is_empty();
        drop(cfg);
        drop(state_ref);

        if venv_empty && mt_empty {
            // No paths configured — check if auto-discovery would find anything
            let auto_found = std::env::current_exe()
                .ok()
                .and_then(|exe| {
                    let exe_dir = exe.parent()?;
                    let project_root = exe_dir.join("..");
                    if project_root.join("manga_translator").is_dir() {
                        return Some(true);
                    }
                    let local_root = exe_dir.join("..").join("..").join("..");
                    if local_root.join("manga_translator").is_dir() {
                        return Some(true);
                    }
                    None
                })
                .unwrap_or(false)
                || std::env::current_dir()
                    .ok()
                    .map(|cwd| {
                        cwd.join("manga_translator").is_dir()
                            || cwd.join("..").join("manga_translator").is_dir()
                    })
                    .unwrap_or(false);

            if !auto_found {
                let w = widgets.borrow();
                w.btn_start.remove_css_class("suggested-action");
                w.btn_start.add_css_class("warning");
                w.btn_start_label
                    .set_label(&i18n::t("Backend nicht konfiguriert"));
                w.btn_start.set_tooltip_text(Some(&i18n::t(
                    "Öffne Menü → Virtuelle Umgebung… um die Pfade zu konfigurieren",
                )));
                return;
            }
        }
    }

    // ── API key check (existing logic) ──────────────────────────────────
    // Clear any backend tooltip
    widgets.borrow().btn_start.set_tooltip_text(None::<&str>);

    let idx = widgets.borrow().settings_panel.translator_index() as usize;
    let required = config::api_required_services();
    let service = match required.get(&idx) {
        Some(s) => *s,
        None => {
            // No API key required — restore normal button state
            let w = widgets.borrow();
            w.btn_start.remove_css_class("warning");
            w.btn_start.add_css_class("suggested-action");
            w.btn_start_label.set_label(&i18n::t("Übersetzen"));
            return;
        }
    };

    // Check if the required key(s) are configured
    let config = state.borrow().config.clone();
    let cfg = config.borrow();
    let has_key = match service {
        "baidu" => {
            !cfg.api_keys.baidu_app_id.is_empty() && !cfg.api_keys.baidu_secret_key.is_empty()
        }
        "deepl" => !cfg.api_keys.deepl.is_empty(),
        "openai" => !cfg.api_keys.openai.is_empty(),
        "deepseek" => !cfg.api_keys.deepseek.is_empty(),
        "groq" => !cfg.api_keys.groq.is_empty(),
        "gemini" => !cfg.api_keys.gemini.is_empty(),
        "caiyun" => !cfg.api_keys.caiyun_token.is_empty(),
        _ => false,
    };
    drop(cfg);

    if has_key {
        let w = widgets.borrow();
        w.btn_start.remove_css_class("warning");
        w.btn_start.add_css_class("suggested-action");
        w.btn_start_label.set_label(&i18n::t("Übersetzen"));
    } else {
        let w = widgets.borrow();
        w.btn_start.remove_css_class("suggested-action");
        w.btn_start.add_css_class("warning");
        w.btn_start_label
            .set_label(&i18n::t("Bitte API-Schlüssel eintragen"));
    }
}

/// Friendly display names for API-key-requiring services (for dialogs).
pub fn service_display_name(service: &str) -> String {
    match service {
        "deepl" => "DeepL".to_string(),
        "openai" => i18n::t("OpenAI / ChatGPT"),
        "gemini" => i18n::t("Google Gemini"),
        "deepseek" => "DeepSeek".to_string(),
        "groq" => "Groq".to_string(),
        "baidu" => "Baidu".to_string(),
        "caiyun" => "Caiyun".to_string(),
        _ => service.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Log Viewer Dialog
// ---------------------------------------------------------------------------

/// Show the log viewer dialog with auto-refresh, copy, and external-open.
pub fn show_log_viewer_dialog(window: &adw::ApplicationWindow, state: &Rc<RefCell<WindowState>>) {
    let log_path = ConfigManager::log_file_path();
    state.borrow_mut().log_dialog_open = true;

    let dialog = adw::Dialog::new();
    dialog.set_title(&i18n::t("Log anzeigen"));
    dialog.set_content_width(750);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(8);
    content.set_margin_bottom(8);

    // ── Header row with file path ─────────────────────────────────────
    let header_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header_row.set_margin_bottom(4);

    let title = gtk::Label::new(Some(&i18n::t("Protokoll")));
    title.add_css_class("title-4");
    title.set_hexpand(true);
    title.set_halign(gtk::Align::Start);
    header_row.append(&title);

    let path_info = gtk::Label::new(Some(&log_path.display().to_string()));
    path_info.add_css_class("caption");
    path_info.add_css_class("dim-label");
    path_info.set_halign(gtk::Align::End);
    header_row.append(&path_info);

    content.append(&header_row);

    // ── ScrolledWindow + TextView ─────────────────────────────────────
    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_min_content_height(450);
    scrolled.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);

    let textview = gtk::TextView::new();
    textview.set_editable(false);
    textview.set_cursor_visible(false);
    textview.set_monospace(true);
    textview.set_wrap_mode(gtk::WrapMode::Char);
    textview.add_css_class("log-textview");
    textview.set_top_margin(4);
    textview.set_bottom_margin(4);
    textview.set_left_margin(8);
    textview.set_right_margin(8);

    let buffer = textview.buffer();

    // Load initial content from log file
    let initial_count = load_log_content(&buffer, &log_path, None);

    scrolled.set_child(Some(&textview));
    content.append(&scrolled);

    // Scroll to bottom after dialog is presented
    let scrolled_for_scroll = scrolled.clone();
    glib::idle_add_local_once(move || {
        let vadj = scrolled_for_scroll.vadjustment();
        vadj.set_value(vadj.upper() - vadj.page_size());
    });

    // ── Auto-scroll state ─────────────────────────────────────────────
    let auto_active = Rc::new(Cell::new(true));
    let line_count = Rc::new(Cell::new(initial_count));
    let dialog_open = Rc::new(Cell::new(true));

    // Periodic timer: reload log file every 2 s, append new lines, scroll
    {
        let auto_active = auto_active.clone();
        let dialog_open = dialog_open.clone();
        let line_count = line_count.clone();
        let buffer = buffer.clone();
        let scrolled = scrolled.clone();
        let log_path = log_path.clone();

        let _source_id =
            glib::timeout_add_local(std::time::Duration::from_millis(2000), move || {
                if !dialog_open.get() {
                    return glib::ControlFlow::Break;
                }
                if !auto_active.get() {
                    return glib::ControlFlow::Continue;
                }

                let new_count = load_log_content(&buffer, &log_path, Some(line_count.get()));
                if new_count != line_count.get() {
                    line_count.set(new_count);
                    let scrolled = scrolled.clone();
                    glib::idle_add_local_once(move || {
                        let vadj = scrolled.vadjustment();
                        vadj.set_value(vadj.upper() - vadj.page_size());
                    });
                }
                glib::ControlFlow::Continue
            });
    }

    // ── Button row ────────────────────────────────────────────────────
    let btn_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    btn_row.set_margin_top(8);

    // Auto-scroll toggle
    let auto_btn = gtk::ToggleButton::with_label(&i18n::t("Auto"));
    auto_btn.set_active(true);
    auto_btn.add_css_class("flat");
    auto_btn.set_tooltip_text(Some(&i18n::t("Automatisch aktualisieren")));
    {
        let auto_active = auto_active.clone();
        auto_btn.connect_toggled(move |btn| {
            auto_active.set(btn.is_active());
        });
    }
    btn_row.append(&auto_btn);

    // Refresh button — full reload from disk
    let refresh_btn = gtk::Button::with_label(&i18n::t("Aktualisieren"));
    refresh_btn.add_css_class("flat");
    {
        let buffer = buffer.clone();
        let log_path = log_path.clone();
        let line_count = line_count.clone();
        let scrolled = scrolled.clone();
        refresh_btn.connect_clicked(move |_| {
            let count = load_log_content(&buffer, &log_path, None);
            line_count.set(count);
            let scrolled = scrolled.clone();
            glib::idle_add_local_once(move || {
                let vadj = scrolled.vadjustment();
                vadj.set_value(vadj.upper() - vadj.page_size());
            });
        });
    }
    btn_row.append(&refresh_btn);

    // Copy button — copy buffer text to system clipboard
    let copy_btn = gtk::Button::with_label(&i18n::t("Kopieren"));
    copy_btn.add_css_class("flat");
    {
        let buffer = buffer.clone();
        copy_btn.connect_clicked(move |_| {
            let (start, end) = (buffer.start_iter(), buffer.end_iter());
            let text = buffer.text(&start, &end, false);
            if let Some(display) = Display::default() {
                display.clipboard().set_text(&text);
            }
        });
    }
    btn_row.append(&copy_btn);

    // Open externally button — xdg-open the log file
    let open_btn = gtk::Button::with_label(&i18n::t("Extern öffnen"));
    open_btn.add_css_class("flat");
    {
        let log_path = log_path.clone();
        open_btn.connect_clicked(move |_| {
            let _ = std::process::Command::new("xdg-open")
                .arg(&log_path)
                .spawn();
        });
    }
    btn_row.append(&open_btn);

    // Spacer — push close button to the right
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    btn_row.append(&spacer);

    // Close button
    let close_btn = gtk::Button::with_label(&i18n::t("Schließen"));
    close_btn.add_css_class("suggested-action");
    {
        let dialog = dialog.clone();
        close_btn.connect_clicked(move |_| {
            dialog.close();
        });
    }
    btn_row.append(&close_btn);

    content.append(&btn_row);

    // ── Dialog close cleanup ──────────────────────────────────────────
    {
        let dialog_open = dialog_open.clone();
        let state = state.clone();
        dialog.connect_closed(move |_| {
            dialog_open.set(false);
            state.borrow_mut().log_dialog_open = false;
        });
    }

    dialog.set_child(Some(&content));
    dialog.present(Some(window));
}

/// Load log file content into a `TextBuffer`.
///
/// * Full load (first time or `prev_count` is `None`): shows the last 500
///   lines with a header indicating how many older lines were hidden.
/// * Incremental load (`prev_count` is `Some(n)`): when the file has grown,
///   only the new lines are appended to avoid scroll jumping.
///
/// Returns the total number of lines in the file (used as the next
/// `prev_count` for incremental loading).
fn load_log_content(buffer: &gtk::TextBuffer, log_path: &Path, prev_count: Option<usize>) -> usize {
    let Ok(mut file) = std::fs::File::open(log_path) else {
        buffer.set_text(&format!(
            "{}\n{}",
            i18n::t("Keine Log-Datei gefunden."),
            log_path.display()
        ));
        return 0;
    };

    let mut content = String::new();
    if file.read_to_string(&mut content).is_err() {
        buffer.set_text(&i18n::t("Fehler beim Laden der Log-Datei."));
        return 0;
    }

    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();

    // Incremental update: only append new lines
    if let Some(prev) = prev_count {
        if prev > 0 && total >= prev {
            let new_lines = &lines[prev..];
            if !new_lines.is_empty() {
                let text = new_lines.join("\n");
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, &format!("\n{}", text));
            }
            return total;
        }
    }

    // Full load (first time, manual refresh, or file shrank)
    let max_lines = 500;
    if total > max_lines {
        let tail = &lines[total - max_lines..];
        let header = format!(
            "... {} ältere Zeilen ausgeblendet ...\n\n",
            total - max_lines
        );
        buffer.set_text(&format!("{}{}", header, tail.join("\n")));
    } else if total == 0 {
        buffer.set_text(&i18n::t("Log-Datei ist leer."));
    } else {
        buffer.set_text(&content);
    }

    total
}

// ---------------------------------------------------------------------------
// API Keys Dialog
// ---------------------------------------------------------------------------

/// Show the API keys management dialog.
///
/// * `config` — shared config manager for reading / writing API keys.
/// * `on_closed` — callback invoked after the dialog closes (e.g. to
///   refresh the start button's API-key warning state).
pub fn show_api_keys_dialog(
    window: &adw::ApplicationWindow,
    config: Rc<RefCell<ConfigManager>>,
    on_closed: Box<dyn Fn()>,
) {
    let dialog = adw::Dialog::builder()
        .title(&i18n::t("API Schlüssel"))
        .content_width(480)
        .build();

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    // ── Main content box ─────────────────────────────────────────────
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(6);
    content.set_margin_bottom(6);

    // Key descriptors: (title, field, is_secondary)
    let key_defs: &[(&str, &str, bool)] = &[
        ("DeepL", "deepl", false),
        ("OpenAI", "openai", false),
        ("Gemini", "gemini", false),
        ("DeepSeek", "deepseek", false),
        ("Groq", "groq", false),
        ("OpenRouter", "openrouter", true),
        ("Baidu App ID", "baidu_app_id", true),
        ("Baidu Secret Key", "baidu_secret_key", true),
        ("Caiyun Token", "caiyun_token", true),
    ];

    // Read all current key values upfront
    let api_keys = config.borrow().api_keys.clone();

    // ── Primary keys group ───────────────────────────────────────────
    let primary_group = adw::PreferencesGroup::builder()
        .title(&i18n::t("API Schlüssel"))
        .description(&i18n::t(
            "Schlüssel für kostenpflichtige Übersetzungsdienste",
        ))
        .build();

    // ── Secondary keys group ─────────────────────────────────────────
    let secondary_group = adw::PreferencesGroup::builder()
        .title(&i18n::t("Weitere Dienste"))
        .build();

    // Track entries for the save-all button
    let entries: Rc<RefCell<Vec<(String, adw::PasswordEntryRow)>>> =
        Rc::new(RefCell::new(Vec::new()));

    for &(title, field, is_secondary) in key_defs {
        let value = match field {
            "deepl" => api_keys.deepl.clone(),
            "openai" => api_keys.openai.clone(),
            "gemini" => api_keys.gemini.clone(),
            "deepseek" => api_keys.deepseek.clone(),
            "groq" => api_keys.groq.clone(),
            "openrouter" => api_keys.openrouter.clone(),
            "baidu_app_id" => api_keys.baidu_app_id.clone(),
            "baidu_secret_key" => api_keys.baidu_secret_key.clone(),
            "caiyun_token" => api_keys.caiyun_token.clone(),
            _ => String::new(),
        };
        let has_key = !value.is_empty();

        let row = adw::PasswordEntryRow::builder()
            .title(title)
            .show_apply_button(true)
            .build();
        row.set_text(&value);

        // Status icon suffix: green check or amber warning
        let status_icon = if has_key {
            let img = gtk::Image::from_icon_name("emblem-ok-symbolic");
            img.add_css_class("success");
            img
        } else {
            let img = gtk::Image::from_icon_name("dialog-warning-symbolic");
            img.add_css_class("warning");
            img
        };
        row.add_suffix(&status_icon);

        // Inline apply: save this key immediately
        let config_apply = config.clone();
        let icon_ref = status_icon.clone();
        let field_owned = field.to_string();
        row.connect_apply(move |r| {
            let text = r.text().to_string();
            {
                let mut cfg = config_apply.borrow_mut();
                match field_owned.as_str() {
                    "deepl" => cfg.api_keys.deepl = text.clone(),
                    "openai" => cfg.api_keys.openai = text.clone(),
                    "gemini" => cfg.api_keys.gemini = text.clone(),
                    "deepseek" => cfg.api_keys.deepseek = text.clone(),
                    "groq" => cfg.api_keys.groq = text.clone(),
                    "openrouter" => cfg.api_keys.openrouter = text.clone(),
                    "baidu_app_id" => cfg.api_keys.baidu_app_id = text.clone(),
                    "baidu_secret_key" => cfg.api_keys.baidu_secret_key = text.clone(),
                    "caiyun_token" => cfg.api_keys.caiyun_token = text.clone(),
                    _ => {}
                }
                cfg.save_api_keys();
            }
            log::info!("API key saved: {}", field_owned);
            // Update status icon
            if text.is_empty() {
                icon_ref.set_icon_name(Some("dialog-warning-symbolic"));
                icon_ref.remove_css_class("success");
                icon_ref.add_css_class("warning");
            } else {
                icon_ref.set_icon_name(Some("emblem-ok-symbolic"));
                icon_ref.add_css_class("success");
                icon_ref.remove_css_class("warning");
            }
        });

        entries.borrow_mut().push((field.to_string(), row.clone()));

        if is_secondary {
            secondary_group.add(&row);
        } else {
            primary_group.add(&row);
        }
    }

    content.append(&primary_group);
    content.append(&secondary_group);

    // ── Info label ───────────────────────────────────────────────────
    let info_label = gtk::Label::new(Some(&i18n::t(
        "Hinweis: Die Schlüssel werden lokal gespeichert.",
    )));
    info_label.add_css_class("caption");
    info_label.add_css_class("dim-label");
    info_label.set_wrap(true);
    info_label.set_xalign(0.0);
    info_label.set_margin_top(4);
    content.append(&info_label);

    toolbar.set_content(Some(&content));
    dialog.set_child(Some(&toolbar));

    // Save-all button in header
    let save_btn = gtk::Button::with_label(&i18n::t("Speichern"));
    save_btn.add_css_class("suggested-action");
    let config_save = config.clone();
    let entries_clone = entries.clone();
    let dialog_ref = dialog.clone();
    save_btn.connect_clicked(move |_| {
        let mut cfg = config_save.borrow_mut();
        let entries_ref = entries_clone.borrow();
        for (field, row) in entries_ref.iter() {
            let value = row.text().to_string();
            match field.as_str() {
                "deepl" => cfg.api_keys.deepl = value,
                "openai" => cfg.api_keys.openai = value,
                "gemini" => cfg.api_keys.gemini = value,
                "deepseek" => cfg.api_keys.deepseek = value,
                "groq" => cfg.api_keys.groq = value,
                "openrouter" => cfg.api_keys.openrouter = value,
                "baidu_app_id" => cfg.api_keys.baidu_app_id = value,
                "baidu_secret_key" => cfg.api_keys.baidu_secret_key = value,
                "caiyun_token" => cfg.api_keys.caiyun_token = value,
                _ => {}
            }
        }
        cfg.save_api_keys();
        log::info!("All API keys saved");
        drop(cfg);
        dialog_ref.close();
    });
    header.pack_end(&save_btn);

    dialog.present(Some(window));

    // Invoke on_closed callback after dialog closes
    let on_closed = Rc::new(on_closed);
    dialog.connect_closed(clone!(
        #[strong]
        on_closed,
        move |_| {
            on_closed();
        }
    ));
}

// ---------------------------------------------------------------------------
// Accent Color Dialog
// ---------------------------------------------------------------------------

/// Show the accent color selection dialog.
///
/// Features:
///   - Dark/Light/System color scheme toggle
///   - 4-column grid of preset color swatches with labels
///   - Custom hex input with live preview and color chooser
///   - Active swatch indicator ring
pub fn show_accent_color_dialog(
    parent: &impl IsA<gtk::Widget>,
    config: Rc<RefCell<ConfigManager>>,
    on_changed: Box<dyn Fn(&str)>,
) {
    let on_changed = Rc::new(on_changed);
    let dialog = adw::Dialog::builder()
        .title(&i18n::t("Akzentfarbe"))
        .content_width(420)
        .content_height(500)
        .build();

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.set_margin_top(16);
    content.set_margin_bottom(16);

    // ── Color scheme toggle (System / Hell / Dunkel) ──
    let scheme_label = gtk::Label::new(Some(&i18n::t("Farbschema")));
    scheme_label.add_css_class("heading");
    scheme_label.set_xalign(0.0);
    content.append(&scheme_label);

    let scheme_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    scheme_box.add_css_class("linked");
    scheme_box.set_halign(gtk::Align::Start);

    let btn_system = gtk::ToggleButton::with_label(&i18n::t("System"));
    let btn_light = gtk::ToggleButton::with_label(&i18n::t("Hell"));
    let btn_dark = gtk::ToggleButton::with_label(&i18n::t("Dunkel"));
    btn_light.set_group(Some(&btn_system));
    btn_dark.set_group(Some(&btn_system));

    let current_scheme = config.borrow().settings.color_scheme.clone();
    match current_scheme.as_str() {
        "light" => btn_light.set_active(true),
        "dark" => btn_dark.set_active(true),
        _ => btn_system.set_active(true),
    }

    // Apply scheme: System
    let config_scheme = config.clone();
    let style_mgr = adw::StyleManager::default();
    btn_system.connect_toggled(clone!(
        #[strong]
        style_mgr,
        #[strong]
        config_scheme,
        move |btn| {
            if btn.is_active() {
                style_mgr.set_color_scheme(adw::ColorScheme::PreferLight);
                config_scheme.borrow_mut().settings.color_scheme = "system".to_string();
                config_scheme.borrow_mut().save_settings();
            }
        }
    ));

    // Apply scheme: Force Light
    let config_scheme = config.clone();
    let style_mgr = adw::StyleManager::default();
    btn_light.connect_toggled(clone!(
        #[strong]
        style_mgr,
        #[strong]
        config_scheme,
        move |btn| {
            if btn.is_active() {
                style_mgr.set_color_scheme(adw::ColorScheme::ForceLight);
                config_scheme.borrow_mut().settings.color_scheme = "light".to_string();
                config_scheme.borrow_mut().save_settings();
            }
        }
    ));

    // Apply scheme: Force Dark
    let config_scheme = config.clone();
    let style_mgr = adw::StyleManager::default();
    btn_dark.connect_toggled(clone!(
        #[strong]
        style_mgr,
        #[strong]
        config_scheme,
        move |btn| {
            if btn.is_active() {
                style_mgr.set_color_scheme(adw::ColorScheme::ForceDark);
                config_scheme.borrow_mut().settings.color_scheme = "dark".to_string();
                config_scheme.borrow_mut().save_settings();
            }
        }
    ));

    scheme_box.append(&btn_system);
    scheme_box.append(&btn_light);
    scheme_box.append(&btn_dark);
    content.append(&scheme_box);

    // ── Separator ──
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    // ── Accent color swatches (4-column grid with labels) ──
    let preset_title = gtk::Label::new(Some(&i18n::t("Akzentfarbe wählen")));
    preset_title.add_css_class("heading");
    preset_title.set_xalign(0.0);
    content.append(&preset_title);

    let grid = gtk::Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(8);
    grid.set_halign(gtk::Align::Center);
    grid.set_margin_top(8);

    // Single CSS provider for all swatches — combines base styles
    // from build_swatch_grid_css() with per-color background rules.
    let mut swatch_css = css::build_swatch_grid_css().to_string();
    let presets = config::accent_presets();
    for preset in &presets {
        if !preset.hex.is_empty() {
            let safe = preset.name.replace('-', "_");
            swatch_css.push_str(&format!(
                ".swatch-{safe} {{ background: {}; color: {}; }}\n",
                preset.hex, preset.fg,
            ));
        }
    }
    let swatch_provider = gtk::CssProvider::new();
    swatch_provider.load_from_data(&swatch_css);

    let current_accent = config.borrow().settings.accent_color.clone();

    for (i, preset) in presets.iter().enumerate() {
        let col = (i % 4) as i32;
        let row = (i / 4) as i32;

        let btn_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        btn_box.set_halign(gtk::Align::Center);

        let btn = gtk::Button::new();
        btn.add_css_class("accent-swatch");

        if preset.hex.is_empty() {
            btn.add_css_class("accent-swatch-system");
            btn.set_icon_name("emblem-system-symbolic");
        } else {
            let safe = preset.name.replace('-', "_");
            btn.add_css_class(&format!("swatch-{safe}"));
        }

        // Apply the shared swatch CSS provider
        gtk::style_context_add_provider_for_display(
            &btn.display(),
            &swatch_provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER,
        );

        // Mark active swatch
        if preset.name == current_accent {
            btn.add_css_class("active");
        }

        let preset_name = preset.name.clone();
        let dialog_weak = dialog.downgrade();
        let config_clone = config.clone();
        let on_changed_clone = on_changed.clone();

        btn.connect_clicked(move |_| {
            apply_accent(&config_clone, &preset_name);
            on_changed_clone(&preset_name);
            if let Some(d) = dialog_weak.upgrade() {
                d.close();
            }
        });

        // Label under swatch
        let label = gtk::Label::new(Some(&config::accent_display_name(&preset.name)));
        label.add_css_class("accent-swatch-label");

        btn_box.append(&btn);
        btn_box.append(&label);
        grid.attach(&btn_box, col, row, 1, 1);
    }

    content.append(&grid);

    // ── Separator ──
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    // ── Custom hex input ──
    let custom_title = gtk::Label::new(Some(&i18n::t("Benutzerdefiniert")));
    custom_title.add_css_class("heading");
    custom_title.set_xalign(0.0);
    content.append(&custom_title);

    let hex_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    hex_row.set_valign(gtk::Align::Center);

    // Live preview swatch
    let preview_swatch = gtk::Box::new(gtk::Orientation::Vertical, 0);
    preview_swatch.set_size_request(36, 36);
    preview_swatch.add_css_class("accent-preview");
    preview_swatch.set_visible(false);

    let hex_entry = gtk::Entry::new();
    hex_entry.set_placeholder_text(Some("#FF5722"));
    hex_entry.set_max_length(7);
    hex_entry.set_hexpand(true);

    // Pre-fill if current accent is a custom hex color
    if current_accent.starts_with('#') {
        hex_entry.set_text(&current_accent);
    }

    hex_entry.connect_changed(clone!(
        #[weak]
        preview_swatch,
        move |entry| {
            let text = entry.text().to_string();
            if is_valid_hex_color(&text) {
                let css = format!(
                    ".accent-preview {{ background: {}; border-radius: 8px; min-width: 36px; min-height: 36px; }}",
                    text
                );
                let provider = gtk::CssProvider::new();
                provider.load_from_data(&css);
                gtk::style_context_add_provider_for_display(
                    &preview_swatch.display(),
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_USER,
                );
                preview_swatch.set_visible(true);
            } else {
                preview_swatch.set_visible(false);
            }
        }
    ));

    hex_row.append(&preview_swatch);
    hex_row.append(&hex_entry);

    // Color chooser button
    let color_btn = gtk::Button::from_icon_name("color-select-symbolic");
    color_btn.set_tooltip_text(Some(&i18n::t("Farbwähler öffnen")));
    color_btn.add_css_class("flat");

    let hex_entry_weak = hex_entry.downgrade();
    color_btn.connect_clicked(move |btn| {
        let window = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        let dialog = gtk::ColorDialog::new();
        dialog.set_title(&i18n::t("Farbe wählen"));

        let hex_entry_ref = hex_entry_weak.clone();
        dialog.choose_rgba(
            window.as_ref(),
            None,
            gio::Cancellable::NONE,
            move |result| {
                if let Ok(rgba) = result {
                    let hex = format!(
                        "#{:02X}{:02X}{:02X}",
                        (rgba.red() * 255.0) as u8,
                        (rgba.green() * 255.0) as u8,
                        (rgba.blue() * 255.0) as u8
                    );
                    if let Some(entry) = hex_entry_ref.upgrade() {
                        entry.set_text(&hex);
                    }
                }
            },
        );
    });

    hex_row.append(&color_btn);

    // Apply button for custom color
    let apply_btn = gtk::Button::with_label(&i18n::t("Anwenden"));
    apply_btn.add_css_class("suggested-action");
    apply_btn.connect_clicked(clone!(
        #[weak]
        hex_entry,
        #[weak]
        dialog,
        #[strong]
        config,
        #[strong]
        on_changed,
        move |_| {
            let text = hex_entry.text().to_string();
            if is_valid_hex_color(&text) {
                apply_accent(&config, &text);
                on_changed(&text);
                dialog.close();
            }
        }
    ));
    hex_row.append(&apply_btn);

    content.append(&hex_row);

    // Wrap in scrolled window
    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scrolled.set_child(Some(&content));

    toolbar.set_content(Some(&scrolled));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

/// Apply an accent color by name or hex code, persisting to config.
/// CSS provider update is handled by the main window via the on_changed callback.
fn apply_accent(config: &Rc<RefCell<ConfigManager>>, name_or_hex: &str) {
    let accent = if let Some(preset) = config::find_preset(name_or_hex) {
        preset
    } else if name_or_hex.starts_with('#') && name_or_hex.len() == 7 {
        let fg = AccentColor::foreground_for(name_or_hex);
        AccentColor {
            name: name_or_hex.to_string(),
            hex: name_or_hex.to_string(),
            fg,
        }
    } else {
        log::warn!("Invalid accent color: {}", name_or_hex);
        return;
    };

    let _ = Display::default(); // CSS provider is managed by main window
    let mut cfg = config.borrow_mut();
    cfg.settings.accent_color = name_or_hex.to_string();
    if name_or_hex.starts_with('#') {
        cfg.settings
            .custom_accent_colors
            .insert(name_or_hex.to_string(), accent.hex.clone());
    }
    cfg.save_settings();
    log::info!("Accent color set to: {}", name_or_hex);
}

/// Validate a hex color string (e.g. "#FF5722").
fn is_valid_hex_color(s: &str) -> bool {
    if s.len() != 7 || !s.starts_with('#') {
        return false;
    }
    s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// About Dialog
// ---------------------------------------------------------------------------

/// Show the about dialog with application info, credits, and license.
pub fn show_about_dialog(parent: &impl IsA<gtk::Widget>) {
    let about = adw::AboutDialog::builder()
        .application_name("Manga Translator")
        .application_icon("applications-graphics")
        .version("0.2.0")
        .comments(&i18n::t(
            "Ein GUI für Manga Image Translator — geschrieben in Rust mit GTK4/libadwaita",
        ))
        .developer_name("Manga Translator Team")
        .license_type(gtk::License::MitX11)
        .website("https://github.com/zyddnys/manga-image-translator")
        .issue_url("https://github.com/zyddnys/manga-image-translator/issues")
        .build();

    about.add_credit_section(Some("Rust GUI"), &["SLOB-CODER", "Contributors"]);
    about.add_credit_section(Some("Python Backend"), &["zyddnys", "Contributors"]);
    about.add_credit_section(
        Some("Powered by"),
        &["GTK4", "libadwaita", "gettext", "MangaTranslator"],
    );

    about.present(Some(parent));
}

// ---------------------------------------------------------------------------
// Manga Home Directory Dialog
// ---------------------------------------------------------------------------

/// Show a dialog to configure the manga home directory.
///
/// The manga home directory is the location opened when the user clicks the
/// home icon in the file browser. It defaults to `~/Manga` (or the system
/// home directory if that doesn't exist).
pub fn show_manga_home_dialog(
    parent: &impl IsA<gtk::Widget>,
    config: Rc<RefCell<ConfigManager>>,
    on_changed: Box<dyn Fn(PathBuf)>,
) {
    let on_changed = Rc::new(on_changed);
    let dialog = adw::Dialog::builder()
        .title(&i18n::t("Manga-Startverzeichnis"))
        .content_width(460)
        .build();

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(12);
    content.set_margin_bottom(12);

    // Description
    let desc = gtk::Label::new(Some(&i18n::t(
        "Dieses Verzeichnis wird geöffnet, wenn du auf das Start-Symbol klickst.",
    )));
    desc.add_css_class("dim-label");
    desc.set_wrap(true);
    desc.set_xalign(0.0);
    content.append(&desc);

    // ── Current directory group ──────────────────────────────────────
    let group = adw::PreferencesGroup::builder()
        .title(&i18n::t("Startverzeichnis"))
        .build();

    // Current path row
    let current_path = {
        let cfg = config.borrow();
        let path_str = cfg.settings.manga_home_directory.clone();
        if path_str.is_empty() {
            let default = ConfigManager::default_manga_dir();
            default.display().to_string()
        } else {
            path_str
        }
    };

    let path_row = adw::ActionRow::builder()
        .title(&i18n::t("Aktuelles Verzeichnis"))
        .subtitle(&current_path)
        .build();
    path_row.add_css_class("monospace-subtitle");

    // Choose button
    let choose_btn = gtk::Button::from_icon_name("folder-open-symbolic");
    choose_btn.add_css_class("flat");
    choose_btn.set_valign(gtk::Align::Center);
    choose_btn.set_tooltip_text(Some(&i18n::t("Verzeichnis wählen")));
    path_row.add_suffix(&choose_btn);

    // Reset to default button
    let reset_btn = gtk::Button::from_icon_name("edit-clear-symbolic");
    reset_btn.add_css_class("flat");
    reset_btn.set_valign(gtk::Align::Center);
    reset_btn.set_tooltip_text(Some(&i18n::t("Auf Standard zurücksetzen")));
    path_row.add_suffix(&reset_btn);

    group.add(&path_row);
    content.append(&group);

    toolbar.set_content(Some(&content));
    dialog.set_child(Some(&toolbar));

    // ── Choose directory ─────────────────────────────────────────────
    let config_choose = config.clone();
    let on_changed_choose = on_changed.clone();
    let path_row_ref = path_row.clone();
    let dialog_weak = dialog.downgrade();
    choose_btn.connect_clicked(move |btn| {
        let window = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        let chooser = gtk::FileDialog::new();
        chooser.set_title(&i18n::t("Manga-Verzeichnis wählen"));

        // Pre-select current path
        let current = {
            let cfg = config_choose.borrow();
            let path_str = cfg.settings.manga_home_directory.clone();
            if path_str.is_empty() {
                ConfigManager::default_manga_dir()
            } else {
                PathBuf::from(&path_str)
            }
        };
        if current.exists() {
            chooser.set_initial_folder(Some(&gio::File::for_path(&current)));
        }

        let config_save = config_choose.clone();
        let on_changed_save = on_changed_choose.clone();
        let row = path_row_ref.clone();
        let dlg = dialog_weak.clone();
        chooser.select_folder(window.as_ref(), gio::Cancellable::NONE, move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    let path_str = path.display().to_string();
                    {
                        let mut cfg = config_save.borrow_mut();
                        cfg.settings.manga_home_directory = path_str.clone();
                        cfg.save_settings();
                    }
                    row.set_subtitle(&path_str);
                    log::info!("Manga home directory set to: {}", path_str);
                    on_changed_save(path);
                }
            }
            // Close the dialog after choosing
            if let Some(d) = dlg.upgrade() {
                d.close();
            }
        });
    });

    // ── Reset to default ─────────────────────────────────────────────
    let config_reset = config.clone();
    let on_changed_reset = on_changed.clone();
    let path_row_reset = path_row.clone();
    reset_btn.connect_clicked(move |_| {
        {
            let mut cfg = config_reset.borrow_mut();
            cfg.settings.manga_home_directory = String::new();
            cfg.save_settings();
        }
        let default = ConfigManager::default_manga_dir();
        path_row_reset.set_subtitle(&default.display().to_string());
        log::info!("Manga home directory reset to default");
        on_changed_reset(default);
    });

    dialog.present(Some(parent));
}

// ---------------------------------------------------------------------------
// Virtual Environment / Python Paths Dialog
// ---------------------------------------------------------------------------

/// Show a dialog for configuring the Python virtual environment and
/// manga-image-translator paths so the GUI can operate standalone.
pub fn show_virtual_env_dialog(
    parent: &impl IsA<gtk::Widget>,
    config: Rc<RefCell<ConfigManager>>,
    bridge: Arc<IpcBridge>,
    on_changed: Box<dyn Fn()>,
) {
    let on_changed = Rc::new(on_changed);
    let dialog = adw::Dialog::builder()
        .title(&i18n::t("Virtuelle Umgebung"))
        .content_width(520)
        .build();

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(12);
    content.set_margin_bottom(12);

    // Description
    let desc = gtk::Label::new(Some(&i18n::t(
        "Konfiguriere die Python-Umgebung und den Speicherort von manga-image-translator, damit die GUI eigenständig funktioniert.",
    )));
    desc.add_css_class("dim-label");
    desc.set_wrap(true);
    desc.set_xalign(0.0);
    content.append(&desc);

    // ── Group 1: Python Virtual Environment ──────────────────────────
    let venv_group = adw::PreferencesGroup::builder()
        .title(&i18n::t("Python-Umgebung"))
        .build();

    let venv_subtitle = {
        let cfg = config.borrow();
        let path_str = cfg.settings.virtual_env_path.clone();
        if path_str.is_empty() {
            i18n::t("Nicht konfiguriert")
        } else {
            path_str
        }
    };

    let venv_row = adw::ActionRow::builder()
        .title(&i18n::t("Virtuelle Umgebung"))
        .subtitle(&venv_subtitle)
        .build();
    venv_row.add_css_class("monospace-subtitle");

    // Choose folder button
    let venv_choose_btn = gtk::Button::from_icon_name("folder-open-symbolic");
    venv_choose_btn.add_css_class("flat");
    venv_choose_btn.set_valign(gtk::Align::Center);
    venv_choose_btn.set_tooltip_text(Some(&i18n::t("Verzeichnis wählen")));
    venv_row.add_suffix(&venv_choose_btn);

    // Reset button
    let venv_reset_btn = gtk::Button::from_icon_name("edit-clear-symbolic");
    venv_reset_btn.add_css_class("flat");
    venv_reset_btn.set_valign(gtk::Align::Center);
    venv_reset_btn.set_tooltip_text(Some(&i18n::t("Zurücksetzen")));
    venv_row.add_suffix(&venv_reset_btn);

    venv_group.add(&venv_row);
    content.append(&venv_group);

    // ── Group 2: Manga Translator ────────────────────────────────────
    let mt_group = adw::PreferencesGroup::builder()
        .title(&i18n::t("Manga Translator"))
        .build();

    let mt_subtitle = {
        let cfg = config.borrow();
        let path_str = cfg.settings.manga_translator_path.clone();
        if path_str.is_empty() {
            i18n::t("Nicht konfiguriert")
        } else {
            path_str
        }
    };

    let mt_row = adw::ActionRow::builder()
        .title(&i18n::t("manga-image-translator"))
        .subtitle(&mt_subtitle)
        .build();
    mt_row.add_css_class("monospace-subtitle");

    // Choose folder button
    let mt_choose_btn = gtk::Button::from_icon_name("folder-open-symbolic");
    mt_choose_btn.add_css_class("flat");
    mt_choose_btn.set_valign(gtk::Align::Center);
    mt_choose_btn.set_tooltip_text(Some(&i18n::t("Verzeichnis wählen")));
    mt_row.add_suffix(&mt_choose_btn);

    // Reset button
    let mt_reset_btn = gtk::Button::from_icon_name("edit-clear-symbolic");
    mt_reset_btn.add_css_class("flat");
    mt_reset_btn.set_valign(gtk::Align::Center);
    mt_reset_btn.set_tooltip_text(Some(&i18n::t("Zurücksetzen")));
    mt_row.add_suffix(&mt_reset_btn);

    mt_group.add(&mt_row);
    content.append(&mt_group);

    // ── Validation section ───────────────────────────────────────────
    let validation_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    validation_box.set_margin_top(8);
    content.append(&validation_box);

    // Helper: re-run validation and update the validation_box children
    let update_validation = {
        let config_v = config.clone();
        let validation_box = validation_box.clone();

        Rc::new(move || {
            // Clear previous children
            while let Some(child) = validation_box.first_child() {
                validation_box.remove(&child);
            }

            let cfg = config_v.borrow();

            // Validate virtual env
            let venv_ok = cfg.resolve_venv_site_packages().is_some();
            let venv_label = if venv_ok {
                let label =
                    gtk::Label::new(Some(&format!("✔ {}", i18n::t("Python-Umgebung gefunden"))));
                label.add_css_class("success");
                label
            } else {
                let label = gtk::Label::new(Some(&format!("✘ {}", i18n::t("Nicht gefunden"))));
                label.add_css_class("error");
                label
            };
            venv_label.set_xalign(0.0);
            validation_box.append(&venv_label);

            // Validate manga translator
            let mt_ok = cfg.manga_translator_dir().is_some();
            let mt_label = if mt_ok {
                let label = gtk::Label::new(Some(&format!(
                    "✔ {}",
                    i18n::t("manga-image-translator gefunden")
                )));
                label.add_css_class("success");
                label
            } else {
                let label = gtk::Label::new(Some(&format!("✘ {}", i18n::t("Nicht gefunden"))));
                label.add_css_class("error");
                label
            };
            mt_label.set_xalign(0.0);
            validation_box.append(&mt_label);
        })
    };

    // Helper: after a path change, update bridge + on_changed + validation
    let notify_paths_changed = {
        let config_n = config.clone();
        let bridge_n = bridge.clone();
        let on_changed_n = on_changed.clone();
        let update_validation_n = update_validation.clone();

        Rc::new(move || {
            let (site_packages, manga_dir) = {
                let cfg = config_n.borrow();
                (cfg.resolve_venv_site_packages(), cfg.manga_translator_dir())
            };
            bridge_n.configure_paths(site_packages, manga_dir);
            on_changed_n();
            update_validation_n();
        })
    };

    // Run initial validation
    update_validation();

    toolbar.set_content(Some(&content));
    dialog.set_child(Some(&toolbar));

    // ── Venv: Choose directory ───────────────────────────────────────
    let config_venv_choose = config.clone();
    let venv_row_ref = venv_row.clone();
    let notify_venv = notify_paths_changed.clone();
    venv_choose_btn.connect_clicked(move |btn| {
        let window = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        let chooser = gtk::FileDialog::new();
        chooser.set_title(&i18n::t("Virtuelle Umgebung wählen"));

        // Pre-select current path
        let current = {
            let cfg = config_venv_choose.borrow();
            let path_str = cfg.settings.virtual_env_path.clone();
            if path_str.is_empty() {
                PathBuf::new()
            } else {
                PathBuf::from(&path_str)
            }
        };
        if current.exists() {
            chooser.set_initial_folder(Some(&gio::File::for_path(&current)));
        }

        let config_save = config_venv_choose.clone();
        let row = venv_row_ref.clone();
        let notify = notify_venv.clone();
        chooser.select_folder(window.as_ref(), gio::Cancellable::NONE, move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    let path_str = path.display().to_string();
                    {
                        let mut cfg = config_save.borrow_mut();
                        cfg.settings.virtual_env_path = path_str.clone();
                        cfg.save_settings();
                    }
                    row.set_subtitle(&path_str);
                    log::info!("Virtual env path set to: {}", path.display());
                    notify();
                }
            }
        });
    });

    // ── Venv: Reset ──────────────────────────────────────────────────
    let config_venv_reset = config.clone();
    let venv_row_reset = venv_row.clone();
    let notify_venv_reset = notify_paths_changed.clone();
    venv_reset_btn.connect_clicked(move |_| {
        {
            let mut cfg = config_venv_reset.borrow_mut();
            cfg.settings.virtual_env_path = String::new();
            cfg.save_settings();
        }
        venv_row_reset.set_subtitle(&i18n::t("Nicht konfiguriert"));
        log::info!("Virtual env path reset");
        notify_venv_reset();
    });

    // ── Manga Translator: Choose directory ───────────────────────────
    let config_mt_choose = config.clone();
    let mt_row_ref = mt_row.clone();
    let notify_mt = notify_paths_changed.clone();
    mt_choose_btn.connect_clicked(move |btn| {
        let window = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        let chooser = gtk::FileDialog::new();
        chooser.set_title(&i18n::t("manga-image-translator Verzeichnis wählen"));

        // Pre-select current path
        let current = {
            let cfg = config_mt_choose.borrow();
            let path_str = cfg.settings.manga_translator_path.clone();
            if path_str.is_empty() {
                PathBuf::new()
            } else {
                PathBuf::from(&path_str)
            }
        };
        if current.exists() {
            chooser.set_initial_folder(Some(&gio::File::for_path(&current)));
        }

        let config_save = config_mt_choose.clone();
        let row = mt_row_ref.clone();
        let notify = notify_mt.clone();
        chooser.select_folder(window.as_ref(), gio::Cancellable::NONE, move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    let path_str = path.display().to_string();
                    {
                        let mut cfg = config_save.borrow_mut();
                        cfg.settings.manga_translator_path = path_str.clone();
                        cfg.save_settings();
                    }
                    row.set_subtitle(&path_str);
                    log::info!("Manga translator path set to: {}", path.display());
                    notify();
                }
            }
        });
    });

    // ── Manga Translator: Reset ──────────────────────────────────────
    let config_mt_reset = config.clone();
    let mt_row_reset = mt_row.clone();
    let notify_mt_reset = notify_paths_changed.clone();
    mt_reset_btn.connect_clicked(move |_| {
        {
            let mut cfg = config_mt_reset.borrow_mut();
            cfg.settings.manga_translator_path = String::new();
            cfg.save_settings();
        }
        mt_row_reset.set_subtitle(&i18n::t("Nicht konfiguriert"));
        log::info!("Manga translator path reset");
        notify_mt_reset();
    });

    dialog.present(Some(parent));
}

// manga-translator-gtk/src/ui/main_window.rs
//
// Main application window — built procedurally (no GObject subclassing).
// Uses `Rc<RefCell<WindowState>>` for shared mutable state and
// `glib::clone!` with `#[weak]` / `#[strong]` for widget signal connections.

use adw::prelude::*;
use gtk::gdk::Display;
use gtk::gio;
use gtk::glib;
use gtk::glib::clone;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::file_browser::FileBrowser;
use super::preview::Preview;
use super::settings_panel::SettingsPanel;
use crate::config::{self, ConfigManager};
use crate::i18n;
use crate::ipc_bridge::IpcBridge;

// ---------------------------------------------------------------------------
// Mutable state (no widget references — pure data)
// ---------------------------------------------------------------------------

/// Mutable state of the main window.
pub struct WindowState {
    /// Configuration manager (shared with SettingsPanel).
    pub config: Rc<RefCell<ConfigManager>>,
    /// Python bridge for backend communication (shared with translation thread).
    pub bridge: Arc<IpcBridge>,
    /// Currently open directory (mirrored from FileBrowser for persistence).
    pub current_directory: PathBuf,
    /// Whether a translation is currently running.
    pub is_processing: bool,
    /// Cancel flag for the running translation.
    pub cancel_flag: Arc<AtomicBool>,
    /// Current search filter text.
    #[allow(dead_code)]
    pub search_text: String,
    /// Current accent color name.
    pub accent_color: String,
    /// CSS provider for accent color overrides.
    pub accent_css_provider: Option<gtk::CssProvider>,
    /// Error count for log button indicator.
    pub error_count: u32,
    /// Whether the log dialog is currently open.
    #[allow(dead_code)]
    pub log_dialog_open: bool,
    /// Log messages collected during translation.
    pub log_entries: Rc<RefCell<Vec<String>>>,
}

// ---------------------------------------------------------------------------
// Widget references (needed across closures)
// ---------------------------------------------------------------------------

/// Widget references that need to be accessed from multiple closures.
pub(crate) struct Widgets {
    file_browser: FileBrowser,
    pub(crate) settings_panel: SettingsPanel,
    preview: Preview,
    path_label: gtk::Label,
    breadcrumb_segments: gtk::Box,
    btn_grid: gtk::ToggleButton,
    btn_list: gtk::ToggleButton,
    status_label: gtk::Label,
    progress_bar: gtk::ProgressBar,
    progress_revealer: gtk::Revealer,
    pub(crate) btn_start: gtk::Button,
    pub(crate) btn_start_spinner: gtk::Spinner,
    pub(crate) btn_start_label: gtk::Label,
    btn_log: gtk::Button,
    btn_home: gtk::Button,
    btn_refresh: gtk::Button,
    #[allow(dead_code)]
    main_paned: gtk::Paned,
    #[allow(dead_code)]
    right_paned: gtk::Paned,
    toast_overlay: adw::ToastOverlay,
    #[allow(dead_code)]
    search_entry: gtk::SearchEntry,
    /// Set to false when the window is closing.
    alive: Rc<Cell<bool>>,
}

// ---------------------------------------------------------------------------
// Translation progress messages (sent from background thread)
// ---------------------------------------------------------------------------

enum TranslationMsg {
    /// Progress update: (fraction 0..1, status message).
    Progress(f64, String),
    /// Translation finished.
    Done(Result<Vec<PathBuf>, String>),
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Build and return the main `adw::ApplicationWindow` with all widgets
/// connected.  No GObject subclassing is used — everything is constructed
/// procedurally.
pub fn build_main_window(app: &adw::Application) -> adw::ApplicationWindow {
    log::debug!("build_main_window() called");
    // ── Shared config manager ────────────────────────────────────────────
    let config = Rc::new(RefCell::new(ConfigManager::new()));

    // ── Shared mutable state ────────────────────────────────────────────
    let state = Rc::new(RefCell::new(WindowState {
        config: config.clone(),
        bridge: Arc::new(IpcBridge::new()),
        current_directory: ConfigManager::default_manga_dir(),
        is_processing: false,
        cancel_flag: Arc::new(AtomicBool::new(false)),
        search_text: String::new(),
        accent_color: "system".to_string(),
        accent_css_provider: None,
        error_count: 0,
        log_dialog_open: false,
        log_entries: Rc::new(RefCell::new(Vec::new())),
    }));

    // ── Configure Python bridge with saved paths ─────────────────────────
    {
        let cfg = config.borrow();
        let site_packages = cfg.resolve_venv_site_packages();
        let manga_dir = cfg.manga_translator_dir();
        let has_paths = site_packages.is_some() || manga_dir.is_some();
        state
            .borrow()
            .bridge
            .configure_paths(site_packages, manga_dir);
        if has_paths {
            log::info!("Python bridge pre-configured with saved paths");
        }
    }

    // ── Window ──────────────────────────────────────────────────────────
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(&i18n::t("Manga Translator"))
        .default_width(1280)
        .default_height(800)
        .build();

    // ── Root: ToastOverlay > ToolbarView ────────────────────────────────
    let toast_overlay = adw::ToastOverlay::new();
    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.set_top_bar_style(adw::ToolbarStyle::Flat);

    // ── Header bar ──────────────────────────────────────────────────────
    let path_label = build_header_bar(&toolbar_view, &state);

    // ── Create key widgets ──────────────────────────────────────────────
    let file_browser = FileBrowser::new();
    file_browser.set_bridge(state.borrow().bridge.clone());
    let settings_panel = SettingsPanel::new();
    settings_panel.set_config(config.clone());

    // ── Wire up OpenRouter model fetch callback ────────────────────────
    //
    // The fetch runs on a background thread (Python HTTP call) and sends
    // results through an async_channel back to the UI thread.  This avoids
    // moving the non-Send SettingsPanel into the worker thread.
    //
    // The OpenRouter /api/v1/models endpoint is public — no API key is
    // required to list models.  The key is passed along if available so
    // that the request includes an Authorization header, but it is not
    // a prerequisite for the fetch.
    {
        let bridge = state.borrow().bridge.clone();
        let config_clone = config.clone();
        let sp = settings_panel.clone();

        settings_panel.set_on_fetch_openrouter_models(move || {
            let api_key = config_clone.borrow().api_keys.openrouter.clone();

            let bridge = bridge.clone();
            let sp = sp.clone();

            // Show loading state
            sp.set_openrouter_fetching(true);

            let (tx, rx) = async_channel::bounded::<Vec<String>>(1);

            std::thread::spawn(move || {
                let model_ids = match bridge.fetch_openrouter_models(&api_key) {
                    Ok(models) => {
                        let ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
                        if ids.is_empty() {
                            log::warn!("OpenRouter returned no vision-capable models");
                        } else {
                            log::info!(
                                "Fetched {} vision-capable models from OpenRouter",
                                ids.len()
                            );
                        }
                        ids
                    }
                    Err(e) => {
                        log::error!("Failed to fetch OpenRouter models: {}", e);
                        Vec::new()
                    }
                };
                // Always send so the UI can clear its loading state
                let _ = tx.send_blocking(model_ids);
            });

            glib::spawn_future_local(async move {
                if let Ok(model_ids) = rx.recv().await {
                    sp.update_openrouter_models(&model_ids);
                }
            });
        });
    }

    // ── File browser area (search + buttons + browser + sort) ───────────
    let (
        left_pane,
        search_entry,
        _sort_dropdown,
        breadcrumb_segments,
        btn_breadcrumb_home,
        btn_breadcrumb_up,
        btn_breadcrumb_refresh,
        btn_grid,
        btn_list,
    ) = build_file_browser_area(&file_browser, &config);

    // ── Preview pane (Preview GObject) ──────────────────────────────────
    let preview_obj = Preview::new();
    let preview_pane: gtk::Widget = preview_obj.clone().upcast();

    // ── Settings widget ─────────────────────────────────────────────────
    let settings_widget = settings_panel.clone();
    settings_widget.set_size_request(250, -1);

    // ── Main content (Paned) ────────────────────────────────────────────
    let paned_pos = config.borrow().settings.paned_position;
    let right_paned_pos = config.borrow().settings.right_paned_position;

    let main_paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    main_paned.set_vexpand(true);
    main_paned.set_position(paned_pos);
    main_paned.set_shrink_start_child(false);
    main_paned.set_shrink_end_child(false);
    main_paned.set_start_child(Some(&left_pane));

    let right_paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    right_paned.set_position(right_paned_pos);
    right_paned.set_shrink_start_child(false);
    right_paned.set_shrink_end_child(false);
    right_paned.set_start_child(Some(&settings_widget));
    right_paned.set_end_child(Some(&preview_pane));

    main_paned.set_end_child(Some(&right_paned));
    toolbar_view.set_content(Some(&main_paned));

    // Track paned positions for persistence
    {
        let cfg = config.clone();
        main_paned.connect_notify_local(Some("position"), move |paned, _| {
            cfg.borrow_mut().settings.paned_position = paned.position();
            cfg.borrow().save_settings();
        });
    }
    {
        let cfg = config.clone();
        right_paned.connect_notify_local(Some("position"), move |paned, _| {
            cfg.borrow_mut().settings.right_paned_position = paned.position();
            cfg.borrow().save_settings();
        });
    }

    // ── Status bar ──────────────────────────────────────────────────────
    let (
        status_label,
        progress_bar,
        progress_revealer,
        btn_start,
        btn_start_spinner,
        btn_start_label,
        btn_log,
    ) = build_status_bar(&toolbar_view, &state);

    toast_overlay.set_child(Some(&toolbar_view));
    window.set_content(Some(&toast_overlay));

    // ── Collect widget references ───────────────────────────────────────
    let alive = Rc::new(Cell::new(true));
    let widgets = Rc::new(RefCell::new(Widgets {
        file_browser,
        settings_panel,
        preview: preview_obj,
        path_label,
        breadcrumb_segments,
        btn_grid: btn_grid.clone(),
        btn_list: btn_list.clone(),
        status_label,
        progress_bar,
        progress_revealer,
        btn_start,
        btn_start_spinner,
        btn_start_label,
        btn_log,
        btn_home: btn_breadcrumb_home.clone(),
        btn_refresh: btn_breadcrumb_refresh.clone(),
        main_paned,
        right_paned,
        toast_overlay,
        search_entry,
        alive,
    }));

    // ── Connect FileBrowser signals ─────────────────────────────────────
    connect_file_browser_signals(&widgets, &state);

    // ── Connect view mode toggle ────────────────────────────────────────
    {
        let w = widgets.clone();
        let cfg = config.clone();
        btn_grid.connect_toggled(move |btn| {
            if btn.is_active() {
                w.borrow().file_browser.set_view_mode("grid");
                cfg.borrow_mut().settings.view_mode = "grid".to_string();
                cfg.borrow().save_settings();
            }
        });
    }
    {
        let w = widgets.clone();
        let cfg = config.clone();
        btn_list.connect_toggled(move |btn| {
            if btn.is_active() {
                w.borrow().file_browser.set_view_mode("list");
                cfg.borrow_mut().settings.view_mode = "list".to_string();
                cfg.borrow().save_settings();
            }
        });
    }

    // ── Connect setting-changed callback for API key pre-flight ────────
    {
        let sp = widgets.borrow().settings_panel.clone();
        let w = widgets.clone();
        let s = state.clone();
        sp.on_setting_changed(move |kind| match kind {
            super::settings_panel::SettingKind::Translator => {
                super::dialogs::check_api_key_status(&w, &s);
            }
            super::settings_panel::SettingKind::Language => {
                w.borrow().toast_overlay.add_toast(
                    adw::Toast::builder()
                        .title(&i18n::t("Sprache wird beim Neustart wirksam."))
                        .timeout(3)
                        .build(),
                );
            }
            _ => {}
        });
    }

    // ── Install GActions ────────────────────────────────────────────────
    {
        let w = widgets.clone();
        let s = state.clone();
        let btn_up = btn_breadcrumb_up.clone();
        btn_breadcrumb_up.connect_clicked(move |_| {
            trigger_nav_flash(&btn_up);
            let current = s.borrow().current_directory.clone();
            if let Some(parent) = current.parent() {
                open_directory_inner(&w, &s, parent);
            }
        });
    }

    // Refresh button — reload current directory + spin animation
    {
        let w = widgets.clone();
        let btn_ref = btn_breadcrumb_refresh.clone();
        btn_breadcrumb_refresh.connect_clicked(move |_| {
            trigger_refresh_spin(&btn_ref);
            w.borrow().file_browser.force_refresh();
        });
    }
    // ── Install GActions ────────────────────────────────────────────────
    install_actions(&window, &state, &widgets);

    // ── Save state on close ─────────────────────────────────────────────
    {
        let s = state.clone();
        let w = widgets.clone();
        window.connect_close_request(move |_| {
            w.borrow().alive.set(false);
            s.borrow().cancel_flag.store(true, Ordering::Relaxed);
            s.borrow().config.borrow().save_settings();
            glib::Propagation::Proceed
        });
    }

    // ── Restore saved state ─────────────────────────────────────────────
    apply_saved_state(&window, &widgets, &state);

    window
}

// ---------------------------------------------------------------------------
// Header bar
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn build_header_bar(
    toolbar_view: &adw::ToolbarView,
    _state: &Rc<RefCell<WindowState>>,
) -> gtk::Label {
    let header = adw::HeaderBar::new();

    // ── Path label (center) ─────────────────────────────────────────────
    let path_label = gtk::Label::new(None);
    path_label.set_ellipsize(pango::EllipsizeMode::Middle);
    path_label.add_css_class("title-4");
    path_label.set_hexpand(true);
    path_label.set_halign(gtk::Align::Center);
    header.set_title_widget(Some(&path_label));

    // ── Right side: gear menu ───────────────────────────────────────────
    let menu_button = gtk::MenuButton::new();
    menu_button.set_icon_name("emblem-system-symbolic");
    menu_button.set_tooltip_text(Some(&i18n::t("Menü")));
    let menu = build_menu_model();
    menu_button.set_menu_model(Some(&menu));

    header.pack_end(&menu_button);
    toolbar_view.add_top_bar(&header);

    path_label
}

// ---------------------------------------------------------------------------
// Menu model
// ---------------------------------------------------------------------------

fn build_menu_model() -> gio::MenuModel {
    let menu = gio::Menu::new();

    let file_section = gio::Menu::new();
    file_section.append(
        Some(&i18n::t("Verzeichnis öffnen…")),
        Some("win.open-directory-dialog"),
    );
    file_section.append(
        Some(&i18n::t("Manga-Startverzeichnis…")),
        Some("win.manga-home-dialog"),
    );
    menu.append_section(None, &file_section);

    let api_section = gio::Menu::new();
    api_section.append(Some(&i18n::t("API Schlüssel")), Some("win.api-keys"));
    api_section.append(
        Some(&i18n::t("Virtuelle Umgebung…")),
        Some("win.virtual-env-dialog"),
    );
    menu.append_section(None, &api_section);

    let accent_section = gio::Menu::new();
    accent_section.append(Some(&i18n::t("Akzentfarbe")), Some("win.accent-colors"));
    menu.append_section(None, &accent_section);

    let log_section = gio::Menu::new();
    log_section.append(Some(&i18n::t("Protokoll")), Some("win.log-viewer"));
    menu.append_section(None, &log_section);

    let shortcuts_section = gio::Menu::new();
    shortcuts_section.append(
        Some(&i18n::t("Tastenkürzel")),
        Some("win.keyboard-shortcuts"),
    );
    menu.append_section(None, &shortcuts_section);

    let about_section = gio::Menu::new();
    about_section.append(Some(&i18n::t("Über Manga Translator")), Some("win.about"));
    menu.append_section(None, &about_section);

    menu.into()
}

// ---------------------------------------------------------------------------
// File browser area
// ---------------------------------------------------------------------------

/// Build the left pane: search bar + Select All/Deselect + FileBrowser + sort.
fn build_file_browser_area(
    file_browser: &FileBrowser,
    config: &Rc<RefCell<ConfigManager>>,
) -> (
    gtk::Widget,
    gtk::SearchEntry,
    gtk::DropDown,
    gtk::Box,
    gtk::Button,
    gtk::Button,
    gtk::Button,
    gtk::ToggleButton,
    gtk::ToggleButton,
) {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.set_size_request(280, -1);

    // ── Breadcrumb navigation bar ──────────────────────────────────────
    let breadcrumb_bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    breadcrumb_bar.add_css_class("breadcrumb-box");
    breadcrumb_bar.set_margin_start(4);
    breadcrumb_bar.set_margin_end(4);
    breadcrumb_bar.set_margin_top(4);
    breadcrumb_bar.set_margin_bottom(2);

    let btn_bc_home = gtk::Button::from_icon_name("go-home-symbolic");
    btn_bc_home.add_css_class("flat");
    btn_bc_home.set_tooltip_text(Some(&i18n::t("Startverzeichnis")));
    use gtk::prelude::ActionableExt;
    btn_bc_home.set_action_name(Some("win.navigate-home"));

    let btn_bc_up = gtk::Button::from_icon_name("go-up-symbolic");
    btn_bc_up.add_css_class("flat");
    btn_bc_up.set_tooltip_text(Some(&i18n::t("Übergeordnetes Verzeichnis")));

    let btn_bc_refresh = gtk::Button::new();
    btn_bc_refresh.add_css_class("flat");
    btn_bc_refresh.set_tooltip_text(Some(&i18n::t("Aktualisieren")));
    btn_bc_refresh.set_child(Some(&gtk::Image::from_icon_name("view-refresh-symbolic")));

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
    scrolled.set_hexpand(true);
    scrolled.set_vexpand(false);
    let breadcrumb_segments = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    breadcrumb_segments.set_valign(gtk::Align::Center);
    scrolled.set_child(Some(&breadcrumb_segments));

    breadcrumb_bar.append(&btn_bc_home);
    breadcrumb_bar.append(&btn_bc_up);
    breadcrumb_bar.append(&btn_bc_refresh);
    breadcrumb_bar.append(&scrolled);
    vbox.append(&breadcrumb_bar);

    // Search bar
    let search_bar = gtk::SearchBar::new();
    let search_entry = gtk::SearchEntry::new();
    search_entry.set_hexpand(true);
    search_entry.set_placeholder_text(Some(&i18n::t("Dateien suchen…")));
    search_bar.set_child(Some(&search_entry));
    search_bar.connect_entry(&search_entry);
    vbox.append(&search_bar);

    // Select All / Deselect buttons row
    let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    action_row.set_margin_start(8);
    action_row.set_margin_end(8);
    action_row.set_margin_top(4);
    action_row.set_margin_bottom(4);

    let btn_select_all = gtk::Button::with_label(&i18n::t("Alle auswählen"));
    btn_select_all.add_css_class("flat");
    btn_select_all.set_hexpand(true);

    let btn_deselect = gtk::Button::with_label(&i18n::t("Auswahl aufheben"));
    btn_deselect.add_css_class("flat");
    btn_deselect.set_hexpand(true);

    // View toggle (grid / list)
    let view_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    view_box.add_css_class("linked");

    let btn_grid = gtk::ToggleButton::new();
    btn_grid.set_icon_name("view-grid-symbolic");
    btn_grid.set_tooltip_text(Some(&i18n::t("Rasteransicht")));
    btn_grid.set_active(true);
    btn_grid.add_css_class("view-toggle");
    btn_grid.add_css_class("flat");

    let btn_list = gtk::ToggleButton::new();
    btn_list.set_icon_name("view-list-symbolic");
    btn_list.set_tooltip_text(Some(&i18n::t("Listenansicht")));
    btn_list.set_group(Some(&btn_grid));
    btn_list.add_css_class("view-toggle");
    btn_list.add_css_class("flat");

    view_box.append(&btn_grid);
    view_box.append(&btn_list);

    action_row.append(&btn_select_all);
    action_row.append(&btn_deselect);
    action_row.append(&view_box);
    vbox.append(&action_row);

    // FileBrowser widget
    file_browser.set_vexpand(true);
    file_browser.set_hexpand(true);
    vbox.append(file_browser);

    // Sort dropdown at the bottom
    let sort_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    sort_row.set_margin_start(8);
    sort_row.set_margin_end(8);
    sort_row.set_margin_top(4);
    sort_row.set_margin_bottom(4);

    let sort_label = gtk::Label::new(Some(&i18n::t("Sortierung:")));
    sort_label.add_css_class("caption");

    let sort_model = gtk::StringList::new(config::options::SORT_METHODS);
    let sort_dropdown = gtk::DropDown::builder()
        .model(&sort_model)
        .selected(config.borrow().settings.sort_method)
        .hexpand(true)
        .build();

    sort_row.append(&sort_label);
    sort_row.append(&sort_dropdown);
    vbox.append(&sort_row);

    // ── Signal connections ──────────────────────────────────────────────

    let fb = file_browser.clone();
    search_entry.connect_search_changed(move |entry| {
        fb.set_search(&entry.text().to_string());
    });

    let fb = file_browser.clone();
    btn_select_all.connect_clicked(move |_| {
        fb.select_all();
    });

    let fb = file_browser.clone();
    btn_deselect.connect_clicked(move |_| {
        fb.deselect_all();
    });

    let fb = file_browser.clone();
    let cfg = config.clone();
    sort_dropdown.connect_notify_local(Some("selected"), move |dropdown, _| {
        let idx = dropdown.selected();
        fb.set_sort_method(idx);
        cfg.borrow_mut().settings.sort_method = idx;
        cfg.borrow().save_settings();
    });

    (
        vbox.upcast(),
        search_entry,
        sort_dropdown,
        breadcrumb_segments,
        btn_bc_home,
        btn_bc_up,
        btn_bc_refresh,
        btn_grid,
        btn_list,
    )
}

// ---------------------------------------------------------------------------
// Preview pane
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
// build_preview_pane() removed — replaced by the Preview GObject (see preview.rs).

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------
#[allow(clippy::type_complexity)]
fn build_status_bar(
    toolbar_view: &adw::ToolbarView,
    _state: &Rc<RefCell<WindowState>>,
) -> (
    gtk::Label,
    gtk::ProgressBar,
    gtk::Revealer,
    gtk::Button,
    gtk::Spinner,
    gtk::Label,
    gtk::Button,
) {
    let action_bar = gtk::ActionBar::new();
    action_bar.add_css_class("status-bar");

    let status_label = gtk::Label::new(Some(&i18n::t("Bereit")));
    status_label.set_hexpand(true);
    status_label.set_xalign(0.0);
    status_label.add_css_class("caption");
    status_label.set_margin_start(12);
    action_bar.pack_start(&status_label);

    let progress_bar = gtk::ProgressBar::new();
    progress_bar.set_fraction(0.0);
    progress_bar.set_show_text(false);
    progress_bar.set_hexpand(true);
    progress_bar.add_css_class("translation-progress");

    let progress_revealer = gtk::Revealer::new();
    progress_revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
    progress_revealer.set_transition_duration(300);
    progress_revealer.set_reveal_child(false);
    progress_revealer.set_child(Some(&progress_bar));
    // Progress bar will be placed in its own row above the action bar
    // for proper vertical centering and spacing from buttons
    progress_revealer.set_margin_top(4);
    progress_revealer.set_margin_bottom(2);
    progress_revealer.set_margin_start(12);
    progress_revealer.set_margin_end(12);

    // ── Log button ──────────────────────────────────────────────────────
    let btn_log = gtk::Button::new();
    btn_log.set_icon_name("document-open-recent-symbolic");
    btn_log.set_tooltip_text(Some(&i18n::t("Protokoll anzeigen")));
    btn_log.add_css_class("flat");
    btn_log.set_valign(gtk::Align::Center);

    // ── Start button with spinner + label ───────────────────────────────
    let btn_start = gtk::Button::new();
    btn_start.add_css_class("start-action-btn");
    btn_start.add_css_class("suggested-action");

    let btn_start_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let btn_start_spinner = gtk::Spinner::new();
    btn_start_spinner.set_visible(false);
    let btn_start_label = gtk::Label::new(Some(&i18n::t("Übersetzen")));
    btn_start_content.append(&btn_start_spinner);
    btn_start_content.append(&btn_start_label);
    btn_start.set_child(Some(&btn_start_content));

    // Pack order: log, start (right to left with pack_end)
    action_bar.pack_end(&btn_log);
    action_bar.pack_end(&btn_start);

    // Place progress bar in its own row above the action bar
    let bottom_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    bottom_box.append(&progress_revealer);
    bottom_box.append(&action_bar);
    toolbar_view.add_bottom_bar(&bottom_box);

    (
        status_label,
        progress_bar,
        progress_revealer,
        btn_start,
        btn_start_spinner,
        btn_start_label,
        btn_log,
    )
}

// ---------------------------------------------------------------------------
// FileBrowser signal connections
// ---------------------------------------------------------------------------

fn connect_file_browser_signals(widgets: &Rc<RefCell<Widgets>>, state: &Rc<RefCell<WindowState>>) {
    // Selection changed → update preview + status bar
    let w = widgets.clone();
    widgets
        .borrow()
        .file_browser
        .on_selection_changed(move |selection| {
            if !w.borrow().alive.get() {
                return;
            }

            let files = selection.all_selected_files();

            // Update preview
            if files.len() == 1 {
                w.borrow().preview.load_original(&files[0]);
            } else if files.len() > 1 {
                // Multiple files — show first image as preview
                w.borrow().preview.load_original(&files[0]);
            } else {
                w.borrow().preview.clear();
            }

            // Update status bar
            let count = files.len();
            if count > 0 {
                w.borrow().status_label.set_label(&format!(
                    "{} {} {}",
                    count,
                    i18n::t("Dateien"),
                    i18n::t("ausgewählt")
                ));
            } else {
                w.borrow().status_label.set_label(&i18n::t("Bereit"));
            }
        });

    // Folder activated → navigate into
    let w = widgets.clone();
    let s = state.clone();
    widgets
        .borrow()
        .file_browser
        .on_folder_activated(move |path| {
            open_directory_inner(&w, &s, path);
        });

    // Drag & drop — navigate to dropped folder/file parent
    {
        let w = widgets.clone();
        let s = state.clone();
        widgets.borrow().file_browser.on_files_dropped(move |path| {
            open_directory_inner(&w, &s, path);
        });
    }
}

// ---------------------------------------------------------------------------
// GActions
// ---------------------------------------------------------------------------

fn install_actions(
    window: &adw::ApplicationWindow,
    state: &Rc<RefCell<WindowState>>,
    widgets: &Rc<RefCell<Widgets>>,
) {
    // ── Open Directory Dialog ───────────────────────────────────────────
    let open_dir_action = gio::SimpleAction::new("open-directory-dialog", None);
    open_dir_action.connect_activate(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        #[weak]
        window,
        move |_, _| {
            let current = state.borrow().current_directory.clone();
            let w = widgets.clone();
            let s = state.clone();
            super::dialogs::show_open_directory_dialog(
                &window,
                current,
                Box::new(move |path| open_directory_inner(&w, &s, path)),
            );
        }
    ));
    window.add_action(&open_dir_action);

    // ── Log Viewer action ───────────────────────────────────────────────
    let log_viewer_action = gio::SimpleAction::new("log-viewer", None);
    log_viewer_action.connect_activate(clone!(
        #[strong]
        state,
        #[weak]
        window,
        move |_, _| {
            super::dialogs::show_log_viewer_dialog(&window, &state);
        }
    ));
    window.add_action(&log_viewer_action);

    // ── API Keys action ─────────────────────────────────────────────────
    let api_action = gio::SimpleAction::new("api-keys", None);
    api_action.connect_activate(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        #[weak]
        window,
        move |_, _| {
            let config = state.borrow().config.clone();
            let w = widgets.clone();
            let s = state.clone();
            super::dialogs::show_api_keys_dialog(
                &window,
                config,
                Box::new(move || super::dialogs::check_api_key_status(&w, &s)),
            );
        }
    ));
    window.add_action(&api_action);

    // ── Virtual Environment action ──────────────────────────────────────
    let venv_action = gio::SimpleAction::new("virtual-env-dialog", None);
    venv_action.connect_activate(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        #[weak]
        window,
        move |_, _| {
            let config = state.borrow().config.clone();
            let bridge = state.borrow().bridge.clone();
            let w = widgets.clone();
            let s = state.clone();
            super::dialogs::show_virtual_env_dialog(
                &window,
                config,
                bridge,
                Box::new(move || {
                    // Re-configure bridge with updated paths
                    let s_ref = s.borrow();
                    let cfg = s_ref.config.borrow();
                    let site_packages = cfg.resolve_venv_site_packages();
                    let manga_dir = cfg.manga_translator_dir();
                    drop(cfg);
                    drop(s_ref);
                    s.borrow().bridge.configure_paths(site_packages, manga_dir);
                    super::dialogs::check_api_key_status(&w, &s);
                }),
            );
        }
    ));
    window.add_action(&venv_action);

    // ── Accent colors action ────────────────────────────────────────────
    let accent_action = gio::SimpleAction::new("accent-colors", None);
    accent_action.connect_activate(clone!(
        #[strong]
        state,
        #[weak]
        window,
        move |_, _| {
            let config = state.borrow().config.clone();
            let state_clone = state.clone();
            super::dialogs::show_accent_color_dialog(
                &window,
                config,
                Box::new(move |name| apply_accent_color(&state_clone, name)),
            );
        }
    ));
    window.add_action(&accent_action);

    // ── About action ────────────────────────────────────────────────────
    let about_action = gio::SimpleAction::new("about", None);
    {
        let window_clone = window.clone();
        about_action.connect_activate(move |_, _| {
            super::dialogs::show_about_dialog(&window_clone);
        });
    }
    window.add_action(&about_action);

    // ── Translate action (triggered by start button) ────────────────────
    let translate_action = gio::SimpleAction::new("translate", None);
    translate_action.connect_activate(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        move |_, _| {
            start_translation(&widgets, &state);
        }
    ));
    window.add_action(&translate_action);

    // ── Navigate home action ────────────────────────────────────────────
    let home_action = gio::SimpleAction::new("navigate-home", None);
    home_action.connect_activate(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        move |_, _| {
            trigger_nav_flash(&widgets.borrow().btn_home);
            let config = state.borrow().config.clone();
            let home = config.borrow().manga_home_dir();
            open_directory_inner(&widgets, &state, &home);
        }
    ));
    window.add_action(&home_action);

    // ── Manga home directory dialog ─────────────────────────────────────
    let manga_home_action = gio::SimpleAction::new("manga-home-dialog", None);
    manga_home_action.connect_activate(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        #[weak]
        window,
        move |_, _| {
            let config = state.borrow().config.clone();
            let w = widgets.clone();
            let s = state.clone();
            super::dialogs::show_manga_home_dialog(
                &window,
                config,
                Box::new(move |path| open_directory_inner(&w, &s, &path)),
            );
        }
    ));
    window.add_action(&manga_home_action);

    // ── Refresh action ──────────────────────────────────────────────────
    let refresh_action = gio::SimpleAction::new("refresh", None);
    refresh_action.connect_activate(clone!(
        #[strong]
        widgets,
        move |_, _| {
            trigger_refresh_spin(&widgets.borrow().btn_refresh);
            widgets.borrow().file_browser.force_refresh();
        }
    ));
    window.add_action(&refresh_action);

    // ── Open-directory (with path parameter, for "open with…") ──────────
    let open_dir_path_action =
        gio::SimpleAction::new("open-directory", Some(&glib::VariantTy::STRING));
    open_dir_path_action.connect_activate(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        move |_, param| {
            if let Some(variant) = param {
                let path_str = variant.get::<String>().unwrap_or_default();
                let path = PathBuf::from(&path_str);
                if path.is_dir() {
                    open_directory_inner(&widgets, &state, &path);
                }
            }
        }
    ));
    window.add_action(&open_dir_path_action);

    // Connect start button — acts as both "Translate" and "Cancel"
    {
        let s = state.clone();
        let w = widgets.clone();
        widgets.borrow().btn_start.connect_clicked(move |_| {
            if s.borrow().is_processing {
                s.borrow().cancel_flag.store(true, Ordering::Relaxed);
                log::info!("Translation cancellation requested");
            } else {
                start_translation(&w, &s);
            }
        });
    }

    // Connect log button
    {
        let s = state.clone();
        widgets.borrow().btn_log.connect_clicked(clone!(
            #[weak]
            window,
            #[strong]
            s,
            move |_| {
                super::dialogs::show_log_viewer_dialog(&window, &s);
            }
        ));
    }

    // ── Select All action ───────────────────────────────────────────────
    let select_all_action = gio::SimpleAction::new("select-all", None);
    select_all_action.connect_activate(clone!(
        #[strong]
        widgets,
        move |_, _| {
            widgets.borrow().file_browser.select_all();
        }
    ));
    window.add_action(&select_all_action);

    // ── Deselect All action ─────────────────────────────────────────────
    let deselect_all_action = gio::SimpleAction::new("deselect-all", None);
    deselect_all_action.connect_activate(clone!(
        #[strong]
        widgets,
        move |_, _| {
            widgets.borrow().file_browser.deselect_all();
        }
    ));
    window.add_action(&deselect_all_action);

    // ── Focus Search action ─────────────────────────────────────────────
    let focus_search_action = gio::SimpleAction::new("focus-search", None);
    focus_search_action.connect_activate(clone!(
        #[strong]
        widgets,
        move |_, _| {
            widgets.borrow().search_entry.grab_focus();
        }
    ));
    window.add_action(&focus_search_action);

    // ── Cancel Translation action (Escape) ──────────────────────────────
    let cancel_translation_action = gio::SimpleAction::new("cancel-translation", None);
    cancel_translation_action.connect_activate(clone!(
        #[strong]
        state,
        move |_, _| {
            if state.borrow().is_processing {
                state.borrow().cancel_flag.store(true, Ordering::Relaxed);
                log::info!("Translation cancellation requested (Escape)");
            }
        }
    ));
    window.add_action(&cancel_translation_action);

    // ── Navigate Back action (Alt+Left) ─────────────────────────────────
    let navigate_back_action = gio::SimpleAction::new("navigate-back", None);
    navigate_back_action.connect_activate(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        move |_, _| {
            let current = state.borrow().current_directory.clone();
            if let Some(parent) = current.parent() {
                open_directory_inner(&widgets, &state, parent);
            }
        }
    ));
    window.add_action(&navigate_back_action);

    // ── Grid View action ────────────────────────────────────────────────
    let grid_view_action = gio::SimpleAction::new("grid-view", None);
    grid_view_action.connect_activate(clone!(
        #[strong]
        widgets,
        move |_, _| {
            widgets.borrow().btn_grid.set_active(true);
        }
    ));
    window.add_action(&grid_view_action);

    // ── List View action ────────────────────────────────────────────────
    let list_view_action = gio::SimpleAction::new("list-view", None);
    list_view_action.connect_activate(clone!(
        #[strong]
        widgets,
        move |_, _| {
            widgets.borrow().btn_list.set_active(true);
        }
    ));
    window.add_action(&list_view_action);

    // ── Toggle Settings Panel action (F9) ───────────────────────────────
    let toggle_settings_action = gio::SimpleAction::new("toggle-settings", None);
    toggle_settings_action.connect_activate(clone!(
        #[strong]
        widgets,
        move |_, _| {
            let visible = widgets.borrow().settings_panel.is_visible();
            widgets.borrow().settings_panel.set_visible(!visible);
        }
    ));
    window.add_action(&toggle_settings_action);

    // ── Keyboard Shortcuts Dialog action ────────────────────────────────
    let shortcuts_action = gio::SimpleAction::new("keyboard-shortcuts", None);
    shortcuts_action.connect_activate(clone!(
        #[weak]
        window,
        move |_, _| {
            super::dialogs::show_shortcuts_dialog(&window);
        }
    ));
    window.add_action(&shortcuts_action);

    // ── Keyboard Shortcut Controller ────────────────────────────────────
    let controller = gtk::ShortcutController::new();
    controller.set_scope(gtk::ShortcutScope::Global);

    let shortcuts_data: &[(&str, &str)] = &[
        ("<Control>o", "win.open-directory-dialog"),
        ("<Control>r", "win.refresh"),
        ("F5", "win.refresh"),
        ("<Alt>Left", "win.navigate-back"),
        ("<Control>t", "win.translate"),
        ("Escape", "win.cancel-translation"),
        ("<Control>a", "win.select-all"),
        ("<Control><Shift>a", "win.deselect-all"),
        ("<Control>l", "win.focus-search"),
        ("<Control>k", "win.api-keys"),
        ("F9", "win.toggle-settings"),
        ("<Control>1", "win.grid-view"),
        ("<Control>2", "win.list-view"),
    ];

    for &(trigger_str, action_name) in shortcuts_data {
        if let Some(trigger) = gtk::ShortcutTrigger::parse_string(trigger_str) {
            let action = gtk::NamedAction::new(action_name);
            let shortcut = gtk::Shortcut::new(Some(trigger), Some(action));
            controller.add_shortcut(shortcut);
        }
    }

    window.add_controller(controller);
}

// ---------------------------------------------------------------------------
// Open Directory dialog
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Keyboard shortcuts dialog
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Directory navigation
// ---------------------------------------------------------------------------

/// Open a directory in the file browser and update the path label.
fn open_directory_inner(
    widgets: &Rc<RefCell<Widgets>>,
    state: &Rc<RefCell<WindowState>>,
    path: &std::path::Path,
) {
    log::debug!("open_directory_inner({})", path.display());
    if !path.is_dir() {
        log::warn!("open_directory_inner: not a directory: {}", path.display());
        return;
    }

    // Update FileBrowser
    widgets.borrow().file_browser.set_directory(path);

    // Update state (for persistence)
    state.borrow_mut().current_directory = path.to_path_buf();
    {
        let cfg = state.borrow().config.clone();
        cfg.borrow_mut().settings.last_directory = path.to_string_lossy().to_string();
        cfg.borrow().save_settings();
    }

    // Update path label
    let display = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path.to_str().unwrap_or("?"));
    widgets.borrow().path_label.set_label(display);

    // Update breadcrumb navigation
    update_breadcrumb(widgets, state);

    log::info!("Opened directory: {}", path.display());
}

fn update_breadcrumb(widgets: &Rc<RefCell<Widgets>>, state: &Rc<RefCell<WindowState>>) {
    let segments_box = widgets.borrow().breadcrumb_segments.clone();
    // Clear existing segments
    while let Some(child) = segments_box.first_child() {
        segments_box.remove(&child);
    }

    let current = state.borrow().current_directory.clone();
    // Build segments from current directory up to root
    let mut path_parts: Vec<(String, PathBuf)> = Vec::new();
    let mut p = current.as_path();
    while let Some(parent) = p.parent() {
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            path_parts.push((name.to_string(), p.to_path_buf()));
        }
        p = parent;
    }
    path_parts.reverse();

    let w = widgets.clone();
    let s = state.clone();
    for (i, (name, path)) in path_parts.iter().enumerate() {
        if i > 0 {
            let sep = gtk::Label::new(Some("›"));
            sep.add_css_class("breadcrumb-separator");
            segments_box.append(&sep);
        }
        let btn = gtk::Button::with_label(name);
        btn.add_css_class("flat");
        btn.add_css_class("breadcrumb-btn");
        let is_last = i == path_parts.len() - 1;
        if is_last {
            btn.add_css_class("heading");
        }
        let path_clone = path.clone();
        let w2 = w.clone();
        let s2 = s.clone();
        btn.connect_clicked(move |_| {
            open_directory_inner(&w2, &s2, &path_clone);
        });
        segments_box.append(&btn);
    }
}

// ---------------------------------------------------------------------------
// Translation
// ---------------------------------------------------------------------------

fn start_translation(widgets: &Rc<RefCell<Widgets>>, state: &Rc<RefCell<WindowState>>) {
    let mut files = widgets.borrow().file_browser.selected_files();
    if files.is_empty() {
        widgets.borrow().toast_overlay.add_toast(
            adw::Toast::builder()
                .title(&i18n::t("Keine Dateien ausgewählt"))
                .timeout(3)
                .build(),
        );
        return;
    }

    if state.borrow().is_processing {
        return;
    }

    // ── API Key pre-flight gate ──────────────────────────────────────────
    {
        let idx = widgets.borrow().settings_panel.translator_index() as usize;
        let required = config::api_required_services();
        if let Some(service) = required.get(&idx) {
            let config = state.borrow().config.clone();
            let cfg = config.borrow();
            let has_key = match *service {
                "baidu" => {
                    !cfg.api_keys.baidu_app_id.is_empty()
                        && !cfg.api_keys.baidu_secret_key.is_empty()
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

            if !has_key {
                let name = super::dialogs::service_display_name(service);
                let msg = format!("{}: {}", i18n::t("API-Schlüssel fehlt"), name);
                let detail = format!(
                    "{}\n{}",
                    i18n::t("Der ausgewählte Übersetzer benötigt einen API-Schlüssel."),
                    i18n::t("Bitte trage den Schlüssel in den API-Einstellungen ein.")
                );
                let alert = gtk::AlertDialog::builder()
                    .message(&msg)
                    .detail(&detail)
                    .modal(true)
                    .build();
                alert.set_buttons(&[&i18n::t("Abbrechen"), &i18n::t("API Schlüssel eintragen")]);
                alert.set_cancel_button(0);
                alert.set_default_button(1);

                // Get parent window from the widget hierarchy
                let parent = widgets
                    .borrow()
                    .btn_start
                    .root()
                    .and_then(|r| r.downcast::<adw::ApplicationWindow>().ok());
                if let Some(parent) = parent {
                    let parent_for_cb = parent.clone();
                    let w = widgets.clone();
                    let s = state.clone();
                    alert.choose(
                        Some(&parent),
                        gio::Cancellable::NONE,
                        clone!(
                            #[strong]
                            w,
                            #[strong]
                            s,
                            move |result| {
                                if let Ok(1) = result {
                                    let config = s.borrow().config.clone();
                                    let w_cb = w.clone();
                                    let s_cb = s.clone();
                                    super::dialogs::show_api_keys_dialog(
                                        &parent_for_cb,
                                        config,
                                        Box::new(move || {
                                            super::dialogs::check_api_key_status(&w_cb, &s_cb);
                                        }),
                                    );
                                }
                            }
                        ),
                    );
                } else {
                    // Fallback: show toast if we can't find the parent window
                    widgets.borrow().toast_overlay.add_toast(
                        adw::Toast::builder()
                            .title(&format!("{}: {}", i18n::t("API-Schlüssel fehlt"), name))
                            .timeout(5)
                            .build(),
                    );
                }
                return; // Block translation
            }
        }
    }

    state.borrow_mut().is_processing = true;
    state.borrow().cancel_flag.store(false, Ordering::Relaxed);

    // Disable UI and transform start button into cancel button
    {
        let w = widgets.borrow();
        w.settings_panel.set_sensitive(false);
        w.file_browser.set_sensitive(false);
        w.btn_start_spinner.stop();
        w.btn_start_spinner.set_visible(false);
        w.btn_start_label.set_label(&i18n::t("Abbrechen"));
        w.btn_start.remove_css_class("suggested-action");
        w.btn_start.add_css_class("destructive-action");
        w.btn_start.remove_css_class("processing");
        w.btn_start.add_css_class("warning");
        w.progress_revealer.set_reveal_child(true);
        w.progress_bar.set_fraction(0.0);
        w.progress_bar.add_css_class("active");
        w.status_label
            .set_label(&i18n::t("Übersetzung wird vorbereitet…"));
    }

    // Get translation parameters from SettingsPanel
    let params = widgets.borrow().settings_panel.build_translation_params();

    // Create async channel for thread-safe progress updates
    let (sender, receiver) = async_channel::unbounded::<TranslationMsg>();

    // Spawn async receiver on the main loop to update UI
    let w = widgets.clone();
    let s = state.clone();
    let alive = widgets.borrow().alive.clone();
    glib::spawn_future_local(async move {
        while let Ok(msg) = receiver.recv().await {
            if !alive.get() {
                break;
            }

            match msg {
                TranslationMsg::Progress(fraction, text) => {
                    {
                        let w = w.borrow();
                        w.progress_bar.set_fraction(fraction);
                        w.status_label.set_label(&text);
                    }
                    s.borrow_mut().log_entries.borrow_mut().push(text.clone());
                }
                TranslationMsg::Done(result) => {
                    // Re-enable UI
                    {
                        let w = w.borrow();
                        w.settings_panel.set_sensitive(true);
                        w.file_browser.set_sensitive(true);
                        w.btn_start_spinner.stop();
                        w.btn_start_spinner.set_visible(false);
                        w.btn_start_label.set_label(&i18n::t("Übersetzen"));
                        w.btn_start.add_css_class("suggested-action");
                        w.btn_start.remove_css_class("destructive-action");
                        w.btn_start.remove_css_class("processing");
                        w.btn_start.remove_css_class("warning");
                        w.progress_bar.remove_css_class("active");
                        w.progress_revealer.set_reveal_child(false);
                    }
                    s.borrow_mut().is_processing = false;

                    match result {
                        Ok(outputs) => {
                            let count = outputs.len();
                            let text = format!(
                                "{} {} {}",
                                i18n::t("Fertig!"),
                                count,
                                i18n::t("Dateien übersetzt")
                            );
                            {
                                let w = w.borrow();
                                w.status_label.set_label(&text);
                                w.btn_log.remove_css_class("log-btn-error");
                                w.progress_bar.set_fraction(1.0);
                                w.toast_overlay.add_toast(
                                    adw::Toast::builder().title(&text).timeout(5).build(),
                                );

                                // Load the first translated output into the preview
                                if let Some(ref first_output) = outputs.first() {
                                    w.preview.load_translated(first_output.as_path());
                                }
                                // Refresh file browser to show translated files
                                w.file_browser.force_refresh();
                            }
                            log::info!("Translation completed: {} files", count);
                        }
                        Err(e) => {
                            let is_cancelled = e.to_lowercase().contains("cancel");
                            let label = if is_cancelled {
                                i18n::t("Abgebrochen")
                            } else {
                                i18n::t("Übersetzung fehlgeschlagen")
                            };
                            s.borrow_mut().error_count += 1;
                            {
                                let w = w.borrow();
                                w.btn_log.add_css_class("log-btn-error");
                            }
                            s.borrow_mut().log_entries.borrow_mut().push(format!(
                                "{}: {}",
                                i18n::t("Fehler"),
                                e
                            ));
                            {
                                let w = w.borrow();
                                w.status_label.set_label(&label);
                                w.toast_overlay.add_toast(
                                    adw::Toast::builder()
                                        .title(&format!("{}: {}", i18n::t("Fehler"), e))
                                        .timeout(8)
                                        .build(),
                                );
                            }
                            log::error!("Translation failed: {}", e);
                        }
                    }
                    break;
                }
            }
        }
    });

    // Prepare data for the background thread
    let bridge = state.borrow().bridge.clone();
    let cancel_flag = state.borrow().cancel_flag.clone();
    let file_count = files.len();

    // Sort files with natural/alphanumeric order so they're processed
    // in order (e.g. 0001.png, 0002.png, 0003.png, ...) — not
    // lexicographic (0001, 0010, 0100). HashSet has no ordering.
    files
        .sort_by(|a, b| alphanumeric_sort::compare_str(&a.to_string_lossy(), &b.to_string_lossy()));

    log::info!("Starting translation of {} file(s)", file_count);

    // Spawn translation on a background thread
    std::thread::spawn(move || {
        let sender_clone = sender.clone();
        let result = bridge.translate(
            &files,
            &params,
            &cancel_flag,
            Box::new(move |progress: f64, msg: &str| {
                let _ = sender_clone.try_send(TranslationMsg::Progress(progress, msg.to_string()));
            }),
        );

        let msg = match result {
            Ok(outputs) => TranslationMsg::Done(Ok(outputs)),
            Err(e) => TranslationMsg::Done(Err(e.to_string())),
        };
        let _ = sender.try_send(msg);
    });
}

// ---------------------------------------------------------------------------
// API Key Pre-Flight Check
// ---------------------------------------------------------------------------

/// Check if the currently selected translator requires an API key and
/// update the start button accordingly (red pulsing warning vs green).

// ---------------------------------------------------------------------------
// API Keys dialog
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Accent color application
// ---------------------------------------------------------------------------

fn apply_accent_color(state: &Rc<RefCell<WindowState>>, name: &str) {
    // Build the accent color struct (preset or custom hex)
    let accent = config::find_preset(name).unwrap_or_else(|| {
        if name.starts_with('#') && name.len() == 7 {
            crate::config::AccentColor {
                name: name.to_string(),
                hex: name.to_string(),
                fg: crate::config::AccentColor::foreground_for(name),
            }
        } else {
            crate::config::AccentColor {
                name: "system".into(),
                hex: String::new(),
                fg: String::new(),
            }
        }
    });

    let css = crate::ui::css::accent_color_css(&accent);

    // Remove old provider
    if let Some(old_provider) = state.borrow_mut().accent_css_provider.take() {
        if let Some(display) = Display::default() {
            gtk::style_context_remove_provider_for_display(&display, &old_provider);
        }
    }

    if !css.is_empty() {
        let provider = gtk::CssProvider::new();
        provider.load_from_data(&css);
        if let Some(display) = Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
        }
        state.borrow_mut().accent_css_provider = Some(provider);
    }

    state.borrow_mut().accent_color = name.to_string();
    {
        let cfg = state.borrow().config.clone();
        cfg.borrow_mut().settings.accent_color = name.to_string();
        cfg.borrow().save_settings();
    }

    log::info!("Accent color set to: {}", name);
}

// ---------------------------------------------------------------------------
// State persistence
// ---------------------------------------------------------------------------

/// Trigger a one-shot flash animation on a breadcrumb navigation button.
/// Uses the same CSS class toggle pattern as count-bounce and selection-pop.
fn trigger_nav_flash(btn: &gtk::Button) {
    btn.remove_css_class("btn-nav-flash");
    let b = btn.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        b.add_css_class("btn-nav-flash");
        let b2 = b.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(700), move || {
            b2.remove_css_class("btn-nav-flash");
            glib::ControlFlow::Break
        });
        glib::ControlFlow::Break
    });
}

/// Trigger a spin animation by replacing the button icon with a Spinner for 1000 ms.
fn trigger_refresh_spin(btn: &gtk::Button) {
    // Prevent double-trigger while spinner is already active
    if let Some(child) = btn.child() {
        if child.downcast::<gtk::Spinner>().is_ok() {
            return;
        }
    }
    let spinner = gtk::Spinner::new();
    spinner.start();
    btn.set_child(Some(&spinner));

    let btn_clone = btn.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(1000), move || {
        spinner.stop();
        let image = gtk::Image::from_icon_name("view-refresh-symbolic");
        btn_clone.set_child(Some(&image));
        glib::ControlFlow::Break
    });
}

fn apply_saved_state(
    window: &adw::ApplicationWindow,
    widgets: &Rc<RefCell<Widgets>>,
    state: &Rc<RefCell<WindowState>>,
) {
    let settings = state.borrow().config.borrow().settings.clone();

    // Restore window size
    window.set_default_size(settings.window_width, settings.window_height);
    if settings.window_maximized {
        window.maximize();
    }

    // Restore accent color
    if settings.accent_color != "system" {
        let accent = settings.accent_color.clone();
        apply_accent_color(state, &accent);
    }

    // Restore color scheme (system / light / dark)
    let style_mgr = adw::StyleManager::default();
    match settings.color_scheme.as_str() {
        "light" => style_mgr.set_color_scheme(adw::ColorScheme::ForceLight),
        "dark" => style_mgr.set_color_scheme(adw::ColorScheme::ForceDark),
        _ => style_mgr.set_color_scheme(adw::ColorScheme::PreferLight),
    }

    // Restore view mode (including toggle button state)
    match settings.view_mode.as_str() {
        "list" => {
            widgets.borrow().file_browser.set_view_mode("list");
            widgets.borrow().btn_list.set_active(true);
        }
        _ => {
            widgets.borrow().file_browser.set_view_mode("grid");
            widgets.borrow().btn_grid.set_active(true);
        }
    }

    // Restore sort method
    widgets
        .borrow()
        .file_browser
        .set_sort_method(settings.sort_method);

    // Restore last directory
    let last_dir = settings.last_directory.clone();
    log::debug!("apply_saved_state: last_directory = {:?}", last_dir);
    if !last_dir.is_empty() {
        let path = PathBuf::from(&last_dir);
        if path.is_dir() {
            log::debug!("apply_saved_state: opening saved dir {}", path.display());
            open_directory_inner(widgets, state, &path);
        } else {
            log::warn!(
                "apply_saved_state: saved dir does not exist: {}",
                path.display()
            );
            let default_dir = ConfigManager::default_manga_dir();
            log::debug!(
                "apply_saved_state: falling back to default dir {}",
                default_dir.display()
            );
            open_directory_inner(widgets, state, &default_dir);
        }
    } else {
        let default_dir = ConfigManager::default_manga_dir();
        log::debug!(
            "apply_saved_state: no saved dir, using default {}",
            default_dir.display()
        );
        open_directory_inner(widgets, state, &default_dir);
    }

    // Check backend availability
    if state.borrow().bridge.is_backend_available() {
        log::info!("Python backend is available");
    } else {
        log::warn!("Python backend is NOT available — translation features disabled");
    }

    // Initial API key pre-flight check
    super::dialogs::check_api_key_status(widgets, state);
}

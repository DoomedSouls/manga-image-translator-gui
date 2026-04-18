// manga-translator-gtk/src/i18n.rs
//
// Minimal internationalization using gettext-rs.
// Provides a `_()` macro for translating UI strings.

use adw::prelude::*;
use gettextrs::{LocaleCategory, bindtextdomain, gettext, setlocale, textdomain};

use std::cell::RefCell;
use std::path::PathBuf;

/// Supported UI languages with their display names.
/// The first entry ("", "Automatisch") represents auto-detection.
#[allow(dead_code)]
pub const SUPPORTED_LANGUAGES: &[(&str, &str)] = &[
    ("", "Automatisch"),
    ("de", "Deutsch"),
    ("en", "English"),
    ("es", "Español"),
    ("fr", "Français"),
    ("it", "Italiano"),
    ("pt_BR", "Português (Brasil)"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("zh_CN", "简体中文"),
];

const TEXT_DOMAIN: &str = "manga-translator";

// ---------------------------------------------------------------------------
// I18n Registry — enables real-time language switching
// ---------------------------------------------------------------------------

thread_local! {
    /// Registered callbacks that re-apply translated strings to widgets.
    /// Each closure captures a weak widget reference and an i18n key.
    static REGISTRY: RefCell<Vec<Box<dyn Fn()>>> = RefCell::new(Vec::new());
}

/// Register a closure that will be called when the language changes.
/// The closure should update a widget's text using the current translation.
pub fn register(entry: Box<dyn Fn()>) {
    REGISTRY.with(|r| r.borrow_mut().push(entry));
}

/// Re-apply all registered translations. Called automatically by set_language().
pub fn retranslate_all() {
    REGISTRY.with(|r| {
        for f in r.borrow().iter() {
            f();
        }
    });
}

/// Register a gtk::Label for retranslation.
pub fn register_label(label: &gtk::Label, msgid: &'static str) {
    let weak = label.downgrade();
    register(Box::new(move || {
        if let Some(label) = weak.upgrade() {
            label.set_label(&t(msgid));
        }
    }));
}

/// Register a gtk::Button's label for retranslation.
pub fn register_button(button: &gtk::Button, msgid: &'static str) {
    let weak = button.downgrade();
    register(Box::new(move || {
        if let Some(btn) = weak.upgrade() {
            btn.set_label(&t(msgid));
        }
    }));
}

/// Register a gtk::ToggleButton's label for retranslation.
pub fn register_toggle(button: &gtk::ToggleButton, msgid: &'static str) {
    let weak = button.downgrade();
    register(Box::new(move || {
        if let Some(btn) = weak.upgrade() {
            btn.set_label(&t(msgid));
        }
    }));
}

/// Register an adw::PreferencesGroup title for retranslation.
pub fn register_group_title(group: &adw::PreferencesGroup, msgid: &'static str) {
    let weak = group.downgrade();
    register(Box::new(move || {
        if let Some(g) = weak.upgrade() {
            g.set_title(&t(msgid));
        }
    }));
}

/// Register an adw::ComboRow title (and optional subtitle) for retranslation.
pub fn register_combo_row(
    row: &adw::ComboRow,
    title_msgid: &'static str,
    subtitle_msgid: Option<&'static str>,
) {
    let weak = row.downgrade();
    let sub = subtitle_msgid.map(|s| s.to_string());
    register(Box::new(move || {
        if let Some(r) = weak.upgrade() {
            r.set_title(&t(title_msgid));
            if let Some(ref sub_id) = sub {
                r.set_subtitle(&t(sub_id));
            }
        }
    }));
}

/// Register an adw::ActionRow title (and optional subtitle) for retranslation.
pub fn register_action_row(
    row: &adw::ActionRow,
    title_msgid: &'static str,
    subtitle_msgid: Option<&'static str>,
) {
    let weak = row.downgrade();
    let sub = subtitle_msgid.map(|s| s.to_string());
    register(Box::new(move || {
        if let Some(r) = weak.upgrade() {
            r.set_title(&t(title_msgid));
            if let Some(ref sub_id) = sub {
                r.set_subtitle(&t(sub_id));
            }
        }
    }));
}

/// Register an adw::SwitchRow title (and optional subtitle) for retranslation.
pub fn register_switch_row(
    row: &adw::SwitchRow,
    title_msgid: &'static str,
    subtitle_msgid: Option<&'static str>,
) {
    let weak = row.downgrade();
    let sub = subtitle_msgid.map(|s| s.to_string());
    register(Box::new(move || {
        if let Some(r) = weak.upgrade() {
            r.set_title(&t(title_msgid));
            if let Some(ref sub_id) = sub {
                r.set_subtitle(&t(sub_id));
            }
        }
    }));
}

/// Register a gtk::Widget's tooltip for retranslation.
pub fn register_tooltip(widget: &impl IsA<gtk::Widget>, msgid: &'static str) {
    let weak = widget.downgrade();
    register(Box::new(move || {
        if let Some(w) = weak.upgrade() {
            w.set_tooltip_text(Some(&t(msgid)));
        }
    }));
}

/// Register a gtk::SearchEntry's placeholder for retranslation.
pub fn register_search_placeholder(entry: &gtk::SearchEntry, msgid: &'static str) {
    let weak = entry.downgrade();
    register(Box::new(move || {
        if let Some(e) = weak.upgrade() {
            e.set_placeholder_text(Some(&t(msgid)));
        }
    }));
}

/// Register an adw::EntryRow's title for retranslation.
pub fn register_entry_row(row: &adw::EntryRow, title_msgid: &'static str) {
    let weak = row.downgrade();
    register(Box::new(move || {
        if let Some(r) = weak.upgrade() {
            r.set_title(&t(title_msgid));
        }
    }));
}

/// Register an adw::ComboRow's dropdown items for retranslation.
///
/// When the language changes, creates a new `gtk::StringList` with translated
/// strings, sets it as the row's model, and restores the previously selected index.
pub fn register_combo_row_items(row: &adw::ComboRow, msgids: &'static [&'static str]) {
    let weak = row.downgrade();
    register(Box::new(move || {
        if let Some(r) = weak.upgrade() {
            let selected = r.selected();
            let translated: Vec<String> = msgids.iter().map(|m| t(m)).collect();
            let strs: Vec<&str> = translated.iter().map(|s| s.as_str()).collect();
            let new_model = gtk::StringList::new(&strs);
            r.set_model(Some(&new_model));
            r.set_selected(selected.min((msgids.len() - 1) as u32));
        }
    }));
}

/// Register a gtk::DropDown's items for retranslation.
///
/// When the language changes, creates a new `gtk::StringList` with translated
/// strings, sets it as the dropdown's model, and restores the previously selected index.
pub fn register_dropdown_items(dropdown: &gtk::DropDown, msgids: &'static [&'static str]) {
    let weak = dropdown.downgrade();
    register(Box::new(move || {
        if let Some(d) = weak.upgrade() {
            let selected = d.selected();
            let translated: Vec<String> = msgids.iter().map(|m| t(m)).collect();
            let strs: Vec<&str> = translated.iter().map(|s| s.as_str()).collect();
            let new_model = gtk::StringList::new(&strs);
            d.set_model(Some(&new_model));
            d.set_selected(selected.min((msgids.len() - 1) as u32));
        }
    }));
}

/// Initialize gettext with the locale directory.
///
/// Looks for translations in:
///   1. `<executable_dir>/../share/locale/`
///   2. `./po/`  (development mode)
///   3. System default paths
pub fn init() {
    setlocale(LocaleCategory::LcAll, "");

    // Try to find the locale directory
    if let Some(locale_dir) = find_locale_dir() {
        bindtextdomain(TEXT_DOMAIN, &locale_dir).ok();
    }

    textdomain(TEXT_DOMAIN).ok();

    log::info!("i18n initialized (domain: {})", TEXT_DOMAIN);
}

/// Set the UI language explicitly.
///
/// Uses the `LANGUAGE` environment variable, which is the primary mechanism
/// gettext uses to select translations. Unlike `setlocale("en.UTF-8")`, the
/// `LANGUAGE` variable works **independently of the system's installed locales**
/// — so it functions correctly even if only `de_CH.utf8` and `en_US.utf8` are
/// present on the system.
///
/// gettext resolves translations in this order:
///   1. `LANGUAGE` env var (our override)
///   2. `LC_ALL`, `LC_MESSAGES`, `LANG` env vars (system default)
#[allow(dead_code)]
pub fn set_language(lang: &str) {
    log::info!("set_language: switching to '{}'", lang);

    // Set LANGUAGE env var — this is what gettext actually reads to pick
    // translations. It accepts bare language codes like "en", "fr", "ja".
    // SAFETY: Setting the LANGUAGE env var is safe — it's a single-threaded
    // startup operation that only affects our own process's gettext lookups.
    unsafe {
        std::env::set_var("LANGUAGE", lang);
    }

    // Reset locale to system default. gettext needs a valid locale for
    // charset handling, but the translation *language* comes from LANGUAGE.
    setlocale(LocaleCategory::LcAll, "");

    // Re-bind text domain in case it was lost
    if let Some(locale_dir) = find_locale_dir() {
        let _ = bindtextdomain(TEXT_DOMAIN, &locale_dir);
    } else {
        log::error!("set_language: NO locale dir found — translations will not work!");
    }
    let _ = textdomain(TEXT_DOMAIN);

    // Verify translation works
    let test = gettext("Abbrechen");
    log::info!(
        "set_language: gettext('Abbrechen') = '{}' (expected: Cancel/Annuler/etc.)",
        test
    );

    // Update all registered widgets with new translations
    retranslate_all();

    log::info!("Language set to: {}", lang);
}

/// Translate a string using the current gettext domain.
///
/// Usage:
/// ```ignore
/// let label = t("Manga Translator");
/// ```
pub fn t(msgid: &str) -> String {
    gettext(msgid)
}

/// Attempt to locate the locale directory.
fn find_locale_dir() -> Option<String> {
    // 1. Compile-time path: CARGO_MANIFEST_DIR points to manga-translator-gtk/
    //    The locale/ directory is at the project root: ../locale/
    {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let project_locale = manifest_dir.join("..").join("locale");
        if project_locale.is_dir() {
            if let Ok(canonical) = project_locale.canonicalize() {
                log::info!(
                    "Locale dir found via CARGO_MANIFEST_DIR: {}",
                    canonical.display()
                );
                return Some(canonical.to_string_lossy().into_owned());
            }
        }
    }

    // 2. Relative to the executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            // Windows: locale/ next to the executable (portable install)
            #[cfg(target_os = "windows")]
            {
                let win_locale = exe_dir.join("locale");
                if win_locale.is_dir() {
                    if let Ok(canonical) = win_locale.canonicalize() {
                        log::info!("Locale dir found next to exe: {}", canonical.display());
                        return Some(canonical.to_string_lossy().into_owned());
                    }
                }
                // Windows installed: <prefix>\bin\ → <prefix>\share\locale\
                let share_locale = exe_dir.join("..").join("share").join("locale");
                if share_locale.is_dir() {
                    if let Ok(canonical) = share_locale.canonicalize() {
                        return Some(canonical.to_string_lossy().into_owned());
                    }
                }
            }
            // Linux installed: <prefix>/bin/ → <prefix>/share/locale/
            #[cfg(not(target_os = "windows"))]
            {
                let share_locale = exe_dir.join("..").join("share").join("locale");
                if share_locale.is_dir() {
                    if let Ok(canonical) = share_locale.canonicalize() {
                        return Some(canonical.to_string_lossy().into_owned());
                    }
                }
            }
            // Development: target/release/ → ../../locale/ (both platforms)
            let dev_locale = exe_dir.join("..").join("..").join("locale");
            if dev_locale.is_dir() {
                if let Ok(canonical) = dev_locale.canonicalize() {
                    return Some(canonical.to_string_lossy().into_owned());
                }
            }
        }
    }

    // 3. Current working directory — ./locale/ and ./po/
    for candidate in &["locale", "po"] {
        let path = PathBuf::from(candidate);
        if path.is_dir() {
            return Some(path.to_string_lossy().into_owned());
        }
    }

    // 4. Platform-specific data directories
    #[cfg(target_os = "windows")]
    {
        // Windows: %APPDATA%\manga-translator\locale
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let appdata_locale = PathBuf::from(appdata)
                .join("manga-translator")
                .join("locale");
            if appdata_locale.is_dir() {
                return Some(appdata_locale.to_string_lossy().into_owned());
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Linux: XDG data directories
        if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
            let xdg_locale = PathBuf::from(data_home).join("locale");
            if xdg_locale.is_dir() {
                return Some(xdg_locale.to_string_lossy().into_owned());
            }
        }
    }

    // 5. Fallback — let gettext use its compiled-in default path
    log::warn!("No locale directory found — translations will not be available");
    None
}

/// Get the display name for a language code.
#[allow(dead_code)]
pub fn get_language_display_name(code: &str) -> String {
    SUPPORTED_LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| code.to_string())
}

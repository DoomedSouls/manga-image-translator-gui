// manga-translator-gtk/src/i18n.rs
//
// Minimal internationalization using gettext-rs.
// Provides a `_()` macro for translating UI strings.

use gettextrs::{LocaleCategory, bindtextdomain, gettext, setlocale, textdomain};
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
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("zh_CN", "简体中文"),
    ("pt_BR", "Português (Brasil)"),
];

const TEXT_DOMAIN: &str = "manga-translator";

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
            // Installed mode: <prefix>/bin/ → <prefix>/share/locale/
            let share_locale = exe_dir.join("..").join("share").join("locale");
            if share_locale.is_dir() {
                if let Ok(canonical) = share_locale.canonicalize() {
                    return Some(canonical.to_string_lossy().into_owned());
                }
            }
            // Development: target/release/ → ../../locale/
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

    // 4. XDG data directories
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        let xdg_locale = PathBuf::from(data_home).join("locale");
        if xdg_locale.is_dir() {
            return Some(xdg_locale.to_string_lossy().into_owned());
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

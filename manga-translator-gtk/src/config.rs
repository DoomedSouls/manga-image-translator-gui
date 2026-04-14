// manga-translator-gtk/src/config.rs
//
// Configuration management: settings persistence, accent colors,
// API keys, and window state.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Accent color definitions
// ---------------------------------------------------------------------------

/// A named accent color with its foreground color for contrast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccentColor {
    pub name: String,
    pub hex: String,
    pub fg: String,
}

impl AccentColor {
    /// Calculate a foreground color (white or black) based on luminance.
    pub fn foreground_for(hex: &str) -> String {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(128) as f64;
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(128) as f64;
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(128) as f64;
        // ITU-R BT.709 relative luminance
        let luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
        if luminance > 0.5 {
            "#1a1a1a".to_string()
        } else {
            "#ffffff".to_string()
        }
    }

    /// Generate the CSS override string for this accent color.
    pub fn to_css(&self) -> String {
        if self.hex.is_empty() {
            return String::new();
        }
        format!(
            r#"@define-color accent_color {accent};
@define-color accent_bg_color {accent};
@define-color accent_fg_color {fg};
"#,
            accent = self.hex,
            fg = self.fg,
        )
    }
}

/// Built-in accent color presets, matching the Python GUI.
pub fn accent_presets() -> Vec<AccentColor> {
    vec![
        AccentColor {
            name: "system".into(),
            hex: String::new(),
            fg: String::new(),
        },
        AccentColor {
            name: "blue".into(),
            hex: "#3584e4".into(),
            fg: "#ffffff".into(),
        },
        AccentColor {
            name: "purple".into(),
            hex: "#9141ac".into(),
            fg: "#ffffff".into(),
        },
        AccentColor {
            name: "teal".into(),
            hex: "#0d9488".into(),
            fg: "#ffffff".into(),
        },
        AccentColor {
            name: "green".into(),
            hex: "#2ec27e".into(),
            fg: "#ffffff".into(),
        },
        AccentColor {
            name: "yellow".into(),
            hex: "#c6a300".into(),
            fg: "#1a1a1a".into(),
        },
        AccentColor {
            name: "orange".into(),
            hex: "#ff7800".into(),
            fg: "#ffffff".into(),
        },
        AccentColor {
            name: "red".into(),
            hex: "#e01b24".into(),
            fg: "#ffffff".into(),
        },
        AccentColor {
            name: "pink".into(),
            hex: "#d6336c".into(),
            fg: "#ffffff".into(),
        },
    ]
}

/// Get the translated display name for an accent preset.
pub fn accent_display_name(name: &str) -> String {
    crate::i18n::t(match name {
        "system" => "System",
        "blue" => "Blau",
        "purple" => "Lila",
        "teal" => "Blaugrün",
        "green" => "Grün",
        "yellow" => "Gelb",
        "orange" => "Orange",
        "red" => "Rot",
        "pink" => "Rosa",
        _ => name,
    })
}

/// Find an accent preset by name.
pub fn find_preset(name: &str) -> Option<AccentColor> {
    accent_presets().into_iter().find(|p| p.name == name)
}

// ---------------------------------------------------------------------------
// API keys
// ---------------------------------------------------------------------------

/// API key storage for translation services.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiKeys {
    #[serde(default)]
    pub deepl: String,
    #[serde(default)]
    pub openai: String,
    #[serde(default)]
    pub gemini: String,
    #[serde(default)]
    pub deepseek: String,
    #[serde(default)]
    pub groq: String,
    #[serde(default)]
    pub openrouter: String,
    #[serde(default)]
    pub baidu_app_id: String,
    #[serde(default)]
    pub baidu_secret_key: String,
    #[serde(default)]
    pub caiyun_token: String,
}

/// Services that require API keys, mapped to their dropdown index.
#[allow(dead_code)]
pub fn api_required_services() -> HashMap<usize, &'static str> {
    let mut m = HashMap::new();
    m.insert(1, "deepl");
    m.insert(3, "baidu");
    m.insert(5, "caiyun");
    m.insert(6, "openai");
    m.insert(7, "deepseek");
    m.insert(8, "groq");
    m.insert(9, "gemini");
    m
}

// ---------------------------------------------------------------------------
// Settings model (persisted to JSON)
// ---------------------------------------------------------------------------

/// Persistent application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    // Window geometry
    #[serde(default = "default_width")]
    pub window_width: i32,
    #[serde(default = "default_height")]
    pub window_height: i32,
    #[serde(default)]
    pub window_maximized: bool,
    #[serde(default = "default_paned_pos")]
    pub paned_position: i32,
    #[serde(default = "default_right_paned_pos")]
    pub right_paned_position: i32,

    // View state
    #[serde(default)]
    pub view_mode: String, // "grid" or "list"

    // Translation options
    #[serde(default = "default_translator")]
    pub translator_index: u32,
    #[serde(default = "default_target_lang")]
    pub target_language: String,
    #[serde(default)]
    pub direction_rtl: bool,

    // Detection options
    #[serde(default = "default_detector")]
    pub detector_index: u32,
    #[serde(default = "default_ocr")]
    pub ocr_index: u32,
    #[serde(default)]
    pub use_mocr_merge: bool,

    // Inpainter
    #[serde(default = "default_inpainter")]
    pub inpainter_index: u32,

    // Upscaler
    #[serde(default)]
    pub upscaler_index: u32,

    // Device
    #[serde(default = "default_device")]
    pub device: String,

    // Sort method (0=Name, 1=Natural, 2=Date)
    #[serde(default = "default_sort")]
    pub sort_method: u32,

    // Accent color
    #[serde(default = "default_accent")]
    pub accent_color: String,
    // Custom accent colors stored as hex → (hex, fg)
    #[serde(default)]
    pub custom_accent_colors: HashMap<String, String>,

    // Color scheme (system, light, dark)
    #[serde(default)]
    pub color_scheme: String,

    // Last opened directory
    #[serde(default)]
    pub last_directory: String,

    // Manga home directory (overrides default ~/Manga for the home button)
    #[serde(default)]
    pub manga_home_directory: String,

    // Language
    #[serde(default)]
    pub ui_language: String,

    // Output directory
    #[serde(default)]
    pub output_directory: String,
    #[serde(default)]
    pub use_original_folder: bool,

    // Translation mode
    #[serde(default = "default_translation_mode")]
    pub translation_mode: String, // "standard", "vlm", "cache", "upscale"
    #[serde(default = "default_translation_mode_index")]
    pub translation_mode_index: u32,

    // VLM settings
    #[serde(default = "default_vlm_type")]
    pub vlm_type: String, // "openrouter", "gemini", "local"
    #[serde(default = "default_vlm_type_index")]
    pub vlm_type_index: u32,
    #[serde(default = "default_gemini_model")]
    pub gemini_model: String,
    #[serde(default = "default_gemini_model_index")]
    pub gemini_model_index: u32,
    #[serde(default)]
    pub openrouter_model: String,
    #[serde(default)]
    pub local_model_path: String,
    #[serde(default = "default_project_name")]
    pub project_name: String,

    // Cache mode
    #[serde(default)]
    pub cache_project: String,

    // Virtual environment paths (for standalone operation)
    #[serde(default)]
    pub virtual_env_path: String,
    #[serde(default)]
    pub manga_translator_path: String,

    /// Cached OpenRouter model list (fetched from API).
    #[serde(default = "default_openrouter_model_cache")]
    pub openrouter_model_cache: Vec<String>,

    // Upscale ratio (depends on selected upscaler)
    #[serde(default = "default_upscale_ratio")]
    pub upscale_ratio: u32,

    // Rendering options
    #[serde(default = "default_renderer")]
    pub renderer_index: u32,
    #[serde(default = "default_alignment")]
    pub alignment_index: u32,
    #[serde(default)]
    pub disable_font_border: bool,
    #[serde(default = "default_font_size_offset")]
    pub font_size_offset: i32,
    #[serde(default = "default_font_size_minimum")]
    pub font_size_minimum: i32,
    #[serde(default = "default_direction")]
    pub direction_index: u32,
    #[serde(default)]
    pub uppercase: bool,
    #[serde(default)]
    pub lowercase: bool,
    #[serde(default = "default_font_color")]
    pub font_color: String,
    #[serde(default)]
    pub no_hyphenation: bool,
    #[serde(default)]
    pub line_spacing: i32, // 0 = auto (Python None)
    #[serde(default)]
    pub font_size: i32, // 0 = auto (Python None)

    // Advanced: mask / kernel
    #[serde(default = "default_mask_dilation_offset")]
    pub mask_dilation_offset: i32,
    #[serde(default = "default_kernel_size")]
    pub kernel_size: i32,

    // Advanced: inpainting
    #[serde(default = "default_inpainting_size")]
    pub inpainting_size: u32,
    #[serde(default = "default_inpainting_precision")]
    pub inpainting_precision_index: u32,

    // Advanced: detection
    #[serde(default = "default_detection_size")]
    pub detection_size: u32,
}

fn default_width() -> i32 {
    1280
}
fn default_height() -> i32 {
    800
}
fn default_paned_pos() -> i32 {
    350
}
fn default_right_paned_pos() -> i32 {
    500
}
fn default_translator() -> u32 {
    0
}
fn default_target_lang() -> String {
    "DEU".into()
}
fn default_detector() -> u32 {
    0
}
fn default_ocr() -> u32 {
    0
}
fn default_inpainter() -> u32 {
    0
}
fn default_sort() -> u32 {
    1
} // Natural sort
fn default_device() -> String {
    "cuda".into()
}
fn default_accent() -> String {
    "system".into()
}
fn default_color_scheme() -> String {
    "system".into()
}
fn default_translation_mode() -> String {
    "standard".into()
}
fn default_translation_mode_index() -> u32 {
    0
}
fn default_vlm_type() -> String {
    "gemini".into()
}
fn default_vlm_type_index() -> u32 {
    1
}
fn default_gemini_model() -> String {
    "gemini-2.5-pro".into()
}
fn default_gemini_model_index() -> u32 {
    4
}
fn default_project_name() -> String {
    "Unbenannt".into()
}
fn default_openrouter_model_cache() -> Vec<String> {
    vec![
        "gpt-4o".into(),
        "gpt-4o-mini".into(),
        "claude-3.5-sonnet".into(),
    ]
}
fn default_upscale_ratio() -> u32 {
    2
}
fn default_renderer() -> u32 {
    0
}
fn default_alignment() -> u32 {
    0
}
fn default_font_size_offset() -> i32 {
    0
}
fn default_font_size_minimum() -> i32 {
    -1
}
fn default_direction() -> u32 {
    0
}
fn default_font_color() -> String {
    String::new()
} // empty = auto (Python None)
fn default_mask_dilation_offset() -> i32 {
    20
}
fn default_kernel_size() -> i32 {
    3
}
fn default_inpainting_size() -> u32 {
    2048
}
fn default_inpainting_precision() -> u32 {
    2
} // bf16
fn default_detection_size() -> u32 {
    2048
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            window_width: default_width(),
            window_height: default_height(),
            window_maximized: false,
            paned_position: default_paned_pos(),
            right_paned_position: default_right_paned_pos(),
            view_mode: "grid".into(),
            translator_index: default_translator(),
            target_language: default_target_lang(),
            direction_rtl: false,
            detector_index: default_detector(),
            ocr_index: default_ocr(),
            use_mocr_merge: false,
            inpainter_index: default_inpainter(),
            upscaler_index: 0,
            device: default_device(),
            sort_method: default_sort(),
            accent_color: default_accent(),
            custom_accent_colors: HashMap::new(),
            color_scheme: default_color_scheme(),
            last_directory: String::new(),
            manga_home_directory: String::new(),
            ui_language: String::new(),
            output_directory: String::new(),
            use_original_folder: false,
            translation_mode: default_translation_mode(),
            translation_mode_index: default_translation_mode_index(),
            vlm_type: default_vlm_type(),
            vlm_type_index: default_vlm_type_index(),
            gemini_model: default_gemini_model(),
            gemini_model_index: default_gemini_model_index(),
            openrouter_model: String::new(),
            local_model_path: String::new(),
            project_name: default_project_name(),
            cache_project: String::new(),
            virtual_env_path: String::new(),
            manga_translator_path: String::new(),
            openrouter_model_cache: default_openrouter_model_cache(),
            upscale_ratio: default_upscale_ratio(),
            renderer_index: default_renderer(),
            alignment_index: default_alignment(),
            disable_font_border: false,
            font_size_offset: default_font_size_offset(),
            font_size_minimum: default_font_size_minimum(),
            direction_index: default_direction(),
            uppercase: false,
            lowercase: false,
            font_color: default_font_color(),
            no_hyphenation: false,
            line_spacing: 0,
            font_size: 0,
            mask_dilation_offset: default_mask_dilation_offset(),
            kernel_size: default_kernel_size(),
            inpainting_size: default_inpainting_size(),
            inpainting_precision_index: default_inpainting_precision(),
            detection_size: default_detection_size(),
        }
    }
}

// ---------------------------------------------------------------------------
// Config manager — reads / writes settings + API keys
// ---------------------------------------------------------------------------

pub struct ConfigManager {
    settings_path: PathBuf,
    api_keys_path: PathBuf,
    pub settings: Settings,
    pub api_keys: ApiKeys,
}

impl ConfigManager {
    /// Create a new config manager.
    /// Reads from XDG-compliant config directory.
    pub fn new() -> Self {
        let config_dir = Self::config_dir();
        fs::create_dir_all(&config_dir).ok();

        let settings_path = config_dir.join("settings.json");
        let api_keys_path = config_dir.join("api_keys.json");

        let settings = Self::load_json::<Settings>(&settings_path).unwrap_or_default();
        let api_keys = Self::load_json::<ApiKeys>(&api_keys_path).unwrap_or_default();

        Self {
            settings_path,
            api_keys_path,
            settings,
            api_keys,
        }
    }

    /// Save current settings to disk.
    pub fn save_settings(&self) {
        Self::save_json(&self.settings_path, &self.settings);
    }

    /// Save API keys to disk.
    pub fn save_api_keys(&self) {
        Self::save_json(&self.api_keys_path, &self.api_keys);
    }

    /// Get the current accent color as an `AccentColor` struct.
    pub fn current_accent(&self) -> AccentColor {
        let name = &self.settings.accent_color;
        if let Some(preset) = find_preset(name) {
            return preset;
        }
        // Check custom colors
        if let Some(hex) = self.settings.custom_accent_colors.get(name) {
            return AccentColor {
                name: name.clone(),
                hex: hex.clone(),
                fg: AccentColor::foreground_for(hex),
            };
        }
        // Fallback to system
        AccentColor {
            name: "system".into(),
            hex: String::new(),
            fg: String::new(),
        }
    }

    /// Set accent color by name and persist.
    pub fn set_accent(&mut self, name: &str, hex: Option<&str>) {
        self.settings.accent_color = name.to_string();
        if let Some(hex_val) = hex {
            if !name.starts_with('#') && hex_val.starts_with('#') {
                self.settings
                    .custom_accent_colors
                    .insert(name.to_string(), hex_val.to_string());
            }
        }
        self.save_settings();
    }

    /// XDG config directory for this application.
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("manga-translator")
    }

    /// Cache directory for thumbnails and logs.
    pub fn cache_dir() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("manga-translator")
    }

    /// Path to the session log file used by the Log Viewer dialog.
    /// Located at `~/.config/manga-translator-gtk/manga-translator.log`.
    pub fn log_file_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("manga-translator-gtk")
            .join("manga-translator.log")
    }

    /// Default manga directory (~/Manga or home directory).
    pub fn default_manga_dir() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let manga_dir = home.join("Manga");
        if manga_dir.is_dir() { manga_dir } else { home }
    }

    /// Manga home directory from user settings, falling back to `default_manga_dir()`.
    pub fn manga_home_dir(&self) -> PathBuf {
        let path = &self.settings.manga_home_directory;
        if !path.is_empty() {
            let p = PathBuf::from(path);
            if p.is_dir() {
                return p;
            }
        }
        Self::default_manga_dir()
    }

    /// Resolve the `site-packages` directory inside the configured virtual environment.
    ///
    /// Checks `{venv}/lib/python3.X/site-packages` for Python 3.6–3.15 on Unix,
    /// then `{venv}/Lib/site-packages` for Windows compatibility.
    /// Returns `None` if `virtual_env_path` is empty or no matching directory exists.
    pub fn resolve_venv_site_packages(&self) -> Option<PathBuf> {
        let venv = &self.settings.virtual_env_path;
        if venv.is_empty() {
            return None;
        }
        let venv_path = PathBuf::from(venv);
        if !venv_path.is_dir() {
            return None;
        }

        // Unix: {venv}/lib/python3.X/site-packages
        for minor in 6..=15 {
            let dir = venv_path
                .join("lib")
                .join(format!("python3.{}", minor))
                .join("site-packages");
            if dir.is_dir() {
                return Some(dir);
            }
        }

        // Windows: {venv}/Lib/site-packages
        let win_dir = venv_path.join("Lib").join("site-packages");
        if win_dir.is_dir() {
            return Some(win_dir);
        }

        None
    }

    /// Return the manga-image-translator base directory if the configured path
    /// exists and contains a `manga_translator/` subdirectory.
    /// Returns `None` if `manga_translator_path` is empty or invalid.
    pub fn manga_translator_dir(&self) -> Option<PathBuf> {
        let path_str = &self.settings.manga_translator_path;
        if path_str.is_empty() {
            return None;
        }
        let path = PathBuf::from(path_str);
        if path.is_dir() && path.join("manga_translator").is_dir() {
            Some(path)
        } else {
            None
        }
    }

    // -- Internal helpers ---------------------------------------------------

    fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
        let data = fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    fn save_json<T: Serialize>(path: &Path, value: &T) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        if let Ok(data) = serde_json::to_string_pretty(value) {
            fs::write(path, data).ok();
        }
    }
}

// ---------------------------------------------------------------------------
// Translation options (dropdown labels)
// ---------------------------------------------------------------------------

pub mod options {
    pub const TRANSLATORS: &[&str] = &[
        "Lokal (Offline)",
        "DeepL",
        "Google (via DeepTranslator)",
        "Baidu",
        "Google",
        "Caiyun",
        "ChatGPT",
        "DeepSeek",
        "Groq",
        "Gemini",
        "Sugoi",
        "None (Text nur erkennen)",
    ];

    pub const TARGET_LANGUAGES: &[&str] = &[
        "DEU", "ENG", "JPN", "KOR", "CHS", "CHT", "FRA", "ITA", "ESP", "POR", "RUS", "THA", "VIE",
        "IND", "ARA", "HIN", "POL", "NLD", "SWE", "FIN", "TUR", "UKR", "CZE", "ROM",
    ];

    pub const DETECTORS: &[&str] = &[
        "CTD — Optimiert für Manga & Comics",
        "Default — Allgemeine Texterkennung",
        "DBConvNext — ConvNext-basiert",
        "PaddleOCR — PaddlePaddle Erkennung",
    ];

    pub const OCR_MODELS: &[&str] = &["48px (Standard)", "mocr (Better Quality)"];

    pub const INPAINTERS: &[&str] = &[
        "LaMA — Standard Inpainting",
        "AOTScan — Kontext-basierte Rekonstruktion",
        "PatchMatch — Patch-basierte Bildreparatur",
        "NSM — Neural Style Migration",
        "LaMA Large — Hochauflösendes Inpainting",
    ];

    pub const UPSCALERS: &[&str] = &[
        "Keiner",
        "ESRGAN — Allgemeine Bildverbesserung",
        "Waifu2x — Anime & Manga optimiert",
        "4x-UltraSharp — 4fache Hochskalierung",
    ];

    /// Valid upscale ratios per upscaler index (matches UPSCALERS order).
    pub const UPSCALER_RATIOS: &[&[u32]] = &[
        &[0],               // Keiner
        &[2, 3, 4],         // ESRGAN
        &[2, 4, 8, 16, 32], // Waifu2x
        &[4],               // 4x-UltraSharp
    ];

    pub const DEVICES: &[&str] = &["CUDA (GPU)", "CPU"];

    pub const SORT_METHODS: &[&str] = &[
        "Name (A-Z)",
        "Natürlich (1, 2, 10)",
        "Datum (neueste zuerst)",
    ];

    #[allow(dead_code)]
    pub const VIEW_MODES: &[&str] = &["Raster", "Liste"];

    // Translation modes (matches Python TranslationMode enum)
    pub const TRANSLATION_MODES: &[&str] = &[
        "Standard Übersetzer",
        "VLM (Vision Language Model)",
        "Text extrahieren",
        "Text einfügen",
        "Nur Upscaling",
    ];

    // VLM backends
    pub const VLM_TYPES: &[&str] = &["OpenRouter (Online)", "Gemini (Online)", "Lokales Modell"];

    /// Map VLM type index to the Python-expected string value.
    pub const VLM_TYPE_MAP: &[&str] = &["openrouter", "gemini", "local"];

    // Gemini models (hardcoded list, matches Python GUI)
    pub const GEMINI_MODELS: &[&str] = &[
        "gemini-3.1-pro-preview-customtools",
        "gemini-3.1-pro-preview",
        "gemini-3.1-flash-lite-preview",
        "gemini-3-flash-preview",
        "gemini-2.5-pro",
        "gemini-2.5-flash",
        "gemini-2.5-flash-lite",
    ];

    /// Map translation mode index to the internal string value.
    pub const TRANSLATION_MODE_MAP: &[&str] = &["standard", "vlm", "extract", "cache", "upscale"];

    // Renderers (index → Python enum value)
    pub const RENDERERS: &[&str] = &[
        "Standard",
        "Manga2Eng (manga2eng_pillow)",
        "Manga2Eng (manga2eng)",
        "Kein Rendering",
    ];
    pub const RENDERER_MAP: &[&str] = &["default", "manga2eng_pillow", "manga2eng", "none"];

    // Alignment
    pub const ALIGNMENTS: &[&str] = &["Auto", "Links", "Zentriert", "Rechts"];
    pub const ALIGNMENT_MAP: &[&str] = &["auto", "left", "center", "right"];

    // Text direction
    pub const DIRECTIONS: &[&str] = &["Auto", "Horizontal", "Vertikal"];
    pub const DIRECTION_MAP: &[&str] = &["auto", "h", "v"];

    // Inpainting precision
    pub const INPAINTING_PRECISIONS: &[&str] = &[
        "FP32 (Langsam, Präzise)",
        "FP16 (Ausgewogen)",
        "BF16 (Schnell, Standard)",
    ];
    pub const INPAINTING_PRECISION_MAP: &[&str] = &["fp32", "fp16", "bf16"];
}

// manga-translator-gtk/src/ui/settings_panel.rs
//
// Translation settings panel — Adw.PreferencesGroup with ComboRow widgets
// for translator, target language, detector, OCR, inpainter, upscaler, etc.
//
// Features:
//   - Adw.ComboRow for each dropdown setting (translator, language, etc.)
//   - Adw.SwitchRow for boolean toggles (RTL, mocr merge)
//   - Adw.ExpanderRow for advanced options
//   - Auto-save on every change via ConfigManager
//   - Disable all widgets during translation processing
//   - Callbacks for parent window to react to setting changes

use adw::prelude::*;

use gio;
use gtk::glib;
use gtk::glib::clone;
use gtk::subclass::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

use std::rc::Rc;

use crate::config::{self, ConfigManager};
use crate::i18n;

// ---------------------------------------------------------------------------
// Settings change callback type
// ---------------------------------------------------------------------------

/// The kind of setting that changed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingKind {
    Translator,
    TargetLanguage,
    Detector,
    Ocr,
    Inpainter,
    Upscaler,
    Device,
    DirectionRtl,
    MocrMerge,
    SortMethod,
    ViewMode,
    TranslationMode,
    VlmType,
    GeminiModel,
    OpenrouterModel,
    LocalModel,
    ProjectName,
    UpscaleRatio,
    Renderer,
    Alignment,
    DisableFontBorder,
    FontSizeOffset,
    FontColor,
    RenderDirection,
    MaskDilationOffset,
    InpaintingSize,
    InpaintingPrecision,
    DetectionSize,
    OutputDirectory,
    Language,
}

/// Callback invoked when any setting changes.
pub type OnSettingChanged = Box<dyn Fn(SettingKind)>;

// ---------------------------------------------------------------------------
// SettingsPanel widget (GObject)
// ---------------------------------------------------------------------------

glib::wrapper! {
    pub struct SettingsPanel(ObjectSubclass<SettingsPanelPrivate>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// Internal state for the settings panel.
pub struct SettingsPanelPrivate {
    /// Reference to the config manager for reading/writing settings.
    config: RefCell<Option<Rc<RefCell<ConfigManager>>>>,
    /// Callback when a setting changes.
    on_setting_changed: RefCell<Option<OnSettingChanged>>,

    // Widget references for enabling/disabling during processing
    container: RefCell<Option<gtk::Box>>,
    page: RefCell<Option<gtk::Box>>,

    // Combo rows
    translator_row: RefCell<Option<adw::ComboRow>>,
    target_lang_row: RefCell<Option<adw::ComboRow>>,
    detector_row: RefCell<Option<adw::ComboRow>>,
    ocr_row: RefCell<Option<adw::ComboRow>>,
    inpainter_row: RefCell<Option<adw::ComboRow>>,
    upscaler_row: RefCell<Option<adw::ComboRow>>,
    device_row: RefCell<Option<adw::ComboRow>>,

    // Switch rows
    rtl_row: RefCell<Option<adw::SwitchRow>>,
    mocr_merge_row: RefCell<Option<adw::SwitchRow>>,

    // Advanced group (collapsible)
    advanced_group: RefCell<Option<adw::PreferencesGroup>>,

    // Translation mode
    mode_row: RefCell<Option<adw::ComboRow>>,

    // VLM section
    vlm_section: RefCell<Option<adw::PreferencesGroup>>,
    vlm_type_row: RefCell<Option<adw::ComboRow>>,
    gemini_model_row: RefCell<Option<adw::ComboRow>>,
    openrouter_model_row: RefCell<Option<adw::ComboRow>>,
    /// Refresh button row for fetching OpenRouter models.
    openrouter_refresh_row: RefCell<Option<adw::ActionRow>>,
    local_model_row: RefCell<Option<adw::EntryRow>>,
    // Revealers for animated show/hide
    vlm_revealer: RefCell<Option<gtk::Revealer>>,

    // Scroll indicators
    scroll_indicator_top: RefCell<Option<gtk::Box>>,
    scroll_indicator_bottom: RefCell<Option<gtk::Box>>,

    /// Callback to request an OpenRouter model list refresh.
    on_fetch_openrouter_models: RefCell<Option<Box<dyn Fn()>>>,

    // Language
    language_row: RefCell<Option<adw::ComboRow>>,

    // Output directory
    use_original_folder_row: RefCell<Option<adw::SwitchRow>>,
    output_directory_row: RefCell<Option<adw::ActionRow>>,
    output_directory_label: RefCell<Option<gtk::Label>>,

    // Upscale ratio (dynamic based on upscaler selection)
    upscale_ratio_row: RefCell<Option<adw::ComboRow>>,

    // Rendering
    renderer_row: RefCell<Option<adw::ComboRow>>,
    alignment_row: RefCell<Option<adw::ComboRow>>,
    disable_font_border_row: RefCell<Option<adw::SwitchRow>>,
    font_size_offset_row: RefCell<Option<adw::SpinRow>>,
    font_color_row: RefCell<Option<adw::ActionRow>>,
    font_color_fg_btn: RefCell<Option<gtk::ColorDialogButton>>,
    font_color_bg_btn: RefCell<Option<gtk::ColorDialogButton>>,
    font_color_switch: RefCell<Option<gtk::Switch>>,
    direction_row: RefCell<Option<adw::ComboRow>>,
    // Advanced
    mask_dilation_offset_row: RefCell<Option<adw::SpinRow>>,
    inpainting_size_row: RefCell<Option<adw::SpinRow>>,
    inpainting_precision_row: RefCell<Option<adw::ComboRow>>,
    detection_size_row: RefCell<Option<adw::SpinRow>>,
}

impl Default for SettingsPanelPrivate {
    fn default() -> Self {
        Self {
            config: RefCell::new(None),
            on_setting_changed: RefCell::new(None),
            container: RefCell::new(None),
            page: RefCell::new(None),
            translator_row: RefCell::new(None),
            target_lang_row: RefCell::new(None),
            detector_row: RefCell::new(None),
            ocr_row: RefCell::new(None),
            inpainter_row: RefCell::new(None),
            upscaler_row: RefCell::new(None),
            device_row: RefCell::new(None),
            rtl_row: RefCell::new(None),
            mocr_merge_row: RefCell::new(None),
            advanced_group: RefCell::new(None),
            mode_row: RefCell::new(None),
            vlm_section: RefCell::new(None),
            vlm_type_row: RefCell::new(None),
            gemini_model_row: RefCell::new(None),
            openrouter_model_row: RefCell::new(None),
            openrouter_refresh_row: RefCell::new(None),
            local_model_row: RefCell::new(None),
            vlm_revealer: RefCell::new(None),
            scroll_indicator_top: RefCell::new(None),
            scroll_indicator_bottom: RefCell::new(None),
            on_fetch_openrouter_models: RefCell::new(None),
            language_row: RefCell::new(None),
            use_original_folder_row: RefCell::new(None),
            output_directory_row: RefCell::new(None),
            output_directory_label: RefCell::new(None),
            upscale_ratio_row: RefCell::new(None),
            renderer_row: RefCell::new(None),
            alignment_row: RefCell::new(None),
            disable_font_border_row: RefCell::new(None),
            font_size_offset_row: RefCell::new(None),
            font_color_row: RefCell::new(None),
            font_color_fg_btn: RefCell::new(None),
            font_color_bg_btn: RefCell::new(None),
            font_color_switch: RefCell::new(None),
            direction_row: RefCell::new(None),
            mask_dilation_offset_row: RefCell::new(None),
            inpainting_size_row: RefCell::new(None),
            inpainting_precision_row: RefCell::new(None),
            detection_size_row: RefCell::new(None),
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for SettingsPanelPrivate {
    const NAME: &'static str = "MangaSettingsPanel";
    type Type = SettingsPanel;
    type ParentType = gtk::Widget;
}

impl ObjectImpl for SettingsPanelPrivate {
    fn constructed(&self) {
        self.parent_constructed();
        let obj = self.obj();
        // SettingsPanel subclasses gtk::Widget (a leaf), so we must provide
        // a layout manager so the internal container child gets size-allocated.
        obj.set_layout_manager(Some(gtk::BoxLayout::new(gtk::Orientation::Vertical)));
        self.build_ui(&obj);
    }

    fn dispose(&self) {
        if let Some(container) = self.container.borrow().as_ref() {
            container.unparent();
        }
    }
}

impl WidgetImpl for SettingsPanelPrivate {}

// ---------------------------------------------------------------------------
// Color conversion helpers for font_color field
// ---------------------------------------------------------------------------

/// Convert gdk::RGBA to hex string "RRGGBB"
fn rgba_to_hex(rgba: &gtk::gdk::RGBA) -> String {
    format!(
        "{:02X}{:02X}{:02X}",
        (rgba.red() * 255.0) as u8,
        (rgba.green() * 255.0) as u8,
        (rgba.blue() * 255.0) as u8,
    )
}

/// Convert hex string "RRGGBB" to gdk::RGBA. Falls back to `default_hex`
/// when the input is empty or invalid.
fn hex_to_rgba(hex: &str, default_hex: &str) -> gtk::gdk::RGBA {
    let s = if hex.len() >= 6 { hex } else { default_hex };
    let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(0) as f32 / 255.0;
    let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(0) as f32 / 255.0;
    let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(0) as f32 / 255.0;
    gtk::gdk::RGBA::new(r, g, b, 1.0)
}

/// Parse font_color string "FG:BG" or "FG" into its parts.
fn parse_font_color(font_color: &str) -> (String, String) {
    if font_color.is_empty() {
        return (String::new(), String::new());
    }
    if let Some((fg, bg)) = font_color.split_once(':') {
        (fg.to_string(), bg.to_string())
    } else {
        (font_color.to_string(), String::new())
    }
}

impl SettingsPanelPrivate {
    /// Build the settings panel UI.
    fn build_ui(&self, obj: &SettingsPanel) {
        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_hexpand(true);
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scrolled.set_size_request(260, -1);

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);

        // ── Language section ──────────────────────────────────────
        let language_group = adw::PreferencesGroup::new();
        language_group.set_title(&i18n::t("Sprache"));
        i18n::register_group_title(&language_group, "Sprache");

        let lang_names: Vec<&str> = i18n::SUPPORTED_LANGUAGES
            .iter()
            .map(|(_, name)| *name)
            .collect();
        let lang_model = gtk::StringList::new(&lang_names);

        let language_row = adw::ComboRow::builder()
            .title(&i18n::t("Sprache"))
            .subtitle(&i18n::t("Wirkt sofort"))
            .model(&lang_model)
            .build();
        i18n::register_combo_row(&language_row, "Sprache", Some("Wirkt sofort"));

        let lang_icon = gtk::Image::from_icon_name("emoji-flags-symbolic");
        lang_icon.add_css_class("dim-label");
        language_row.add_prefix(&lang_icon);

        language_group.add(&language_row);

        // ── Translation group ──────────────────────────────────────
        let translation_group = adw::PreferencesGroup::new();
        translation_group.set_title(&i18n::t("Übersetzung"));
        i18n::register_group_title(&translation_group, "Übersetzung");
        translation_group.set_description(Some(&i18n::t("Einstellungen für die Übersetzung")));
        {
            let weak = translation_group.downgrade();
            i18n::register(Box::new(move || {
                if let Some(g) = weak.upgrade() {
                    g.set_description(Some(&i18n::t("Einstellungen für die Übersetzung")));
                }
            }));
        }

        // Translation mode ComboRow
        let mode_model = gtk::StringList::new(config::options::TRANSLATION_MODES);
        let mode_row = adw::ComboRow::builder()
            .title(&i18n::t("Modus"))
            .subtitle(&i18n::t("Wähle den Übersetzungsmodus"))
            .model(&mode_model)
            .build();
        i18n::register_combo_row(&mode_row, "Modus", Some("Wähle den Übersetzungsmodus"));
        i18n::register_combo_row_items(&mode_row, config::options::TRANSLATION_MODES);

        // Translator ComboRow
        let translator_model = gtk::StringList::new(config::options::TRANSLATORS);
        let translator_row = adw::ComboRow::builder()
            .title(&i18n::t("Übersetzer"))
            .subtitle(&i18n::t("Wähle den Übersetzungsdienst"))
            .model(&translator_model)
            .build();
        i18n::register_combo_row(
            &translator_row,
            "Übersetzer",
            Some("Wähle den Übersetzungsdienst"),
        );
        i18n::register_combo_row_items(&translator_row, config::options::TRANSLATORS);
        translator_row.add_css_class("settings-translator");

        // Target language ComboRow
        let lang_model = gtk::StringList::new(config::options::TARGET_LANGUAGES);
        let target_lang_row = adw::ComboRow::builder()
            .title(&i18n::t("Zielsprache"))
            .subtitle(&i18n::t("Sprache der Übersetzung"))
            .model(&lang_model)
            .build();
        i18n::register_combo_row(
            &target_lang_row,
            "Zielsprache",
            Some("Sprache der Übersetzung"),
        );

        // RTL direction switch
        let rtl_row = adw::SwitchRow::builder()
            .title(&i18n::t("RTL Richtung"))
            .subtitle(&i18n::t("Text von rechts nach links (Arabisch, Hebräisch)"))
            .build();
        i18n::register_switch_row(
            &rtl_row,
            "RTL Richtung",
            Some("Text von rechts nach links (Arabisch, Hebräisch)"),
        );

        translation_group.add(&mode_row);
        translation_group.add(&translator_row);
        translation_group.add(&target_lang_row);
        translation_group.add(&rtl_row);

        // ── Output section ─────────────────────────────────────────
        let output_group = adw::PreferencesGroup::new();
        output_group.set_title(&i18n::t("Ausgabe"));
        i18n::register_group_title(&output_group, "Ausgabe");
        output_group.set_description(Some(&i18n::t(
            "Wählen Sie den Speicherort für übersetzte Bilder",
        )));
        {
            let weak = output_group.downgrade();
            i18n::register(Box::new(move || {
                if let Some(g) = weak.upgrade() {
                    g.set_description(Some(&i18n::t(
                        "Wählen Sie den Speicherort für übersetzte Bilder",
                    )));
                }
            }));
        }

        // Use original folder switch
        let use_original_folder_row = adw::SwitchRow::builder()
            .title(&i18n::t("Original-Ordner verwenden"))
            .subtitle(&i18n::t(
                "Speichert die Übersetzungen im selben Ordner wie die Originaldateien",
            ))
            .build();
        i18n::register_switch_row(
            &use_original_folder_row,
            "Original-Ordner verwenden",
            Some("Speichert die Übersetzungen im selben Ordner wie die Originaldateien"),
        );

        output_group.add(&use_original_folder_row);

        // Custom output directory row
        let output_directory_row = adw::ActionRow::builder()
            .title(&i18n::t("Ausgabe-Ordner"))
            .subtitle(&i18n::t(
                "Benutzerdefinierter Speicherort für Übersetzungen",
            ))
            .build();
        i18n::register_action_row(
            &output_directory_row,
            "Ausgabe-Ordner",
            Some("Benutzerdefinierter Speicherort für Übersetzungen"),
        );

        let output_directory_label = gtk::Label::new(Some(&i18n::t("Standard (result/)")));
        i18n::register_label(&output_directory_label, "Standard (result/)");
        output_directory_label.set_halign(gtk::Align::Start);
        output_directory_label.add_css_class("caption");
        output_directory_label.add_css_class("dim-label");

        let output_button = gtk::Button::with_label(&i18n::t("Auswählen"));
        i18n::register_button(&output_button, "Auswählen");
        output_button.set_valign(gtk::Align::Center);
        output_button.add_css_class("flat");

        output_directory_row.add_suffix(&output_button);
        output_directory_row.add_suffix(&output_directory_label);
        output_group.add(&output_directory_row);

        // ── VLM section (hidden by default, shown when mode=VLM) ───
        let vlm_section = adw::PreferencesGroup::new();
        vlm_section.set_title(&i18n::t("VLM Einstellungen"));
        i18n::register_group_title(&vlm_section, "VLM Einstellungen");
        vlm_section.set_description(Some(&i18n::t("Vision Language Model Konfiguration")));
        {
            let weak = vlm_section.downgrade();
            i18n::register(Box::new(move || {
                if let Some(g) = weak.upgrade() {
                    g.set_description(Some(&i18n::t("Vision Language Model Konfiguration")));
                }
            }));
        }

        // VLM Type ComboRow
        let vlm_type_model = gtk::StringList::new(config::options::VLM_TYPES);
        let vlm_type_row = adw::ComboRow::builder()
            .title(&i18n::t("VLM Typ"))
            .subtitle(&i18n::t("Wähle das VLM-Backend"))
            .model(&vlm_type_model)
            .build();
        i18n::register_combo_row(&vlm_type_row, "VLM Typ", Some("Wähle das VLM-Backend"));
        i18n::register_combo_row_items(&vlm_type_row, config::options::VLM_TYPES);

        // Gemini Model ComboRow
        let gemini_model_list = gtk::StringList::new(config::options::GEMINI_MODELS);
        let gemini_model_row = adw::ComboRow::builder()
            .title(&i18n::t("Gemini Modell"))
            .subtitle(&i18n::t("Modell für VLM-Korrektur"))
            .model(&gemini_model_list)
            .build();
        i18n::register_combo_row(
            &gemini_model_row,
            "Gemini Modell",
            Some("Modell für VLM-Korrektur"),
        );

        // OpenRouter Model ComboRow (initially empty — populated from cache or API fetch)
        let openrouter_model_list = gtk::StringList::new(&[]);
        let openrouter_model_row = adw::ComboRow::builder()
            .title(&i18n::t("OpenRouter Modell"))
            .subtitle(&i18n::t("Vision-fähiges Modell"))
            .model(&openrouter_model_list)
            .build();
        i18n::register_combo_row(
            &openrouter_model_row,
            "OpenRouter Modell",
            Some("Vision-fähiges Modell"),
        );

        // Refresh row for fetching OpenRouter models from the API
        let openrouter_refresh_row = adw::ActionRow::builder()
            .title(&i18n::t("Modellliste aktualisieren"))
            .subtitle(&i18n::t("Modelle vom OpenRouter API laden"))
            .activatable(true)
            .selectable(false)
            .build();
        i18n::register_action_row(
            &openrouter_refresh_row,
            "Modellliste aktualisieren",
            Some("Modelle vom OpenRouter API laden"),
        );
        openrouter_refresh_row.add_css_class("suggested-action");
        let refresh_icon = gtk::Image::from_icon_name("view-refresh-symbolic");
        openrouter_refresh_row.add_suffix(&refresh_icon);

        // Local model path EntryRow
        let local_model_row = adw::EntryRow::builder()
            .title(&i18n::t("Lokales Modell (.gguf Pfad)"))
            .build();
        i18n::register_entry_row(&local_model_row, "Lokales Modell (.gguf Pfad)");
        local_model_row.set_visible(false);

        vlm_section.add(&vlm_type_row);
        vlm_section.add(&gemini_model_row);
        vlm_section.add(&openrouter_model_row);
        vlm_section.add(&openrouter_refresh_row);
        vlm_section.add(&local_model_row);

        // Wrap VLM section in a Revealer for slide-down animation
        let vlm_revealer = gtk::Revealer::new();
        vlm_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        vlm_revealer.set_transition_duration(300);
        vlm_revealer.set_reveal_child(false);
        vlm_revealer.set_child(Some(&vlm_section));
        // Initially show Gemini model (default VLM type index = 1)
        gemini_model_row.set_visible(true);
        openrouter_model_row.set_visible(false);

        // ── Detection group ────────────────────────────────────────
        let detection_group = adw::PreferencesGroup::new();
        detection_group.set_title(&i18n::t("Texterkennung"));
        i18n::register_group_title(&detection_group, "Texterkennung");
        detection_group.set_description(Some(&i18n::t(
            "Einstellungen für Blasen- und Texterkennung",
        )));
        {
            let weak = detection_group.downgrade();
            i18n::register(Box::new(move || {
                if let Some(g) = weak.upgrade() {
                    g.set_description(Some(&i18n::t(
                        "Einstellungen für Blasen- und Texterkennung",
                    )));
                }
            }));
        }

        // Detector ComboRow
        let detector_model = gtk::StringList::new(config::options::DETECTORS);
        let detector_row = adw::ComboRow::builder()
            .title(&i18n::t("Detektor"))
            .subtitle(&i18n::t("Methode zur Blasenerkennung"))
            .model(&detector_model)
            .build();
        i18n::register_combo_row(
            &detector_row,
            "Detektor",
            Some("Methode zur Blasenerkennung"),
        );
        i18n::register_combo_row_items(&detector_row, config::options::DETECTORS);

        // OCR model ComboRow
        let ocr_model = gtk::StringList::new(config::options::OCR_MODELS);
        let ocr_row = adw::ComboRow::builder()
            .title(&i18n::t("OCR Modell"))
            .subtitle(&i18n::t("Optical Character Recognition"))
            .model(&ocr_model)
            .build();
        i18n::register_combo_row(
            &ocr_row,
            "OCR Modell",
            Some("Optical Character Recognition"),
        );
        i18n::register_combo_row_items(&ocr_row, config::options::OCR_MODELS);

        // mocr merge switch
        let mocr_merge_row = adw::SwitchRow::builder()
            .title(&i18n::t("BBox Zusammenführung"))
            .subtitle(&i18n::t("Erkennungsboxen zusammenführen (mocr)"))
            .build();
        i18n::register_switch_row(
            &mocr_merge_row,
            "BBox Zusammenführung",
            Some("Erkennungsboxen zusammenführen (mocr)"),
        );

        detection_group.add(&detector_row);
        detection_group.add(&ocr_row);
        detection_group.add(&mocr_merge_row);

        // ── Image processing group ─────────────────────────────────
        let processing_group = adw::PreferencesGroup::new();
        processing_group.set_title(&i18n::t("Bildverarbeitung"));
        i18n::register_group_title(&processing_group, "Bildverarbeitung");
        processing_group
            .set_description(Some(&i18n::t("Einstellungen für Inpainting und Upscaling")));
        {
            let weak = processing_group.downgrade();
            i18n::register(Box::new(move || {
                if let Some(g) = weak.upgrade() {
                    g.set_description(Some(&i18n::t("Einstellungen für Inpainting und Upscaling")));
                }
            }));
        }

        // Inpainter ComboRow
        let inpainter_model = gtk::StringList::new(config::options::INPAINTERS);
        let inpainter_row = adw::ComboRow::builder()
            .title(&i18n::t("Inpainting"))
            .subtitle(&i18n::t("Methode zur Bildreparatur"))
            .model(&inpainter_model)
            .build();
        i18n::register_combo_row(
            &inpainter_row,
            "Inpainting",
            Some("Methode zur Bildreparatur"),
        );
        i18n::register_combo_row_items(&inpainter_row, config::options::INPAINTERS);

        // Upscaler ComboRow
        let upscaler_model = gtk::StringList::new(config::options::UPSCALERS);
        let upscaler_row = adw::ComboRow::builder()
            .title(&i18n::t("Upscaler"))
            .subtitle(&i18n::t("Bildvergrößerung vor der Übersetzung"))
            .model(&upscaler_model)
            .build();
        i18n::register_combo_row(
            &upscaler_row,
            "Upscaler",
            Some("Bildvergrößerung vor der Übersetzung"),
        );
        i18n::register_combo_row_items(&upscaler_row, config::options::UPSCALERS);

        // Upscale ratio ComboRow (dynamic — populated based on upscaler)
        let upscale_ratio_row = adw::ComboRow::builder()
            .title(&i18n::t("Upscale Faktor"))
            .subtitle(&i18n::t("Vergrößerungsfaktor"))
            .build();
        i18n::register_combo_row(
            &upscale_ratio_row,
            "Upscale Faktor",
            Some("Vergrößerungsfaktor"),
        );
        // Initially hidden (no upscaler selected)
        upscale_ratio_row.set_visible(false);

        // Device ComboRow
        let device_model = gtk::StringList::new(config::options::DEVICES);
        let device_row = adw::ComboRow::builder()
            .title(&i18n::t("Gerät"))
            .subtitle(&i18n::t("Recheneinheit für die Übersetzung"))
            .model(&device_model)
            .build();
        i18n::register_combo_row(
            &device_row,
            "Gerät",
            Some("Recheneinheit für die Übersetzung"),
        );
        i18n::register_combo_row_items(&device_row, config::options::DEVICES);

        processing_group.add(&inpainter_row);
        processing_group.add(&upscaler_row);
        processing_group.add(&upscale_ratio_row);
        processing_group.add(&device_row);

        // ── Rendering group ─────────────────────────────────────────
        let rendering_group = adw::PreferencesGroup::new();
        rendering_group.set_title(&i18n::t("Darstellung"));
        i18n::register_group_title(&rendering_group, "Darstellung");
        rendering_group.set_description(Some(&i18n::t("Einstellungen für die Textdarstellung")));
        {
            let weak = rendering_group.downgrade();
            i18n::register(Box::new(move || {
                if let Some(g) = weak.upgrade() {
                    g.set_description(Some(&i18n::t("Einstellungen für die Textdarstellung")));
                }
            }));
        }

        // Renderer ComboRow
        let renderer_model = gtk::StringList::new(config::options::RENDERERS);
        let renderer_row = adw::ComboRow::builder()
            .title(&i18n::t("Renderer"))
            .subtitle(&i18n::t("Methode zur Texteinblendung"))
            .model(&renderer_model)
            .build();
        i18n::register_combo_row(
            &renderer_row,
            "Renderer",
            Some("Methode zur Texteinblendung"),
        );
        i18n::register_combo_row_items(&renderer_row, config::options::RENDERERS);

        // Alignment ComboRow
        let alignment_model = gtk::StringList::new(config::options::ALIGNMENTS);
        let alignment_row = adw::ComboRow::builder()
            .title(&i18n::t("Ausrichtung"))
            .subtitle(&i18n::t("Textausrichtung in Blasen"))
            .model(&alignment_model)
            .build();
        i18n::register_combo_row(
            &alignment_row,
            "Ausrichtung",
            Some("Textausrichtung in Blasen"),
        );
        i18n::register_combo_row_items(&alignment_row, config::options::ALIGNMENTS);

        // Text direction ComboRow
        let direction_model = gtk::StringList::new(config::options::DIRECTIONS);
        let direction_row = adw::ComboRow::builder()
            .title(&i18n::t("Textrichtung"))
            .subtitle(&i18n::t("Erzwinge horizontale/vertikale Richtung"))
            .model(&direction_model)
            .build();
        i18n::register_combo_row(
            &direction_row,
            "Textrichtung",
            Some("Erzwinge horizontale/vertikale Richtung"),
        );
        i18n::register_combo_row_items(&direction_row, config::options::DIRECTIONS);

        // Disable font border switch
        let disable_font_border_row = adw::SwitchRow::builder()
            .title(&i18n::t("Schrift randlos"))
            .subtitle(&i18n::t("Kein Rand/Umriss um den Text"))
            .build();
        i18n::register_switch_row(
            &disable_font_border_row,
            "Schrift randlos",
            Some("Kein Rand/Umriss um den Text"),
        );

        // Font size offset SpinRow
        let font_size_offset_adj = gtk::Adjustment::new(0.0, -20.0, 20.0, 1.0, 5.0, 0.0);
        let font_size_offset_row = adw::SpinRow::new(Some(&font_size_offset_adj), 1.0, 0);
        font_size_offset_row.set_title(&i18n::t("Schriftgrößenversatz"));
        font_size_offset_row.set_subtitle(&i18n::t("Positive Werte = größerer Text"));
        {
            let weak = font_size_offset_row.downgrade();
            i18n::register(Box::new(move || {
                if let Some(r) = weak.upgrade() {
                    r.set_title(&i18n::t("Schriftgrößenversatz"));
                    r.set_subtitle(&i18n::t("Positive Werte = größerer Text"));
                }
            }));
        }

        // Font color ActionRow with Switch + two ColorDialogButtons
        let font_color_row = adw::ActionRow::builder()
            .title(&i18n::t("Schriftfarbe"))
            .subtitle(&i18n::t("Benutzerdefinierte Text-/Randfarbe"))
            .activatable(false)
            .selectable(false)
            .build();
        i18n::register_action_row(
            &font_color_row,
            "Schriftfarbe",
            Some("Benutzerdefinierte Text-/Randfarbe"),
        );

        let color_dialog = gtk::ColorDialog::builder().with_alpha(false).build();

        // Foreground (text) color button — default black
        let font_color_fg_btn = gtk::ColorDialogButton::new(Some(color_dialog.clone()));
        font_color_fg_btn.set_tooltip_text(Some(&i18n::t("Textfarbe (Vordergrund)")));
        i18n::register_tooltip(&font_color_fg_btn, "Textfarbe (Vordergrund)");
        font_color_fg_btn.set_rgba(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0));
        font_color_fg_btn.set_sensitive(false);

        // Background (border) color button — default white
        let font_color_bg_btn = gtk::ColorDialogButton::new(Some(color_dialog));
        font_color_bg_btn.set_tooltip_text(Some(&i18n::t("Randfarbe (Hintergrund)")));
        i18n::register_tooltip(&font_color_bg_btn, "Randfarbe (Hintergrund)");
        font_color_bg_btn.set_rgba(&gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 1.0));
        font_color_bg_btn.set_sensitive(false);

        // Switch: ON = custom colors, OFF = auto (OCR-detected)
        let font_color_switch = gtk::Switch::new();
        font_color_switch.set_tooltip_text(Some(&i18n::t("Eigene Farben verwenden")));
        i18n::register_tooltip(&font_color_switch, "Eigene Farben verwenden");
        font_color_switch.set_valign(gtk::Align::Center);

        let fg_label = gtk::Label::new(Some(&i18n::t("Text:")));
        i18n::register_label(&fg_label, "Text:");
        let bg_label = gtk::Label::new(Some(&i18n::t("Rand:")));
        i18n::register_label(&bg_label, "Rand:");

        let color_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        color_box.set_valign(gtk::Align::Center);
        color_box.append(&fg_label);
        color_box.append(&font_color_fg_btn);
        color_box.append(&bg_label);
        color_box.append(&font_color_bg_btn);

        font_color_row.add_suffix(&color_box);
        font_color_row.add_suffix(&font_color_switch);

        rendering_group.add(&renderer_row);
        rendering_group.add(&alignment_row);
        rendering_group.add(&direction_row);
        rendering_group.add(&disable_font_border_row);
        rendering_group.add(&font_size_offset_row);
        rendering_group.add(&font_color_row);

        // ── Advanced group (collapsible) ────────────────────────────
        let advanced_group = adw::PreferencesGroup::new();
        advanced_group.set_title(&i18n::t("Erweitert"));
        i18n::register_group_title(&advanced_group, "Erweitert");
        advanced_group.set_description(Some(&i18n::t(
            "Zusätzliche Optionen für fortgeschrittene Benutzer",
        )));
        {
            let weak = advanced_group.downgrade();
            i18n::register(Box::new(move || {
                if let Some(g) = weak.upgrade() {
                    g.set_description(Some(&i18n::t(
                        "Zusätzliche Optionen für fortgeschrittene Benutzer",
                    )));
                }
            }));
        }

        // Advanced: Mask dilation offset SpinRow
        let mask_dilation_adj = gtk::Adjustment::new(20.0, 0.0, 200.0, 5.0, 20.0, 0.0);
        let mask_dilation_offset_row = adw::SpinRow::new(Some(&mask_dilation_adj), 5.0, 0);
        mask_dilation_offset_row.set_title(&i18n::t("Masken-Erweiterung"));
        mask_dilation_offset_row.set_subtitle(&i18n::t("Textmaske erweitern um (px)"));
        {
            let weak = mask_dilation_offset_row.downgrade();
            i18n::register(Box::new(move || {
                if let Some(r) = weak.upgrade() {
                    r.set_title(&i18n::t("Masken-Erweiterung"));
                    r.set_subtitle(&i18n::t("Textmaske erweitern um (px)"));
                }
            }));
        }

        // Advanced: Inpainting size SpinRow
        let inpainting_size_adj = gtk::Adjustment::new(2048.0, 256.0, 4096.0, 256.0, 512.0, 0.0);
        let inpainting_size_row = adw::SpinRow::new(Some(&inpainting_size_adj), 256.0, 0);
        inpainting_size_row.set_title(&i18n::t("Inpainting Größe"));
        inpainting_size_row.set_subtitle(&i18n::t("Auflösung für Bildreparatur"));
        {
            let weak = inpainting_size_row.downgrade();
            i18n::register(Box::new(move || {
                if let Some(r) = weak.upgrade() {
                    r.set_title(&i18n::t("Inpainting Größe"));
                    r.set_subtitle(&i18n::t("Auflösung für Bildreparatur"));
                }
            }));
        }

        // Advanced: Inpainting precision ComboRow
        let inpainting_precision_model =
            gtk::StringList::new(config::options::INPAINTING_PRECISIONS);
        let inpainting_precision_row = adw::ComboRow::builder()
            .title(&i18n::t("Inpainting Präzision"))
            .subtitle(&i18n::t("Berechnungsgenauigkeit"))
            .model(&inpainting_precision_model)
            .build();
        i18n::register_combo_row(
            &inpainting_precision_row,
            "Inpainting Präzision",
            Some("Berechnungsgenauigkeit"),
        );
        i18n::register_combo_row_items(
            &inpainting_precision_row,
            config::options::INPAINTING_PRECISIONS,
        );

        // Advanced: Detection size SpinRow
        let detection_size_adj = gtk::Adjustment::new(2048.0, 256.0, 4096.0, 256.0, 512.0, 0.0);
        let detection_size_row = adw::SpinRow::new(Some(&detection_size_adj), 256.0, 0);
        detection_size_row.set_title(&i18n::t("Erkennungsgröße"));
        detection_size_row.set_subtitle(&i18n::t("Auflösung für Texterkennung"));
        {
            let weak = detection_size_row.downgrade();
            i18n::register(Box::new(move || {
                if let Some(r) = weak.upgrade() {
                    r.set_title(&i18n::t("Erkennungsgröße"));
                    r.set_subtitle(&i18n::t("Auflösung für Texterkennung"));
                }
            }));
        }

        advanced_group.add(&mask_dilation_offset_row);
        advanced_group.add(&inpainting_size_row);
        advanced_group.add(&inpainting_precision_row);
        advanced_group.add(&detection_size_row);

        // ── Add groups to page ─────────────────────────────────────
        page.append(&language_group);
        page.append(&translation_group);
        page.append(&output_group);
        page.append(&vlm_revealer);
        page.append(&detection_group);
        page.append(&processing_group);
        page.append(&rendering_group);
        page.append(&advanced_group);

        scrolled.set_child(Some(&page));

        // ── Connect signals ────────────────────────────────────────
        language_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_language_changed();
                }
            ),
        );

        mode_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_mode_changed();
                }
            ),
        );

        translator_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_translator_changed();
                }
            ),
        );

        target_lang_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_target_lang_changed();
                }
            ),
        );

        detector_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_detector_changed();
                }
            ),
        );

        ocr_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_ocr_changed();
                }
            ),
        );

        inpainter_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_inpainter_changed();
                }
            ),
        );

        upscaler_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_upscaler_changed();
                }
            ),
        );

        upscale_ratio_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_upscale_ratio_changed();
                }
            ),
        );

        device_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_device_changed();
                }
            ),
        );

        rtl_row.connect_notify_local(
            Some("active"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_rtl_changed();
                }
            ),
        );

        mocr_merge_row.connect_notify_local(
            Some("active"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_mocr_merge_changed();
                }
            ),
        );

        use_original_folder_row.connect_notify_local(
            Some("active"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_use_original_folder_changed();
                }
            ),
        );

        output_button.connect_clicked(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                this.on_select_output_directory();
            }
        ));

        vlm_type_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_vlm_type_changed();
                }
            ),
        );

        gemini_model_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_gemini_model_changed();
                }
            ),
        );

        openrouter_model_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_openrouter_model_changed();
                }
            ),
        );

        openrouter_refresh_row.connect_activated(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                if let Some(cb) = this.on_fetch_openrouter_models.borrow().as_ref() {
                    cb();
                }
            }
        ));

        local_model_row.connect_changed(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                this.on_local_model_changed();
            }
        ));

        renderer_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_renderer_changed();
                }
            ),
        );

        alignment_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_alignment_changed();
                }
            ),
        );

        direction_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_render_direction_changed();
                }
            ),
        );

        disable_font_border_row.connect_notify_local(
            Some("active"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_disable_font_border_changed();
                }
            ),
        );

        font_size_offset_row.connect_notify_local(
            Some("value"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_font_size_offset_changed();
                }
            ),
        );

        font_color_fg_btn.connect_notify_local(
            Some("rgba"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_font_color_changed();
                }
            ),
        );

        font_color_bg_btn.connect_notify_local(
            Some("rgba"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_font_color_changed();
                }
            ),
        );

        font_color_switch.connect_notify_local(
            Some("active"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_font_color_switch_toggled();
                }
            ),
        );

        mask_dilation_offset_row.connect_notify_local(
            Some("value"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_mask_dilation_offset_changed();
                }
            ),
        );

        inpainting_size_row.connect_notify_local(
            Some("value"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_inpainting_size_changed();
                }
            ),
        );

        inpainting_precision_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_inpainting_precision_changed();
                }
            ),
        );

        detection_size_row.connect_notify_local(
            Some("value"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.on_detection_size_changed();
                }
            ),
        );

        // ── Store references ───────────────────────────────────────
        *self.page.borrow_mut() = Some(page);
        *self.mode_row.borrow_mut() = Some(mode_row);
        *self.translator_row.borrow_mut() = Some(translator_row);
        *self.target_lang_row.borrow_mut() = Some(target_lang_row);
        *self.detector_row.borrow_mut() = Some(detector_row);
        *self.ocr_row.borrow_mut() = Some(ocr_row);
        *self.inpainter_row.borrow_mut() = Some(inpainter_row);
        *self.upscaler_row.borrow_mut() = Some(upscaler_row);
        *self.upscale_ratio_row.borrow_mut() = Some(upscale_ratio_row);
        *self.device_row.borrow_mut() = Some(device_row);
        *self.rtl_row.borrow_mut() = Some(rtl_row);
        *self.mocr_merge_row.borrow_mut() = Some(mocr_merge_row);
        *self.advanced_group.borrow_mut() = Some(advanced_group);
        *self.vlm_section.borrow_mut() = Some(vlm_section);
        *self.vlm_type_row.borrow_mut() = Some(vlm_type_row);
        *self.gemini_model_row.borrow_mut() = Some(gemini_model_row);
        *self.openrouter_model_row.borrow_mut() = Some(openrouter_model_row);
        *self.openrouter_refresh_row.borrow_mut() = Some(openrouter_refresh_row);
        *self.local_model_row.borrow_mut() = Some(local_model_row);
        *self.vlm_revealer.borrow_mut() = Some(vlm_revealer);
        *self.renderer_row.borrow_mut() = Some(renderer_row);
        *self.alignment_row.borrow_mut() = Some(alignment_row);
        *self.direction_row.borrow_mut() = Some(direction_row);
        *self.disable_font_border_row.borrow_mut() = Some(disable_font_border_row);
        *self.font_size_offset_row.borrow_mut() = Some(font_size_offset_row);
        *self.font_color_row.borrow_mut() = Some(font_color_row);
        *self.font_color_fg_btn.borrow_mut() = Some(font_color_fg_btn);
        *self.font_color_bg_btn.borrow_mut() = Some(font_color_bg_btn);
        *self.font_color_switch.borrow_mut() = Some(font_color_switch);
        *self.mask_dilation_offset_row.borrow_mut() = Some(mask_dilation_offset_row);
        *self.inpainting_size_row.borrow_mut() = Some(inpainting_size_row);
        *self.inpainting_precision_row.borrow_mut() = Some(inpainting_precision_row);
        *self.detection_size_row.borrow_mut() = Some(detection_size_row);
        *self.language_row.borrow_mut() = Some(language_row);
        *self.use_original_folder_row.borrow_mut() = Some(use_original_folder_row);
        *self.output_directory_row.borrow_mut() = Some(output_directory_row);
        *self.output_directory_label.borrow_mut() = Some(output_directory_label);

        // Wrap scrolled with scroll indicators
        let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);
        wrapper.set_vexpand(true);
        wrapper.set_hexpand(true);

        let scroll_top = gtk::Box::new(gtk::Orientation::Vertical, 0);
        scroll_top.add_css_class("scroll-indicator-top");
        scroll_top.add_css_class("scroll-hidden");

        let scroll_bottom = gtk::Box::new(gtk::Orientation::Vertical, 0);
        scroll_bottom.add_css_class("scroll-indicator-bottom");

        wrapper.append(&scroll_top);
        wrapper.append(&scrolled);
        wrapper.append(&scroll_bottom);

        *self.scroll_indicator_top.borrow_mut() = Some(scroll_top.clone());
        *self.scroll_indicator_bottom.borrow_mut() = Some(scroll_bottom.clone());

        // Monitor scroll position to show/hide indicators via CSS class
        // (toggle class instead of set_visible for smooth CSS transitions)
        {
            let scroll_top = scroll_top;
            let scroll_bottom = scroll_bottom;
            scrolled.vadjustment().connect_value_changed(move |adj| {
                let at_top = adj.value() < 1.0;
                let at_bottom = adj.value() + adj.page_size() >= adj.upper() - 1.0;
                if at_top {
                    scroll_top.add_css_class("scroll-hidden");
                } else {
                    scroll_top.remove_css_class("scroll-hidden");
                }
                if at_bottom {
                    scroll_bottom.add_css_class("scroll-hidden");
                } else {
                    scroll_bottom.remove_css_class("scroll-hidden");
                }
            });
        }

        *self.container.borrow_mut() = Some(wrapper.clone());
        wrapper.set_parent(obj);
    }

    // ── Signal handlers ─────────────────────────────────────────────

    fn on_language_changed(&self) {
        if let Some(row) = self.language_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            if let Some((code, _name)) = i18n::SUPPORTED_LANGUAGES.get(idx) {
                if let Some(cfg) = self.config.borrow().as_ref() {
                    cfg.borrow_mut().settings.ui_language = code.to_string();
                    cfg.borrow().save_settings();
                }
            }
        }
        self.fire_setting_changed(SettingKind::Language);
    }

    fn on_mode_changed(&self) {
        if let Some(row) = self.mode_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            let mode = config::options::TRANSLATION_MODE_MAP
                .get(idx)
                .unwrap_or(&"standard")
                .to_string();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.translation_mode = mode.clone();
                cfg.borrow_mut().settings.translation_mode_index = idx as u32;
                cfg.borrow_mut().save_settings();
            }
            // Toggle VLM section visibility
            let is_vlm = mode == "vlm";
            if let Some(revealer) = self.vlm_revealer.borrow().as_ref() {
                revealer.set_reveal_child(is_vlm);
            }
            self.fire_setting_changed(SettingKind::TranslationMode);
        }
    }

    fn on_translator_changed(&self) {
        if let Some(row) = self.translator_row.borrow().as_ref() {
            let idx = row.selected();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.translator_index = idx;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::Translator);
        }
    }

    fn on_target_lang_changed(&self) {
        if let Some(row) = self.target_lang_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            if let Some(cfg) = self.config.borrow().as_ref() {
                if let Some(lang) = config::options::TARGET_LANGUAGES.get(idx) {
                    cfg.borrow_mut().settings.target_language = lang.to_string();
                    cfg.borrow_mut().save_settings();
                }
            }
            self.fire_setting_changed(SettingKind::TargetLanguage);
        }
    }

    fn on_detector_changed(&self) {
        if let Some(row) = self.detector_row.borrow().as_ref() {
            let idx = row.selected();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.detector_index = idx;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::Detector);
        }
    }

    fn on_ocr_changed(&self) {
        if let Some(row) = self.ocr_row.borrow().as_ref() {
            let idx = row.selected();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.ocr_index = idx;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::Ocr);
        }
    }

    fn on_inpainter_changed(&self) {
        if let Some(row) = self.inpainter_row.borrow().as_ref() {
            let idx = row.selected();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.inpainter_index = idx;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::Inpainter);
        }
    }

    fn on_upscaler_changed(&self) {
        if let Some(row) = self.upscaler_row.borrow().as_ref() {
            let idx = row.selected();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.upscaler_index = idx;
                cfg.borrow_mut().save_settings();
            }
            self.refresh_upscale_ratios();
            self.fire_setting_changed(SettingKind::Upscaler);
        }
    }

    fn on_upscale_ratio_changed(&self) {
        if let Some(row) = self.upscale_ratio_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            // Extract ratio value from the string list model
            let ratio = row
                .model()
                .and_then(|m| m.item(idx as u32))
                .and_then(|obj| {
                    obj.downcast_ref::<gtk::StringObject>()
                        .map(|s| s.string().to_string())
                })
                .and_then(|s| s.trim_end_matches('x').parse::<u32>().ok())
                .unwrap_or(2);
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.upscale_ratio = ratio;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::UpscaleRatio);
        }
    }

    /// Update the upscale ratio dropdown based on the selected upscaler.
    fn refresh_upscale_ratios(&self) {
        let upscaler_idx = self
            .upscaler_row
            .borrow()
            .as_ref()
            .map(|r| r.selected())
            .unwrap_or(0) as usize;

        let ratios = config::options::UPSCALER_RATIOS
            .get(upscaler_idx)
            .copied()
            .unwrap_or(&[0]);

        // "Keiner" (index 0) has only [0] — hide the ratio row
        let is_none = upscaler_idx == 0;

        if let Some(row) = self.upscale_ratio_row.borrow().as_ref() {
            row.set_visible(!is_none);

            let labels: Vec<String> = ratios.iter().map(|r| format!("{}x", r)).collect();
            let label_strs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
            let model = gtk::StringList::new(&label_strs);
            row.set_model(Some(&model));

            // Try to select the saved ratio, otherwise default to first
            let saved_ratio = self
                .config
                .borrow()
                .as_ref()
                .map(|cfg| cfg.borrow().settings.upscale_ratio)
                .unwrap_or(2);

            let select_idx = ratios.iter().position(|r| *r == saved_ratio).unwrap_or(0) as u32;
            row.set_selected(select_idx);
        }
    }

    fn on_device_changed(&self) {
        if let Some(row) = self.device_row.borrow().as_ref() {
            let idx = row.selected();
            let device_name = match idx {
                0 => "cuda",
                _ => "cpu",
            };
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.device = device_name.to_string();
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::Device);
        }
    }

    fn on_rtl_changed(&self) {
        if let Some(row) = self.rtl_row.borrow().as_ref() {
            let active = row.is_active();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.direction_rtl = active;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::DirectionRtl);
        }
    }

    fn on_mocr_merge_changed(&self) {
        if let Some(row) = self.mocr_merge_row.borrow().as_ref() {
            let active = row.is_active();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.use_mocr_merge = active;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::MocrMerge);
        }
    }

    fn on_use_original_folder_changed(&self) {
        if let Some(row) = self.use_original_folder_row.borrow().as_ref() {
            let use_original = row.is_active();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.use_original_folder = use_original;
                cfg.borrow().save_settings();
            }
            // Disable output directory row when using original folder
            if let Some(output_row) = self.output_directory_row.borrow().as_ref() {
                output_row.set_sensitive(!use_original);
            }
            self.fire_setting_changed(SettingKind::OutputDirectory);
        }
    }

    fn on_select_output_directory(&self) {
        let root = self.container.borrow().as_ref().and_then(|c| c.root());
        let root_window = root.and_then(|r| r.downcast::<adw::ApplicationWindow>().ok());

        let dialog = gtk::FileDialog::new();
        dialog.set_title(&i18n::t("Ausgabe-Ordner wählen"));

        // Set initial folder to current output directory if set
        if let Some(cfg) = self.config.borrow().as_ref() {
            let current_dir = cfg.borrow().settings.output_directory.clone();
            if !current_dir.is_empty() && std::path::Path::new(&current_dir).is_dir() {
                dialog.set_initial_folder(Some(&gio::File::for_path(&current_dir)));
            }
        }

        let label = self.output_directory_label.borrow().clone();
        let config = self.config.borrow().clone();

        dialog.select_folder(
            root_window.as_ref(),
            gio::Cancellable::NONE,
            move |result| {
                if let Some(folder) = result.ok().and_then(|f| f.path()) {
                    let path_str = folder.to_string_lossy().to_string();
                    if let Some(cfg) = config.as_ref() {
                        cfg.borrow_mut().settings.output_directory = path_str.clone();
                        cfg.borrow().save_settings();
                    }
                    if let Some(lbl) = label.as_ref() {
                        lbl.set_label(&path_str);
                    }
                }
            },
        );
    }

    fn on_vlm_type_changed(&self) {
        if let Some(row) = self.vlm_type_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            let vlm_type = config::options::VLM_TYPE_MAP
                .get(idx)
                .unwrap_or(&"gemini")
                .to_string();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.vlm_type = vlm_type.clone();
                cfg.borrow_mut().settings.vlm_type_index = idx as u32;
                cfg.borrow_mut().save_settings();
            }
            // Toggle sub-row visibility based on VLM type
            if let Some(gemini) = self.gemini_model_row.borrow().as_ref() {
                gemini.set_visible(vlm_type == "gemini");
            }
            if let Some(openrouter) = self.openrouter_model_row.borrow().as_ref() {
                openrouter.set_visible(vlm_type == "openrouter");
            }
            if let Some(local) = self.local_model_row.borrow().as_ref() {
                local.set_visible(vlm_type == "local");
            }
            if let Some(row) = self.openrouter_refresh_row.borrow().as_ref() {
                row.set_visible(vlm_type == "openrouter");
            }
            // Auto-fetch OpenRouter models when switching to OpenRouter type
            if vlm_type == "openrouter" {
                if let Some(cb) = self.on_fetch_openrouter_models.borrow().as_ref() {
                    cb();
                }
            }
            self.fire_setting_changed(SettingKind::VlmType);
        }
    }

    fn on_gemini_model_changed(&self) {
        if let Some(row) = self.gemini_model_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            if let Some(model) = config::options::GEMINI_MODELS.get(idx) {
                if let Some(cfg) = self.config.borrow().as_ref() {
                    cfg.borrow_mut().settings.gemini_model = model.to_string();
                    cfg.borrow_mut().settings.gemini_model_index = idx as u32;
                    cfg.borrow_mut().save_settings();
                }
            }
            self.fire_setting_changed(SettingKind::GeminiModel);
        }
    }

    fn on_openrouter_model_changed(&self) {
        if let Some(row) = self.openrouter_model_row.borrow().as_ref() {
            let idx = row.selected();
            // Get the model name from the string list
            if let Some(model) = row.model().and_then(|m| m.item(idx)) {
                let model_str = model
                    .downcast_ref::<gtk::StringObject>()
                    .map(|s| s.string().to_string())
                    .unwrap_or_default();
                if let Some(cfg) = self.config.borrow().as_ref() {
                    cfg.borrow_mut().settings.openrouter_model = model_str;
                    cfg.borrow_mut().save_settings();
                }
            }
            self.fire_setting_changed(SettingKind::OpenrouterModel);
        }
    }

    fn on_local_model_changed(&self) {
        if let Some(row) = self.local_model_row.borrow().as_ref() {
            let text = row.text().to_string();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.local_model_path = text;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::LocalModel);
        }
    }

    fn on_renderer_changed(&self) {
        if let Some(row) = self.renderer_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.renderer_index = idx as u32;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::Renderer);
        }
    }

    fn on_alignment_changed(&self) {
        if let Some(row) = self.alignment_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.alignment_index = idx as u32;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::Alignment);
        }
    }

    fn on_render_direction_changed(&self) {
        if let Some(row) = self.direction_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.direction_index = idx as u32;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::RenderDirection);
        }
    }

    fn on_disable_font_border_changed(&self) {
        if let Some(row) = self.disable_font_border_row.borrow().as_ref() {
            let active = row.is_active();
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.disable_font_border = active;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::DisableFontBorder);
        }
    }

    fn on_font_size_offset_changed(&self) {
        if let Some(row) = self.font_size_offset_row.borrow().as_ref() {
            let value = row.value() as i32;
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.font_size_offset = value;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::FontSizeOffset);
        }
    }

    fn on_font_color_changed(&self) {
        // Only update when the switch is ON (custom mode)
        let is_custom = self
            .font_color_switch
            .borrow()
            .as_ref()
            .map(|s| s.is_active())
            .unwrap_or(false);
        if !is_custom {
            return;
        }

        let fg = self
            .font_color_fg_btn
            .borrow()
            .as_ref()
            .map(|b| rgba_to_hex(&b.rgba()));
        let bg = self
            .font_color_bg_btn
            .borrow()
            .as_ref()
            .map(|b| rgba_to_hex(&b.rgba()));
        let combined = match (fg, bg) {
            (Some(f), Some(b)) => format!("{}:{}", f, b),
            (Some(f), None) => f,
            _ => String::new(),
        };
        if let Some(cfg) = self.config.borrow().as_ref() {
            cfg.borrow_mut().settings.font_color = combined;
            cfg.borrow_mut().save_settings();
        }
        self.fire_setting_changed(SettingKind::FontColor);
    }

    fn on_font_color_switch_toggled(&self) {
        let is_custom = self
            .font_color_switch
            .borrow()
            .as_ref()
            .map(|s| s.is_active())
            .unwrap_or(false);

        // Toggle button sensitivity
        if let Some(btn) = self.font_color_fg_btn.borrow().as_ref() {
            btn.set_sensitive(is_custom);
        }
        if let Some(btn) = self.font_color_bg_btn.borrow().as_ref() {
            btn.set_sensitive(is_custom);
        }

        if is_custom {
            // Read current button colors and write to config
            self.on_font_color_changed();
        } else {
            // Clear custom color → auto mode
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.font_color = String::new();
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::FontColor);
        }
    }

    fn on_mask_dilation_offset_changed(&self) {
        if let Some(row) = self.mask_dilation_offset_row.borrow().as_ref() {
            let value = row.value() as i32;
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.mask_dilation_offset = value;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::MaskDilationOffset);
        }
    }

    fn on_inpainting_size_changed(&self) {
        if let Some(row) = self.inpainting_size_row.borrow().as_ref() {
            let value = row.value() as u32;
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.inpainting_size = value;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::InpaintingSize);
        }
    }

    fn on_inpainting_precision_changed(&self) {
        if let Some(row) = self.inpainting_precision_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.inpainting_precision_index = idx as u32;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::InpaintingPrecision);
        }
    }

    fn on_detection_size_changed(&self) {
        if let Some(row) = self.detection_size_row.borrow().as_ref() {
            let value = row.value() as u32;
            if let Some(cfg) = self.config.borrow().as_ref() {
                cfg.borrow_mut().settings.detection_size = value;
                cfg.borrow_mut().save_settings();
            }
            self.fire_setting_changed(SettingKind::DetectionSize);
        }
    }

    /// Invoke the setting-changed callback, if set.
    fn fire_setting_changed(&self, kind: SettingKind) {
        if let Some(cb) = self.on_setting_changed.borrow().as_ref() {
            cb(kind);
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

impl SettingsPanel {
    /// Create a new settings panel widget.
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Set the config manager and restore UI state from saved settings.
    ///
    /// This must be called after construction, before the widget is shown,
    /// to restore the saved dropdown indices and toggle states.
    pub fn set_config(&self, config: Rc<RefCell<ConfigManager>>) {
        let priv_ = self.imp();
        let settings = config.borrow().settings.clone();

        // Restore UI state from settings
        if let Some(row) = priv_.mode_row.borrow().as_ref() {
            row.set_selected(settings.translation_mode_index);
        }
        if let Some(row) = priv_.translator_row.borrow().as_ref() {
            row.set_selected(settings.translator_index);
        }
        if let Some(row) = priv_.target_lang_row.borrow().as_ref() {
            // Find the index for the saved target language
            let lang_idx = config::options::TARGET_LANGUAGES
                .iter()
                .position(|l| *l == settings.target_language)
                .unwrap_or(0) as u32;
            row.set_selected(lang_idx);
        }
        if let Some(row) = priv_.detector_row.borrow().as_ref() {
            row.set_selected(settings.detector_index);
        }
        if let Some(row) = priv_.ocr_row.borrow().as_ref() {
            row.set_selected(settings.ocr_index);
        }
        if let Some(row) = priv_.inpainter_row.borrow().as_ref() {
            row.set_selected(settings.inpainter_index);
        }
        if let Some(row) = priv_.upscaler_row.borrow().as_ref() {
            row.set_selected(settings.upscaler_index);
        }

        // Populate and restore upscale ratio (must come after upscaler restore)
        priv_.refresh_upscale_ratios();
        if let Some(row) = priv_.device_row.borrow().as_ref() {
            let device_idx = match settings.device.as_str() {
                "cpu" => 1,
                _ => 0,
            };
            row.set_selected(device_idx);
        }
        if let Some(row) = priv_.rtl_row.borrow().as_ref() {
            row.set_active(settings.direction_rtl);
        }
        if let Some(row) = priv_.mocr_merge_row.borrow().as_ref() {
            row.set_active(settings.use_mocr_merge);
        }

        // Restore VLM settings
        if let Some(row) = priv_.vlm_type_row.borrow().as_ref() {
            row.set_selected(settings.vlm_type_index);
        }
        if let Some(row) = priv_.gemini_model_row.borrow().as_ref() {
            row.set_selected(settings.gemini_model_index);
        }
        if let Some(row) = priv_.local_model_row.borrow().as_ref() {
            row.set_text(&settings.local_model_path);
        }

        // Toggle VLM section visibility based on mode
        let is_vlm = settings.translation_mode == "vlm";
        if let Some(section) = priv_.vlm_section.borrow().as_ref() {
            section.set_visible(is_vlm);
        }
        if let Some(revealer) = priv_.vlm_revealer.borrow().as_ref() {
            revealer.set_reveal_child(is_vlm);
        }

        // Set VLM sub-row visibility based on VLM type
        let vlm_type = settings.vlm_type.as_str();
        if let Some(gemini) = priv_.gemini_model_row.borrow().as_ref() {
            gemini.set_visible(vlm_type == "gemini");
        }
        if let Some(openrouter) = priv_.openrouter_model_row.borrow().as_ref() {
            openrouter.set_visible(vlm_type == "openrouter");
        }
        if let Some(local) = priv_.local_model_row.borrow().as_ref() {
            local.set_visible(vlm_type == "local");
        }
        // Show/hide refresh row alongside OpenRouter row
        if let Some(row) = priv_.openrouter_refresh_row.borrow().as_ref() {
            row.set_visible(vlm_type == "openrouter");
        }

        // Populate OpenRouter combo from cached model list
        if !settings.openrouter_model_cache.is_empty() {
            if let Some(row) = priv_.openrouter_model_row.borrow().as_ref() {
                let cache = &settings.openrouter_model_cache;
                let model_strs: Vec<&str> = cache.iter().map(|s| s.as_str()).collect();
                row.set_model(Some(&gtk::StringList::new(&model_strs)));

                // Restore previously selected model
                if !settings.openrouter_model.is_empty() {
                    if let Some(idx) = cache.iter().position(|m| m == &settings.openrouter_model) {
                        row.set_selected(idx as u32);
                    }
                }
            }
        }

        // Restore rendering settings
        if let Some(row) = priv_.renderer_row.borrow().as_ref() {
            row.set_selected(settings.renderer_index);
        }
        if let Some(row) = priv_.alignment_row.borrow().as_ref() {
            row.set_selected(settings.alignment_index);
        }
        if let Some(row) = priv_.direction_row.borrow().as_ref() {
            row.set_selected(settings.direction_index);
        }
        if let Some(row) = priv_.disable_font_border_row.borrow().as_ref() {
            row.set_active(settings.disable_font_border);
        }
        if let Some(row) = priv_.font_size_offset_row.borrow().as_ref() {
            row.set_value(settings.font_size_offset as f64);
        }
        // Restore font color: parse "FG:BG" and set buttons + switch
        let (fg_hex, bg_hex) = parse_font_color(&settings.font_color);
        let has_custom = !settings.font_color.is_empty();
        if let Some(btn) = priv_.font_color_fg_btn.borrow().as_ref() {
            btn.set_rgba(&hex_to_rgba(&fg_hex, "000000"));
            btn.set_sensitive(has_custom);
        }
        if let Some(btn) = priv_.font_color_bg_btn.borrow().as_ref() {
            btn.set_rgba(&hex_to_rgba(&bg_hex, "FFFFFF"));
            btn.set_sensitive(has_custom);
        }
        if let Some(sw) = priv_.font_color_switch.borrow().as_ref() {
            sw.set_active(has_custom);
        }

        // Restore advanced settings
        if let Some(row) = priv_.mask_dilation_offset_row.borrow().as_ref() {
            row.set_value(settings.mask_dilation_offset as f64);
        }
        if let Some(row) = priv_.inpainting_size_row.borrow().as_ref() {
            row.set_value(settings.inpainting_size as f64);
        }
        if let Some(row) = priv_.inpainting_precision_row.borrow().as_ref() {
            row.set_selected(settings.inpainting_precision_index);
        }
        if let Some(row) = priv_.detection_size_row.borrow().as_ref() {
            row.set_value(settings.detection_size as f64);
        }

        // Restore output directory settings
        if let Some(row) = priv_.use_original_folder_row.borrow().as_ref() {
            row.set_active(settings.use_original_folder);
        }
        if let Some(output_row) = priv_.output_directory_row.borrow().as_ref() {
            output_row.set_sensitive(!settings.use_original_folder);
        }
        if let Some(label) = priv_.output_directory_label.borrow().as_ref() {
            if settings.output_directory.is_empty() {
                label.set_label(&i18n::t("Standard (result/)"));
            } else {
                label.set_label(&settings.output_directory);
            }
        }

        // Restore language selection
        if let Some(row) = priv_.language_row.borrow().as_ref() {
            let saved = &settings.ui_language;
            if saved.is_empty() {
                row.set_selected(0); // "Automatisch"
            } else {
                for (i, (code, _)) in i18n::SUPPORTED_LANGUAGES.iter().enumerate() {
                    if *code == saved {
                        row.set_selected(i as u32);
                        break;
                    }
                }
            }
        }

        // Store config reference
        *priv_.config.borrow_mut() = Some(config);
    }

    /// Set a callback to invoke when any setting changes.
    ///
    /// The callback receives the kind of setting that changed.
    pub fn on_setting_changed<F: Fn(SettingKind) + 'static>(&self, callback: F) {
        let priv_ = self.imp();
        *priv_.on_setting_changed.borrow_mut() = Some(Box::new(callback));
    }

    /// Set a callback to invoke when the user clicks the OpenRouter refresh button.
    ///
    /// The callback should spawn a background fetch via the Python bridge
    /// and then call [`update_openrouter_models`] with the results on the
    /// UI thread.
    pub fn set_on_fetch_openrouter_models<F: Fn() + 'static>(&self, callback: F) {
        let priv_ = self.imp();
        *priv_.on_fetch_openrouter_models.borrow_mut() = Some(Box::new(callback));
    }

    /// Show or hide a loading state on the OpenRouter refresh row.
    ///
    /// When `loading` is true, the subtitle changes to a loading message
    /// and the row is disabled to prevent double-fetches.  When false,
    /// the normal subtitle is restored and the row re-enabled.
    pub fn set_openrouter_fetching(&self, loading: bool) {
        let priv_ = self.imp();
        if let Some(row) = priv_.openrouter_refresh_row.borrow().as_ref() {
            if loading {
                row.set_subtitle(&i18n::t("Modelle werden geladen…"));
                row.set_sensitive(false);
            } else {
                row.set_subtitle(&i18n::t("Modelle vom OpenRouter API laden"));
                row.set_sensitive(true);
            }
        }
    }

    /// Update the OpenRouter model combo row with a fresh model list.
    ///
    /// Typically called from the UI thread after a background fetch completes.
    /// The model list is persisted to the config cache so it survives restarts.
    /// If the previously selected model is present in the new list, it is
    /// re-selected automatically.
    ///
    /// If the list is empty, the subtitle of the refresh row is updated to
    /// show an error message instead of silently discarding the result.
    pub fn update_openrouter_models(&self, models: &[String]) {
        let priv_ = self.imp();

        // Always clear the loading state
        self.set_openrouter_fetching(false);

        if models.is_empty() {
            // Show error feedback on the refresh row instead of silently returning
            if let Some(row) = priv_.openrouter_refresh_row.borrow().as_ref() {
                row.set_subtitle(&i18n::t("Keine Modelle gefunden"));
            }
            log::warn!("update_openrouter_models called with empty model list");
            return;
        }

        // Remember the previously selected model
        let prev_model = if let Some(cfg) = priv_.config.borrow().as_ref() {
            cfg.borrow().settings.openrouter_model.clone()
        } else {
            String::new()
        };

        // Update the combo row
        if let Some(row) = priv_.openrouter_model_row.borrow().as_ref() {
            let strs: Vec<&str> = models.iter().map(|s| s.as_str()).collect();
            let new_list = gtk::StringList::new(&strs);
            row.set_model(Some(&new_list));

            // Try to restore the previously selected model
            if !prev_model.is_empty() {
                if let Some(idx) = models.iter().position(|m| m == &prev_model) {
                    row.set_selected(idx as u32);
                }
            }
        }

        // Persist the model list to config cache
        if let Some(cfg) = priv_.config.borrow().as_ref() {
            cfg.borrow_mut().settings.openrouter_model_cache = models.to_vec();
            cfg.borrow().save_settings();
        }
    }

    /// Enable or disable all settings widgets.
    ///
    /// Used during translation processing to prevent the user from
    /// changing settings mid-translation.
    pub fn set_sensitive(&self, sensitive: bool) {
        let priv_ = self.imp();

        let rows: Vec<Option<adw::ComboRow>> = vec![
            priv_.language_row.borrow().clone(),
            priv_.mode_row.borrow().clone(),
            priv_.translator_row.borrow().clone(),
            priv_.target_lang_row.borrow().clone(),
            priv_.detector_row.borrow().clone(),
            priv_.ocr_row.borrow().clone(),
            priv_.inpainter_row.borrow().clone(),
            priv_.upscaler_row.borrow().clone(),
            priv_.device_row.borrow().clone(),
            priv_.vlm_type_row.borrow().clone(),
            priv_.gemini_model_row.borrow().clone(),
            priv_.openrouter_model_row.borrow().clone(),
            priv_.upscale_ratio_row.borrow().clone(),
        ];

        for row in rows.into_iter().flatten() {
            row.set_sensitive(sensitive);
        }

        if let Some(row) = priv_.rtl_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.mocr_merge_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.local_model_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }

        if let Some(row) = priv_.upscale_ratio_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.openrouter_refresh_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.renderer_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.alignment_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.direction_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.disable_font_border_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.font_size_offset_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.font_color_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        // Color buttons: respect both row sensitivity and switch state
        let switch_active = priv_
            .font_color_switch
            .borrow()
            .as_ref()
            .map(|s| s.is_active())
            .unwrap_or(false);
        let btn_sensitive = sensitive && switch_active;
        if let Some(btn) = priv_.font_color_fg_btn.borrow().as_ref() {
            btn.set_sensitive(btn_sensitive);
        }
        if let Some(btn) = priv_.font_color_bg_btn.borrow().as_ref() {
            btn.set_sensitive(btn_sensitive);
        }
        if let Some(row) = priv_.mask_dilation_offset_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.inpainting_size_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.inpainting_precision_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.detection_size_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        if let Some(row) = priv_.use_original_folder_row.borrow().as_ref() {
            row.set_sensitive(sensitive);
        }
        // Output directory row: respect both processing state and original-folder switch
        let use_original = priv_
            .use_original_folder_row
            .borrow()
            .as_ref()
            .map(|r| r.is_active())
            .unwrap_or(false);
        if let Some(row) = priv_.output_directory_row.borrow().as_ref() {
            row.set_sensitive(sensitive && !use_original);
        }
    }

    // ── Getters for current values ──────────────────────────────────

    /// Get the currently selected translation mode string.
    pub fn translation_mode(&self) -> String {
        let priv_ = self.imp();
        if let Some(cfg) = priv_.config.borrow().as_ref() {
            return cfg.borrow().settings.translation_mode.clone();
        }
        let idx = priv_
            .mode_row
            .borrow()
            .as_ref()
            .map(|r| r.selected())
            .unwrap_or(0) as usize;
        config::options::TRANSLATION_MODE_MAP
            .get(idx)
            .unwrap_or(&"standard")
            .to_string()
    }

    /// Check if VLM mode is currently selected.
    pub fn is_vlm_mode(&self) -> bool {
        self.translation_mode() == "vlm"
    }

    /// Get the currently selected VLM type string.
    pub fn vlm_type(&self) -> String {
        let priv_ = self.imp();
        if let Some(cfg) = priv_.config.borrow().as_ref() {
            return cfg.borrow().settings.vlm_type.clone();
        }
        let idx = priv_
            .vlm_type_row
            .borrow()
            .as_ref()
            .map(|r| r.selected())
            .unwrap_or(1) as usize;
        config::options::VLM_TYPE_MAP
            .get(idx)
            .unwrap_or(&"gemini")
            .to_string()
    }

    /// Get the currently selected Gemini model name.
    pub fn gemini_model(&self) -> String {
        let priv_ = self.imp();
        if let Some(cfg) = priv_.config.borrow().as_ref() {
            return cfg.borrow().settings.gemini_model.clone();
        }
        let idx = priv_
            .gemini_model_row
            .borrow()
            .as_ref()
            .map(|r| r.selected())
            .unwrap_or(4) as usize;
        config::options::GEMINI_MODELS
            .get(idx)
            .unwrap_or(&"gemini-2.5-pro")
            .to_string()
    }

    /// Get the currently selected OpenRouter model name.
    pub fn openrouter_model(&self) -> String {
        let priv_ = self.imp();
        if let Some(cfg) = priv_.config.borrow().as_ref() {
            return cfg.borrow().settings.openrouter_model.clone();
        }
        // Fallback: read from combo row
        if let Some(row) = priv_.openrouter_model_row.borrow().as_ref() {
            let idx = row.selected();
            if let Some(model) = row.model().and_then(|m| m.item(idx)) {
                return model
                    .downcast_ref::<gtk::StringObject>()
                    .map(|s| s.string().to_string())
                    .unwrap_or_default();
            }
        }
        String::new()
    }

    /// Get the local model path.
    pub fn local_model_path(&self) -> String {
        let priv_ = self.imp();
        if let Some(cfg) = priv_.config.borrow().as_ref() {
            return cfg.borrow().settings.local_model_path.clone();
        }
        if let Some(row) = priv_.local_model_row.borrow().as_ref() {
            return row.text().to_string();
        }
        String::new()
    }

    /// Get the VLM project name.
    pub fn project_name(&self) -> String {
        let priv_ = self.imp();
        if let Some(cfg) = priv_.config.borrow().as_ref() {
            return cfg.borrow().settings.project_name.clone();
        }
        "Unbenannt".to_string()
    }

    /// Get the currently selected upscale ratio.
    pub fn upscale_ratio(&self) -> u32 {
        let priv_ = self.imp();
        if let Some(cfg) = priv_.config.borrow().as_ref() {
            return cfg.borrow().settings.upscale_ratio;
        }
        if let Some(row) = priv_.upscale_ratio_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            return row
                .model()
                .and_then(|m| m.item(idx as u32))
                .and_then(|obj| {
                    obj.downcast_ref::<gtk::StringObject>()
                        .map(|s| s.string().to_string())
                })
                .and_then(|s| s.trim_end_matches('x').parse::<u32>().ok())
                .unwrap_or(2);
        }
        2
    }

    /// Get the currently selected translator index.
    pub fn translator_index(&self) -> u32 {
        let priv_ = self.imp();
        priv_
            .translator_row
            .borrow()
            .as_ref()
            .map(|r| r.selected())
            .unwrap_or(0)
    }

    /// Get the translator name string.
    pub fn translator_name(&self) -> String {
        let idx = self.translator_index() as usize;
        config::options::TRANSLATORS
            .get(idx)
            .unwrap_or(&"Lokal (Offline)")
            .to_string()
    }

    /// Get the currently selected target language code.
    pub fn target_language(&self) -> String {
        let priv_ = self.imp();
        if let Some(cfg) = priv_.config.borrow().as_ref() {
            return cfg.borrow().settings.target_language.clone();
        }
        // Fallback: read from the combo row
        if let Some(row) = priv_.target_lang_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            if let Some(lang) = config::options::TARGET_LANGUAGES.get(idx) {
                return lang.to_string();
            }
        }
        "DEU".to_string()
    }

    /// Get the currently selected detector index.
    pub fn detector_index(&self) -> u32 {
        let priv_ = self.imp();
        priv_
            .detector_row
            .borrow()
            .as_ref()
            .map(|r| r.selected())
            .unwrap_or(0)
    }

    /// Get the detector name string.
    pub fn detector_name(&self) -> String {
        let idx = self.detector_index() as usize;
        config::options::DETECTORS
            .get(idx)
            .unwrap_or(&"Standard (CTD)")
            .to_string()
    }

    /// Get the currently selected OCR model index.
    pub fn ocr_index(&self) -> u32 {
        let priv_ = self.imp();
        priv_
            .ocr_row
            .borrow()
            .as_ref()
            .map(|r| r.selected())
            .unwrap_or(0)
    }

    /// Get the OCR model name string.
    pub fn ocr_name(&self) -> String {
        let idx = self.ocr_index() as usize;
        config::options::OCR_MODELS
            .get(idx)
            .unwrap_or(&"48px (Standard)")
            .to_string()
    }

    /// Get the currently selected inpainter index.
    pub fn inpainter_index(&self) -> u32 {
        let priv_ = self.imp();
        priv_
            .inpainter_row
            .borrow()
            .as_ref()
            .map(|r| r.selected())
            .unwrap_or(0)
    }

    /// Get the inpainter name string.
    pub fn inpainter_name(&self) -> String {
        let idx = self.inpainter_index() as usize;
        config::options::INPAINTERS
            .get(idx)
            .unwrap_or(&"Standard (LaMA)")
            .to_string()
    }

    /// Get the currently selected upscaler index.
    pub fn upscaler_index(&self) -> u32 {
        let priv_ = self.imp();
        priv_
            .upscaler_row
            .borrow()
            .as_ref()
            .map(|r| r.selected())
            .unwrap_or(0)
    }

    /// Get the upscaler name string.
    pub fn upscaler_name(&self) -> String {
        let idx = self.upscaler_index() as usize;
        config::options::UPSCALERS
            .get(idx)
            .unwrap_or(&"Keiner")
            .to_string()
    }

    /// Get the currently selected device name string ("cuda" or "cpu").
    pub fn device_name(&self) -> String {
        let priv_ = self.imp();
        let idx = priv_
            .device_row
            .borrow()
            .as_ref()
            .map(|r| r.selected())
            .unwrap_or(0);
        match idx {
            0 => "cuda".to_string(),
            _ => "cpu".to_string(),
        }
    }

    /// Check if RTL direction is enabled.
    pub fn is_rtl(&self) -> bool {
        let priv_ = self.imp();
        priv_
            .rtl_row
            .borrow()
            .as_ref()
            .map(|r| r.is_active())
            .unwrap_or(false)
    }

    /// Check if mocr merge is enabled.
    pub fn is_mocr_merge(&self) -> bool {
        let priv_ = self.imp();
        priv_
            .mocr_merge_row
            .borrow()
            .as_ref()
            .map(|r| r.is_active())
            .unwrap_or(false)
    }

    pub fn renderer(&self) -> String {
        let priv_ = self.imp();
        priv_
            .renderer_row
            .borrow()
            .as_ref()
            .map(|r| {
                let idx = r.selected() as usize;
                config::options::RENDERER_MAP
                    .get(idx)
                    .unwrap_or(&"default")
                    .to_string()
            })
            .unwrap_or_else(|| "default".into())
    }

    pub fn alignment(&self) -> String {
        let priv_ = self.imp();
        priv_
            .alignment_row
            .borrow()
            .as_ref()
            .map(|r| {
                let idx = r.selected() as usize;
                config::options::ALIGNMENT_MAP
                    .get(idx)
                    .unwrap_or(&"auto")
                    .to_string()
            })
            .unwrap_or_else(|| "auto".into())
    }

    pub fn render_direction(&self) -> String {
        let priv_ = self.imp();
        priv_
            .direction_row
            .borrow()
            .as_ref()
            .map(|r| {
                let idx = r.selected() as usize;
                config::options::DIRECTION_MAP
                    .get(idx)
                    .unwrap_or(&"auto")
                    .to_string()
            })
            .unwrap_or_else(|| "auto".into())
    }

    pub fn is_disable_font_border(&self) -> bool {
        let priv_ = self.imp();
        priv_
            .disable_font_border_row
            .borrow()
            .as_ref()
            .map(|r| r.is_active())
            .unwrap_or(false)
    }

    pub fn font_size_offset(&self) -> i32 {
        let priv_ = self.imp();
        priv_
            .font_size_offset_row
            .borrow()
            .as_ref()
            .map(|r| r.value() as i32)
            .unwrap_or(0)
    }

    pub fn font_color(&self) -> String {
        let priv_ = self.imp();
        let is_custom = priv_
            .font_color_switch
            .borrow()
            .as_ref()
            .map(|s| s.is_active())
            .unwrap_or(false);
        if !is_custom {
            return String::new();
        }
        let fg = priv_
            .font_color_fg_btn
            .borrow()
            .as_ref()
            .map(|b| rgba_to_hex(&b.rgba()));
        let bg = priv_
            .font_color_bg_btn
            .borrow()
            .as_ref()
            .map(|b| rgba_to_hex(&b.rgba()));
        match (fg, bg) {
            (Some(f), Some(b)) => format!("{}:{}", f, b),
            (Some(f), None) => f,
            _ => String::new(),
        }
    }

    pub fn mask_dilation_offset(&self) -> i32 {
        let priv_ = self.imp();
        priv_
            .mask_dilation_offset_row
            .borrow()
            .as_ref()
            .map(|r| r.value() as i32)
            .unwrap_or(20)
    }

    pub fn inpainting_size(&self) -> u32 {
        let priv_ = self.imp();
        priv_
            .inpainting_size_row
            .borrow()
            .as_ref()
            .map(|r| r.value() as u32)
            .unwrap_or(2048)
    }

    pub fn inpainting_precision(&self) -> String {
        let priv_ = self.imp();
        priv_
            .inpainting_precision_row
            .borrow()
            .as_ref()
            .map(|r| {
                let idx = r.selected() as usize;
                config::options::INPAINTING_PRECISION_MAP
                    .get(idx)
                    .unwrap_or(&"bf16")
                    .to_string()
            })
            .unwrap_or_else(|| "bf16".into())
    }

    pub fn detection_size(&self) -> u32 {
        let priv_ = self.imp();
        priv_
            .detection_size_row
            .borrow()
            .as_ref()
            .map(|r| r.value() as u32)
            .unwrap_or(2048)
    }

    pub fn is_rtl_render(&self) -> bool {
        let priv_ = self.imp();
        priv_
            .rtl_row
            .borrow()
            .as_ref()
            .map(|r| r.is_active())
            .unwrap_or(true)
    }

    /// Get the configured output directory path.
    ///
    /// Returns the path from config if set, or an empty string otherwise.
    pub fn output_directory(&self) -> String {
        let priv_ = self.imp();
        if let Some(cfg) = priv_.config.borrow().as_ref() {
            return cfg.borrow().settings.output_directory.clone();
        }
        String::new()
    }

    /// Check if "use original folder" (save in source folder) is enabled.
    ///
    /// Reads from the UI switch row if available, otherwise falls back to
    /// the config value.
    pub fn use_original_folder(&self) -> bool {
        let priv_ = self.imp();
        if let Some(row) = priv_.use_original_folder_row.borrow().as_ref() {
            return row.is_active();
        }
        // Fallback: read from config
        if let Some(cfg) = priv_.config.borrow().as_ref() {
            return cfg.borrow().settings.use_original_folder;
        }
        false
    }

    /// Get the currently selected UI language code from the language combo row.
    pub fn ui_language_code(&self) -> String {
        let priv_ = self.imp();
        if let Some(row) = priv_.language_row.borrow().as_ref() {
            let idx = row.selected() as usize;
            if let Some((code, _)) = i18n::SUPPORTED_LANGUAGES.get(idx) {
                return code.to_string();
            }
        }
        String::new()
    }

    /// Build translation parameters from current UI state.
    ///
    /// This creates a `TranslationParams` instance that can be
    /// passed to the Python bridge's `translate()` method.
    pub fn build_translation_params(&self) -> crate::ipc_bridge::TranslationParams {
        // Extract API keys from config for VLM mode and translator pre-flight checks
        let (gemini_key, openrouter_key, api_keys) = {
            let priv_ = self.imp();
            if let Some(cfg) = priv_.config.borrow().as_ref() {
                let keys = &cfg.borrow().api_keys;
                let mut map = HashMap::new();
                if !keys.deepl.is_empty() {
                    map.insert("deepl".into(), keys.deepl.clone());
                }
                if !keys.openai.is_empty() {
                    map.insert("openai".into(), keys.openai.clone());
                }
                if !keys.gemini.is_empty() {
                    map.insert("gemini".into(), keys.gemini.clone());
                }
                if !keys.deepseek.is_empty() {
                    map.insert("deepseek".into(), keys.deepseek.clone());
                }
                if !keys.groq.is_empty() {
                    map.insert("groq".into(), keys.groq.clone());
                }
                if !keys.openrouter.is_empty() {
                    map.insert("openrouter".into(), keys.openrouter.clone());
                }
                if !keys.baidu_app_id.is_empty() {
                    map.insert("baidu_app_id".into(), keys.baidu_app_id.clone());
                }
                if !keys.baidu_secret_key.is_empty() {
                    map.insert("baidu_secret_key".into(), keys.baidu_secret_key.clone());
                }
                if !keys.caiyun_token.is_empty() {
                    map.insert("caiyun_token".into(), keys.caiyun_token.clone());
                }
                (keys.gemini.clone(), keys.openrouter.clone(), map)
            } else {
                (String::new(), String::new(), HashMap::new())
            }
        };

        crate::ipc_bridge::TranslationParams {
            translator: self.translator_name(),
            target_lang: self.target_language(),
            detector: self.detector_name(),
            ocr: self.ocr_name(),
            inpainter: self.inpainter_name(),
            upscaler: self.upscaler_name(),
            direction: if self.is_rtl() {
                "rtl".to_string()
            } else {
                "auto".to_string()
            },
            use_mocr_merge: self.is_mocr_merge(),
            device: self.device_name(),
            output_directory: self.output_directory(),
            use_original_folder: self.use_original_folder(),
            // VLM-specific fields
            translation_mode: self.translation_mode(),
            vlm_type: self.vlm_type(),
            gemini_model: self.gemini_model(),
            openrouter_model: self.openrouter_model(),
            local_model_path: self.local_model_path(),
            project_name: self.project_name(),
            // API keys for VLM backends
            gemini_api_key: gemini_key,
            openrouter_api_key: openrouter_key,
            upscale_ratio: self.upscale_ratio(),
            renderer: self.renderer(),
            alignment: self.alignment(),
            disable_font_border: self.is_disable_font_border(),
            font_size_offset: self.font_size_offset(),
            font_size_minimum: {
                let priv_ = self.imp();
                priv_
                    .config
                    .borrow()
                    .as_ref()
                    .map(|cfg| cfg.borrow().settings.font_size_minimum)
                    .unwrap_or(-1)
            },
            render_direction: self.render_direction(),
            uppercase: {
                let priv_ = self.imp();
                priv_
                    .config
                    .borrow()
                    .as_ref()
                    .map(|cfg| cfg.borrow().settings.uppercase)
                    .unwrap_or(false)
            },
            lowercase: {
                let priv_ = self.imp();
                priv_
                    .config
                    .borrow()
                    .as_ref()
                    .map(|cfg| cfg.borrow().settings.lowercase)
                    .unwrap_or(false)
            },
            font_color: self.font_color(),
            no_hyphenation: {
                let priv_ = self.imp();
                priv_
                    .config
                    .borrow()
                    .as_ref()
                    .map(|cfg| cfg.borrow().settings.no_hyphenation)
                    .unwrap_or(false)
            },
            line_spacing: {
                let priv_ = self.imp();
                priv_
                    .config
                    .borrow()
                    .as_ref()
                    .map(|cfg| cfg.borrow().settings.line_spacing)
                    .unwrap_or(0)
            },
            font_size: {
                let priv_ = self.imp();
                priv_
                    .config
                    .borrow()
                    .as_ref()
                    .map(|cfg| cfg.borrow().settings.font_size)
                    .unwrap_or(0)
            },
            rtl: self.is_rtl_render(),
            mask_dilation_offset: self.mask_dilation_offset(),
            kernel_size: {
                let priv_ = self.imp();
                priv_
                    .config
                    .borrow()
                    .as_ref()
                    .map(|cfg| cfg.borrow().settings.kernel_size)
                    .unwrap_or(3)
            },
            inpainting_size: self.inpainting_size(),
            inpainting_precision: self.inpainting_precision(),
            detection_size: self.detection_size(),
            api_keys,
        }
    }
}

// ---------------------------------------------------------------------------
// Default impl
// ---------------------------------------------------------------------------

impl Default for SettingsPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setting_kind_equality() {
        assert_eq!(SettingKind::Translator, SettingKind::Translator);
        assert_ne!(SettingKind::Translator, SettingKind::Detector);
    }

    #[test]
    fn test_setting_kind_debug() {
        let kind = SettingKind::TargetLanguage;
        let debug_str = format!("{:?}", kind);
        assert!(debug_str.contains("TargetLanguage"));
    }

    #[test]
    fn test_panel_default_creation() {
        // Just verify the type can be referenced
        let _ = SettingsPanel::default();
    }

    #[test]
    fn test_build_translation_params_defaults() {
        let panel = SettingsPanel::new();
        let params = panel.build_translation_params();
        assert_eq!(params.target_lang, "DEU");
        assert_eq!(params.direction, "auto");
        assert!(!params.use_mocr_merge);
    }

    #[test]
    fn test_getter_fallbacks() {
        let panel = SettingsPanel::new();
        assert_eq!(panel.translator_index(), 0);
        assert_eq!(panel.detector_index(), 0);
        assert_eq!(panel.ocr_index(), 0);
        assert_eq!(panel.inpainter_index(), 0);
        assert_eq!(panel.upscaler_index(), 0);
        assert!(!panel.is_rtl());
        assert!(!panel.is_mocr_merge());
    }

    #[test]
    fn test_translator_name_default() {
        let panel = SettingsPanel::new();
        assert_eq!(panel.translator_name(), "Lokal (Offline)");
    }

    #[test]
    fn test_target_language_default() {
        let panel = SettingsPanel::new();
        assert_eq!(panel.target_language(), "DEU");
    }

    #[test]
    fn test_detector_name_default() {
        let panel = SettingsPanel::new();
        assert_eq!(panel.detector_name(), "Standard (CTD)");
    }

    #[test]
    fn test_ocr_name_default() {
        let panel = SettingsPanel::new();
        assert_eq!(panel.ocr_name(), "48px (Standard)");
    }

    #[test]
    fn test_inpainter_name_default() {
        let panel = SettingsPanel::new();
        assert_eq!(panel.inpainter_name(), "Standard (LaMA)");
    }

    #[test]
    fn test_upscaler_name_default() {
        let panel = SettingsPanel::new();
        assert_eq!(panel.upscaler_name(), "Keiner");
    }
}

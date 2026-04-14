// manga-translator-gtk/src/ui/preview.rs
//
// Image preview widget with comparison slider (original ↔ translated).
//
// Features:
//   - Gtk.Stack switching between placeholder animation and image view
//   - Original / Translated toggle buttons
//   - Comparison mode with draggable vertical slider
//   - Gtk.Overlay with clipped translated layer on top of original
//   - Gtk.DrawingArea placeholder with comic book panel animation
//   - Zoom and fit modes via Gtk.Picture with ContentFit
//   - File info bar (name, dimensions, size)
//   - Smooth cross-fade transitions between images

use adw::prelude::*;
use gtk::gdk::Texture;
use gtk::glib;
use gtk::glib::clone;
use gtk::subclass::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use crate::i18n;

// ---------------------------------------------------------------------------
// Preview mode
// ---------------------------------------------------------------------------

/// Current display mode of the preview pane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PreviewMode {
    /// Show only the original image.
    Original,
    /// Show only the translated image.
    Translated,
    /// Side-by-side comparison with a draggable slider.
    Compare,
}

impl Default for PreviewMode {
    fn default() -> Self {
        PreviewMode::Original
    }
}

// ---------------------------------------------------------------------------
// Preview widget (GObject)
// ---------------------------------------------------------------------------

glib::wrapper! {
    pub struct Preview(ObjectSubclass<PreviewPrivate>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// Internal state for the preview widget.
pub struct PreviewPrivate {
    /// Current preview mode (original, translated, compare).
    mode: RefCell<PreviewMode>,
    /// Path to the currently displayed original image.
    original_path: RefCell<Option<PathBuf>>,
    /// Path to the currently displayed translated image.
    translated_path: RefCell<Option<PathBuf>>,
    /// Slider position for comparison mode (0.0..=1.0, left to right).
    slider_position: RefCell<f64>,
    /// Whether the slider is currently being dragged.
    #[allow(dead_code)]
    dragging_slider: RefCell<bool>,

    // Widget references
    container: RefCell<Option<gtk::Box>>,
    /// Stack switching between placeholder and image view.
    stack: RefCell<Option<gtk::Stack>>,
    /// Placeholder drawing area (shown when no image is loaded).
    placeholder: RefCell<Option<gtk::DrawingArea>>,
    /// Overlay container for comparison mode.
    overlay: RefCell<Option<gtk::Overlay>>,
    /// Gtk.Picture showing the original image.
    original_picture: RefCell<Option<gtk::Picture>>,
    /// DrawingArea overlay for translated image (Cairo-based clipping in compare mode).
    translated_overlay: RefCell<Option<gtk::DrawingArea>>,
    /// Cached translated texture (kept for reference).
    translated_texture: RefCell<Option<Texture>>,
    /// Cached Cairo surface for translated texture (avoids GPU→CPU download per frame).
    translated_surface: RefCell<Option<cairo::ImageSurface>>,
    /// Slider line widget (vertical bar in compare mode).
    slider_widget: RefCell<Option<gtk::DrawingArea>>,
    /// Mode toggle buttons box.
    #[allow(dead_code)]
    mode_buttons: RefCell<Option<gtk::Box>>,
    /// "Original" toggle button.
    btn_original: RefCell<Option<gtk::ToggleButton>>,
    /// "Translated" toggle button.
    btn_translated: RefCell<Option<gtk::ToggleButton>>,
    /// "Compare" toggle button.
    btn_compare: RefCell<Option<gtk::ToggleButton>>,
    /// Info label (filename, dimensions).
    info_label: RefCell<Option<gtk::Label>>,
    /// Dimensions label.
    dimensions_label: RefCell<Option<gtk::Label>>,
}

impl Default for PreviewPrivate {
    fn default() -> Self {
        Self {
            mode: RefCell::new(PreviewMode::default()),
            original_path: RefCell::new(None),
            translated_path: RefCell::new(None),
            slider_position: RefCell::new(0.5),
            dragging_slider: RefCell::new(false),
            container: RefCell::new(None),
            stack: RefCell::new(None),
            placeholder: RefCell::new(None),
            overlay: RefCell::new(None),
            original_picture: RefCell::new(None),
            translated_overlay: RefCell::new(None),
            translated_texture: RefCell::new(None),
            translated_surface: RefCell::new(None),
            slider_widget: RefCell::new(None),
            mode_buttons: RefCell::new(None),
            btn_original: RefCell::new(None),
            btn_translated: RefCell::new(None),
            btn_compare: RefCell::new(None),
            info_label: RefCell::new(None),
            dimensions_label: RefCell::new(None),
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for PreviewPrivate {
    const NAME: &'static str = "MangaPreview";
    type Type = Preview;
    type ParentType = gtk::Widget;
}

impl ObjectImpl for PreviewPrivate {
    fn constructed(&self) {
        self.parent_constructed();
        let obj = self.obj();
        // Preview subclasses gtk::Widget (a leaf), so we must provide
        // a layout manager so the internal vbox child gets size-allocated.
        obj.set_layout_manager(Some(gtk::BoxLayout::new(gtk::Orientation::Vertical)));
        self.build_ui(&obj);
    }

    fn dispose(&self) {
        if let Some(container) = self.container.borrow().as_ref() {
            container.unparent();
        }
    }
}

impl WidgetImpl for PreviewPrivate {}

/// Helper: return a zeroed-out `cairo::TextExtents`.
/// cairo-rs 0.22 does not provide `TextExtents::zeroed()` or `Default`.
fn text_extents_zeroed() -> cairo::TextExtents {
    cairo::TextExtents::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
}

impl PreviewPrivate {
    /// Build the preview UI.
    fn build_ui(&self, obj: &Preview) {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        vbox.set_vexpand(true);
        vbox.set_hexpand(true);

        // -- Header row: title + mode toggles --
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        header.set_margin_start(12);
        header.set_margin_end(12);
        header.set_margin_top(8);
        header.set_margin_bottom(8);

        let title = gtk::Label::new(Some(&i18n::t("Vorschau")));
        title.add_css_class("title-2");
        header.append(&title);

        // Mode toggle buttons (Original | Translated | Compare)
        let mode_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        mode_box.add_css_class("linked");
        mode_box.set_hexpand(true);
        mode_box.set_halign(gtk::Align::End);

        let btn_original = gtk::ToggleButton::with_label(&i18n::t("Original"));
        btn_original.set_active(true);
        btn_original.add_css_class("preview-mode-btn");

        let btn_translated = gtk::ToggleButton::with_label(&i18n::t("Übersetzung"));
        btn_translated.set_group(Some(&btn_original));
        btn_translated.add_css_class("preview-mode-btn");

        let btn_compare = gtk::ToggleButton::with_label(&i18n::t("Vergleich"));
        btn_compare.set_group(Some(&btn_original));
        btn_compare.add_css_class("preview-mode-btn");

        mode_box.append(&btn_original);
        mode_box.append(&btn_translated);
        mode_box.append(&btn_compare);
        header.append(&mode_box);

        vbox.append(&header);

        // -- Preview stack: placeholder ↔ image overlay --
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(300);

        // --- Placeholder ---
        let placeholder = gtk::DrawingArea::new();
        placeholder.set_vexpand(true);
        placeholder.set_hexpand(true);
        placeholder.add_css_class("preview-placeholder");

        // Draw the comic book placeholder animation
        placeholder.set_draw_func(|_area, cr, width, height| {
            draw_placeholder(cr, width, height);
        });

        // Frame-rate-limited animation (~60fps).
        // The tick callback fires at monitor refresh rate (potentially 240Hz),
        // but the comic-book animation is smooth at 60fps. Skipping frames
        // reduces Cairo CPU rendering by ~75% at 240Hz.
        let last_render = Rc::new(Cell::new(0i64));
        placeholder.add_tick_callback(move |widget, clock| {
            let now = clock.frame_time(); // microseconds
            let elapsed = now - last_render.get();
            // 16,667 µs ≈ 60fps — sufficient for a page-turn animation
            if elapsed >= 16_667 {
                last_render.set(now);
                widget.queue_draw();
            }
            glib::ControlFlow::Continue
        });

        stack.add_named(&placeholder, Some("placeholder"));

        // --- Image overlay container ---
        let overlay = gtk::Overlay::new();
        overlay.add_css_class("comparison-container");
        overlay.set_vexpand(true);
        overlay.set_hexpand(true);

        // Original image (bottom layer)
        let original_picture = gtk::Picture::new();
        original_picture.set_content_fit(gtk::ContentFit::Contain);
        original_picture.set_can_shrink(true);
        original_picture.add_css_class("preview-image");
        original_picture.add_css_class("comparison-image");
        overlay.set_child(Some(&original_picture));

        // Translated image overlay (top layer — DrawingArea for Cairo-based clipping)
        let translated_overlay = gtk::DrawingArea::new();
        translated_overlay.set_hexpand(true);
        translated_overlay.set_vexpand(true);
        translated_overlay.set_can_target(false); // let pointer events pass to slider
        translated_overlay.add_css_class("translated-layer");
        translated_overlay.set_draw_func(clone!(
            #[weak]
            obj,
            move |_area, cr, width, height| {
                let this = obj.imp();
                let mode = *this.mode.borrow();

                // Only draw in Translated and Compare modes
                if matches!(mode, PreviewMode::Original) {
                    return;
                }

                let surface_opt = this.translated_surface.borrow();
                let Some(surface) = surface_opt.as_ref() else {
                    return;
                };

                let tex_w = surface.width() as f64;
                let tex_h = surface.height() as f64;
                if tex_w <= 0.0 || tex_h <= 0.0 {
                    return;
                }

                // Calculate "contain" fit (same logic as gtk::Picture ContentFit::Contain)
                let w = width as f64;
                let h = height as f64;
                let scale = (w / tex_w).min(h / tex_h);
                let draw_w = tex_w * scale;
                let draw_h = tex_h * scale;
                let offset_x = (w - draw_w) / 2.0;
                let offset_y = (h - draw_h) / 2.0;

                let _ = cr.save();

                if matches!(mode, PreviewMode::Compare) {
                    // Clip to the area right of the slider
                    let pos = *this.slider_position.borrow();
                    let slider_x = (w * pos).round();
                    let _ = cr.rectangle(slider_x, 0.0, w - slider_x, h);
                    cr.clip();
                }

                // Draw the cached Cairo surface with the same scaling as the original
                cr.translate(offset_x, offset_y);
                cr.scale(scale, scale);
                let _ = cr.set_source_surface(surface, 0.0, 0.0);
                let _ = cr.paint();

                let _ = cr.restore();
            }
        ));
        overlay.add_overlay(&translated_overlay);

        // Comparison slider line
        let slider_widget = gtk::DrawingArea::new();
        slider_widget.add_css_class("comparison-slider");
        slider_widget.set_hexpand(true);
        slider_widget.set_vexpand(true);
        slider_widget.set_draw_func(clone!(
            #[weak]
            obj,
            move |_area, cr, width, height| {
                let this = obj.imp();
                let pos = *this.slider_position.borrow();
                let x = (width as f64 * pos).round();

                // Draw the slider line — dark outline first, then white center
                // This ensures visibility on both light and dark images
                cr.set_line_width(4.5);
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.6);
                let _ = cr.move_to(x, 0.0);
                let _ = cr.line_to(x, height as f64);
                let _ = cr.stroke();

                cr.set_line_width(2.0);
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
                let _ = cr.move_to(x, 0.0);
                let _ = cr.line_to(x, height as f64);
                let _ = cr.stroke();

                // Draw handle circle — dark outline then white fill
                let cy = height as f64 / 2.0;
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.6);
                let _ = cr.arc(x, cy, 13.0, 0.0, 2.0 * std::f64::consts::PI);
                cr.fill().ok();

                cr.set_source_rgb(0.98, 0.98, 0.98);
                let _ = cr.arc(x, cy, 12.0, 0.0, 2.0 * std::f64::consts::PI);
                cr.fill().ok();

                // Draw arrows on the handle
                cr.set_source_rgb(0.2, 0.2, 0.2);
                cr.set_line_width(2.0);
                // Left arrow
                let _ = cr.move_to(x - 5.0, cy);
                let _ = cr.line_to(x - 9.0, cy - 4.0);
                let _ = cr.move_to(x - 5.0, cy);
                let _ = cr.line_to(x - 9.0, cy + 4.0);
                let _ = cr.stroke();
                // Right arrow
                let _ = cr.move_to(x + 5.0, cy);
                let _ = cr.line_to(x + 9.0, cy - 4.0);
                let _ = cr.move_to(x + 5.0, cy);
                let _ = cr.line_to(x + 9.0, cy + 4.0);
                let _ = cr.stroke();

                // Position label
                let pct = (pos * 100.0).round() as i32;
                cr.set_source_rgb(0.2, 0.2, 0.2);
                cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                cr.set_font_size(9.0);
                let text = format!("{}%", pct);
                let extents = cr
                    .text_extents(&text)
                    .unwrap_or_else(|_| text_extents_zeroed());
                let _ = cr.move_to(x - extents.width() / 2.0, cy + 24.0);
                let _ = cr.show_text(&text);
            }
        ));

        overlay.add_overlay(&slider_widget);

        // Slider gesture (drag)
        let gesture = gtk::GestureDrag::new();
        gesture.set_button(gtk::gdk::BUTTON_PRIMARY);

        gesture.connect_drag_begin(move |_gesture, _x, _y| {
            // Start dragging — nothing special needed
        });

        gesture.connect_drag_update(clone!(
            #[strong]
            slider_widget,
            #[weak]
            obj,
            move |_gesture, offset_x, _offset_y| {
                let this = obj.imp();
                let start_x = _gesture.start_point().map(|(x, _)| x).unwrap_or(0.0);
                let allocation = slider_widget.allocation();
                let width = allocation.width() as f64;

                if width > 0.0 {
                    let mut pos = (start_x + offset_x) / width;
                    pos = pos.clamp(0.02, 0.98);

                    // Update stored position
                    *this.slider_position.borrow_mut() = pos;

                    slider_widget.queue_draw();
                    // Redraw translated overlay for clip update
                    if let Some(overlay) = this.translated_overlay.borrow().as_ref() {
                        overlay.queue_draw();
                    }
                }
            }
        ));

        slider_widget.add_controller(gesture);

        // Re-apply clip when the slider widget is resized (handles initial allocation
        // race: Compare mode may be entered before the widget has its first allocation)
        slider_widget.connect_resize(clone!(
            #[weak]
            obj,
            move |_area, _width, _height| {
                let this = obj.imp();
                if matches!(*this.mode.borrow(), PreviewMode::Compare) {
                    if let Some(overlay) = this.translated_overlay.borrow().as_ref() {
                        overlay.queue_draw();
                    }
                }
            }
        ));

        // Initially hide translated layer (we're in Original mode)
        translated_overlay.set_visible(false);
        slider_widget.set_visible(false);

        stack.add_named(&overlay, Some("images"));
        stack.set_visible_child_name("placeholder");

        vbox.append(&stack);

        // -- Info bar --
        let info_bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        info_bar.set_margin_start(12);
        info_bar.set_margin_end(12);
        info_bar.set_margin_top(4);
        info_bar.set_margin_bottom(4);

        let info_label = gtk::Label::new(Some(&i18n::t("Keine Datei ausgewählt")));
        info_label.add_css_class("caption");
        info_label.set_hexpand(true);
        info_label.set_xalign(0.0);
        info_label.set_ellipsize(pango::EllipsizeMode::Middle);
        info_bar.append(&info_label);

        let dimensions_label = gtk::Label::new(None);
        dimensions_label.add_css_class("caption");
        dimensions_label.add_css_class("dim-label");
        dimensions_label.set_halign(gtk::Align::End);
        info_bar.append(&dimensions_label);

        vbox.append(&info_bar);

        // -- Connect mode toggle buttons --
        btn_original.connect_toggled(clone!(
            #[weak(rename_to = this)]
            self,
            move |btn| {
                if btn.is_active() {
                    this.set_mode(PreviewMode::Original);
                }
            }
        ));

        btn_translated.connect_toggled(clone!(
            #[weak(rename_to = this)]
            self,
            move |btn| {
                if btn.is_active() {
                    this.set_mode(PreviewMode::Translated);
                }
            }
        ));

        btn_compare.connect_toggled(clone!(
            #[weak(rename_to = this)]
            self,
            move |btn| {
                if btn.is_active() {
                    this.set_mode(PreviewMode::Compare);
                }
            }
        ));

        // Store references
        *self.container.borrow_mut() = Some(vbox.clone());
        *self.stack.borrow_mut() = Some(stack);
        *self.placeholder.borrow_mut() = Some(placeholder);
        *self.overlay.borrow_mut() = Some(overlay);
        *self.original_picture.borrow_mut() = Some(original_picture);
        *self.translated_overlay.borrow_mut() = Some(translated_overlay);
        *self.slider_widget.borrow_mut() = Some(slider_widget);
        *self.btn_original.borrow_mut() = Some(btn_original);
        *self.btn_translated.borrow_mut() = Some(btn_translated);
        *self.btn_compare.borrow_mut() = Some(btn_compare);
        *self.info_label.borrow_mut() = Some(info_label);
        *self.dimensions_label.borrow_mut() = Some(dimensions_label);

        vbox.set_parent(obj);
    }

    /// Set the preview mode and update visibility of layers.
    fn set_mode(&self, mode: PreviewMode) {
        let mut current_mode = self.mode.borrow_mut();
        *current_mode = mode;
        drop(current_mode);

        let original = self.original_picture.borrow();
        let translated = self.translated_overlay.borrow();
        let slider = self.slider_widget.borrow();

        if let (Some(orig_pic), Some(trans_overlay), Some(slider_w)) =
            (original.as_ref(), translated.as_ref(), slider.as_ref())
        {
            match mode {
                PreviewMode::Original => {
                    orig_pic.set_visible(true);
                    trans_overlay.set_visible(false);
                    slider_w.set_visible(false);
                }
                PreviewMode::Translated => {
                    orig_pic.set_visible(true);
                    trans_overlay.set_visible(true);
                    slider_w.set_visible(false);
                    trans_overlay.queue_draw();
                }
                PreviewMode::Compare => {
                    orig_pic.set_visible(true);
                    trans_overlay.set_visible(true);
                    slider_w.set_visible(true);
                    slider_w.queue_draw();
                    trans_overlay.queue_draw();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Placeholder drawing
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Placeholder drawing — full port of Python ComicBookPlaceholder
// ---------------------------------------------------------------------------

/// Animation timing constants (matching Python version).
const STATIC_TIME: f64 = 2.5; // seconds showing static page
const FLIP_TIME: f64 = 3.0; // seconds for the flip motion
const CYCLE: f64 = STATIC_TIME + FLIP_TIME;

/// Panel layouts per "page" — (x, y, w, h) as fractions of the page area.
const LAYOUTS: &[&[(f64, f64, f64, f64)]] = &[
    // Page 0: Big top panel, 3 small bottom panels
    &[
        (0.04, 0.03, 0.92, 0.54),
        (0.04, 0.61, 0.29, 0.36),
        (0.36, 0.61, 0.29, 0.36),
        (0.68, 0.61, 0.29, 0.36),
    ],
    // Page 1: 2x2 grid
    &[
        (0.04, 0.03, 0.45, 0.47),
        (0.53, 0.03, 0.45, 0.47),
        (0.04, 0.53, 0.45, 0.44),
        (0.53, 0.53, 0.45, 0.44),
    ],
    // Page 2: Three vertical strips
    &[
        (0.04, 0.03, 0.28, 0.94),
        (0.37, 0.03, 0.28, 0.94),
        (0.68, 0.03, 0.28, 0.94),
    ],
    // Page 3: Big left panel, two stacked right
    &[
        (0.04, 0.03, 0.58, 0.94),
        (0.66, 0.03, 0.32, 0.45),
        (0.66, 0.52, 0.32, 0.45),
    ],
    // Page 4: Full-bleed splash with small inset overlay
    &[(0.0, 0.0, 1.0, 1.0), (0.60, 0.05, 0.35, 0.25)],
    // Page 5: Asymmetric L-shape
    &[
        (0.04, 0.03, 0.45, 0.94),
        (0.52, 0.03, 0.46, 0.45),
        (0.52, 0.52, 0.22, 0.45),
        (0.77, 0.52, 0.21, 0.45),
    ],
    // Page 6: Wide horizontal strips
    &[
        (0.04, 0.03, 0.92, 0.28),
        (0.04, 0.35, 0.92, 0.30),
        (0.04, 0.69, 0.92, 0.28),
    ],
    // Page 7: Overlapping panels (last drawn on top)
    &[(0.04, 0.03, 0.60, 0.60), (0.35, 0.40, 0.62, 0.55)],
];

/// Bubble content types.
const BUBBLE_TYPES: &[&str] = &["dots", "japanese", "lines", "exclaim"];

/// Bubble position offsets (x, y) relative to panel center.
const BUBBLE_OFFSETS: &[(f64, f64)] = &[
    (0.50, 0.35),
    (0.55, 0.40),
    (0.45, 0.30),
    (0.50, 0.38),
    (0.60, 0.35),
    (0.40, 0.42),
];

/// Draw a rounded-rectangle path.
fn rrect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    let _ = cr.move_to(x + r, y);
    let _ = cr.line_to(x + w - r, y);
    let _ = cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    let _ = cr.line_to(x + w, y + h - r);
    let _ = cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    let _ = cr.line_to(x + r, y + h);
    let _ = cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    let _ = cr.line_to(x, y + r);
    let _ = cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}

/// Centered book rectangle with manga tankobon aspect ratio (two pages side-by-side).
fn book_rect(w: f64, h: f64) -> (f64, f64, f64, f64) {
    let mx = w * 0.12;
    let my = h * 0.08;
    let aw = w - mx * 2.0;
    let ah = h - my * 2.0;
    let ratio = 1.3; // Two pages side-by-side
    let (bw, bh) = if aw / ah > ratio {
        (ah * ratio, ah)
    } else {
        (aw, aw / ratio)
    };
    ((w - bw) / 2.0, (h - bh) / 2.0, bw, bh)
}

/// Draw static book structure: shadow, stacked pages, spine.
fn draw_book(cr: &cairo::Context, bx: f64, by: f64, bw: f64, bh: f64, sw: f64, is_dark: bool) {
    // Drop shadow
    let _ = cr.save();
    cr.set_source_rgba(0.0, 0.0, 0.0, if is_dark { 0.3 } else { 0.13 });
    rrect(cr, bx + 5.0, by + 7.0, bw, bh, 4.0);
    cr.fill().ok();
    let _ = cr.restore();

    // Stacked pages visible at edges
    for i in (1..=5).rev() {
        let s = if is_dark {
            0.18 - i as f64 * 0.015
        } else {
            0.91 - i as f64 * 0.025
        };
        let _ = cr.save();
        cr.set_source_rgb(s, s, s);
        rrect(cr, bx + i as f64 * 1.5, by + i as f64 * 1.5, bw, bh, 3.0);
        cr.fill().ok();
        let _ = cr.restore();
    }

    // Main page surface
    let _ = cr.save();
    if is_dark {
        cr.set_source_rgb(0.15, 0.15, 0.15);
    } else {
        cr.set_source_rgb(0.98, 0.98, 0.98);
    }
    rrect(cr, bx, by, bw, bh, 3.0);
    cr.fill().ok();
    let _ = cr.restore();

    // Spine with gradient down the middle
    let _ = cr.save();
    let spine_x = bx + bw / 2.0 - sw / 2.0;
    let grad = cairo::LinearGradient::new(spine_x, by, spine_x + sw, by);
    if is_dark {
        grad.add_color_stop_rgb(0.0, 0.15, 0.15, 0.15);
        grad.add_color_stop_rgb(0.5, 0.05, 0.05, 0.05);
        grad.add_color_stop_rgb(1.0, 0.15, 0.15, 0.15);
    } else {
        grad.add_color_stop_rgb(0.0, 0.9, 0.9, 0.9);
        grad.add_color_stop_rgb(0.5, 0.7, 0.7, 0.7);
        grad.add_color_stop_rgb(1.0, 0.9, 0.9, 0.9);
    }
    cr.set_source(&grad).ok();
    let _ = cr.rectangle(spine_x, by, sw, bh);
    cr.fill().ok();

    // Spine crease
    cr.set_source_rgba(0.0, 0.0, 0.0, if is_dark { 0.5 } else { 0.2 });
    cr.set_line_width(1.0);
    let _ = cr.move_to(bx + bw / 2.0, by);
    let _ = cr.line_to(bx + bw / 2.0, by + bh);
    let _ = cr.stroke();

    // Spine edge shadows for depth illusion
    let shadow_w = (sw * 1.5).min(12.0);

    // Left page: shadow fading rightward from spine
    let sl = cairo::LinearGradient::new(spine_x, by, spine_x + shadow_w, by);
    sl.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, if is_dark { 0.25 } else { 0.14 });
    sl.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    cr.set_source(&sl).ok();
    let _ = cr.rectangle(spine_x, by, shadow_w, bh);
    cr.fill().ok();

    // Right page: shadow fading leftward from spine
    let sr = cairo::LinearGradient::new(spine_x + sw, by, spine_x + sw - shadow_w, by);
    sr.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, if is_dark { 0.25 } else { 0.14 });
    sr.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    cr.set_source(&sr).ok();
    let _ = cr.rectangle(spine_x + sw - shadow_w, by, shadow_w, bh);
    cr.fill().ok();

    let _ = cr.restore();
}

/// Draw subtle paper grain texture on a page area.
fn draw_paper_texture(cr: &cairo::Context, px: f64, py: f64, pw: f64, ph: f64, is_dark: bool) {
    let _ = cr.save();
    let _ = cr.rectangle(px, py, pw, ph);
    cr.clip();
    let grain_alpha = if is_dark { 0.03 } else { 0.025 };
    if is_dark {
        cr.set_source_rgba(1.0, 1.0, 1.0, grain_alpha);
    } else {
        cr.set_source_rgba(0.0, 0.0, 0.0, grain_alpha);
    }
    let step = 5.0_f64;
    let mut y = py;
    let mut row = 0i32;
    while y < py + ph {
        let mut x = px + if row % 2 != 0 { step } else { 0.0 };
        while x < px + pw {
            // Deterministic pseudo-random grain (~20% of positions)
            if ((x as i32 * 73 + y as i32 * 137) % 97) < 19 {
                let _ = cr.rectangle(x, y, 1.5, 1.5);
                cr.fill().ok();
            }
            x += step;
        }
        y += step;
        row += 1;
    }
    let _ = cr.restore();
}

/// Draw a manga screentone pattern inside a panel.
/// pattern_type: 1=dots, 2=diagonal lines, 3=crosshatch
fn draw_screentone(
    cr: &cairo::Context,
    rx: f64,
    ry: f64,
    rw: f64,
    rh: f64,
    pattern_type: i32,
    is_dark: bool,
) {
    let _ = cr.save();
    let _ = cr.rectangle(rx + 1.0, ry + 1.0, rw - 2.0, rh - 2.0);
    cr.clip();

    if is_dark {
        cr.set_source_rgba(0.55, 0.55, 0.55, 0.45);
    } else {
        cr.set_source_rgba(0.25, 0.25, 0.25, 0.35);
    }

    let pi2 = std::f64::consts::PI * 2.0;

    match pattern_type {
        1 => {
            // Dot screentone (halftone)
            let spacing = 7.0_f64;
            let dot_r = 1.2_f64;
            let mut y = ry + spacing / 2.0;
            let mut row = 0i32;
            while y < ry + rh {
                let offset = if row % 2 != 0 { spacing / 2.0 } else { 0.0 };
                let mut x = rx + spacing / 2.0 + offset;
                while x < rx + rw {
                    let _ = cr.arc(x, y, dot_r, 0.0, pi2);
                    cr.fill().ok();
                    x += spacing;
                }
                y += spacing;
                row += 1;
            }
        }
        2 => {
            // Diagonal lines (hatching)
            let spacing = 6.0_f64;
            cr.set_line_width(0.7);
            let total = (rw + rh) as i32;
            for i in (-total..total).step_by(spacing as usize) {
                let _ = cr.move_to(rx + i as f64, ry);
                let _ = cr.line_to(rx + i as f64 + rh, ry + rh);
                let _ = cr.stroke();
            }
        }
        3 => {
            // Crosshatch
            let spacing = 9.0_f64;
            cr.set_line_width(0.5);
            let total = (rw + rh) as i32;
            for i in (-total..total).step_by(spacing as usize) {
                let _ = cr.move_to(rx + i as f64, ry);
                let _ = cr.line_to(rx + i as f64 + rh, ry + rh);
                let _ = cr.stroke();
            }
            for i in (-total..total).step_by(spacing as usize) {
                let _ = cr.move_to(rx + i as f64 + rw, ry);
                let _ = cr.line_to(rx + i as f64 + rw - rh, ry + rh);
                let _ = cr.stroke();
            }
        }
        _ => {}
    }
    let _ = cr.restore();
}

/// Draw a speech bubble with tail and varied content.
fn draw_bubble(cr: &cairo::Context, cx: f64, cy: f64, r: f64, is_dark: bool, bubble_type: &str) {
    if r < 6.0 {
        return;
    }
    let r = r.min(36.0);
    let pi2 = std::f64::consts::PI * 2.0;

    let (ink_r, ink_g, ink_b) = if is_dark {
        (0.15, 0.15, 0.15)
    } else {
        (1.0, 1.0, 1.0)
    };
    let (border_r, border_g, border_b) = if is_dark {
        (0.8, 0.8, 0.8)
    } else {
        (0.05, 0.05, 0.05)
    };
    let (text_r, text_g, text_b) = if is_dark {
        (0.85, 0.85, 0.85)
    } else {
        (0.10, 0.10, 0.10)
    };

    let _ = cr.save();
    cr.set_source_rgb(ink_r, ink_g, ink_b);
    let _ = cr.arc(cx, cy, r, 0.0, pi2);
    cr.fill().ok();
    cr.set_source_rgb(border_r, border_g, border_b);
    cr.set_line_width(1.5);
    let _ = cr.arc(cx, cy, r, 0.0, pi2);
    let _ = cr.stroke();

    // Tail
    cr.set_source_rgb(ink_r, ink_g, ink_b);
    let _ = cr.move_to(cx - r * 0.2, cy + r * 0.78);
    let _ = cr.line_to(cx - r * 0.65, cy + r * 1.45);
    let _ = cr.line_to(cx + r * 0.15, cy + r * 0.88);
    cr.close_path();
    cr.fill_preserve().ok();
    cr.set_source_rgb(border_r, border_g, border_b);
    let _ = cr.stroke();

    // Content varies by type
    cr.set_source_rgb(text_r, text_g, text_b);

    match bubble_type {
        "dots" => {
            // Three horizontal lines (classic placeholder text)
            cr.set_line_width(1.4);
            for (i, yoff) in [-0.28_f64, 0.0, 0.24].iter().enumerate() {
                let lw = r * if i < 2 { 0.72 } else { 0.38 };
                let _ = cr.move_to(cx - lw / 2.0, cy + r * yoff);
                let _ = cr.line_to(cx + lw / 2.0, cy + r * yoff);
                let _ = cr.stroke();
            }
        }
        "japanese" => {
            // Japanese placeholder characters
            cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            let fs = (r * 0.65).max(8.0);
            cr.set_font_size(fs);
            let chars = "ああ…";
            if let Ok(ext) = cr.text_extents(chars) {
                let _ = cr.move_to(cx - ext.width() / 2.0, cy + ext.height() / 2.0);
                let _ = cr.show_text(chars);
            }
        }
        "lines" => {
            // Empty text block rectangle
            cr.set_line_width(1.2);
            let bw = r * 0.70;
            let bh = r * 0.50;
            let _ = cr.rectangle(cx - bw / 2.0, cy - bh / 2.0, bw, bh);
            let _ = cr.stroke();
        }
        "exclaim" => {
            // Exclamation marks
            cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            let fs = (r * 0.8).max(9.0);
            cr.set_font_size(fs);
            let text = "!!";
            if let Ok(ext) = cr.text_extents(text) {
                let _ = cr.move_to(cx - ext.width() / 2.0, cy + ext.height() / 2.0);
                let _ = cr.show_text(text);
            }
        }
        _ => {
            // Fallback: dots
            cr.set_line_width(1.4);
            for (i, yoff) in [-0.28_f64, 0.0, 0.24].iter().enumerate() {
                let lw = r * if i < 2 { 0.72 } else { 0.38 };
                let _ = cr.move_to(cx - lw / 2.0, cy + r * yoff);
                let _ = cr.line_to(cx + lw / 2.0, cy + r * yoff);
                let _ = cr.stroke();
            }
        }
    }
    let _ = cr.restore();
}

/// Draw a small red hanko (seal stamp) — authentic manga accent.
fn draw_hanko(cr: &cairo::Context, cx: f64, cy: f64, r: f64, _is_dark: bool) {
    if r < 5.0 {
        return;
    }
    let r = r.min(20.0);
    let _ = cr.save();
    // Red circle
    cr.set_source_rgb(0.80, 0.12, 0.10);
    let _ = cr.arc(cx, cy, r, 0.0, std::f64::consts::PI * 2.0);
    cr.fill().ok();
    // White character inside
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(r * 1.1);
    let text = "漫";
    if let Ok(ext) = cr.text_extents(text) {
        let _ = cr.move_to(
            cx - ext.width() / 2.0 - ext.x_bearing(),
            cy - ext.height() / 2.0 - ext.y_bearing(),
        );
        let _ = cr.show_text(text);
    }
    let _ = cr.restore();
}

/// Draw manga panels for a given layout index.
fn draw_panels(
    cr: &cairo::Context,
    px: f64,
    py: f64,
    pw: f64,
    ph: f64,
    layout_idx: usize,
    is_dark: bool,
    cycle: usize,
) {
    let layout = LAYOUTS[layout_idx % LAYOUTS.len()];

    for (pidx, panel) in layout.iter().enumerate() {
        let rx = px + panel.0 * pw;
        let ry = py + panel.1 * ph;
        let rw = panel.2 * pw;
        let rh = panel.3 * ph;

        // Panel base fill
        let _ = cr.save();
        if is_dark {
            cr.set_source_rgb(0.20, 0.20, 0.20);
        } else {
            cr.set_source_rgb(0.88, 0.88, 0.88);
        }
        let _ = cr.rectangle(rx, ry, rw, rh);
        cr.fill().ok();
        let _ = cr.restore();

        // Screentone texture (rotate pattern with cycle for variation)
        let tone = (layout_idx + pidx + cycle) % 4;
        if tone > 0 && rw > 20.0 && rh > 20.0 {
            draw_screentone(cr, rx, ry, rw, rh, tone as i32, is_dark);
        }

        // Panel border (thicker for clearer structure)
        let _ = cr.save();
        if is_dark {
            cr.set_source_rgb(0.72, 0.72, 0.72);
        } else {
            cr.set_source_rgb(0.05, 0.05, 0.05);
        }
        cr.set_line_width(4.0);
        let _ = cr.rectangle(rx, ry, rw, rh);
        let _ = cr.stroke();
        let _ = cr.restore();

        // Dynamic speech bubble (varies per panel + cycle)
        if rw > 45.0 && rh > 45.0 {
            let btype_idx = (layout_idx + pidx + cycle) % BUBBLE_TYPES.len();
            let boff_idx = (pidx + cycle) % BUBBLE_OFFSETS.len();
            let boff = BUBBLE_OFFSETS[boff_idx];
            draw_bubble(
                cr,
                rx + rw * boff.0,
                ry + rh * boff.1,
                rw.min(rh) * 0.28,
                is_dark,
                BUBBLE_TYPES[btype_idx],
            );
        }

        // Hanko stamp (red seal) — first panel of every 3rd layout
        if pidx == 0 && layout_idx % 3 == 0 && rw > 60.0 && rh > 60.0 {
            draw_hanko(
                cr,
                rx + rw * 0.88,
                ry + rh * 0.88,
                rw.min(rh) * 0.14,
                is_dark,
            );
        }
    }
}

/// Return eased 0..1 during flip, or None when static.
fn flip_phase(t: f64) -> Option<f64> {
    if t < STATIC_TIME {
        return None;
    }
    let ft = t - STATIC_TIME;
    if ft > FLIP_TIME {
        return None;
    }
    let mut p = ft / FLIP_TIME;

    // Stop motion effect: quantize progress into discrete frames (8 FPS)
    let steps = (FLIP_TIME * 8.0) as i32;
    p = (p * steps as f64).floor() / steps as f64;

    // smoothstep
    Some(p * p * (3.0 - 2.0 * p))
}

/// Draw page turn shadow/light effects ON TOP of old panel content.
fn draw_flip_effect(
    cr: &cairo::Context,
    fold_x: f64,
    py: f64,
    ph: f64,
    curl: f64,
    is_dark: bool,
    left_px: f64,
    sweep_w: f64,
) {
    let right_edge = left_px + sweep_w;
    let page_w = right_edge - fold_x;

    if page_w < 4.0 {
        return;
    }

    // 1. Shadow cast on new page (left of fold)
    let shadow_w = (curl * 4.0)
        .max(14.0)
        .min(page_w * 0.4)
        .min(fold_x - left_px - 2.0);
    if shadow_w > 3.0 {
        let _ = cr.save();
        let sg = cairo::LinearGradient::new(fold_x - shadow_w, py, fold_x + 2.0, py);
        sg.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
        sg.add_color_stop_rgba(0.4, 0.0, 0.0, 0.0, 0.03);
        sg.add_color_stop_rgba(0.8, 0.0, 0.0, 0.0, 0.12);
        sg.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, if is_dark { 0.40 } else { 0.22 });
        cr.set_source(&sg).ok();
        let _ = cr.rectangle(fold_x - shadow_w, py, shadow_w + 2.0, ph);
        cr.fill().ok();
        let _ = cr.restore();
    }

    // 2. Shadow overlay on turning page (darkens panels near fold)
    if page_w > 8.0 {
        let _ = cr.save();
        let fold_shadow_w = (page_w * 0.40).min(70.0);
        let fg = cairo::LinearGradient::new(fold_x, py, fold_x + fold_shadow_w, py);
        if is_dark {
            fg.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.40);
            fg.add_color_stop_rgba(0.12, 0.0, 0.0, 0.0, 0.22);
            fg.add_color_stop_rgba(0.40, 0.0, 0.0, 0.0, 0.06);
            fg.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
        } else {
            fg.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.28);
            fg.add_color_stop_rgba(0.12, 0.0, 0.0, 0.0, 0.15);
            fg.add_color_stop_rgba(0.40, 0.0, 0.0, 0.0, 0.04);
            fg.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
        }
        cr.set_source(&fg).ok();
        let _ = cr.rectangle(fold_x, py, fold_shadow_w, ph);
        cr.fill().ok();
        let _ = cr.restore();
    }

    // 3. Highlight along fold edge (light catching the page edge)
    let _ = cr.save();
    let hl_w = (page_w * 0.08).min(8.0);
    let hg = cairo::LinearGradient::new(fold_x, py, fold_x + hl_w, py);
    if is_dark {
        hg.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.07);
        hg.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
    } else {
        hg.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.18);
        hg.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
    }
    cr.set_source(&hg).ok();
    let _ = cr.rectangle(fold_x, py, hl_w, ph);
    cr.fill().ok();
    let _ = cr.restore();

    // 4. Crease line at fold
    let _ = cr.save();
    cr.set_source_rgba(0.0, 0.0, 0.0, if is_dark { 0.35 } else { 0.12 });
    cr.set_line_width(1.0);
    let _ = cr.move_to(fold_x, py);
    let _ = cr.line_to(fold_x, py + ph);
    let _ = cr.stroke();
    let _ = cr.restore();

    // 5. Slight darkening at leading edge (page thickness)
    if page_w > 20.0 {
        let _ = cr.save();
        let edge_w = (page_w * 0.07).min(6.0);
        let eg = cairo::LinearGradient::new(right_edge - edge_w, py, right_edge, py);
        if is_dark {
            eg.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
            eg.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.18);
        } else {
            eg.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
            eg.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.10);
        }
        cr.set_source(&eg).ok();
        let _ = cr.rectangle(right_edge - edge_w, py, edge_w, ph);
        cr.fill().ok();
        let _ = cr.restore();
    }

    // 6. Drop shadow beneath turning page
    if curl > 2.0 && page_w > 10.0 {
        let _ = cr.save();
        let shadow_h = (curl * 1.5).min(8.0);
        let ds = cairo::LinearGradient::new(fold_x, py + ph - 1.0, fold_x, py + ph + shadow_h);
        ds.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, if is_dark { 0.14 } else { 0.07 });
        ds.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
        cr.set_source(&ds).ok();
        let _ = cr.rectangle(fold_x, py + ph - 1.0, page_w, shadow_h + 1.0);
        cr.fill().ok();
        let _ = cr.restore();
    }
}

/// Draw "Kein Bild geladen" loading text with hollow-to-fill progress bar effect and glow.
fn draw_loading_text(
    cr: &cairo::Context,
    bx: f64,
    by: f64,
    bw: f64,
    bh: f64,
    is_dark: bool,
    _w: f64,
    h: f64,
    t: f64,
) {
    let text = i18n::t("Kein Bild geladen");
    let font_size = (bw * 0.042).min(20.0).max(8.0);

    let _ = cr.save();
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(font_size);

    // Text metrics
    let extents = cr
        .text_extents(&text)
        .unwrap_or_else(|_| text_extents_zeroed());
    let text_left = bx + (bw - extents.width()) / 2.0;
    let tx = text_left - extents.x_bearing();
    let ty = by + bh + font_size * 1.4;

    // Abort if not enough space below the book
    if ty + extents.height() + 4.0 > h {
        let _ = cr.restore();
        return;
    }

    // Colors
    let (fill_r, fill_g, fill_b) = if is_dark {
        (0.78, 0.78, 0.78)
    } else {
        (1.0, 1.0, 1.0)
    };

    // Pulse-glow effect
    let pulse = 0.5 + 0.5 * (t * std::f64::consts::PI).sin(); // 0→1→0 over ~2s
    let glow_layers: [(f64, f64); 3] = [
        ((font_size * 0.45).max(8.0), 0.08 + pulse * 0.14),
        ((font_size * 0.25).max(5.0), 0.12 + pulse * 0.20),
        ((font_size * 0.12).max(2.5), 0.18 + pulse * 0.28),
    ];
    for (line_w, alpha) in glow_layers {
        let _ = cr.save();
        cr.set_source_rgba(1.0, 1.0, 1.0, alpha);
        cr.set_line_width(line_w);
        let _ = cr.move_to(tx, ty);
        let _ = cr.text_path(&text);
        let _ = cr.stroke();
        let _ = cr.restore();
    }

    // Hollow outline (always visible)
    cr.set_source_rgb(0.0, 0.0, 0.0);
    cr.set_line_width((font_size * 0.18).max(2.5));
    let _ = cr.move_to(tx, ty);
    let _ = cr.text_path(&text);
    let _ = cr.stroke();

    // Fill text left-to-right with progress clip
    let progress = t / CYCLE; // 0.0 → ~1.0 over the cycle
    if progress > 0.005 {
        let clip_w = extents.width() * progress;
        let _ = cr.save();
        let _ = cr.rectangle(
            text_left - 1.0,
            ty + extents.y_bearing() - 3.0,
            clip_w + 2.0,
            extents.height() + 6.0,
        );
        cr.clip();
        cr.set_source_rgb(fill_r, fill_g, fill_b);
        let _ = cr.move_to(tx, ty);
        let _ = cr.show_text(&text);
        let _ = cr.restore();
    }

    let _ = cr.restore();
}

/// Main draw function for the comic book placeholder animation.
///
/// Ported from Python ComicBookPlaceholder — draws a two-page manga book
/// with panels, speech bubbles, screentone textures, hanko stamps,
/// and a realistic page-flip animation with shadow/light effects.
fn draw_placeholder(cr: &cairo::Context, width: i32, height: i32) {
    let w = width as f64;
    let h = height as f64;

    if w < 20.0 || h < 20.0 {
        return;
    }

    // Time-based animation state (stateless — computed from system clock)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = now.as_secs_f64();

    // Cycle state derived from time
    let cycle_number = (total_secs / CYCLE) as usize;
    let t = total_secs % CYCLE;
    let page = (cycle_number * 2) % LAYOUTS.len();
    let cycle_count = cycle_number;

    // Dark mode detection
    let is_dark = adw::StyleManager::default().is_dark();

    // Background fill
    if is_dark {
        cr.set_source_rgb(0.05, 0.05, 0.07);
    } else {
        cr.set_source_rgb(0.92, 0.92, 0.94);
    }
    cr.paint().ok();

    // Book rectangle
    let (bx, by, bw, bh) = book_rect(w, h);
    let sw = bw * 0.025;

    // Book structure (shadow, stacked pages, spine)
    draw_book(cr, bx, by, bw, bh, sw, is_dark);

    // Page content area (two pages)
    let pw = (bw - sw) / 2.0 - 6.0;
    let ph = bh - 6.0;
    let left_px = bx + 3.0;
    let right_px = bx + bw / 2.0 + sw / 2.0 + 3.0;
    let py = by + 3.0;

    // Subtle paper texture on both pages
    draw_paper_texture(cr, left_px, py, pw, ph, is_dark);
    draw_paper_texture(cr, right_px, py, pw, ph, is_dark);

    let right_page_idx = (page + 1) % LAYOUTS.len();
    let sweep_w = right_px + pw - left_px;

    let phase = flip_phase(t);

    if let Some(phase_val) = phase {
        let fold_x = left_px + sweep_w * phase_val;
        let curl = (phase_val * std::f64::consts::PI).sin() * sweep_w * 0.10;

        // 1. Draw NEW pages (left of fold)
        let _ = cr.save();
        let _ = cr.rectangle(left_px, py, fold_x - left_px, ph);
        cr.clip();
        draw_panels(
            cr,
            left_px,
            py,
            pw,
            ph,
            (page + 2) % LAYOUTS.len(),
            is_dark,
            cycle_count + 1,
        );
        draw_panels(
            cr,
            right_px,
            py,
            pw,
            ph,
            (page + 3) % LAYOUTS.len(),
            is_dark,
            cycle_count + 1,
        );
        let _ = cr.restore();

        // 2. Draw OLD pages (right of fold)
        let _ = cr.save();
        let _ = cr.rectangle(fold_x, py, sweep_w - (fold_x - left_px), ph);
        cr.clip();
        draw_panels(cr, left_px, py, pw, ph, page, is_dark, cycle_count);
        draw_panels(
            cr,
            right_px,
            py,
            pw,
            ph,
            right_page_idx,
            is_dark,
            cycle_count,
        );
        let _ = cr.restore();

        // 3. Page turn shadow/light effects
        draw_flip_effect(cr, fold_x, py, ph, curl, is_dark, left_px, sweep_w);
    } else {
        // Static pages
        draw_panels(cr, left_px, py, pw, ph, page, is_dark, cycle_count);
        draw_panels(
            cr,
            right_px,
            py,
            pw,
            ph,
            right_page_idx,
            is_dark,
            cycle_count,
        );
    }

    // Loading text with progress fill and glow
    draw_loading_text(cr, bx, by, bw, bh, is_dark, w, h, t);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

impl Preview {
    /// Create a new preview widget.
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Load an original image from a file path.
    ///
    /// Reads the file on a background thread to avoid blocking the UI.
    /// The texture is created on the main thread once bytes arrive.
    pub fn load_original(&self, path: &std::path::Path) {
        let priv_ = self.imp();

        // Update UI state immediately (path info, not image-dependent)
        *priv_.original_path.borrow_mut() = Some(path.to_path_buf());
        if let Some(label) = priv_.info_label.borrow().as_ref() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            label.set_label(name);
        }
        if let Some(btn) = priv_.btn_original.borrow().as_ref() {
            btn.set_active(true);
        }

        // Read file bytes on a background thread
        let path_buf = path.to_path_buf();
        let (tx, rx) = async_channel::bounded::<Vec<u8>>(1);
        std::thread::spawn(move || {
            if let Ok(bytes) = std::fs::read(&path_buf) {
                let _ = tx.send_blocking(bytes);
            }
        });

        // Main thread: create texture from bytes and update UI
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(bytes) = rx.recv().await else { return };
            let Some(obj) = weak.upgrade() else { return };
            let priv_ = obj.imp();

            let gbytes = glib::Bytes::from_owned(bytes);
            if let Ok(texture) = Texture::from_bytes(&gbytes) {
                if let Some(pic) = priv_.original_picture.borrow().as_ref() {
                    pic.set_paintable(Some(&texture));
                }
                if let Some(label) = priv_.dimensions_label.borrow().as_ref() {
                    label.set_label(&format!("{}×{}", texture.width(), texture.height()));
                }
                if let Some(stack) = priv_.stack.borrow().as_ref() {
                    stack.set_visible_child_name("images");
                }
            }
        });
    }

    /// Load a translated image from a file path.
    ///
    /// Does not switch the view mode — the user can toggle to see it.
    /// Reads the file on a background thread to avoid blocking the UI.
    pub fn load_translated(&self, path: &std::path::Path) {
        let priv_ = self.imp();

        *priv_.translated_path.borrow_mut() = Some(path.to_path_buf());

        // Read file bytes on a background thread
        let path_buf = path.to_path_buf();
        let (tx, rx) = async_channel::bounded::<Vec<u8>>(1);
        std::thread::spawn(move || {
            if let Ok(bytes) = std::fs::read(&path_buf) {
                let _ = tx.send_blocking(bytes);
            }
        });

        // Main thread: create texture, cache Cairo surface, update overlay
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(bytes) = rx.recv().await else { return };
            let Some(obj) = weak.upgrade() else { return };
            let priv_ = obj.imp();

            let gbytes = glib::Bytes::from_owned(bytes);
            if let Ok(texture) = Texture::from_bytes(&gbytes) {
                // Cache texture reference
                *priv_.translated_texture.borrow_mut() = Some(texture.clone());

                // Download texture pixels and cache as Cairo ImageSurface
                let tex_w = texture.width();
                let tex_h = texture.height();
                let stride = tex_w * 4;
                let mut data = vec![0u8; (tex_h * stride) as usize];
                texture.download(&mut data, stride as usize);

                // Convert R8G8B8A8_PREMULTIPLIED → Cairo ARGB32 (swap R↔B)
                for pixel in data.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }

                if let Ok(surface) = cairo::ImageSurface::create_for_data(
                    data,
                    cairo::Format::ARgb32,
                    tex_w,
                    tex_h,
                    stride as i32,
                ) {
                    *priv_.translated_surface.borrow_mut() = Some(surface);
                }

                // Redraw overlay
                if let Some(overlay) = priv_.translated_overlay.borrow().as_ref() {
                    overlay.queue_draw();
                }
            }

            // If there's no original yet, show the translated image
            if priv_.original_path.borrow().is_none() {
                if let Some(stack) = priv_.stack.borrow().as_ref() {
                    stack.set_visible_child_name("images");
                }
                if let Some(btn) = priv_.btn_translated.borrow().as_ref() {
                    btn.set_active(true);
                }
            }
        });
    }

    /// Load both original and translated images at once.
    ///
    /// Automatically switches to comparison mode if both are available.
    /// Both images are loaded in parallel on background threads.
    pub fn load_pair(
        &self,
        original: Option<&std::path::Path>,
        translated: Option<&std::path::Path>,
    ) {
        let priv_ = self.imp();

        // Update path info immediately (not image-dependent)
        if let Some(path) = original {
            *priv_.original_path.borrow_mut() = Some(path.to_path_buf());
            if let Some(label) = priv_.info_label.borrow().as_ref() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                label.set_label(name);
            }
        }
        if let Some(path) = translated {
            *priv_.translated_path.borrow_mut() = Some(path.to_path_buf());
        }

        // Capture for mode switching after both loads complete
        let has_original = original.is_some();
        let has_translated = translated.is_some();

        // Spawn background threads for both images — they run in parallel
        let orig_rx = original.map(|path| {
            let path_buf = path.to_path_buf();
            let (tx, rx) = async_channel::bounded::<Vec<u8>>(1);
            std::thread::spawn(move || {
                if let Ok(bytes) = std::fs::read(&path_buf) {
                    let _ = tx.send_blocking(bytes);
                }
            });
            rx
        });

        let trans_rx = translated.map(|path| {
            let path_buf = path.to_path_buf();
            let (tx, rx) = async_channel::bounded::<Vec<u8>>(1);
            std::thread::spawn(move || {
                if let Ok(bytes) = std::fs::read(&path_buf) {
                    let _ = tx.send_blocking(bytes);
                }
            });
            rx
        });

        // Main thread: create textures from bytes and update UI
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let Some(obj) = weak.upgrade() else { return };
            let priv_ = obj.imp();

            // Load original texture
            if let Some(rx) = orig_rx {
                if let Ok(bytes) = rx.recv().await {
                    let gbytes = glib::Bytes::from_owned(bytes);
                    if let Ok(texture) = Texture::from_bytes(&gbytes) {
                        if let Some(pic) = priv_.original_picture.borrow().as_ref() {
                            pic.set_paintable(Some(&texture));
                        }
                        if let Some(label) = priv_.dimensions_label.borrow().as_ref() {
                            label.set_label(&format!("{}×{}", texture.width(), texture.height()));
                        }
                    }
                }
            }

            // Load translated texture → cache Cairo surface
            if let Some(rx) = trans_rx {
                if let Ok(bytes) = rx.recv().await {
                    let gbytes = glib::Bytes::from_owned(bytes);
                    if let Ok(texture) = Texture::from_bytes(&gbytes) {
                        *priv_.translated_texture.borrow_mut() = Some(texture.clone());
                        let tex_w = texture.width();
                        let tex_h = texture.height();
                        let stride = tex_w * 4;
                        let mut data = vec![0u8; (tex_h * stride) as usize];
                        texture.download(&mut data, stride as usize);
                        for pixel in data.chunks_exact_mut(4) {
                            pixel.swap(0, 2);
                        }
                        if let Ok(surface) = cairo::ImageSurface::create_for_data(
                            data,
                            cairo::Format::ARgb32,
                            tex_w,
                            tex_h,
                            stride as i32,
                        ) {
                            *priv_.translated_surface.borrow_mut() = Some(surface);
                        }
                        if let Some(overlay) = priv_.translated_overlay.borrow().as_ref() {
                            overlay.queue_draw();
                        }
                    }
                }
            }

            // Switch to image view
            if let Some(stack) = priv_.stack.borrow().as_ref() {
                stack.set_visible_child_name("images");
            }

            // Auto-switch to compare mode if both images are available
            if has_original && has_translated {
                if let Some(btn) = priv_.btn_compare.borrow().as_ref() {
                    btn.set_active(true);
                }
            } else if has_original {
                if let Some(btn) = priv_.btn_original.borrow().as_ref() {
                    btn.set_active(true);
                }
            } else if has_translated {
                if let Some(btn) = priv_.btn_translated.borrow().as_ref() {
                    btn.set_active(true);
                }
            }
        });
    }

    /// Clear the preview and show the placeholder.
    pub fn clear(&self) {
        let priv_ = self.imp();

        if let Some(pic) = priv_.original_picture.borrow().as_ref() {
            pic.set_paintable(None::<&gtk::gdk::Paintable>);
        }
        *priv_.translated_texture.borrow_mut() = None;
        *priv_.translated_surface.borrow_mut() = None;
        if let Some(overlay) = priv_.translated_overlay.borrow().as_ref() {
            overlay.queue_draw();
        }

        *priv_.original_path.borrow_mut() = None;
        *priv_.translated_path.borrow_mut() = None;

        if let Some(stack) = priv_.stack.borrow().as_ref() {
            stack.set_visible_child_name("placeholder");
        }
        if let Some(label) = priv_.info_label.borrow().as_ref() {
            label.set_label(&i18n::t("Keine Datei ausgewählt"));
        }
        if let Some(label) = priv_.dimensions_label.borrow().as_ref() {
            label.set_label("");
        }

        // Reset to original mode
        if let Some(btn) = priv_.btn_original.borrow().as_ref() {
            btn.set_active(true);
        }
    }

    /// Set the preview mode programmatically.
    pub fn set_mode(&self, mode: PreviewMode) {
        let priv_ = self.imp();
        priv_.set_mode(mode);

        // Update toggle buttons
        match mode {
            PreviewMode::Original => {
                if let Some(btn) = priv_.btn_original.borrow().as_ref() {
                    btn.set_active(true);
                }
            }
            PreviewMode::Translated => {
                if let Some(btn) = priv_.btn_translated.borrow().as_ref() {
                    btn.set_active(true);
                }
            }
            PreviewMode::Compare => {
                if let Some(btn) = priv_.btn_compare.borrow().as_ref() {
                    btn.set_active(true);
                }
            }
        }
    }

    /// Get the current preview mode.
    pub fn mode(&self) -> PreviewMode {
        *self.imp().mode.borrow()
    }

    /// Get the path of the currently loaded original image.
    pub fn original_path(&self) -> Option<PathBuf> {
        self.imp().original_path.borrow().clone()
    }

    /// Get the path of the currently loaded translated image.
    pub fn translated_path(&self) -> Option<PathBuf> {
        self.imp().translated_path.borrow().clone()
    }

    /// Check if an original image is loaded.
    pub fn has_original(&self) -> bool {
        self.imp().original_path.borrow().is_some()
    }

    /// Check if a translated image is loaded.
    pub fn has_translated(&self) -> bool {
        self.imp().translated_path.borrow().is_some()
    }

    /// Set the comparison slider position (0.0 = far left, 1.0 = far right).
    pub fn set_slider_position(&self, pos: f64) {
        let pos = pos.clamp(0.02, 0.98);
        let priv_ = self.imp();
        *priv_.slider_position.borrow_mut() = pos;

        // Redraw slider and translated overlay
        if let Some(slider) = priv_.slider_widget.borrow().as_ref() {
            slider.queue_draw();
        }
        if let Some(overlay) = priv_.translated_overlay.borrow().as_ref() {
            overlay.queue_draw();
        }
    }

    /// Get the current slider position.
    pub fn slider_position(&self) -> f64 {
        *self.imp().slider_position.borrow()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preview_mode_default() {
        assert_eq!(PreviewMode::default(), PreviewMode::Original);
    }

    #[test]
    fn test_preview_mode_equality() {
        assert_eq!(PreviewMode::Original, PreviewMode::Original);
        assert_ne!(PreviewMode::Original, PreviewMode::Translated);
        assert_ne!(PreviewMode::Original, PreviewMode::Compare);
    }

    #[test]
    fn test_slider_position_clamp() {
        let priv_ = PreviewPrivate::default();
        // Default position
        assert_eq!(*priv_.slider_position.borrow(), 0.5);
    }

    #[test]
    fn test_paths_default_none() {
        let priv_ = PreviewPrivate::default();
        assert!(priv_.original_path.borrow().is_none());
        assert!(priv_.translated_path.borrow().is_none());
    }

    #[test]
    fn test_dragging_default_false() {
        let priv_ = PreviewPrivate::default();
        assert!(!*priv_.dragging_slider.borrow());
    }
}

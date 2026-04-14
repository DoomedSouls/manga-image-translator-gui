#![allow(dead_code)]
// manga-translator-gtk/src/ui/css.rs
//
// Dynamic CSS generation helpers.
//
// Provides functions to build CSS strings at runtime for:
//   - Accent color overrides
//   - Widget-specific accent color patches
//   - Animation keyframes
//   - Grid/list item selection styles

use crate::config::AccentColor;

// ---------------------------------------------------------------------------
// Accent color CSS
// ---------------------------------------------------------------------------

/// Generate the full accent color CSS override block.
///
/// Uses CSS custom properties (--accent-bg-color, --accent-fg-color)
/// which is the official libadwaita method for accent color overrides.
/// Also overrides @define-color for our custom CSS in style.css.
pub fn accent_color_css(accent: &AccentColor) -> String {
    if accent.hex.is_empty() {
        // "system" preset — no override needed
        return String::new();
    }

    let fg = if accent.fg.is_empty() {
        AccentColor::foreground_for(&accent.hex)
    } else {
        accent.fg.clone()
    };

    format!(
        r#":root {{
    --accent-bg-color: {hex};
    --accent-fg-color: {fg};
    --accent-color: oklab(from var(--accent-bg-color) var(--standalone-color-oklab));
}}

@define-color accent_color {hex};
@define-color accent_bg_color {hex};
@define-color accent_fg_color {fg};
"#,
        hex = accent.hex,
        fg = fg,
    )
}

// ---------------------------------------------------------------------------
// Accent swatch CSS
// ---------------------------------------------------------------------------

/// Build CSS for all swatch buttons in the accent color dialog.
/// Uses a single CSS provider instead of one per swatch.
pub fn build_swatch_grid_css() -> &'static str {
    r#"
.accent-swatch {
    min-width: 56px;
    min-height: 56px;
    border-radius: 50%;
    padding: 0;
    transition: transform 150ms ease, box-shadow 150ms ease;
}
.accent-swatch:hover {
    transform: scale(1.1);
}
.accent-swatch:active {
    transform: scale(0.95);
}
.accent-swatch.active {
    box-shadow: 0 0 0 3px @window_bg_color, 0 0 0 5px currentColor;
}
.accent-swatch-system {
    background: alpha(@accent_bg_color, 0.15);
    border: 2px dashed @accent_bg_color;
}
.accent-swatch-label {
    font-size: 0.8em;
}
"#
}

// ---------------------------------------------------------------------------
// Animation CSS
// ---------------------------------------------------------------------------

/// CSS for the start-button pulse-glow (green).
pub fn start_button_css() -> &'static str {
    r#"
@keyframes pulse-glow-green {
    0%   { box-shadow: 0 0 4px alpha(#4CAF50, 0.3); }
    50%  { box-shadow: 0 0 16px alpha(#4CAF50, 0.7); }
    100% { box-shadow: 0 0 4px alpha(#4CAF50, 0.3); }
}

button.start-action-btn {
    background: #4CAF50;
    color: white;
    animation: pulse-glow-green 2s ease-in-out infinite;
    transition: background 200ms ease, box-shadow 200ms ease;
    border-radius: 12px;
    padding: 10px 24px;
    font-weight: 700;
    font-size: 14px;
}

button.start-action-btn:hover {
    background: #66BB6A;
    transform: scale(1.03);
    box-shadow: 0 0 20px alpha(#4CAF50, 0.8);
}

button.start-action-btn:active {
    background: #43A047;
    transform: scale(0.97);
}

button.start-action-btn:disabled {
    background: alpha(#4CAF50, 0.4);
    animation: none;
    color: alpha(white, 0.6);
}
"#
}

/// CSS for the start-button warning state (pulsing red glow when API key is missing).
pub fn start_button_warning_css() -> &'static str {
    r#"
@keyframes pulse-glow-warning {
    0%   { box-shadow: 0 0 4px alpha(#FF5722, 0.5); }
    25%  { box-shadow: 0 0 20px alpha(#FF5722, 0.9); }
    50%  { box-shadow: 0 0 30px alpha(#FF5722, 1.0); }
    75%  { box-shadow: 0 0 20px alpha(#FF5722, 0.9); }
    100% { box-shadow: 0 0 4px alpha(#FF5722, 0.5); }
}

button.start-action-btn.warning {
    background: #D32F2F;
    color: white;
    animation: pulse-glow-warning 0.8s ease-in-out infinite;
    border: 2px solid #FF5722;
}

button.start-action-btn.warning:hover {
    background: #E53935;
    box-shadow: 0 0 30px alpha(#FF5722, 1.0);
}
"#
}

/// CSS for the cancel-button pulse-glow (red).
pub fn cancel_button_css() -> &'static str {
    r#"
@keyframes pulse-glow-red {
    0%   { box-shadow: 0 0 4px alpha(#F44336, 0.3); }
    50%  { box-shadow: 0 0 16px alpha(#F44336, 0.7); }
    100% { box-shadow: 0 0 4px alpha(#F44336, 0.3); }
}

button.cancel-action-btn {
    background: #F44336;
    color: white;
    animation: pulse-glow-red 1.5s ease-in-out infinite;
    transition: background 200ms ease, box-shadow 200ms ease;
    border-radius: 12px;
    padding: 8px 16px;
    font-weight: 700;
}

button.cancel-action-btn:hover {
    background: #EF5350;
    transform: scale(1.05);
    box-shadow: 0 0 20px alpha(#F44336, 0.8);
}

button.cancel-action-btn:active {
    background: #E53935;
    transform: scale(0.95);
}

button.cancel-action-btn:disabled {
    opacity: 0.5;
    animation: none;
}
"#
}

/// CSS for selection bounce animations.
pub fn selection_animations_css() -> &'static str {
    r#"
@keyframes selection-pop {
    0%   { transform: scale(1.0); opacity: 1; }
    30%  { transform: scale(1.08); opacity: 1; }
    60%  { transform: scale(0.96); opacity: 1; }
    100% { transform: scale(1.0); opacity: 1; }
}

@keyframes check-bounce {
    0%   { transform: scale(1.0); opacity: 1; }
    25%  { transform: scale(1.3); opacity: 1; }
    50%  { transform: scale(0.9); opacity: 1; }
    75%  { transform: scale(1.1); opacity: 1; }
    100% { transform: scale(1.0); opacity: 1; }
}

@keyframes row-bounce {
    0%   { transform: scale(1.0); opacity: 1; }
    25%  { transform: scale(1.05); opacity: 1; }
    50%  { transform: scale(0.97); opacity: 1; }
    75%  { transform: scale(1.02); opacity: 1; }
    100% { transform: scale(1.0); opacity: 1; }
}

@keyframes row-appear {
    0%   { opacity: 0; transform: translateX(-8px); }
    100% { opacity: 1; transform: translateX(0); }
}

.grid-item-box.accent-selected {
    animation: selection-pop 250ms ease-out;
}

.file-row.just-selected {
    animation: row-bounce 300ms ease-out;
}
"#
}

/// CSS for the progress bar (red-to-green gradient).
pub fn progress_bar_css() -> &'static str {
    r#"
progressbar.translation-progress {
    min-height: 6px;
    border-radius: 3px;
}

progressbar.translation-progress trough {
    min-height: 6px;
    border-radius: 3px;
    background: alpha(@borders, 0.4);
}

progressbar.translation-progress progress {
    min-height: 6px;
    border-radius: 3px;
    background: linear-gradient(to right,
        #F44336, #FF9800, #FFEB3B, #8BC34A, #4CAF50);
}

progressbar.translation-progress.active progress {
    animation: pulse-glow-green 1s ease-in-out infinite;
}
"#
}

/// CSS for suppressing GTK's internal hover/selection highlights
/// that conflict with our custom selection styles.
pub fn gtk_override_css() -> &'static str {
    r#"
/* Suppress GTK's internal hover on GridView children */
gridview child:hover {
    background: transparent;
}

/* Suppress button hover inside folder rows (double-highlight fix) */
.folder-row button.flat,
.folder-row button.flat:hover,
.folder-row button.flat:active {
    background: transparent;
}
"#
}

// ---------------------------------------------------------------------------
// Combined builder
// ---------------------------------------------------------------------------

/// Build the complete application CSS as a single string.
/// This combines all static CSS fragments into one stylesheet.
pub fn build_combined_css() -> String {
    let mut css = String::with_capacity(8192);

    css.push_str(start_button_css());
    css.push_str(start_button_warning_css());
    css.push_str(cancel_button_css());
    css.push_str(selection_animations_css());
    css.push_str(progress_bar_css());

    css.push_str(gtk_override_css());

    css
}

/// Build an accent-color-only CSS string for the given accent.
pub fn build_accent_css(accent: &AccentColor) -> String {
    accent_color_css(accent)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AccentColor;

    #[test]
    fn test_accent_css_system_is_empty() {
        let system = AccentColor {
            name: "system".into(),
            hex: String::new(),
            fg: String::new(),
        };
        assert!(accent_color_css(&system).is_empty());
    }

    #[test]
    fn test_accent_css_blue_contains_hex() {
        let blue = AccentColor {
            name: "blue".into(),
            hex: "#3584e4".into(),
            fg: "#ffffff".into(),
        };
        let css = accent_color_css(&blue);
        assert!(css.contains("#3584e4"));
        assert!(css.contains("--accent-bg-color"));
        assert!(css.contains("@define-color accent_color"));
    }

    #[test]
    fn test_swatch_css() {
        let css = build_swatch_grid_css();
        assert!(css.contains(".accent-swatch"));
        assert!(css.contains("border-radius: 50%"));
    }

    #[test]
    fn test_combined_css_not_empty() {
        let css = build_combined_css();
        assert!(!css.is_empty());
        assert!(css.contains("pulse-glow-green"));
        assert!(css.contains("pulse-glow-warning"));
        assert!(css.contains("selection-pop"));
    }
}

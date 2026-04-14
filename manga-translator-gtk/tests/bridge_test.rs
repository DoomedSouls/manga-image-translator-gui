// Integration tests for the IPC bridge.
//
// These tests validate:
//   1. TranslationParams construction and defaults
//   2. IpcBridge creation and lifecycle
//   3. Pure-Rust methods (list_directories, list_image_files)
//   4. Backend availability check (IPC subprocess ping)
//   5. OpenRouter model fetching via IPC
//
// Tests that require the full Python backend are gated behind
// a runtime availability check and are silently skipped.

use manga_translator_gtk::ipc_bridge::{IpcBridge, TranslationParams};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Unit tests — no Python backend required
// ---------------------------------------------------------------------------

#[test]
fn test_translation_params_defaults() {
    let params = TranslationParams::default();
    assert_eq!(params.translator, "offline");
    assert_eq!(params.target_lang, "DEU");
    assert_eq!(params.detector, "ctd");
    assert_eq!(params.ocr, "48px");
    assert_eq!(params.inpainter, "lama");
    assert_eq!(params.upscaler, "none");
    assert_eq!(params.direction, "auto");
    assert!(!params.use_mocr_merge);
    assert_eq!(params.device, "cuda");
    assert_eq!(params.output_directory, "");
    assert!(params.use_original_folder);
}

#[test]
fn test_translation_params_custom() {
    let params = TranslationParams {
        translator: "DeepL".into(),
        target_lang: "ENG".into(),
        detector: "Standard (CTD)".into(),
        ocr: "48px (Standard)".into(),
        inpainter: "Standard (LaMA)".into(),
        upscaler: "Keiner".into(),
        direction: "rtl".into(),
        use_mocr_merge: true,
        device: "cpu".into(),
        output_directory: String::new(),
        use_original_folder: true,
        translation_mode: "standard".into(),
        vlm_type: String::new(),
        gemini_model: String::new(),
        openrouter_model: String::new(),
        local_model_path: String::new(),
        project_name: String::new(),
        gemini_api_key: String::new(),
        openrouter_api_key: String::new(),
        upscale_ratio: 2,
        renderer: "default".into(),
        alignment: "auto".into(),
        disable_font_border: false,
        font_size_offset: 0,
        font_size_minimum: -1,
        render_direction: "auto".into(),
        uppercase: false,
        lowercase: false,
        font_color: String::new(),
        no_hyphenation: false,
        line_spacing: 0,
        font_size: 0,
        rtl: false,
        mask_dilation_offset: 20,
        kernel_size: 3,
        inpainting_size: 2048,
        inpainting_precision: "bf16".into(),
        detection_size: 2048,
        api_keys: HashMap::new(),
    };
    assert_eq!(params.translator, "DeepL");
    assert_eq!(params.target_lang, "ENG");
    assert_eq!(params.device, "cpu");
    assert!(params.use_mocr_merge);
}

// ---------------------------------------------------------------------------
// Bridge tests — require Python backend (skip gracefully if unavailable)
// ---------------------------------------------------------------------------

#[test]
fn test_bridge_creation() {
    // IpcBridge::new() does NOT start the subprocess — safe to call without backend
    let bridge = IpcBridge::new();
    // Should not panic or fail
    drop(bridge);
}

#[test]
fn test_bridge_is_backend_available() {
    let bridge = IpcBridge::new();
    // This will try to start the backend subprocess and ping it.
    // It's OK if this returns false — we just want to make sure it doesn't panic
    let _available = bridge.is_backend_available();
}

#[test]
fn test_ipc_bridge_list_directories() {
    // Validates that list_directories works without the backend (pure Rust).
    let bridge = IpcBridge::new();

    // List the current directory — should always work
    let result = bridge.list_directories(std::path::Path::new("."));
    assert!(result.is_ok(), "list_directories should succeed for CWD");

    // List a nonexistent directory — should return empty, not error
    let result = bridge.list_directories(std::path::Path::new("/nonexistent_xyz_12345"));
    assert!(
        result.is_ok(),
        "list_directories should return Ok for missing dir"
    );
    assert!(
        result.unwrap().is_empty(),
        "Should return empty vec for missing dir"
    );
}

#[test]
fn test_ipc_bridge_list_image_files() {
    // Validates that list_image_files works without the backend (pure Rust).
    let bridge = IpcBridge::new();

    // List image files in the backend directory — should find server.py but no images
    let result = bridge.list_image_files(std::path::Path::new("backend"));
    assert!(result.is_ok(), "list_image_files should succeed");
    // backend/ has no image files
    assert!(
        result.unwrap().is_empty(),
        "backend/ should have no image files"
    );

    // List image files in a nonexistent directory — should error
    let result = bridge.list_image_files(std::path::Path::new("/nonexistent_xyz_12345"));
    assert!(
        result.is_err(),
        "list_image_files should fail for missing dir"
    );
}

#[test]
fn test_bridge_fetch_openrouter_models() {
    // Test that the IpcBridge can call fetch_openrouter_models
    // and get back a list of vision-capable models via the Python backend.
    // Requires network access to openrouter.ai — skips gracefully if unavailable.
    let bridge = IpcBridge::new();

    let result = bridge.fetch_openrouter_models("");

    match result {
        Ok(models) => {
            assert!(!models.is_empty(), "Expected at least one OpenRouter model");
            // Check that model IDs look reasonable
            for m in &models {
                assert!(!m.id.is_empty(), "Model ID should not be empty");
            }
            eprintln!(
                "Fetched {} OpenRouter models (first: {})",
                models.len(),
                models.first().map(|m| m.id.as_str()).unwrap_or("?")
            );
        }
        Err(e) => {
            eprintln!(
                "SKIP: Could not fetch OpenRouter models (network?): {:?}",
                e
            );
        }
    }
}

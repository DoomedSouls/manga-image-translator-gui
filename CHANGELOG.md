# Changelog

All notable changes to this project will be documented in this file.

## [0.2.1] — 2026-05-15

### Added
- **Flatpak build** — GNOME 50 SDK, rustup-based CI, NVIDIA GL extension + ROCm support
  - `--device=all` for CUDA/ROCm GPU compute
  - `org.freedesktop.Platform.GL` extension for NVIDIA driver access
  - Matugen color theme support (`xdg-config/gtk-4.0`, `xdg-data/themes`)
  - App-ID: `com.mangatranslator.gui`
- **AppImage build** — openSUSE Tumbleweed container, manual lib bundling, type2 runtime
- **Windows build** — MSYS2/mingw64, MSIX + ZIP dual packaging, self-signed code signing
- **Hotpath profiler** — 16 instrumented functions, zero-cost when disabled (`--features hotpath`)
- Windows: `#![windows_subsystem = "windows"]` (no CMD flash)
- Windows: `bind_textdomain_codeset("UTF-8")` (fixes gettext crash)
- Windows: `setup_windows_panic_hook()` shows MessageBox on crash

### Changed
- Removed `build-linux.yml` (superseded by `build-appimage.yml` and `build-flatpak.yml`)

## [0.2.0] — 2026-04-15

### Added
- Live UI retranslation for dropdowns and menus on language change
- Italian translation (9 languages total: DE, EN, ES, FR, IT, JA, KO, PT-BR, ZH-CN)

### Changed
- Per-chapter `_Text/` directories for translation storage (replaces global cache)
- VLM: `verbose=False` fix — Pass 2 and MTPE now find `_translations.txt`
- New translation mode: **"Text extrahieren"** (extract) — OCR + translate, no rendering
- "Cache-Modus" renamed to **"Text einfügen"**
- UI sensitivity fixes: file browser and BBox row disabled during translation

### Fixed
- `python_bin` derivation: 3x `.parent()` (was 2x, resolved to `venv/lib/` instead of `venv/`)
- VLM/Cache revealer collapsed on startup even with saved mode

## [0.1.0] — 2026-04-15

### Added
- GTK4/libadwaita GUI with folder-based manga browsing
- Standard, VLM, extract, and insert translation modes
- IPC subprocess backend (Python 3.10+ with any venv)
- Live preview with original/translated comparison slider
- 8 languages: DE, EN, ES, FR, JA, KO, PT-BR, ZH-CN
- Configurable accent colors and theme support
- API key management for translation services

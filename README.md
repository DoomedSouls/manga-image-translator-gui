<img width="3802" height="2051" alt="3" src="https://github.com/user-attachments/assets/1f667946-51e5-4bb4-852d-a92f89bdd398" />

<img width="3802" height="2051" alt="2" src="https://github.com/user-attachments/assets/845de1d8-b552-4d36-8b3a-f8a9cf553ee5" />

<img width="3802" height="2051" alt="1" src="https://github.com/user-attachments/assets/d227c372-1692-4156-b567-e222c510bd1f" />

![License](https://img.shields.io/badge/license-MIT-blue)
![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows-informational)
![Release](https://img.shields.io/badge/release-v0.2.1-blue)

# Manga Image Translator — GTK4 GUI

A modern desktop GUI for [manga-image-translator](https://github.com/zyddnys/manga-image-translator), built with **GTK4 / libadwaita** in Rust.

The Rust frontend communicates with the Python backend (`backend/server.py`) via an IPC subprocess — JSON over stdin/stdout. This allows any Python 3.10+ environment to be used, provides crash isolation, and simplifies packaging.

## Downloads

Pre-built binaries are available on the [Releases page](https://github.com/SlobCoder/manga-image-translator-gui/releases):

| Format | File | Notes |
|---|---|---|
| **Flatpak** | `MangaTranslator-0.2.1-x86_64.flatpak` | GNOME 50 runtime, NVIDIA/ROCm GPU support, matugen colors |
| **AppImage** | `MangaTranslator-0.2.1-x86_64.AppImage` | Portable, no installation needed |
| **Windows MSIX** | `MangaTranslator-0.2.1-x86_64.msix` | Installable Windows package |
| **Windows ZIP** | `MangaTranslator-0.2.1-x86_64.zip` | Portable Windows binary |

### Install Flatpak

```bash
flatpak install MangaTranslator-0.2.1-x86_64.flatpak
```

### Install AppImage

```bash
chmod +x MangaTranslator-0.2.1-x86_64.AppImage
./MangaTranslator-0.2.1-x86_64.AppImage
```

## Features

- **Folder-based browsing** — open manga chapter directories, grid/list view, search, sort
- **Multiple translation modes**:
  - **Standard** — full pipeline: detect → OCR → translate → inpaint → render
  - **VLM** — two-pass Vision Language Model translation with Gemini, OpenRouter, or local .gguf models
  - **Extract Text** — OCR + translate + save `_translations.txt` (Pass 1 only, no rendering)
  - **Insert Text** — render from previously saved `_translations.txt` files
  - **Upscale Only** — upscale images without translation
- **Live preview** — original, translated, and side-by-side comparison slider
- **Settings panel** — translator, target language, detector, OCR, inpainting, upscaler, renderer, font options
- **VLM configuration** — Gemini (online), OpenRouter (online), or local .gguf models
- **API key management** — securely stored per-service (DeepL, OpenRouter, Gemini, etc.)
- **Virtual environment configuration** — point to any Python venv / micromamba env
- **Manga home directory** — configurable home directory for the file browser home button
- **Accent colors** — system/light/dark theme + 9 accent color presets + custom hex input
- **9 UI languages** — DE, EN, ES, FR, IT, JA, KO, PT-BR, ZH-CN
- **Keyboard shortcuts** — Ctrl+O, Ctrl+T, F5, Ctrl+K, etc.
- **Log viewer** — real-time log with auto-refresh, copy, open externally

## Requirements

### System
- **GTK4** 4.10+
- **libadwaita** 1.5+
- **Python** 3.10+ (in a virtual environment)
- **Rust** 1.85+ (edition 2024, only needed when building from source)

### Python dependencies
A virtual environment with [manga-image-translator](https://github.com/zyddnys/manga-image-translator) installed. For example with micromamba:

```bash
micromamba create -n manga-translator python=3.12
micromamba activate manga-translator
pip install manga-translator
```

### Arch Linux

```bash
sudo pacman -S rust gtk4 libadwaita
```

### Ubuntu / Debian

```bash
sudo apt install rustc libgtk-4-dev libadwaita-1-dev
```

### Fedora

```bash
sudo dnf install rust gtk4-devel libadwaita-devel
```

## Building from Source

```bash
# Clone the repository
git clone https://github.com/SlobCoder/manga-image-translator-gui.git
cd manga-image-translator-gui

# Build (release)
cd manga-translator-gtk
cargo build --release

# Run
cargo run --release
```

The release binary is ~4.7 MB and will be at `target/release/manga-translator-gtk`.

## First-time Setup

1. Start the application
2. Open the menu (⚙ gear icon) → **Virtual Environment…**
3. Set the path to your Python virtual environment (e.g. `~/.local/share/mamba/envs/manga-translator`)
4. Set the path to the manga-image-translator directory (e.g. `~/manga-image-translator`)
5. Open **API Keys** (Ctrl+K) and enter keys for the services you want to use (DeepL, OpenRouter, etc.)
6. Open a manga directory via **Open Directory…** (Ctrl+O)
7. Select files and click **Translate** (Ctrl+T)

## Configuration

Settings and API keys are stored in the XDG config directory:

| File | Content |
|---|---|
| `~/.config/manga-translator-gtk/settings.json` | All GUI settings |
| `~/.config/manga-translator-gtk/api_keys.json` | API keys (DeepL, OpenRouter, Gemini, etc.) |
| `~/.config/manga-translator-gtk/manga-translator.log` | Application log |

**Flatpak:** Config is stored in `~/.var/app/com.mangatranslator.gui/config/manga-translator-gtk/`.

## Translation Modes

| Mode | Description |
|---|---|
| **Standard** | Full pipeline: detect text regions → OCR → translate → inpaint (fill original text) → render translated text |
| **VLM** | Two-pass Vision Language Model translation. Pass 1: OCR + translate + save `_translations.txt`. Pass 2: VLM reviews and corrects translations, then renders onto images |
| **Extract Text** | Pass 1 only: OCR + translate + save `_translations.txt`. No rendering — useful for batch extraction and later review |
| **Insert Text** | Load previously saved `_translations.txt` files and render the translated text onto images. No re-translation |
| **Upscale Only** | Upscale images using the configured upscaler without any text detection, OCR, or translation |

Translations are stored per-chapter in `_Text/` directories next to the originals:

```
Manga Chapter/
├── 0001.jpg
├── 0002.jpg
├── _Text/
│   ├── 0001_translations.txt
│   └── 0002_translations.txt
```

### VLM Backends

The VLM mode supports three backend types:

| Backend | Description |
|---|---|
| **OpenRouter (Online)** | Uses OpenRouter API to access various VLM models. Requires an OpenRouter API key |
| **Gemini (Online)** | Uses Google Gemini API. Supports models: gemini-3.1-pro, gemini-3-flash, gemini-2.5-pro, gemini-2.5-flash, gemini-2.5-flash-lite. Requires a Gemini API key |
| **Local Model** | Uses a local `.gguf` model file. No API key required, but requires sufficient RAM/VRAM. Set the model path in the VLM settings |

### VLM Workflow

1. **Pass 1 (Extract Text)**: The manga-image-translator pipeline detects text regions, performs OCR, and translates the text. Results are saved as `_translations.txt` in the `_Text/` directory.
2. **MTPE (Machine Translation Post-Editing)**: Optionally review and edit translations before rendering.
3. **Pass 2 (VLM correction)**: The VLM reviews the original image alongside the translations and can correct context-dependent errors (e.g., honorifics, idioms, off-screen context).
4. **Rendering**: Final translations are rendered onto the images with the configured font and inpainting settings.

## Preview and Comparison

The preview pane on the right side of the window supports three modes:

| Mode | Description |
|---|---|
| **Original** | Shows the unmodified source image |
| **Translated** | Shows the translated/processed image |
| **Compare** | Side-by-side comparison with a draggable slider. The left side shows the original, the right side shows the translated image. A vertical line with a dark outline marks the divider |

Switch between modes using the toggle buttons above the preview, or use the dropdown. The comparison slider can be dragged left/right to compare specific regions of the image.

## Settings Panel

Press **F9** to toggle the settings sidebar. All settings are organized into sections:

### Translation Settings

| Setting | Options | Description |
|---|---|---|
| **Mode** | Standard, VLM, Extract Text, Insert Text, Upscale Only | Translation mode (see above) |
| **Translator** | Various (DeepL, Google, etc.) | Translation service to use |
| **Target Language** | Various | Output language for translations |
| **Detector** | Various (default, craft, etc.) | Text region detection algorithm |
| **OCR** | Various (default, manga_ocr, etc.) | Optical character recognition engine |
| **Inpainter** | Various (default, la_ma, etc.) | Method used to fill in the original text regions |
| **Upscaler** | Various (esrgan, etc.) | Image upscaling model |

### Renderer Settings

| Setting | Options | Description |
|---|---|---|
| **Renderer** | Standard, Manga2Eng (Pillow), Manga2Eng, No Rendering | How translated text is rendered onto the image. "Manga2Eng" restructures text for English manga style. "No Rendering" skips rendering entirely |
| **Alignment** | Auto, Left, Center, Right | Text alignment within rendered regions |
| **Direction** | Auto, Horizontal, Vertical | Text rendering direction: auto-detected, forced horizontal, or forced vertical |

### Inpainting Precision

| Option | Description |
|---|---|
| **FP32 (Slow, Precise)** | Full 32-bit floating point — slowest but most accurate inpainting |
| **FP16 (Balanced)** | 16-bit floating point — balanced speed and quality |
| **BF16 (Fast, Default)** | BFloat16 — fastest, good quality, recommended default |

### Font Settings

| Setting | Description |
|---|---|
| **Font** | Font family and size for rendered translations |
| **Font Color** | Override color for rendered text (hex code) |
| **Font Size Minimum** | Minimum font size in pixels |
| **Font Size Maximum** | Maximum font size in pixels |
| **Line Spacing** | Space between lines of rendered text |
| **BBox Merge** | Merge nearby bounding boxes before OCR (reduces fragmented text) |

### Appearance Settings

| Setting | Options | Description |
|---|---|---|
| **Color Scheme** | System, Light, Dark | Application color scheme |
| **Accent Color** | System, Blue, Purple, Teal, Green, Yellow, Orange, Red, Pink + custom hex | Changes the accent color across the entire UI |
| **Language** | Auto, Deutsch, English, Español, Français, Italiano, Português (Brasil), 日本語, 한국어, 简体中文 | UI language. "Auto" follows the system locale |

### VLM Section (visible when mode is VLM)

| Setting | Description |
|---|---|
| **VLM Backend** | OpenRouter (Online), Gemini (Online), Local Model |
| **Gemini Model** | Select from available Gemini models (visible when Gemini is selected) |
| **Model Path** | Path to local `.gguf` model file (visible when Local Model is selected) |

## Manga Home Directory

The **Manga Home Directory** is the folder opened when you click the home icon (🏠) in the file browser. It defaults to `~/Manga`.

To configure it:
1. Open the menu (⚙) → **Manga Home Directory…**
2. Enter a custom path or click the folder button to browse
3. Click **Save** to apply, or **Reset** to revert to `~/Manga`

## Keyboard Shortcuts

### General

| Shortcut | Action |
|---|---|
| `Ctrl+O` | Open directory |
| `Ctrl+R` / `F5` | Refresh file browser |
| `Alt+←` | Navigate back to parent directory |
| `Ctrl+T` | Start translation |
| `Escape` | Cancel active translation |
| `Ctrl+K` | Open API key management |
| `F9` | Toggle settings panel |

### File Selection

| Shortcut | Action |
|---|---|
| `Ctrl+A` | Select all files |
| `Ctrl+Shift+A` | Deselect all files |
| `Ctrl+L` | Focus the search bar |

### View

| Shortcut | Action |
|---|---|
| `Ctrl+1` | Switch to grid view |
| `Ctrl+2` | Switch to list view |

## i18n — Adding / Updating Translations

The project uses `gettext-rs` for internationalization. Source strings are in German. The UI supports real-time language switching — all registered widgets are automatically retranslated when the language changes.

### Compile .po → .mo

```bash
cd manga-image-translator-gui
for lang in de en es fr it ja ko pt_BR zh_CN; do
  msgfmt -o "locale/$lang/LC_MESSAGES/manga-translator.mo" \
         "locale/$lang/LC_MESSAGES/manga-translator.po"
done
```

### Add a new language

1. Create `locale/<lang>/LC_MESSAGES/` directory
2. Copy an existing `.po` file and update the `Language:` header
3. Translate all `msgstr` entries
4. Compile with `msgfmt`
5. Add the language to `SUPPORTED_LANGUAGES` in `src/i18n.rs`

### Currently supported languages

| Code | Language |
|---|---|
| `de` | Deutsch (source) |
| `en` | English |
| `es` | Español |
| `fr` | Français |
| `it` | Italiano |
| `ja` | 日本語 |
| `ko` | 한국어 |
| `pt_BR` | Português (Brasil) |
| `zh_CN` | 简体中文 |

## Project Structure

```
manga-image-translator-gui/
├── .github/workflows/
│   ├── build-flatpak.yml          # Flatpak CI (GNOME 50, rustup)
│   ├── build-appimage.yml         # AppImage CI (openSUSE Tumbleweed)
│   └── build-windows.yml          # Windows CI (MSYS2, MSIX + ZIP)
├── flatpak/
│   └── com.mangatranslator.gui.yaml  # Flatpak manifest
├── packaging/
│   ├── manga-translator-gtk.desktop
│   ├── manga-translator-gtk.svg
│   └── metainfo/                  # AppStream metadata
├── windows/                       # Windows packaging (AppxManifest, etc.)
├── locale/                        # Translations (.po + .mo)
│   ├── manga-translator.pot       # Translation template
│   ├── de/LC_MESSAGES/
│   ├── en/LC_MESSAGES/
│   └── ...                        # 7 more languages
└── manga-translator-gtk/
    ├── Cargo.toml                 # Rust dependencies (GTK4, libadwaita, etc.)
    ├── Cargo.lock
    ├── backend/
    │   └── server.py              # Python IPC backend (~1050 lines)
    ├── resources/
    │   └── style.css              # Application CSS
    ├── src/
    │   ├── main.rs                # Entry point
    │   ├── lib.rs                 # Module exports
    │   ├── config.rs              # Settings, API keys, ConfigManager
    │   ├── i18n.rs                # gettext internationalization + widget registry
    │   ├── ipc_bridge.rs          # IPC subprocess bridge (Rust ↔ Python)
    │   └── ui/
    │       ├── main_window.rs     # Main application window
    │       ├── settings_panel.rs  # Settings sidebar
    │       ├── file_browser.rs    # File/folder browser (grid + list)
    │       ├── preview.rs         # Image preview + comparison slider
    │       ├── dialogs.rs         # All dialogs (API keys, colors, shortcuts, etc.)
    │       └── css.rs             # Dynamic CSS generation (accent colors)
    └── tests/
        └── bridge_test.rs         # IPC bridge integration tests
```

## Architecture

```
┌─────────────────────────────────────────────┐
│           Rust GUI (GTK4/libadwaita)        │
│                                             │
│  main_window ← settings_panel               │
│       ↕          file_browser               │
│  ipc_bridge ←   preview                     │
│       ↕          dialogs                    │
│       ↕          css (dynamic styles)        │
├───────┬─────────────────────────────────────┤
│ stdin/stdout (JSON)                         │
├───────┴─────────────────────────────────────┤
│         Python Backend (server.py)          │
│                                             │
│  MangaTranslatorLocal → manga_translator    │
│  Progress reporting → JSON responses        │
└─────────────────────────────────────────────┘
```

- **Three Mutexes**: stdin_writer, response_rx, child process — independent locking for responsive UI
- **Cancel support**: AtomicBool + IPC cancel command + Python threading.Event
- **Auto-discovery**: server.py relative to executable or CWD
- **Real-time i18n**: widget registry with weak references, retranslated on language change without restart

## CI/CD

All three platforms are built automatically on tag push (`v*`) or manual trigger:

| Workflow | Platform | Runtime | Output |
|---|---|---|---|
| `build-flatpak.yml` | Ubuntu 24.04 | GNOME 50 SDK | `.flatpak` |
| `build-appimage.yml` | openSUSE Tumbleweed | Bundled libs | `.AppImage` |
| `build-windows.yml` | Windows Server | MSYS2/mingw64 | `.msix` + `.zip` |

Artifacts are uploaded to the GitHub Release automatically.

## Troubleshooting

### "Backend not configured" warning

The virtual environment paths are not set. Open ⚙ → **Virtual Environment…** and configure both paths:
- **Virtual Environment**: path to your Python venv (e.g. `~/.local/share/mamba/envs/manga-translator`)
- **manga-image-translator Directory**: path to the manga-image-translator source/install directory

The dialog shows live validation (✔ / ✘) for both paths.

### Translation fails with NumPy / Python version mismatch

The GUI uses the Python from your configured virtual environment. If the venv's Python version doesn't match the installed packages, you'll get errors. Make sure:
- The venv has Python 3.10+ (3.12 recommended)
- `manga-translator` and all dependencies are installed in that venv
- The correct venv path is configured (should point to the venv root, not `bin/` or `lib/`)

### Translations not appearing / empty _Text/ folder

- Make sure the **Extract Text** or **VLM** mode was used first to generate `_translations.txt` files
- For **Insert Text** mode, the `_Text/` folder must already exist next to the images with valid `.txt` files
- Check the log viewer (⚙ → **Log**) for detailed error messages

### GUI is in the wrong language

- Open the settings panel (F9) → **Language** dropdown
- Select your preferred language (or "Auto" for system default)
- The UI switches immediately without restart

### AppImage doesn't start

- Make sure the file is executable: `chmod +x MangaTranslator-*.AppImage`
- Try with debug output: `APPIMAGE_DEBUG=1 ./MangaTranslator-*.AppImage`
- On some systems you may need FUSE: `sudo apt install libfuse2` (Ubuntu/Debian)

### Flatpak: CUDA error / GPU not found

The Flatpak has `--device=all` and the NVIDIA GL extension for GPU compute support. If you still get CUDA errors:
- Install the NVIDIA Flatpak driver: `flatpak install flathub org.freedesktop.Platform.GL.nvidia`
- Alternatively, disable GPU in the manga-image-translator settings (use CPU mode)

### Accent color doesn't change

- The accent color is applied via CSS overrides. Some custom GTK themes may override it
- Try switching to the "System" color scheme first, then setting the accent color
- Custom hex colors must be in `#RRGGBB` format (e.g. `#ff6600`)

## License

MIT

## Credits

| Component | Credits |
|---|---|
| Rust GUI | SLOB-CODER, Contributors |
| Python Backend | zyddnys, Contributors |
| Powered by | GTK4, libadwaita, gettext, MangaTranslator |

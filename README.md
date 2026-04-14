<img width="3802" height="2051" alt="preview" src="https://github.com/user-attachments/assets/8657b60d-19d4-474c-bc28-d7d1ac895b8b" />

# Manga Image Translator — GTK4 GUI

A modern desktop GUI for [manga-image-translator](https://github.com/zyddnys/manga-image-translator), built with **GTK4 / libadwaita** in Rust.

The Rust frontend communicates with the Python backend (`backend/server.py`) via an IPC subprocess — JSON over stdin/stdout. This allows any Python 3.10+ environment to be used, provides crash isolation, and simplifies packaging.

## Features

- **Folder-based browsing** — open manga chapter directories, grid/list view, search
- **Multiple translation modes**:
  - **Standard** — translate all selected images
  - **VLM** — two-pass Vision Language Model translation with Gemini/OpenRouter
  - **Text extrahieren** — OCR + translate + save (Pass 1 only)
  - **Text einfügen** — render from previously saved translations
- **Live preview** — original, translated, and side-by-side comparison slider
- **Settings panel** — translator, target language, detector, OCR, inpainting, upscaler, renderer, font options
- **VLM configuration** — Gemini, OpenRouter, or local .gguf models
- **API key management** — securely stored per-service
- **Virtual environment configuration** — point to any Python venv / micromamba env
- **Accent colors** — system/light/dark theme + 9 accent colors + custom hex
- **8 languages** — DE, EN, ES, FR, JA, KO, PT-BR, ZH-CN
- **Keyboard shortcuts** — Ctrl+O, Ctrl+T, F5, Ctrl+K, etc.
- **Log viewer** — real-time log with auto-refresh, copy, open externally

## Requirements

### System
- **Rust** 1.85+ (edition 2024)
- **GTK4** 4.10+
- **libadwaita** 1.5+
- **Python** 3.10+ (in a virtual environment)

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

## Building

```bash
# Clone the repository
git clone https://github.com/your-repo/manga-image-translator-gui.git
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
2. Open the menu (⚙ gear icon) → **Virtuelle Umgebung…**
3. Set the path to your Python virtual environment (e.g. `~/.local/share/mamba/envs/manga-translator`)
4. Set the path to the manga-image-translator directory (e.g. `~/manga-image-translator`)
5. Open **API Schlüssel** (Ctrl+K) and enter keys for the services you want to use (DeepL, OpenRouter, etc.)
6. Open a manga directory via **Verzeichnis öffnen…** (Ctrl+O)
7. Select files and click **Übersetzen** (Ctrl+T)

## Configuration

Settings and API keys are stored in the XDG config directory:

| File | Content |
|---|---|
| `~/.config/manga-translator-gtk/settings.json` | All GUI settings |
| `~/.config/manga-translator-gtk/api_keys.json` | API keys (DeepL, OpenRouter, Gemini, etc.) |
| `~/.config/manga-translator-gui/manga-translator-gui.log` | Application log |

## Translation Modes

| Mode | Description |
|---|---|
| **Standard** | Full pipeline: detect → OCR → translate → inpaint → render |
| **VLM** | Two-pass: Pass 1 (OCR + translate + save), Pass 2 (VLM correction + render) |
| **Text extrahieren** | Pass 1 only: OCR + translate + save `_translations.txt` |
| **Text einfügen** | Load saved translations and render onto images |

Translations are stored per-chapter in `_Text/` directories next to the originals:

```
Manga Chapter/
├── 0001.jpg
├── 0002.jpg
├── _Text/
│   ├── 0001_translations.txt
│   └── 0002_translations.txt
```

## i18n — Adding / Updating Translations

The project uses `gettext-rs` for internationalization. Source strings are in German.

### Compile .po → .mo

```bash
cd manga-image-translator-gui
for lang in de en es fr ja ko pt_BR zh_CN; do
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

## Project Structure

```
manga-image-translator-gui/
├── README.md
├── .gitignore
├── locale/                          # Translations (.po + .mo)
│   ├── de/LC_MESSAGES/
│   ├── en/LC_MESSAGES/
│   ├── es/LC_MESSAGES/
│   ├── fr/LC_MESSAGES/
│   ├── ja/LC_MESSAGES/
│   ├── ko/LC_MESSAGES/
│   ├── pt_BR/LC_MESSAGES/
│   └── zh_CN/LC_MESSAGES/
└── manga-translator-gtk/
    ├── Cargo.toml
    ├── Cargo.lock
    ├── backend/
    │   └── server.py                # Python IPC backend
    ├── resources/
    │   └── style.css                # Application CSS
    ├── src/
    │   ├── main.rs                  # Entry point
    │   ├── lib.rs                   # Module exports
    │   ├── config.rs                # Settings, API keys, ConfigManager
    │   ├── i18n.rs                  # gettext internationalization
    │   ├── ipc_bridge.rs            # IPC subprocess bridge (Rust ↔ Python)
    │   └── ui/
    │       ├── main_window.rs       # Main application window
    │       ├── settings_panel.rs    # Settings sidebar
    │       ├── file_browser.rs      # File/folder browser
    │       ├── preview.rs           # Image preview + comparison slider
    │       ├── dialogs.rs           # All dialogs (API keys, colors, etc.)
    │       └── css.rs               # Dynamic CSS generation
    └── tests/
        └── bridge_test.rs           # IPC bridge integration tests
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
├───────┬─────────────────────────────────────┤
│ stdin/stdout (JSON)                         │
├───────┴─────────────────────────────────────┤
│         Python Backend (server.py)          │
│                                             │
│  MangaTranslatorLocal → manga_translator    │
│  Progress reporting → JSON responses        │
└─────────────────────────────────────────────┘
```

- **Three Mutexes**: stdin_writer, response_rx, child process
- **Cancel support**: AtomicBool + IPC cancel command + Python threading.Event
- **Auto-discovery**: server.py relative to executable or CWD

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+O` | Open directory |
| `Ctrl+R` / `F5` | Refresh |
| `Alt+←` | Navigate back |
| `Ctrl+T` | Start translation |
| `Escape` | Cancel translation |
| `Ctrl+K` | API keys |
| `F9` | Toggle settings panel |
| `Ctrl+A` | Select all |
| `Ctrl+Shift+A` | Deselect all |
| `Ctrl+L` | Focus search |
| `Ctrl+1` | Grid view |
| `Ctrl+2` | List view |

## License

MIT

## Credits

| Component | Credits |
|---|---|
| Rust GUI | SLOB-CODER, Contributors |
| Python Backend | zyddnys, Contributors |
| Powered by | GTK4, libadwaita, gettext, MangaTranslator |

#!/bin/bash
# build-appimage.sh — Build an AppImage for manga-translator-gtk
#
# Usage:
#   ./build-appimage.sh              # Build with defaults
#   ./build-appimage.sh --skip-build # Skip cargo build (use existing binary)
#   ./build-appimage.sh --clean      # Remove AppDir and AppImage before building
#
# Requirements:
#   - Rust toolchain (edition 2024)
#   - GTK4 + libadwaita development headers
#   - curl, wget, or similar for downloading linuxdeploy tools
#
# Environment variables:
#   ARCH            — Target architecture (default: x86_64)
#   APP_NAME        — Application name (default: manga-translator-gtk)
#   APP_VERSION     — Version string (default: 0.1.0)
#   LINUXDEPLOY_URL — Override linuxdeploy download URL
#   OUTPUT_DIR      — Where to place the final AppImage (default: dist/)
set -euo pipefail

# ── Configuration ─────────────────────────────────────────────────────────────
ARCH="${ARCH:-$(uname -m)}"
APP_NAME="manga-translator-gtk"
APP_VERSION="${APP_VERSION:-0.2.0}"
OUTPUT_DIR="${OUTPUT_DIR:-dist}"
SKIP_BUILD=0
CLEAN=0

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="${SCRIPT_DIR}"
GTK_DIR="${PROJECT_ROOT}/manga-translator-gtk"
PACKAGING_DIR="${PROJECT_ROOT}/packaging"

APPDIR="${PROJECT_ROOT}/build/${APP_NAME}.AppDir"
BINARY="${GTK_DIR}/target/release/${APP_NAME}"
APPIMAGE_OUTPUT="${PROJECT_ROOT}/${OUTPUT_DIR}/${APP_NAME}-${APP_VERSION}-${ARCH}.AppImage"

# linuxdeploy release URLs
LINUXDEPLOY_URL="${LINUXDEPLOY_URL:-https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${ARCH}.AppImage}"
LINUXDEPLOY_GTK_URL="https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh"
LINUXDEPLOY_APPIMAGETOOL_URL="https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-${ARCH}.AppImage"

# ── Parse arguments ───────────────────────────────────────────────────────────
for arg in "$@"; do
    case "$arg" in
        --skip-build) SKIP_BUILD=1 ;;
        --clean)      CLEAN=1 ;;
        -h|--help)
            echo "Usage: $0 [--skip-build] [--clean]"
            echo ""
            echo "  --skip-build  Use existing release binary (skip cargo build)"
            echo "  --clean       Remove AppDir and output before building"
            exit 0
            ;;
        *)
            echo "Unknown argument: $arg"
            exit 1
            ;;
    esac
done

# ── Helpers ───────────────────────────────────────────────────────────────────
info()  { echo -e "\033[1;34m[INFO]\033[0m  $*"; }
ok()    { echo -e "\033[1;32m[OK]\033[0m    $*"; }
warn()  { echo -e "\033[1;33m[WARN]\033[0m  $*"; }
error() { echo -e "\033[1;31m[ERROR]\033[0m $*" >&2; exit 1; }

step() {
    echo ""
    echo -e "\033[1;36m══════════════════════════════════════════════════════════════"
    echo -e "  $*"
    echo -e "══════════════════════════════════════════════════════════════\033[0m"
}

download_if_missing() {
    local url="$1"
    local dest="$2"
    local mode="${3:-755}"

    if [ -f "$dest" ]; then
        ok "Already downloaded: $(basename "$dest")"
        return
    fi

    info "Downloading $(basename "$dest")..."
    mkdir -p "$(dirname "$dest")"

    if command -v curl &>/dev/null; then
        curl -fSL -o "$dest" "$url" || error "Failed to download $url"
    elif command -v wget &>/dev/null; then
        wget -O "$dest" "$url" || error "Failed to download $url"
    else
        error "Neither curl nor wget found. Install one to continue."
    fi

    chmod "$mode" "$dest"
    ok "Downloaded: $(basename "$dest")"
}

# ── Preflight checks ─────────────────────────────────────────────────────────
step "Preflight checks"

command -v cargo &>/dev/null || error "cargo not found. Install Rust: https://rustup.rs/"
command -v pkg-config &>/dev/null || warn "pkg-config not found — GTK detection may fail"
pkg-config --exists gtk4 2>/dev/null || warn "gtk4 not found via pkg-config. Build may fail."
pkg-config --exists libadwaita-1 2>/dev/null || warn "libadwaita not found via pkg-config. Build may fail."

info "ARCH       = ${ARCH}"
info "APP_NAME   = ${APP_NAME}"
info "APP_VERSION= ${APP_VERSION}"
info "APPDIR     = ${APPDIR}"
info "OUTPUT     = ${APPIMAGE_OUTPUT}"

# ── Clean (optional) ─────────────────────────────────────────────────────────
if [ "$CLEAN" -eq 1 ]; then
    step "Cleaning previous build artifacts"
    info "Removing ${APPDIR}..."
    rm -rf "${APPDIR}"
    info "Removing ${PROJECT_ROOT}/${OUTPUT_DIR}..."
    rm -rf "${PROJECT_ROOT}/${OUTPUT_DIR}"
    ok "Cleaned"
fi

# ── Step 1: Build release binary ─────────────────────────────────────────────
if [ "$SKIP_BUILD" -eq 1 ]; then
    step "Skipping cargo build (using existing binary)"
    [ -f "$BINARY" ] || error "Binary not found at ${BINARY}. Run without --skip-build first."
else
    step "Building release binary"
    info "Running: cargo build --release"
    (cd "$GTK_DIR" && cargo build --release 2>&1) || error "Cargo build failed"
    ok "Build complete"
fi

BINARY_SIZE=$(du -h "$BINARY" | cut -f1)
info "Binary size: ${BINARY_SIZE}"

# ── Step 2: Create AppDir structure ──────────────────────────────────────────
step "Creating AppDir structure"

mkdir -p "${APPDIR}/usr/bin"
mkdir -p "${APPDIR}/usr/share/applications"
mkdir -p "${APPDIR}/usr/share/icons/hicolor/scalable/apps"
mkdir -p "${APPDIR}/usr/share/icons/hicolor/256x256/apps"
mkdir -p "${APPDIR}/usr/share/manga-translator-gtk/backend"
mkdir -p "${APPDIR}/usr/share/locale"
mkdir -p "${APPDIR}/usr/share/metainfo"
mkdir -p "${APPDIR}/usr/lib"

# ── Step 3: Install files into AppDir ────────────────────────────────────────
step "Installing files into AppDir"

# Binary
info "Installing binary..."
cp "${BINARY}" "${APPDIR}/usr/bin/${APP_NAME}"
chmod 755 "${APPDIR}/usr/bin/${APP_NAME}"
strip --strip-unneeded "${APPDIR}/usr/bin/${APP_NAME}" 2>/dev/null || warn "strip failed (non-critical)"
ok "Binary installed"

# Desktop entry
info "Installing desktop entry..."
cp "${PACKAGING_DIR}/${APP_NAME}.desktop" \
   "${APPDIR}/usr/share/applications/${APP_NAME}.desktop"
cp "${PACKAGING_DIR}/${APP_NAME}.desktop" \
   "${APPDIR}/${APP_NAME}.desktop"
ok "Desktop entry installed"

# Icon (SVG)
info "Installing icon..."
cp "${PACKAGING_DIR}/${APP_NAME}.svg" \
   "${APPDIR}/usr/share/icons/hicolor/scalable/apps/${APP_NAME}.svg"
cp "${PACKAGING_DIR}/${APP_NAME}.svg" \
   "${APPDIR}/${APP_NAME}.svg"
# Also generate a 256x256 PNG from SVG for compatibility
if command -v rsvg-convert &>/dev/null; then
    rsvg-convert -w 256 -h 256 "${PACKAGING_DIR}/${APP_NAME}.svg" \
        -o "${APPDIR}/usr/share/icons/hicolor/256x256/apps/${APP_NAME}.png"
    ok "Generated 256x256 PNG icon"
elif command -v convert &>/dev/null; then
    convert -background none -resize 256x256 "${PACKAGING_DIR}/${APP_NAME}.svg" \
        "${APPDIR}/usr/share/icons/hicolor/256x256/apps/${APP_NAME}.png" 2>/dev/null && \
        ok "Generated 256x256 PNG icon (ImageMagick)" || \
        warn "Could not convert SVG to PNG (non-critical)"
else
    warn "Neither rsvg-convert nor ImageMagick found — skipping PNG icon (non-critical)"
fi

# Backend (server.py)
info "Installing Python backend..."
cp "${GTK_DIR}/backend/server.py" \
   "${APPDIR}/usr/share/manga-translator-gtk/backend/server.py"
ok "Backend installed"

# AppStream metainfo
info "Installing metainfo..."
METAINFO_SRC="${PACKAGING_DIR}/metainfo/io.github.manga_translator_gtk.appdata.xml"
if [ -f "$METAINFO_SRC" ]; then
    cp "$METAINFO_SRC" "${APPDIR}/usr/share/metainfo/io.github.manga_translator_gtk.appdata.xml"
    ok "Metainfo installed"
else
    warn "Metainfo not found at ${METAINFO_SRC} (non-critical)"
fi

# Locale files (.mo)
info "Installing locale files..."
LOCALE_SRC="${PROJECT_ROOT}/locale"
if [ -d "$LOCALE_SRC" ]; then
    LOCALE_COUNT=0
    for po_dir in "${LOCALE_SRC}"/*/LC_MESSAGES; do
        lang="$(basename "$(dirname "$po_dir")")"
        for mo_file in "${po_dir}"/*.mo; do
            [ -f "$mo_file" ] || continue
            dest_dir="${APPDIR}/usr/share/locale/${lang}/LC_MESSAGES"
            mkdir -p "$dest_dir"
            cp "$mo_file" "$dest_dir/$(basename "$mo_file")"
            LOCALE_COUNT=$((LOCALE_COUNT + 1))
        done
    done
    ok "Installed ${LOCALE_COUNT} locale files"
else
    warn "No locale/ directory found at ${LOCALE_SRC}"
fi

# AppRun
info "Installing AppRun..."
cp "${PACKAGING_DIR}/AppRun" "${APPDIR}/AppRun"
chmod 755 "${APPDIR}/AppRun"
ok "AppRun installed"

# ── Step 4: Download linuxdeploy tools ───────────────────────────────────────
step "Downloading linuxdeploy tools"

TOOLS_DIR="${PROJECT_ROOT}/build/tools"
mkdir -p "$TOOLS_DIR"

download_if_missing "$LINUXDEPLOY_URL"           "${TOOLS_DIR}/linuxdeploy"
download_if_missing "$LINUXDEPLOY_GTK_URL"       "${TOOLS_DIR}/linuxdeploy-plugin-gtk.sh"
download_if_missing "$LINUXDEPLOY_APPIMAGETOOL_URL" "${TOOLS_DIR}/appimagetool"

# ── Step 5: Bundle dependencies with linuxdeploy ─────────────────────────────
step "Bundling dependencies with linuxdeploy"

export ARCH
export APPDIR
export VERBOSE="${VERBOSE:-0}"

info "Running linuxdeploy (this may take a moment)..."

# linuxdeploy reads the desktop file to determine the executable
# and bundles all shared library dependencies automatically.
# The GTK plugin additionally bundles GTK4, libadwaita, GDK-Pixbuf loaders,
# GSettings schemas, etc.
#
# We deliberately do NOT use --output appimage here because that triggers
# appstreamcli validation which may fail on non-critical metadata issues.
# Instead, we only bundle dependencies and create the AppImage with
# appimagetool in a separate step.
(
    cd "${TOOLS_DIR}"
    ./linuxdeploy --appdir "${APPDIR}" \
        --plugin gtk \
        --verbosity "${VERBOSE}" 2>&1 || true
)

# ── Step 6: Create AppImage ──────────────────────────────────────────────────
step "Creating AppImage"

mkdir -p "${PROJECT_ROOT}/${OUTPUT_DIR}"

info "Running appimagetool..."
"${TOOLS_DIR}/appimagetool" --no-appstream "${APPDIR}" "$APPIMAGE_OUTPUT" 2>&1 || {
    # Retry with explicit gzip compression if default fails
    warn "Retrying with --comp gzip..."
    "${TOOLS_DIR}/appimagetool" --no-appstream --comp gzip "${APPDIR}" "$APPIMAGE_OUTPUT" 2>&1 || {
        error "Failed to create AppImage. Try running with VERBOSE=1 for more output."
    }
}
ok "AppImage created: ${APPIMAGE_OUTPUT}"

# ── Done ──────────────────────────────────────────────────────────────────────
FINAL_SIZE=$(du -h "$APPIMAGE_OUTPUT" | cut -f1)

echo ""
echo -e "\033[1;32m╔══════════════════════════════════════════════════════════╗"
echo -e "║  AppImage built successfully!                            ║"
echo -e "╠══════════════════════════════════════════════════════════╣"
echo -e "║  Output:  ${APPIMAGE_OUTPUT}"
echo -e "║  Size:    ${FINAL_SIZE}"
echo -e "╠══════════════════════════════════════════════════════════╣"
echo -e "║  Run with:                                               ║"
echo -e "║    chmod +x ${APPIMAGE_OUTPUT}"
echo -e "║    ./${APPIMAGE_OUTPUT}                                  ║"
echo -e "╠══════════════════════════════════════════════════════════╣"
echo -e "║  First-time setup:                                       ║"
echo -e "║    1. Open ⚙ menu → Virtuelle Umgebung…                ║"
echo -e "║    2. Set Python venv path (e.g. micromamba env)         ║"
echo -e "║    3. Set manga-image-translator path                    ║"
echo -e "║    4. Open Ctrl+K → Enter API keys                       ║"
echo -e "╚══════════════════════════════════════════════════════════╝\033[0m"

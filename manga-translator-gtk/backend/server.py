#!/usr/bin/env python3
"""
Manga Translator IPC Backend Server
====================================

JSON-over-stdin/stdout bridge between the Rust GTK4 GUI and the Python
manga_translator backend.  The Rust process starts this script as a
subprocess and communicates via newline-delimited JSON messages.

Protocol
--------
Request  (Rust → Python):  {"id": <int>, "method": "<name>", "params": {…}}
Response (Python → Rust):  {"id": <int>, "result": <any>}
                            {"id": <int>, "error":  {"type": "…", "message": "…"}}
Progress (during long ops): {"id": <int>, "progress": <float>, "message": "…"}

Usage
-----
    python3 server.py                  # interactive / subprocess mode
    python3 server.py --test           # run self-test
"""

from __future__ import annotations

import argparse
import asyncio
import base64
import io
import json
import logging
import os
import queue
import shutil
import signal
import sys
import threading
import traceback
from pathlib import Path
from typing import Any, Dict, List, Optional

# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------

LOG_FORMAT = "[%(asctime)s %(levelname)-5s] %(message)s"
logging.basicConfig(level=logging.INFO, format=LOG_FORMAT)
logger = logging.getLogger("backend")

# ---------------------------------------------------------------------------
# Global state
# ---------------------------------------------------------------------------

_cancelled = threading.Event()
_command_queue = queue.Queue()  # filled by background stdin reader thread
_translator_instance = None  # persistent MangaTranslatorLocal
_translator_device_key = None  # reuse key (device string)


# ---------------------------------------------------------------------------
# Display-name → Python-enum mappings
# (These match the Rust-side labels in settings_panel.rs / main_window.rs)
# ---------------------------------------------------------------------------

TRANSLATOR_MAP = {
    "Lokal (Offline)": "offline",
    "DeepL": "deepl",
    "Baidu": "baidu",
    "Caiyun": "caiyun",
    "ChatGPT": "chatgpt",
    "DeepSeek": "deepseek",
    "Groq": "groq",
    "Gemini": "gemini",
    "Sugoi": "sugoi",
    "None (Text nur erkennen)": "none",
}

DETECTOR_MAP = {
    "CTD — Optimiert für Manga & Comics": "ctd",
    "Default — Allgemeine Texterkennung": "default",
    "DBConvNext — ConvNext-basiert": "dbconvnext",
    "PaddleOCR — PaddlePaddle Erkennung": "paddle",
}

OCR_MAP = {
    "48px (Standard)": "48px",
    "mocr (Better Quality)": "mocr",
}

INPAINTER_MAP = {
    "LaMA — Standard Inpainting": "lama_large",
    "LaMA Large — Hochauflösendes Inpainting": "lama_large",
    "AOTScan — Kontext-basierte Rekonstruktion": "default",
    "PatchMatch — Patch-basierte Bildreparatur": "sd",
    "NSM — Neural Style Migration": "lama_mpe",
}

UPSCALER_MAP = {
    "ESRGAN — Allgemeine Bildverbesserung": "esrgan",
    "Waifu2x — Anime & Manga optimiert": "waifu2x",
    "4x-UltraSharp — 4fache Hochskalierung": "4xultrasharp",
}


# ---------------------------------------------------------------------------
# API-key injection
# ---------------------------------------------------------------------------

API_KEY_MAP = [
    # (env var name,        Rust HashMap key,      Python module path)
    ("DEEPL_AUTH_KEY", "deepl", "manga_translator.translators.deepl"),
    ("OPENAI_API_KEY", "openai", "manga_translator.translators.chatgpt"),
    ("GEMINI_API_KEY", "gemini", "manga_translator.translators.gemini"),
    ("DEEPSEEK_API_KEY", "deepseek", "manga_translator.translators.deepseek"),
    ("GROQ_API_KEY", "groq", "manga_translator.translators.groq"),
    ("BAIDU_APP_ID", "baidu_app_id", "manga_translator.translators.baidu"),
    ("BAIDU_SECRET_KEY", "baidu_secret_key", "manga_translator.translators.baidu"),
    ("CAIYUN_TOKEN", "caiyun_token", "manga_translator.translators.caiyun"),
]


def inject_api_keys(api_keys: Dict[str, str]) -> None:
    """Push API keys from the Rust GUI into Python's os.environ and modules."""
    import importlib

    for env_var, key_name, mod_path in API_KEY_MAP:
        value = api_keys.get(key_name, "")
        if not value:
            continue

        # Layer 1: os.environ
        os.environ[env_var] = value

        # Layer 2: keys.py module-level variables (may already be imported)
        try:
            keys_mod = importlib.import_module("manga_translator.translators.keys")
            setattr(keys_mod, env_var, value)
        except Exception:
            pass

        # Layer 3: individual translator modules
        try:
            module = importlib.import_module(mod_path)
            setattr(module, env_var, value)
        except Exception:
            pass

    logger.info("API keys injected: %s", ", ".join(k for k, v in api_keys.items() if v))


# ---------------------------------------------------------------------------
# MangaTranslatorLocal lifecycle
# ---------------------------------------------------------------------------


def _get_or_create_translator(params: dict) -> object:
    """Create or reuse a MangaTranslatorLocal instance.

    The instance is reused as long as the *device* key doesn't change,
    because switching between GPU and CPU requires reloading models.
    """
    global _translator_instance, _translator_device_key

    from manga_translator.mode.local import MangaTranslatorLocal

    device = params.get("device", "cuda")
    device_key = device

    if _translator_instance is not None and device_key == _translator_device_key:
        return _translator_instance

    logger.info("Creating MangaTranslatorLocal (device=%s)…", device)
    use_gpu = device == "cuda"

    init_params = {
        "use_gpu": use_gpu,
        "ignore_errors": True,
        "kernel_size": params.get("kernel_size", 3),
        "verbose": False,
        "models_ttl": 0,
        "batch_size": 1,
        "attempts": 0,
        "save_quality": 100,
        "input": [],
        "disable_memory_optimization": True,
    }

    instance = MangaTranslatorLocal(init_params)
    _translator_instance = instance
    _translator_device_key = device_key
    logger.info("MangaTranslatorLocal instance created")
    return instance


# ---------------------------------------------------------------------------
# Config builder
# ---------------------------------------------------------------------------


def _build_config(params: dict) -> object:
    """Build a manga_translator Config from the flat params dict sent by Rust."""
    from manga_translator.config import (
        Config,
        DetectorConfig,
        InpainterConfig,
        OcrConfig,
        RenderConfig,
        TranslatorConfig,
        UpscaleConfig,
    )

    # --- TranslatorConfig ---
    py_translator = TRANSLATOR_MAP.get(
        params.get("translator", ""), params.get("translator", "offline")
    )
    tc = TranslatorConfig(
        translator=py_translator,
        target_lang=params.get("target_lang", "DEU"),
    )

    # --- DetectorConfig ---
    py_detector = DETECTOR_MAP.get(
        params.get("detector", ""), params.get("detector", "ctd")
    )
    dc = DetectorConfig(
        detector=py_detector,
        detection_size=params.get("detection_size", 2048),
    )

    # --- OcrConfig ---
    py_ocr = OCR_MAP.get(params.get("ocr", ""), params.get("ocr", "48px"))
    oc = OcrConfig(
        ocr=py_ocr,
        use_mocr_merge=params.get("use_mocr_merge", False),
    )

    # --- InpainterConfig ---
    py_inpainter = INPAINTER_MAP.get(
        params.get("inpainter", ""), params.get("inpainter", "lama_large")
    )
    ic = InpainterConfig(
        inpainter=py_inpainter,
        inpainting_size=params.get("inpainting_size", 2048),
        inpainting_precision=params.get("inpainting_precision", "bf16"),
    )

    # --- RenderConfig ---
    rc_kwargs: dict[str, Any] = {
        "renderer": params.get("renderer", "default"),
        "alignment": params.get("alignment", "auto"),
        "disable_font_border": params.get("disable_font_border", False),
        "font_size_offset": params.get("font_size_offset", 0),
        "direction": params.get("render_direction", "auto"),
        "uppercase": params.get("uppercase", False),
        "lowercase": params.get("lowercase", False),
        "no_hyphenation": params.get("no_hyphenation", False),
        "rtl": params.get("rtl", False),
    }
    font_size_minimum = params.get("font_size_minimum", -1)
    if font_size_minimum >= 0:
        rc_kwargs["font_size_minimum"] = font_size_minimum
    font_color = params.get("font_color", "")
    if font_color:
        rc_kwargs["font_color"] = font_color
    line_spacing = params.get("line_spacing", 0)
    if line_spacing > 0:
        rc_kwargs["line_spacing"] = line_spacing
    font_size = params.get("font_size", 0)
    if font_size > 0:
        rc_kwargs["font_size"] = font_size
    rc = RenderConfig(**rc_kwargs)

    # --- UpscaleConfig (optional) ---
    upscaler_display = params.get("upscaler", "")
    py_upscaler = UPSCALER_MAP.get(upscaler_display, None)

    config_kwargs: dict[str, Any] = {
        "translator": tc,
        "detector": dc,
        "ocr": oc,
        "inpainter": ic,
        "render": rc,
        "mask_dilation_offset": params.get("mask_dilation_offset", 20),
        "kernel_size": params.get("kernel_size", 3),
    }

    if py_upscaler:
        uc_kwargs: dict[str, Any] = {"upscaler": py_upscaler}
        upscale_ratio = params.get("upscale_ratio", 0)
        if upscale_ratio > 0:
            uc_kwargs["upscale_ratio"] = upscale_ratio
        config_kwargs["upscale"] = UpscaleConfig(**uc_kwargs)

    config = Config(**config_kwargs)
    return config


# ---------------------------------------------------------------------------
# Logging redirection
# ---------------------------------------------------------------------------

_log_file_path: Optional[str] = None


def _redirect_logging(log_path: str) -> None:
    """Redirect Python-side logging + loguru to the shared log file."""
    global _log_file_path
    _log_file_path = log_path

    # stdlib logging
    handler = logging.FileHandler(log_path, mode="a", encoding="utf-8")
    handler.setFormatter(logging.Formatter("[Python %(levelname)s] %(message)s"))
    mt_logger = logging.getLogger("manga_translator")
    mt_logger.addHandler(handler)
    mt_logger.setLevel(logging.DEBUG)

    # loguru (manga_translator uses loguru, not stdlib logging)
    try:
        from loguru import logger as loguru_logger

        loguru_logger.remove()
        loguru_logger.add(
            log_path,
            mode="a",
            encoding="utf-8",
            level="DEBUG",
            format="[loguru {time:HH:mm:ss} {level}] {message}",
        )
        logger.info("Loguru redirected to: %s", log_path)
    except ImportError:
        pass

    logger.info("Python logging redirected to: %s", log_path)


# ---------------------------------------------------------------------------
# Pre-flight validation
# ---------------------------------------------------------------------------


def _check_api_keys(params: dict) -> Optional[str]:
    """Validate API keys for the selected translator.

    Returns an error message string if a required key is missing,
    or ``None`` if everything is OK.
    """
    api_keys = params.get("api_keys", {})
    translator = params.get("translator", "")
    translation_mode = params.get("translation_mode", "standard")

    def _key(name: str) -> bool:
        return bool(api_keys.get(name, ""))

    if translator == "DeepL" and not _key("deepl"):
        return "DeepL"
    if translator == "ChatGPT" and not _key("openai"):
        return "ChatGPT"
    if translator == "DeepSeek" and not _key("deepseek"):
        return "DeepSeek"
    if translator == "Groq" and not _key("groq"):
        return "Groq"
    if translator == "Gemini" and not _key("gemini"):
        return "Gemini"
    if translator == "Caiyun" and not _key("caiyun_token"):
        return "Caiyun"
    if translator == "Baidu":
        if not _key("baidu_app_id"):
            return "Baidu App ID"
        if not _key("baidu_secret_key"):
            return "Baidu Secret Key"

    # VLM-specific keys
    if translation_mode == "vlm":
        vlm_type = params.get("vlm_type", "")
        if (
            vlm_type == "gemini"
            and not params.get("gemini_api_key")
            and not _key("gemini")
        ):
            return "Gemini (VLM)"
        if (
            vlm_type == "openrouter"
            and not params.get("openrouter_api_key")
            and not _key("openrouter")
        ):
            return "OpenRouter (VLM)"

    return None


# ---------------------------------------------------------------------------
# Progress helper
# ---------------------------------------------------------------------------


def _send_progress(
    out: io.TextIOBase, req_id: int, progress: float, message: str
) -> None:
    """Write a progress JSON object to stdout (called from any thread)."""
    _write_json(
        out,
        {
            "id": req_id,
            "progress": progress,
            "message": message,
        },
    )


# ---------------------------------------------------------------------------
# Core command handlers
# ---------------------------------------------------------------------------


def handle_ping(params: dict) -> str:
    return "pong"


def handle_python_version(params: dict) -> str:
    return sys.version


def handle_is_backend_available(params: dict) -> bool:
    try:
        import manga_translator.mode.local  # noqa: F401

        return True
    except ImportError as e:
        logger.warning("manga_translator backend not available: %s", e)
        return False


def handle_generate_thumbnail(params: dict) -> str:
    """Generate a thumbnail and return it as a base64-encoded PNG string."""
    from PIL import Image

    path = params["path"]
    max_w = params.get("max_w", 112)
    max_h = params.get("max_h", 112)

    img = Image.open(path)
    orig_w, orig_h = img.size
    scale = min(max_w / orig_w, max_h / orig_h)
    new_w = max(int(orig_w * scale), 1)
    new_h = max(int(orig_h * scale), 1)

    thumb = img.resize((new_w, new_h))
    rgba = thumb.convert("RGBA")

    buf = io.BytesIO()
    rgba.save(buf, format="PNG", optimize=True)
    return base64.b64encode(buf.getvalue()).decode("ascii")


def handle_get_cached_translation(params: dict) -> Optional[str]:
    """Check if a cached translation exists for the given source image."""
    path_str = params["path"]

    # Try the paths module
    try:
        import paths as paths_mod  # type: ignore

        cache_dir = paths_mod.get_translation_cache_dir()
        path = Path(path_str)
        project_name = path.parent.name if path.parent.name else "default"
        file_stem = path.stem or "unknown"

        candidates = [
            Path(cache_dir) / project_name / f"{file_stem}.png",
            Path(cache_dir) / project_name / f"{file_stem}_translated.png",
        ]
        for c in candidates:
            if c.exists():
                return str(c)
    except Exception:
        pass

    # Fallback: check result/ directory
    path = Path(path_str)
    parent = path.parent
    result_dir = parent / "result"
    if result_dir.is_dir():
        file_stem = path.stem or "unknown"
        for ext in ("png", "jpg"):
            candidate = result_dir / f"{file_stem}.{ext}"
            if candidate.exists():
                return str(candidate)

    return None


def handle_fetch_openrouter_models(params: dict) -> list:
    """Fetch vision-capable models from the OpenRouter API."""
    from manga_translator.vlm.mtpe import fetch_openrouter_models

    api_key = params.get("api_key", "")
    return fetch_openrouter_models(api_key)


# ---------------------------------------------------------------------------
# Path helpers for per-image translations storage
# ---------------------------------------------------------------------------
# Instead of a global CACHE/<project>/ folder, translations are saved next
# to the original images in a single shared folder per chapter:
#   /path/to/manga/Name 1/0021.png
#   /path/to/manga/Name 1/Name 1_Text/0021_translations.txt
#   /path/to/manga/Name 1/Name 1_Text/0022_translations.txt
#
# The internal save_text / load_text mechanism still uses a temp cache
# folder under result/.  These helpers bridge between the two locations.


def _text_dir_for_image(file_path: str) -> str:
    """Return the ``{parent_dir}_Text`` directory shared by all images."""
    parent = os.path.dirname(file_path)
    parent_name = os.path.basename(parent) or "default"
    d = os.path.join(parent, f"{parent_name}_Text")
    os.makedirs(d, exist_ok=True)
    return d


def _translations_txt_path(file_path: str) -> str:
    """Path to ``{stem}_translations.txt`` next to the original image."""
    stem = os.path.splitext(os.path.basename(file_path))[0]
    return os.path.join(_text_dir_for_image(file_path), f"{stem}_translations.txt")


def _cache_txt_path(file_path: str) -> str:
    """Temp cache path where ``manga_translator`` saves via ``save_text``."""
    from manga_translator.vlm.mtpe import get_result_dir

    stem = os.path.splitext(os.path.basename(file_path))[0]
    return os.path.join(get_result_dir(), "CACHE", "_tmp", f"{stem}_translations.txt")


def _move_cache_to_image(file_path: str) -> Optional[str]:
    """Move translations from temp cache to per-image ``_Text`` dir.

    Returns the new path on success, ``None`` if the cache file was not found.
    """
    cache_path = _cache_txt_path(file_path)
    new_path = _translations_txt_path(file_path)
    if os.path.exists(cache_path):
        shutil.move(cache_path, new_path)
        logger.info("Moved translations: %s → %s", cache_path, new_path)
        return new_path
    logger.warning("Cache file not found: %s", cache_path)
    return None


def _copy_image_to_cache(file_path: str) -> bool:
    """Copy translations from per-image ``_Text`` dir to temp cache.

    Returns ``True`` on success, ``False`` if the source file was not found.
    """
    new_path = _translations_txt_path(file_path)
    cache_path = _cache_txt_path(file_path)
    if not os.path.exists(new_path):
        logger.warning("Translations file not found: %s", new_path)
        return False
    os.makedirs(os.path.dirname(cache_path), exist_ok=True)
    shutil.copy2(new_path, cache_path)
    logger.info("Copied translations: %s → %s", new_path, cache_path)
    return True


def handle_translate(params: dict, out: io.TextIOBase, req_id: int) -> dict:
    """Execute a translation job.

    This is the most complex handler: it creates/reuses the translator,
    builds the config, and processes files one by one with progress reporting.
    """
    global _translator_instance

    files = params.get("files", [])
    if not files:
        raise BackendError("No files selected for translation")

    # Pre-flight: check files exist
    missing = [f for f in files if not os.path.exists(f)]
    if missing:
        raise BackendError(f"File not found: {missing[0]}")

    # Pre-flight: API keys
    missing_service = _check_api_keys(params)
    if missing_service:
        raise BackendError(
            f"API key missing for service: {missing_service}",
            error_type="ApiKeyMissing",
        )

    # Reset cancel flag
    _cancelled.clear()

    # Inject API keys into Python environment
    api_keys = params.get("api_keys", {})
    inject_api_keys(api_keys)

    # Redirect logging
    log_path = params.get("log_path", "")
    if log_path:
        _redirect_logging(log_path)

    # Create or reuse translator instance
    translator = _get_or_create_translator(params)

    # Build Config object
    config = _build_config(params)

    # VLM / extract / cache mode setup
    is_vlm = params.get("translation_mode", "standard") == "vlm"
    is_extract = params.get("translation_mode", "standard") == "extract"
    is_cache = params.get("translation_mode", "standard") == "cache"

    if is_vlm or is_extract:
        # save_text=True → manga_translator writes _translations.txt to a
        # temp cache (CACHE/_tmp).  After translate_file we move the file
        # to a per-image _Text/ directory next to the source image.
        translator.result_sub_folder = "CACHE/_tmp"
        translator.save_text = True
        translator.load_text = False
        logger.info("%s mode enabled", "VLM" if is_vlm else "Extract")

    if is_cache:
        # load_text=True → manga_translator reads from temp cache.
        # We copy the file from per-image _Text/ dir there first.
        translator.result_sub_folder = "CACHE/_tmp"
        translator.save_text = False
        translator.load_text = True
        logger.info("Text einfügen mode enabled")

    # Progress tracking
    file_count = len(files)
    steps_per_file = 3.0 if is_vlm else 1.0
    total_steps = file_count * steps_per_file
    results: list[str] = []
    file_errors: list[dict] = []

    output_dir = params.get("output_directory", "")
    use_original_folder = params.get("use_original_folder", True)

    for i, file_path in enumerate(files):
        # Check for pending cancel commands from the queue
        while not _command_queue.empty():
            try:
                cmd = _command_queue.get_nowait()
                if cmd is None or cmd.get("method") == "cancel":
                    _cancelled.set()
                    break
            except queue.Empty:
                break

        if _cancelled.is_set():
            _send_progress(out, req_id, i / total_steps, "Abgebrochen")
            raise BackendError("Translation cancelled", error_type="Cancelled")

        file_name = os.path.basename(file_path)

        # Compute output path
        if use_original_folder or not output_dir:
            parent = os.path.dirname(file_path)
            dest_dir = f"{parent}-translated"
            os.makedirs(dest_dir, exist_ok=True)
            dest = os.path.join(dest_dir, file_name)
        else:
            dest = os.path.join(output_dir, file_name)

        if is_vlm:
            # ── VLM Pass 1: translate → save _translations.txt ──
            _send_progress(
                out,
                req_id,
                (i * steps_per_file) / total_steps,
                f"VLM Pass 1: {file_name}…",
            )

            try:
                # verbose must stay False — otherwise _result_path adds a
                # timestamp image-subfolder and Pass 2 / MTPE cannot find
                # the saved translations.txt.
                translator.input_files = [file_path]
                coroutine = translator.translate_file(file_path, "", {}, config)
                asyncio.new_event_loop().run_until_complete(coroutine)
            except Exception as e:
                logger.error("VLM pass 1 failed for %s: %s", file_name, e)
                file_errors.append(
                    {
                        "file": file_name,
                        "step": "vlm_pass1",
                        "reason": str(e),
                    }
                )
                continue
            finally:
                translator.verbose = False

            # Move translations from temp cache to per-image _Text/ dir
            txt_path = _move_cache_to_image(file_path)
            if not txt_path:
                logger.warning("VLM pass 1: no translations for %s", file_name)
                file_errors.append(
                    {
                        "file": file_name,
                        "step": "vlm_pass1",
                        "reason": "Translations file not created in cache",
                    }
                )
                continue

            # ── VLM MTPE: correct translations via VLM API ──
            if _cancelled.is_set():
                _send_progress(
                    out, req_id, (i * steps_per_file + 1) / total_steps, "Abgebrochen"
                )
                raise BackendError("Translation cancelled", error_type="Cancelled")

            _send_progress(
                out,
                req_id,
                (i * steps_per_file + 1) / total_steps,
                f"VLM Korrektur: {file_name}…",
            )

            try:
                from manga_translator.vlm.mtpe import run_vlm_mtpe

                txt_path = _translations_txt_path(file_path)
                mtpe_result = run_vlm_mtpe(
                    file_path,
                    txt_path,
                    params.get("vlm_type", "gemini"),
                    params.get("target_lang", "DEU"),
                    params.get("gemini_api_key", ""),
                    params.get("gemini_model", "gemini-2.5-pro"),
                    params.get("openrouter_api_key", ""),
                    params.get("openrouter_model", ""),
                    None,
                )

                if mtpe_result.get("success"):
                    count = mtpe_result.get("corrected_count", 0)
                    logger.info(
                        "VLM MTPE successful for %s (%d entries corrected)",
                        file_name,
                        count,
                    )
                else:
                    msg = mtpe_result.get("message", "unknown")
                    logger.warning(
                        "VLM MTPE skipped for %s: %s — using original translations",
                        file_name,
                        msg,
                    )
            except Exception as e:
                logger.error("VLM MTPE failed for %s: %s", file_name, e)
                file_errors.append(
                    {
                        "file": file_name,
                        "step": "vlm_mtpe",
                        "reason": str(e),
                    }
                )
                # Continue to pass 2 with original translations

            # ── VLM Pass 2: re-run with load_text=True ──
            if _cancelled.is_set():
                _send_progress(
                    out, req_id, (i * steps_per_file + 2) / total_steps, "Abgebrochen"
                )
                raise BackendError("Translation cancelled", error_type="Cancelled")

            translator.save_text = False
            translator.load_text = True

            # Copy translations from per-image _Text/ dir to temp cache
            if not _copy_image_to_cache(file_path):
                logger.warning("VLM pass 2: no translations for %s", file_name)
                file_errors.append(
                    {
                        "file": file_name,
                        "step": "vlm_pass2",
                        "reason": "Translations file not found in _Text/ dir",
                    }
                )
                translator.save_text = True
                translator.load_text = False
                continue

            _send_progress(
                out,
                req_id,
                (i * steps_per_file + 2) / total_steps,
                f"VLM Pass 2: {file_name}…",
            )

            try:
                coroutine = translator.translate_file(
                    file_path, dest, {"overwrite": True}, config
                )
                asyncio.new_event_loop().run_until_complete(coroutine)

                if os.path.exists(dest):
                    results.append(dest)
                    _send_progress(
                        out,
                        req_id,
                        ((i + 1) * steps_per_file) / total_steps,
                        f"Fertig: {file_name}",
                    )
                else:
                    logger.warning("VLM output not created: %s", dest)
                    file_errors.append(
                        {
                            "file": file_name,
                            "step": "vlm_pass2",
                            "reason": "Output file not created",
                        }
                    )
            except Exception as e:
                logger.error("VLM pass 2 failed for %s: %s", file_name, e)
                file_errors.append(
                    {
                        "file": file_name,
                        "step": "vlm_pass2",
                        "reason": str(e),
                    }
                )
            finally:
                translator.verbose = False

            # Reset translator state for next image
            translator.save_text = True
            translator.load_text = False

        elif is_extract:
            # ── Extract Mode: OCR + translate → save _translations.txt ──
            _send_progress(
                out,
                req_id,
                i / total_steps,
                f"Extrahiere Text: {file_name}…",
            )

            try:
                translator.input_files = [file_path]
                coroutine = translator.translate_file(file_path, "", {}, config)
                asyncio.new_event_loop().run_until_complete(coroutine)
            except Exception as e:
                logger.error("Extract failed for %s: %s", file_name, e)
                file_errors.append(
                    {"file": file_name, "step": "extract", "reason": str(e)}
                )
                continue

            # Move translations from temp cache to per-image _Text/ dir
            txt_path = _move_cache_to_image(file_path)
            if txt_path:
                results.append(txt_path)
                _send_progress(
                    out,
                    req_id,
                    (i + 1) / total_steps,
                    f"Fertig: {file_name}",
                )
            else:
                logger.warning("Translations file not created for %s", file_name)
                file_errors.append(
                    {
                        "file": file_name,
                        "step": "extract",
                        "reason": "Translations file not created",
                    }
                )

        elif is_cache:
            # ── Text einfügen: load _translations.txt → render image ──
            # Copy from per-image _Text/ dir to temp cache for load_text
            if not _copy_image_to_cache(file_path):
                logger.warning("No translations file for %s", file_name)
                file_errors.append(
                    {
                        "file": file_name,
                        "step": "cache",
                        "reason": f"Translations file not found: "
                        f"{_translations_txt_path(file_path)}",
                    }
                )
                continue

            _send_progress(
                out,
                req_id,
                i / total_steps,
                f"Text einfügen: {file_name}…",
            )

            try:
                translator.input_files = [file_path]
                coroutine = translator.translate_file(
                    file_path, dest, {"overwrite": True}, config
                )
                asyncio.new_event_loop().run_until_complete(coroutine)

                if os.path.exists(dest):
                    results.append(dest)
                    _send_progress(
                        out,
                        req_id,
                        (i + 1) / total_steps,
                        f"Fertig: {file_name}",
                    )
                else:
                    logger.warning("Output not created: %s", dest)
                    file_errors.append(
                        {
                            "file": file_name,
                            "step": "cache",
                            "reason": "Output file not created",
                        }
                    )
            except Exception as e:
                logger.error("Text einfügen failed for %s: %s", file_name, e)
                file_errors.append(
                    {"file": file_name, "step": "cache", "reason": str(e)}
                )

        else:
            # ── Standard Mode: single-pass translation ──
            _send_progress(out, req_id, i / total_steps, f"Übersetze {file_name}…")

            logger.info(
                "translate_file(%s, %s, {{overwrite: True}}, config)", file_path, dest
            )

            # Diagnostic: translator state
            logger.info(
                "Translator state: save_text=%s, load_text=%s",
                getattr(translator, "save_text", "?"),
                getattr(translator, "load_text", "?"),
            )

            try:
                translator.verbose = True
                translator.input_files = [file_path]

                # Disable ignore_errors for first file to surface errors
                if i == 0:
                    translator.ignore_errors = False
                    logger.warning("DEBUG: ignore_errors disabled for first file")

                coroutine = translator.translate_file(
                    file_path, dest, {"overwrite": True}, config
                )
                bool_result = asyncio.new_event_loop().run_until_complete(coroutine)

                logger.info(
                    "translate_file returned: %s for %s — output exists: %s",
                    bool_result,
                    file_name,
                    os.path.exists(dest),
                )

                if os.path.exists(dest):
                    results.append(dest)
                    _send_progress(
                        out, req_id, (i + 1) / total_steps, f"Fertig: {file_name}"
                    )
                else:
                    logger.warning(
                        "Output not created despite success: %s (returned %s)",
                        dest,
                        bool_result,
                    )
                    file_errors.append(
                        {
                            "file": file_name,
                            "step": "translate",
                            "reason": f"Output not created (Python returned {bool_result})",
                        }
                    )
            except Exception as e:
                logger.error("Translation failed for %s: %s", file_name, e)
                logger.error("Traceback:\n%s", traceback.format_exc())
                file_errors.append(
                    {
                        "file": file_name,
                        "step": "translate",
                        "reason": str(e),
                    }
                )
            finally:
                translator.verbose = False
                if i == 0:
                    translator.ignore_errors = True
                    logger.info("DEBUG: ignore_errors restored to True")

    # ── Build result ──
    logger.info("Translation complete: %d/%d files succeeded", len(results), file_count)

    # Restore translator state & clean up temp cache
    if is_vlm or is_extract or is_cache:
        translator.save_text = False
        translator.load_text = False
        translator.result_sub_folder = ""
        # Remove temp cache directory (files have been moved / copied)
        try:
            from manga_translator.vlm.mtpe import get_result_dir

            _tmp_dir = os.path.join(get_result_dir(), "CACHE", "_tmp")
            if os.path.isdir(_tmp_dir):
                shutil.rmtree(_tmp_dir, ignore_errors=True)
        except Exception:
            pass

    if not results and file_errors:
        summary = "; ".join(
            f"{e['file']} ({e['step']}): {e['reason']}" for e in file_errors
        )
        raise BackendError(f"All {len(file_errors)} file(s) failed: {summary}")

    if file_errors:
        logger.warning(
            "%d/%d files failed during translation", len(file_errors), file_count
        )
        _send_progress(
            out,
            req_id,
            1.0,
            f"Fertig: {len(results)}/{file_count} erfolgreich, "
            f"{len(file_errors)} fehlgeschlagen",
        )

    return {
        "results": results,
        "errors": file_errors,
    }


def handle_cancel(params: dict) -> str:
    """Set the cancellation flag."""
    _cancelled.set()
    logger.info("Translation cancellation requested")
    return "ok"


# ---------------------------------------------------------------------------
# Error type
# ---------------------------------------------------------------------------


class BackendError(Exception):
    """Structured error for IPC communication."""

    def __init__(self, message: str, error_type: str = "BackendError"):
        super().__init__(message)
        self.error_type = error_type


# ---------------------------------------------------------------------------
# JSON I/O helpers
# ---------------------------------------------------------------------------


def _write_json(out: io.TextIOBase, obj: dict) -> None:
    """Write a JSON object followed by a newline (thread-safe)."""
    line = json.dumps(obj, ensure_ascii=False) + "\n"
    with _write_lock:
        out.write(line)
        out.flush()


def _read_json(infile: io.TextIOBase) -> Optional[dict]:
    """Read one JSON object from stdin. Returns None on EOF."""
    line = infile.readline()
    if not line:
        return None
    line = line.strip()
    if not line:
        return None
    return json.loads(line)


_write_lock = threading.Lock()


# ---------------------------------------------------------------------------
# Dispatch table
# ---------------------------------------------------------------------------

# Commands that do NOT need access to the output stream
SIMPLE_COMMANDS = {
    "ping": handle_ping,
    "python_version": handle_python_version,
    "is_backend_available": handle_is_backend_available,
    "generate_thumbnail": handle_generate_thumbnail,
    "get_cached_translation": handle_get_cached_translation,
    "fetch_openrouter_models": handle_fetch_openrouter_models,
    "cancel": handle_cancel,
}


# ---------------------------------------------------------------------------
# Background stdin reader
# ---------------------------------------------------------------------------


def _stdin_reader(infile: io.TextIOBase, cmd_queue: queue.Queue) -> None:
    """Background thread that reads JSON commands from *infile* into *cmd_queue*.

    Runs as a daemon thread so it will not prevent the process from exiting.
    Parsed JSON dicts are placed into *cmd_queue*.  On EOF a ``None``
    sentinel is pushed to signal orderly shutdown.  Invalid JSON lines
    are logged and skipped.
    """
    while True:
        line = infile.readline()
        if not line:
            # EOF — signal shutdown
            cmd_queue.put(None)
            break
        line = line.strip()
        if not line:
            continue
        try:
            cmd_queue.put(json.loads(line))
        except json.JSONDecodeError as exc:
            logger.error("Invalid JSON from stdin: %s", exc)


# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------


def main_loop(
    infile: io.TextIOBase = sys.stdin, outfile: io.TextIOBase = sys.stdout
) -> None:
    """Read requests from *infile*, dispatch, write responses to *outfile*.

    A background daemon thread reads from stdin and places parsed JSON
    commands into ``_command_queue``.  This allows cancel commands to be
    processed even during long-running translation jobs (see the queue
    drain in :func:`handle_translate`).
    """
    logger.info("Backend server started (PID %d)", os.getpid())

    # Start background stdin reader thread
    reader_thread = threading.Thread(
        target=_stdin_reader,
        args=(infile, _command_queue),
        daemon=True,
        name="stdin-reader",
    )
    reader_thread.start()

    _write_json(outfile, {"id": 0, "method": "ready", "result": "pong"})

    while True:
        request = _command_queue.get()

        if request is None:
            logger.info("EOF on stdin — shutting down")
            break

        req_id = request.get("id", 0)
        method = request.get("method", "")
        params = request.get("params", {})

        logger.debug("Request: id=%d method=%s", req_id, method)

        # Shutdown command
        if method == "shutdown":
            _write_json(outfile, {"id": req_id, "result": "ok"})
            break

        # Simple commands
        if method in SIMPLE_COMMANDS:
            try:
                result = SIMPLE_COMMANDS[method](params)
                _write_json(outfile, {"id": req_id, "result": result})
            except BackendError as e:
                _write_json(
                    outfile,
                    {
                        "id": req_id,
                        "error": {"type": e.error_type, "message": str(e)},
                    },
                )
            except Exception as e:
                logger.error("Error handling %s: %s", method, e)
                _write_json(
                    outfile,
                    {
                        "id": req_id,
                        "error": {"type": "InternalError", "message": str(e)},
                    },
                )
            continue

        # Translate command (needs progress stream)
        if method == "translate":
            try:
                result = handle_translate(params, outfile, req_id)
                _write_json(outfile, {"id": req_id, "result": result})
            except BackendError as e:
                _write_json(
                    outfile,
                    {
                        "id": req_id,
                        "error": {"type": e.error_type, "message": str(e)},
                    },
                )
            except Exception as e:
                logger.error("Translation error: %s\n%s", e, traceback.format_exc())
                _write_json(
                    outfile,
                    {
                        "id": req_id,
                        "error": {"type": "InternalError", "message": str(e)},
                    },
                )
            continue

        # Unknown command
        _write_json(
            outfile,
            {
                "id": req_id,
                "error": {
                    "type": "UnknownMethod",
                    "message": f"Unknown method: {method}",
                },
            },
        )

    logger.info("Backend server shut down")


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------


def self_test() -> None:
    """Run a quick self-test to verify the server starts and responds."""
    import subprocess
    import time

    script = os.path.abspath(__file__)
    proc = subprocess.Popen(
        [sys.executable, script],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )

    # Read the ready message sent at startup (before any input)
    response_line = proc.stdout.readline()
    response = json.loads(response_line) if response_line else None
    print(f"Ready message: {response}")

    # Send ping
    request = {"id": 1, "method": "ping", "params": {}}
    proc.stdin.write(json.dumps(request) + "\n")
    proc.stdin.flush()

    response_line = proc.stdout.readline()
    response = json.loads(response_line) if response_line else None
    print(f"Ping response: {response}")

    # Send python_version
    request = {"id": 2, "method": "python_version", "params": {}}
    proc.stdin.write(json.dumps(request) + "\n")
    proc.stdin.flush()
    response_line = proc.stdout.readline()
    response = json.loads(response_line) if response_line else None
    print(f"Version response: {response}")

    # Shutdown
    request = {"id": 3, "method": "shutdown", "params": {}}
    proc.stdin.write(json.dumps(request) + "\n")
    proc.stdin.flush()

    proc.wait(timeout=5)
    print(f"Exit code: {proc.returncode}")


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Manga Translator IPC Backend")
    parser.add_argument("--test", action="store_true", help="Run self-test")
    args = parser.parse_args()

    if args.test:
        self_test()
    else:
        # Ignore SIGINT — the Rust parent controls lifecycle via "shutdown"
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        main_loop()

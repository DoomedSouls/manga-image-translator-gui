// manga-translator-gtk/src/ipc_bridge.rs
//
// IPC bridge between the Rust GTK4 GUI and the Python manga_translator backend.
//
// Replaces python_bridge.rs with subprocess-based communication.  The Python
// backend runs as a separate process (backend/server.py), communicating via
// newline-delimited JSON over stdin/stdout.
//
// Benefits over the PyO3 embedded approach:
//   - No Python version lock — any Python 3.10+ works
//   - Backend crash doesn't kill the GUI
//   - Clean packaging (AppImage only needs GTK4 libs)
//   - Simpler build (no PyO3 compilation)
//
// Protocol:
//   Request:    {"id": N, "method": "...", "params": {...}}
//   Response:   {"id": N, "result": <any>}
//   Error:      {"id": N, "error":  {"type": "...", "message": "..."}}
//   Progress:   {"id": N, "progress": 0.5, "message": "..."}
//
// Dependency note:
//   Requires `base64` crate in Cargo.toml:
//     base64 = "0.22"

use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors produced by the IPC bridge.
#[derive(Debug)]
pub enum BridgeError {
    Io(std::io::Error),
    Message(String),
    NotInitialized,
    Cancelled,
    ApiKeyMissing {
        service: String,
    },
    FileNotFound {
        path: PathBuf,
    },
    TranslationFailed {
        file: String,
        step: String,
        reason: String,
    },
    VlmError(String),
    /// IPC communication error (subprocess died, invalid JSON, etc.)
    Ipc(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Io(e) => write!(f, "IO error: {}", e),
            BridgeError::Message(s) => write!(f, "{}", s),
            BridgeError::NotInitialized => write!(f, "IPC bridge not initialized"),
            BridgeError::Cancelled => write!(f, "Translation cancelled"),
            BridgeError::ApiKeyMissing { service } => {
                write!(f, "API key missing for service: {}", service)
            }
            BridgeError::FileNotFound { path } => {
                write!(f, "File not found: {}", path.display())
            }
            BridgeError::TranslationFailed { file, step, reason } => {
                write!(f, "Translation failed for {} ({}): {}", file, step, reason)
            }
            BridgeError::VlmError(s) => write!(f, "VLM error: {}", s),
            BridgeError::Ipc(s) => write!(f, "IPC error: {}", s),
        }
    }
}

impl From<std::io::Error> for BridgeError {
    fn from(e: std::io::Error) -> Self {
        BridgeError::Io(e)
    }
}

impl From<String> for BridgeError {
    fn from(e: String) -> Self {
        BridgeError::Message(e)
    }
}

impl From<&str> for BridgeError {
    fn from(e: &str) -> Self {
        BridgeError::Message(e.to_string())
    }
}

impl From<serde_json::Error> for BridgeError {
    fn from(e: serde_json::Error) -> Self {
        BridgeError::Ipc(e.to_string())
    }
}

/// Shorthand result type.
pub type BridgeResult<T> = Result<T, BridgeError>;

// ---------------------------------------------------------------------------
// Data types (same shape as python_bridge.rs for drop-in compatibility)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FileError {
    pub file: String,
    pub step: String,
    pub reason: String,
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}): {}", self.file, self.step, self.reason)
    }
}

#[derive(Debug, Clone)]
pub struct ImageEntry {
    pub path: PathBuf,
    pub name: String,
    pub size_bytes: u64,
    pub modified_timestamp: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct FolderEntry {
    pub path: PathBuf,
    pub name: String,
    pub image_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslationParams {
    pub translator: String,
    pub target_lang: String,
    pub detector: String,
    pub ocr: String,
    pub inpainter: String,
    pub upscaler: String,
    pub direction: String,
    pub use_mocr_merge: bool,
    pub device: String,
    pub output_directory: String,
    pub use_original_folder: bool,
    pub translation_mode: String,
    pub vlm_type: String,
    pub gemini_model: String,
    pub openrouter_model: String,
    pub local_model_path: String,
    pub project_name: String,
    pub gemini_api_key: String,
    pub openrouter_api_key: String,
    pub upscale_ratio: u32,
    pub renderer: String,
    pub alignment: String,
    pub disable_font_border: bool,
    pub font_size_offset: i32,
    pub font_size_minimum: i32,
    pub render_direction: String,
    pub uppercase: bool,
    pub lowercase: bool,
    pub font_color: String,
    pub no_hyphenation: bool,
    pub line_spacing: i32,
    pub font_size: i32,
    pub rtl: bool,
    pub mask_dilation_offset: i32,
    pub kernel_size: i32,
    pub inpainting_size: u32,
    pub inpainting_precision: String,
    pub detection_size: u32,
    pub api_keys: HashMap<String, String>,
}

impl Default for TranslationParams {
    fn default() -> Self {
        Self {
            translator: "offline".into(),
            target_lang: "DEU".into(),
            detector: "ctd".into(),
            ocr: "48px".into(),
            inpainter: "lama".into(),
            upscaler: "none".into(),
            direction: "auto".into(),
            use_mocr_merge: false,
            device: "cuda".into(),
            output_directory: String::new(),
            use_original_folder: true,
            translation_mode: "standard".into(),
            vlm_type: "gemini".into(),
            gemini_model: "gemini-2.5-pro".into(),
            openrouter_model: String::new(),
            local_model_path: String::new(),
            project_name: "Unbenannt".into(),
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
            rtl: true,
            mask_dilation_offset: 20,
            kernel_size: 3,
            inpainting_size: 2048,
            inpainting_precision: "bf16".into(),
            detection_size: 2048,
            api_keys: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// Stored configuration for the Python backend.
struct BridgeConfig {
    venv_site_packages: Option<PathBuf>,
    manga_translator_dir: Option<PathBuf>,
    python_bin: String,
}

// ---------------------------------------------------------------------------
// IpcBridge
// ---------------------------------------------------------------------------

/// IPC bridge to the Python manga_translator backend.
///
/// Manages a subprocess running `backend/server.py`.  Communication is
/// via newline-delimited JSON over stdin/stdout.  A background reader
/// thread forwards responses from stdout into an MPSC channel.
///
/// Three separate mutexes allow concurrent access:
///   - `stdin_writer` — for sending requests
///   - `response_rx`  — for receiving responses (held during translate)
///   - `child`        — for process lifecycle
///
/// The cancel flow works because cancel only needs `stdin_writer`
/// (separate from `response_rx` held by translate).
pub struct IpcBridge {
    stdin_writer: Mutex<Option<BufWriter<ChildStdin>>>,
    response_rx: Mutex<Option<Receiver<Value>>>,
    child: Mutex<Option<Child>>,
    config: Mutex<BridgeConfig>,
    initialized: AtomicBool,
    next_id: AtomicU64,
}

impl IpcBridge {
    /// Create a new bridge (does NOT start the backend yet).
    pub fn new() -> Self {
        Self {
            stdin_writer: Mutex::new(None),
            response_rx: Mutex::new(None),
            child: Mutex::new(None),
            config: Mutex::new(BridgeConfig {
                venv_site_packages: None,
                manga_translator_dir: None,
                python_bin: String::new(),
            }),
            initialized: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
        }
    }

    // -----------------------------------------------------------------------
    // Configuration
    // -----------------------------------------------------------------------

    /// Configure paths to the virtual environment and manga_translator directory.
    ///
    /// Must be called **before** `ensure_initialized()` for the paths to
    /// take effect.  Resets `initialized` so the next call will restart
    /// the backend process with the updated configuration.
    pub fn configure_paths(
        &self,
        venv_site_packages: Option<PathBuf>,
        manga_translator_dir: Option<PathBuf>,
    ) {
        let mut cfg = self.config.lock().unwrap();
        cfg.venv_site_packages = venv_site_packages.clone();
        cfg.manga_translator_dir = manga_translator_dir.clone();

        // Derive python binary from venv: site-packages → python3.X → lib → venv root
        if let Some(ref sp) = venv_site_packages {
            // Linux:   /path/to/venv/lib/python3.X/site-packages → /path/to/venv/bin/python3
            // Windows: C:\venv\Lib\site-packages → C:\venv\Scripts\python.exe
            if let Some(venv_root) = sp
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
            {
                #[cfg(target_os = "windows")]
                let bin = venv_root.join("Scripts").join("python.exe");
                #[cfg(not(target_os = "windows"))]
                let bin = venv_root.join("bin").join("python3");
                if bin.exists() {
                    cfg.python_bin = bin.to_string_lossy().to_string();
                }
            }
        }

        self.initialized.store(false, Ordering::SeqCst);
        log::info!(
            "IPC bridge paths configured: venv_site_packages={:?}, manga_translator_dir={:?}, python_bin={}",
            cfg.venv_site_packages,
            cfg.manga_translator_dir,
            cfg.python_bin,
        );
    }

    // -----------------------------------------------------------------------
    // Subprocess lifecycle
    // -----------------------------------------------------------------------

    /// Ensure the backend process is running.  Starts it if necessary.
    pub fn ensure_initialized(&self) -> BridgeResult<()> {
        if self.initialized.load(Ordering::SeqCst) {
            return Ok(());
        }
        self.start_process()
    }

    /// Start (or restart) the Python backend process.
    fn start_process(&self) -> BridgeResult<()> {
        // Stop any existing process first
        self.stop_process();

        let python_bin = {
            let cfg = self.config.lock().unwrap();
            if cfg.python_bin.is_empty() {
                #[cfg(target_os = "windows")]
                {
                    "python".to_string()
                }
                #[cfg(not(target_os = "windows"))]
                {
                    "python3".to_string()
                }
            } else {
                cfg.python_bin.clone()
            }
        };

        let server_script = self.find_server_script()?;
        let pythonpath = self.build_pythonpath();

        log::info!(
            "Starting backend: {} {}",
            python_bin,
            server_script.display()
        );
        log::debug!("PYTHONPATH={}", pythonpath);

        let mut child = Command::new(&python_bin)
            .arg(&server_script)
            .env("PYTHONPATH", &pythonpath)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                BridgeError::Message(format!(
                    "Failed to start backend: {} — Is '{}' a valid Python interpreter?",
                    e, python_bin
                ))
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| BridgeError::Message("Failed to acquire stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BridgeError::Message("Failed to acquire stdout".into()))?;

        let (tx, rx) = mpsc::channel();

        // Background thread: read JSON lines from stdout → channel
        std::thread::Builder::new()
            .name("ipc-reader".into())
            .spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            let trimmed = line.trim().to_string();
                            if trimmed.is_empty() {
                                continue;
                            }
                            match serde_json::from_str::<Value>(&trimmed) {
                                Ok(value) => {
                                    if tx.send(value).is_err() {
                                        break; // Channel disconnected
                                    }
                                }
                                Err(e) => {
                                    log::error!("Invalid JSON from backend: {}", e);
                                }
                            }
                        }
                        Err(_) => break, // EOF
                    }
                }
                log::info!("IPC reader thread exiting");
            })
            .map_err(|e| BridgeError::Message(format!("Failed to spawn reader thread: {}", e)))?;

        // Store state
        *self.stdin_writer.lock().unwrap() = Some(BufWriter::new(stdin));
        *self.child.lock().unwrap() = Some(child);
        *self.response_rx.lock().unwrap() = Some(rx);

        // Wait for the "ready" message (backend sends it immediately on startup)
        {
            let rx_guard = self.response_rx.lock().unwrap();
            let rx = rx_guard
                .as_ref()
                .ok_or_else(|| BridgeError::Message("No response channel".into()))?;

            match rx.recv_timeout(Duration::from_secs(30)) {
                Ok(msg) => {
                    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
                    if method == "ready" {
                        log::info!("Backend process ready");
                    } else {
                        log::warn!("Unexpected first message from backend: {:?}", msg);
                    }
                }
                Err(e) => {
                    // Clean up on failure
                    drop(rx_guard);
                    self.stop_process();
                    return Err(BridgeError::Message(format!(
                        "Backend did not start within 30s: {}",
                        e
                    )));
                }
            }
        }

        self.initialized.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Stop the backend process (graceful shutdown → kill).
    fn stop_process(&self) {
        // Send shutdown command
        {
            let mut writer_guard = self.stdin_writer.lock().unwrap();
            if let Some(ref mut writer) = *writer_guard {
                let _ = writeln!(writer, "{}", r#"{"id":0,"method":"shutdown","params":{}}"#);
                let _ = writer.flush();
            }
            *writer_guard = None;
        }

        // Kill the process
        {
            let mut child_guard = self.child.lock().unwrap();
            if let Some(ref mut child) = *child_guard {
                match child.try_wait() {
                    Ok(Some(_)) => {} // Already exited
                    Ok(None) => {
                        // Give it 500ms to exit gracefully, then kill
                        std::thread::sleep(Duration::from_millis(500));
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    Err(_) => {
                        let _ = child.kill();
                    }
                }
            }
            *child_guard = None;
        }

        // Clear the response channel (don't lock if translate holds it)
        if let Ok(mut rx_guard) = self.response_rx.try_lock() {
            *rx_guard = None;
        }
        // If we couldn't get the lock (translate is running), the channel
        // will disconnect naturally when the reader thread exits (stdout closed).

        self.initialized.store(false, Ordering::SeqCst);
        log::info!("Backend process stopped");
    }

    // -----------------------------------------------------------------------
    // Low-level IPC
    // -----------------------------------------------------------------------

    /// Send a JSON request to the backend.  Returns the request ID.
    fn send_request(&self, method: &str, params: Value) -> BridgeResult<u64> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = json!({
            "id": id,
            "method": method,
            "params": params,
        });

        let mut writer_guard = self.stdin_writer.lock().unwrap();
        let writer = writer_guard
            .as_mut()
            .ok_or_else(|| BridgeError::NotInitialized)?;

        let line = serde_json::to_string(&request)?;
        writeln!(writer, "{}", line).map_err(|e| BridgeError::Ipc(e.to_string()))?;
        writer
            .flush()
            .map_err(|e| BridgeError::Ipc(e.to_string()))?;

        Ok(id)
    }

    /// Fire-and-forget cancel command to the Python backend.
    fn send_cancel(&self) {
        match self.send_request("cancel", json!({})) {
            Ok(_) => log::info!("Cancel command sent to backend"),
            Err(e) => log::warn!("Could not send cancel to backend: {}", e),
        }
    }

    /// Send a request and wait for a single response (no progress).
    fn call(&self, method: &str, params: Value) -> BridgeResult<Value> {
        let id = self.send_request(method, params)?;

        let rx_guard = self.response_rx.lock().unwrap();
        let rx = rx_guard
            .as_ref()
            .ok_or_else(|| BridgeError::NotInitialized)?;

        loop {
            match rx.recv_timeout(Duration::from_secs(300)) {
                Ok(response) => {
                    let resp_id = response.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                    if resp_id != id {
                        log::debug!("Skipping response id={} (expected {})", resp_id, id);
                        continue;
                    }
                    if let Some(error) = response.get("error") {
                        return Err(self.parse_error(error));
                    }
                    return Ok(response.get("result").cloned().unwrap_or(Value::Null));
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(BridgeError::Ipc("Backend timed out (300s)".into()));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.initialized.store(false, Ordering::SeqCst);
                    return Err(BridgeError::Ipc("Backend process died".into()));
                }
            }
        }
    }

    /// Send a translate request and handle streamed progress + result.
    fn call_translate(
        &self,
        params: Value,
        cancel_flag: &AtomicBool,
        on_progress: Box<dyn Fn(f64, &str) + Send>,
    ) -> BridgeResult<Vec<PathBuf>> {
        let id = self.send_request("translate", params)?;
        let mut cancel_sent = false;

        let rx_guard = self.response_rx.lock().unwrap();
        let rx = rx_guard
            .as_ref()
            .ok_or_else(|| BridgeError::NotInitialized)?;

        loop {
            // Check local cancel flag
            if !cancel_sent && cancel_flag.load(Ordering::Relaxed) {
                // send_cancel uses stdin_writer mutex — no deadlock with response_rx
                self.send_cancel();
                cancel_sent = true;
            }

            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(response) => {
                    let resp_id = response.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                    if resp_id != id {
                        // Skip responses for other requests (e.g. cancel ack)
                        continue;
                    }

                    // Progress?
                    if let Some(progress) = response.get("progress").and_then(|v| v.as_f64()) {
                        let msg = response
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        on_progress(progress, msg);
                        continue;
                    }

                    // Error?
                    if let Some(error) = response.get("error") {
                        return Err(self.parse_error(error));
                    }

                    // Result
                    let result = response.get("result").cloned().unwrap_or(Value::Null);
                    let results: Vec<String> = result
                        .get("results")
                        .and_then(|r| serde_json::from_value(r.clone()).ok())
                        .unwrap_or_default();
                    return Ok(results.into_iter().map(PathBuf::from).collect());
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Continue loop to check cancel flag again
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.initialized.store(false, Ordering::SeqCst);
                    return Err(BridgeError::Ipc(
                        "Backend process died during translation".into(),
                    ));
                }
            }
        }
    }

    /// Convert a Python error dict to a BridgeError.
    fn parse_error(&self, error: &Value) -> BridgeError {
        let error_type = error
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("BackendError");
        let message = error
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");

        match error_type {
            "ApiKeyMissing" => BridgeError::ApiKeyMissing {
                service: message.to_string(),
            },
            "Cancelled" => BridgeError::Cancelled,
            "FileNotFound" => BridgeError::FileNotFound {
                path: PathBuf::from(message),
            },
            "TranslationFailed" => BridgeError::TranslationFailed {
                file: message.to_string(),
                step: String::new(),
                reason: String::new(),
            },
            "VlmError" => BridgeError::VlmError(message.to_string()),
            _ => BridgeError::Message(message.to_string()),
        }
    }

    // -----------------------------------------------------------------------
    // Path discovery helpers
    // -----------------------------------------------------------------------

    /// Find the `backend/server.py` script relative to the executable or CWD.
    fn find_server_script(&self) -> BridgeResult<PathBuf> {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                // cargo build → target/debug/manga-translator-gtk
                let candidate = exe_dir.join("..").join("backend").join("server.py");
                if candidate.is_file() {
                    return Ok(candidate);
                }
                // cargo build --release → target/release/manga-translator-gtk
                let candidate = exe_dir
                    .join("..")
                    .join("..")
                    .join("backend")
                    .join("server.py");
                if candidate.is_file() {
                    return Ok(candidate);
                }

                // Installed / AppImage mode: <prefix>/bin/ → <prefix>/share/manga-translator-gtk/backend/
                let candidate = exe_dir
                    .join("..")
                    .join("share")
                    .join("manga-translator-gtk")
                    .join("backend")
                    .join("server.py");
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }

        // Try relative to CWD
        let candidate = PathBuf::from("backend/server.py");
        if candidate.is_file() {
            return Ok(candidate);
        }

        // Try from configured manga_translator_path
        let cfg = self.config.lock().unwrap();
        if let Some(ref mt_dir) = cfg.manga_translator_dir {
            let candidate = mt_dir
                .join("..")
                .join("manga-translator-gtk")
                .join("backend")
                .join("server.py");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }

        Err(BridgeError::Message(
            "backend/server.py not found. Expected next to the executable or in CWD.".into(),
        ))
    }

    /// Build the PYTHONPATH environment variable value.
    fn build_pythonpath(&self) -> String {
        let mut paths = Vec::new();

        let cfg = self.config.lock().unwrap();

        // Configured paths (highest priority)
        if let Some(ref mt_dir) = cfg.manga_translator_dir {
            paths.push(mt_dir.to_string_lossy().to_string());
        }
        if let Some(ref venv_sp) = cfg.venv_site_packages {
            paths.push(venv_sp.to_string_lossy().to_string());
        }

        // Auto-discovery fallback
        if paths.is_empty() {
            if let Ok(exe) = std::env::current_exe() {
                if let Some(exe_dir) = exe.parent() {
                    // development: target/debug/../../  →  manga-image-translator/
                    let project_root = exe_dir.join("..").join("..");
                    if project_root.join("manga_translator").is_dir() {
                        paths.push(project_root.to_string_lossy().to_string());
                    }
                    // Also try one level up
                    let one_up = exe_dir.join("..");
                    if one_up.join("manga_translator").is_dir() {
                        paths.push(one_up.to_string_lossy().to_string());
                    }
                }
            }
            if let Ok(cwd) = std::env::current_dir() {
                if cwd.join("manga_translator").is_dir() {
                    paths.push(cwd.to_string_lossy().to_string());
                }
                let parent = cwd.join("..");
                if parent.join("manga_translator").is_dir() {
                    paths.push(parent.to_string_lossy().to_string());
                }
            }
        }

        #[cfg(target_os = "windows")]
        let separator = ";";
        #[cfg(not(target_os = "windows"))]
        let separator = ":";
        paths.join(separator)
    }

    // -----------------------------------------------------------------------
    // Public API — queries
    // -----------------------------------------------------------------------

    /// Get the Python version string from the backend.
    pub fn python_version(&self) -> BridgeResult<String> {
        self.ensure_initialized()?;
        let result = self.call("python_version", json!({}))?;
        result
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BridgeError::Ipc("Invalid python_version response".into()))
    }

    /// Check if the manga_translator backend is available.
    ///
    /// Returns `false` (instead of erroring) when the backend is not
    /// configured or the Python modules cannot be imported.
    pub fn is_backend_available(&self) -> bool {
        if let Err(e) = self.ensure_initialized() {
            log::info!("is_backend_available: initialization failed: {}", e);
            return false;
        }
        match self.call("is_backend_available", json!({})) {
            Ok(v) => v.as_bool().unwrap_or(false),
            Err(e) => {
                log::warn!("is_backend_available call failed: {}", e);
                false
            }
        }
    }

    // -----------------------------------------------------------------------
    // Public API — thumbnails
    // -----------------------------------------------------------------------

    /// Generate a thumbnail for an image file, returning PNG bytes.
    pub fn generate_thumbnail(&self, path: &Path, max_size: (i32, i32)) -> BridgeResult<Vec<u8>> {
        self.ensure_initialized()?;

        let result = self.call(
            "generate_thumbnail",
            json!({
                "path": path.to_string_lossy().as_ref(),
                "max_w": max_size.0,
                "max_h": max_size.1,
            }),
        )?;

        let base64_str = result
            .as_str()
            .ok_or_else(|| BridgeError::Ipc("Invalid thumbnail response (not a string)".into()))?;

        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(base64_str)
            .map_err(|e| BridgeError::Ipc(format!("Base64 decode error: {}", e)))
    }

    // -----------------------------------------------------------------------
    // Public API — cache
    // -----------------------------------------------------------------------

    /// Check if a cached translation exists for the given source image.
    pub fn get_cached_translation(&self, path: &Path) -> BridgeResult<Option<PathBuf>> {
        self.ensure_initialized()?;

        let result = self.call(
            "get_cached_translation",
            json!({
                "path": path.to_string_lossy().as_ref(),
            }),
        )?;

        match result {
            Value::Null => Ok(None),
            s if s.is_string() => Ok(Some(PathBuf::from(s.as_str().expect("checked is_string")))),
            _ => Ok(None),
        }
    }

    // -----------------------------------------------------------------------
    // Public API — OpenRouter
    // -----------------------------------------------------------------------

    /// Fetch available vision-capable models from the OpenRouter API.
    pub fn fetch_openrouter_models(&self, api_key: &str) -> BridgeResult<Vec<ModelInfo>> {
        self.ensure_initialized()?;

        let result = self.call("fetch_openrouter_models", json!({ "api_key": api_key }))?;

        let models: Vec<ModelInfo> = result
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let id = item.get("id")?.as_str()?.to_string();
                        let name = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&id)
                            .to_string();
                        Some(ModelInfo { id, name })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    // -----------------------------------------------------------------------
    // Public API — translation
    // -----------------------------------------------------------------------

    /// Translate one or more image files.
    ///
    /// Sends the job to the Python backend and streams progress via
    /// `on_progress`.  Checks `cancel_flag` every 500 ms and forwards
    /// a cancel command to the backend when set.
    pub fn translate(
        &self,
        file_paths: &[PathBuf],
        params: &TranslationParams,
        cancel_flag: &AtomicBool,
        on_progress: Box<dyn Fn(f64, &str) + Send>,
    ) -> BridgeResult<Vec<PathBuf>> {
        self.ensure_initialized()?;

        // Pre-flight: empty file list
        if file_paths.is_empty() {
            return Err(BridgeError::Message(
                "No files selected for translation".into(),
            ));
        }

        // Pre-flight: missing files (local check, no backend round-trip)
        let missing: Vec<&PathBuf> = file_paths.iter().filter(|p| !p.exists()).collect();
        if !missing.is_empty() {
            return Err(BridgeError::FileNotFound {
                path: missing[0].clone(),
            });
        }

        // Build params dict: serialize TranslationParams + add extra fields
        let mut params_value = serde_json::to_value(params)
            .map_err(|e| BridgeError::Ipc(format!("Failed to serialize params: {}", e)))?;

        // Add file list
        params_value["files"] = serde_json::to_value(file_paths)?;

        // Add log file path (for Python-side logging redirection)
        let log_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("manga-translator-gtk")
            .join("manga-translator.log");
        params_value["log_path"] = json!(log_path.to_string_lossy().as_ref());

        self.call_translate(params_value, cancel_flag, on_progress)
    }

    /// Request translation cancellation.
    ///
    /// Sets the local cancel flag AND sends a cancel command to the
    /// Python backend.  The translate loop will receive the error
    /// response and return `BridgeError::Cancelled`.
    pub fn cancel_translation(&self, cancel_flag: &AtomicBool) {
        cancel_flag.store(true, Ordering::Relaxed);
        log::info!("Translation cancellation requested");
        self.send_cancel();
    }

    // -----------------------------------------------------------------------
    // Public API — file listing (pure Rust, no backend needed)
    // -----------------------------------------------------------------------

    /// List subdirectories in a directory.
    ///
    /// Pure Rust — no backend round-trip required.
    pub fn list_directories(&self, path: &Path) -> BridgeResult<Vec<PathBuf>> {
        let path = path.to_path_buf();
        if !path.is_dir() {
            return Ok(vec![]);
        }

        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&path)?
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect();

        dirs.sort();
        Ok(dirs)
    }

    /// List image files in a directory (supports common manga formats).
    ///
    /// Pure Rust — no backend round-trip required.
    pub fn list_image_files(&self, directory: &Path) -> BridgeResult<Vec<ImageEntry>> {
        let image_extensions = ["png", "jpg", "jpeg", "bmp", "tiff", "tif", "webp", "gif"];
        let mut entries = Vec::new();

        let read_dir = std::fs::read_dir(directory)?;
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !image_extensions.contains(&ext.to_lowercase().as_str()) {
                continue;
            }

            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();
            let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());

            entries.push(ImageEntry {
                path,
                name: file_name,
                size_bytes: file_size,
                modified_timestamp: modified,
            });
        }

        Ok(entries)
    }
}

// ---------------------------------------------------------------------------
// Cleanup on drop
// ---------------------------------------------------------------------------

impl Drop for IpcBridge {
    fn drop(&mut self) {
        // Best-effort shutdown — send command, then kill.
        if let Some(ref mut writer) = *self.stdin_writer.get_mut().unwrap() {
            let _ = writeln!(writer, "{}", r#"{"id":0,"method":"shutdown","params":{}}"#);
            let _ = writer.flush();
        }
        if let Some(ref mut child) = *self.child.get_mut().unwrap() {
            std::thread::sleep(Duration::from_millis(200));
            let _ = child.kill();
            let _ = child.wait();
        }
        log::info!("IpcBridge dropped, backend cleaned up");
    }
}

//! ClamAV-based virus scanner
//!
//! Uses clamav-sys crate to directly call libclamav for file scanning.

use std::path::Path;
use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;
use tracing::{info, warn};

/// ClamAV engine instance (singleton)
static CLAMAV_ENGINE: Lazy<Arc<Mutex<Option<ClamAvContext>>>> = Lazy::new(|| {
    Arc::new(Mutex::new(None))
});

struct ClamAvContext {
    engine: *mut clamav_sys::cl_engine,
    db_path: String,
}

// Safety: ClamAvContext is protected by a Mutex, so it's safe to send between threads
unsafe impl Send for ClamAvContext {}

impl ClamAvContext {
    fn new() -> Result<Self, String> {
        let db_path = get_clamav_db_path();
        info!("[clamav] Initializing ClamAV with database: {}", db_path);

        if !Path::new(&db_path).exists() {
            return Err(format!(
                "ClamAV database not found at: {}. Run 'freshclam' to download.",
                db_path
            ));
        }

        unsafe {
            let engine = clamav_sys::cl_engine_new();
            if engine.is_null() {
                return Err("Failed to create ClamAV engine".to_string());
            }

            let mut signo: u32 = 0;
            let db_ret = clamav_sys::cl_load(
                db_path.as_ptr() as *const std::os::raw::c_char,
                engine,
                &mut signo,
                clamav_sys::CL_DB_STDOPT,
            );

            if db_ret != clamav_sys::cl_error_t::CL_SUCCESS {
                let err_msg = format!("Failed to load ClamAV database: {}", db_ret);
                clamav_sys::cl_engine_free(engine);
                return Err(err_msg);
            }

            let compile_ret = clamav_sys::cl_engine_compile(engine);
            if compile_ret != clamav_sys::cl_error_t::CL_SUCCESS {
                let err_msg = format!("Failed to compile ClamAV engine: {}", compile_ret);
                clamav_sys::cl_engine_free(engine);
                return Err(err_msg);
            }

            info!(
                "[clamav] ClamAV engine initialized successfully ({} signatures)",
                signo
            );

            Ok(Self { engine, db_path })
        }
    }

    /// Scan a single file with mutex protection
    fn scan_file(&self, path: &str) -> Result<ScanResult, String> {
        unsafe {
            let mut scanned: u64 = 0;
            let mut virname: *const std::os::raw::c_char = std::ptr::null();
            let mut scan_opts: clamav_sys::cl_scan_options = std::mem::zeroed();
            scan_opts.parse = clamav_sys::CL_SCAN_PARSE_ARCHIVE
                | clamav_sys::CL_SCAN_PARSE_MAIL
                | clamav_sys::CL_SCAN_PARSE_OLE2
                | clamav_sys::CL_SCAN_PARSE_PDF
                | clamav_sys::CL_SCAN_PARSE_HTML
                | clamav_sys::CL_SCAN_PARSE_SWF
                | clamav_sys::CL_SCAN_PARSE_PE
                | clamav_sys::CL_SCAN_PARSE_ELF
                | clamav_sys::CL_SCAN_PARSE_XMLDOCS;

            let path_ptr = path.as_ptr() as *const std::os::raw::c_char;
            let ret = clamav_sys::cl_scanfile(
                path_ptr,
                &mut virname,
                &mut scanned,
                self.engine,
                &mut scan_opts,
            );

            match ret {
                _ if ret == clamav_sys::cl_error_t::CL_VIRUS => {
                    let virus_name = if !virname.is_null() {
                        let len = strlen(virname);
                        let slice = std::slice::from_raw_parts(virname as *const u8, len);
                        String::from_utf8_lossy(slice).to_string()
                    } else {
                        "Unknown virus".to_string()
                    };
                    Ok(ScanResult {
                        path: path.to_string(),
                        is_infected: true,
                        threat_name: Some(virus_name),
                    })
                }
                _ if ret == clamav_sys::cl_error_t::CL_CLEAN => Ok(ScanResult {
                    path: path.to_string(),
                    is_infected: false,
                    threat_name: None,
                }),
                _ => Err(format!("Scan error: {}", ret)),
            }
        }
    }
}

impl Drop for ClamAvContext {
    fn drop(&mut self) {
        unsafe {
            clamav_sys::cl_engine_free(self.engine);
        }
        info!("[clamav] ClamAV engine freed");
    }
}

fn strlen(s: *const std::os::raw::c_char) -> usize {
    if s.is_null() {
        return 0;
    }
    let mut len = 0;
    unsafe {
        while *s.add(len) != 0 {
            len += 1;
        }
    }
    len
}

fn get_clamav_db_path() -> String {
    // User-local database path (cross-platform, highest priority)
    let user_db_path = dirs::home_dir()
        .map(|h| h.join(".isearch").join("clamav_db"))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/tmp/clamav_db".to_string());
    if Path::new(&format!("{}/main.cvd", user_db_path)).exists()
        || Path::new(&format!("{}/main.cld", user_db_path)).exists()
    {
        return user_db_path.to_string();
    }

    #[cfg(target_os = "macos")]
    let system_paths = vec![
        "/opt/homebrew/share/clamav", // macOS ARM (Apple Silicon)
        "/usr/local/share/clamav",     // macOS Intel / Linux
        "/opt/local/share/clamav",     // macOS
    ];

    #[cfg(target_os = "linux")]
    let system_paths = vec![
        "/usr/local/share/clamav",
        "/usr/share/clamav",
        "/var/lib/clamav",
    ];

    #[cfg(target_os = "windows")]
    let system_paths = vec![
        "C:\\Program Files\\ClamAV\\",
        "C:\\ProgramData\\ClamAV\\",
        "C:\\clamav\\",
        "C:\\Program Files (x86)\\ClamAV\\",
    ];

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let system_paths: Vec<&str> = Vec::new();

    for path in &system_paths {
        let db_path = format!("{}/main.cvd", path);
        let db_path_alt = format!("{}/main.cld", path);
        if Path::new(&db_path).exists() || Path::new(&db_path_alt).exists() {
            return path.to_string();
        }
    }

    user_db_path.to_string()
}

/// Download a single ClamAV database file
fn download_single_db(db_path: &str, filename: &str, url: &str) -> Result<(), String> {
    use std::process::Command;

    let dest_path = format!("{}/{}", db_path, filename);

    // Skip if already exists
    if Path::new(&dest_path).exists() {
        info!("[clamav] {} already exists, skipping", filename);
        return Ok(());
    }

    // Try curl first, then wget as fallback
    let downloader = if Command::new("curl").arg("--version").output().is_ok() {
        "curl"
    } else if Command::new("wget").arg("--version").output().is_ok() {
        "wget"
    } else {
        return Err("Neither curl nor wget found. Please install curl or wget.".to_string());
    };

    info!("[clamav] Downloading {} using {}...", filename, downloader);

    match downloader {
        "curl" => {
            let output = Command::new("curl")
                .args(["-fsSL", "--connect-timeout", "30", "--max-time", "600", "-A", "ClamAV/1.0"])
                .arg("-o")
                .arg(&dest_path)
                .arg(url)
                .output()
                .map_err(|e| format!("Failed to run curl: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("curl failed to download {}: {}", filename, stderr));
            }
        },
        "wget" => {
            let output = Command::new("wget")
                .args(["--timeout=30", "--tries=3", "-O", &dest_path, "-U", "ClamAV/1.0"])
                .arg(url)
                .output()
                .map_err(|e| format!("Failed to run wget: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("wget failed to download {}: {}", filename, stderr));
            }
        },
        _ => unreachable!(),
    }

    // Verify file was downloaded
    if !Path::new(&dest_path).exists() {
        return Err(format!("Download completed but {} not found at {}", filename, dest_path));
    }

    info!("[clamav] {} downloaded successfully", filename);
    Ok(())
}

/// Download all ClamAV database files
fn download_all_clamav_dbs(db_path: &str) -> Result<(), String> {
    // Create directory if it doesn't exist
    std::fs::create_dir_all(db_path)
        .map_err(|e| format!("Failed to create db directory: {}", e))?;

    info!("[clamav] Downloading ClamAV databases to {}...", db_path);

    // Download main.cvd (main virus database - changes rarely)
    download_single_db(db_path, "main.cvd", "https://database.clamav.net/main.cvd")?;

    // Download daily.cvd (daily updates - should update daily)
    download_single_db(db_path, "daily.cvd", "https://database.clamav.net/daily.cvd")?;

    // Download bytecode.cvd (bytecode rules - enhances detection)
    download_single_db(db_path, "bytecode.cvd", "https://database.clamav.net/bytecode.cvd")?;

    info!("[clamav] All databases downloaded successfully");
    Ok(())
}

/// Get the last daily update check date from metadata file
fn get_last_daily_update(db_path: &str) -> Option<String> {
    let meta_path = format!("{}/.last_daily_update", db_path);
    std::fs::read_to_string(&meta_path).ok()
}

/// Update the last daily update check date
fn set_last_daily_update(db_path: &str) -> Result<(), String> {
    let meta_path = format!("{}/.last_daily_update", db_path);
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    std::fs::write(&meta_path, today).map_err(|e| format!("Failed to write metadata: {}", e))?;
    Ok(())
}

/// Check if daily update is needed (first use today)
fn needs_daily_update(db_path: &str) -> bool {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    if let Some(last_update) = get_last_daily_update(db_path) {
        last_update != today
    } else {
        true // Never updated
    }
}

/// Check if ClamAV main database exists
fn clamav_db_exists(db_path: &str) -> bool {
    Path::new(&format!("{}/main.cvd", db_path)).exists()
        || Path::new(&format!("{}/main.cld", db_path)).exists()
}

/// Check if all ClamAV databases are complete
fn is_clamav_db_complete(db_path: &str) -> bool {
    Path::new(&format!("{}/main.cvd", db_path)).exists()
        || Path::new(&format!("{}/main.cld", db_path)).exists()
}

pub fn init_clamav() -> Result<(), String> {
    let mut guard = CLAMAV_ENGINE.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        info!("[clamav] Already initialized");
        return Ok(());
    }

    let db_path = get_clamav_db_path();

    // If database doesn't exist, download all databases
    if !clamav_db_exists(&db_path) {
        info!("[clamav] ClamAV database not found at {}, attempting to download...", db_path);
        download_all_clamav_dbs(&db_path)?;
        set_last_daily_update(&db_path)?;
    } else if needs_daily_update(&db_path) {
        // Check for daily updates on first use each day
        info!("[clamav] Checking for daily database updates...");
        download_single_db(&db_path, "daily.cvd", "https://database.clamav.net/daily.cvd")?;
        set_last_daily_update(&db_path)?;
    }

    match ClamAvContext::new() {
        Ok(ctx) => {
            *guard = Some(ctx);
            Ok(())
        }
        Err(e) => {
            warn!("[clamav] Failed to initialize: {}", e);
            Err(e)
        }
    }
}

pub fn is_clamav_ready() -> bool {
    if let Ok(guard) = CLAMAV_ENGINE.lock() {
        guard.is_some()
    } else {
        false
    }
}

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub path: String,
    pub is_infected: bool,
    pub threat_name: Option<String>,
}

pub fn scan_file(path: &str) -> Result<ScanResult, String> {
    let guard = CLAMAV_ENGINE.lock().map_err(|e| e.to_string())?;
    match &*guard {
        Some(ctx) => ctx.scan_file(path),
        None => Err("ClamAV not initialized. Call init_clamav() first.".to_string()),
    }
}

/// Scan files sequentially with mutex protection
/// This is slower but memory-efficient (only one engine instance)
pub fn scan_files(paths: &[String]) -> Vec<ScanResult> {
    if paths.is_empty() {
        return Vec::new();
    }

    let guard = match CLAMAV_ENGINE.lock() {
        Ok(guard) => guard,
        Err(e) => {
            warn!("[clamav] Failed to lock engine: {}", e);
            return Vec::new();
        }
    };

    let ctx = match guard.as_ref() {
        Some(ctx) => ctx,
        None => {
            warn!("[clamav] ClamAV not initialized");
            return Vec::new();
        }
    };

    let engine = ctx.engine;
    let mut all_threats = Vec::new();

    for path in paths {
        unsafe {
            let mut scanned: u64 = 0;
            let mut virname: *const std::os::raw::c_char = std::ptr::null();
            let mut scan_opts: clamav_sys::cl_scan_options = std::mem::zeroed();
            scan_opts.parse = clamav_sys::CL_SCAN_PARSE_ARCHIVE
                | clamav_sys::CL_SCAN_PARSE_MAIL
                | clamav_sys::CL_SCAN_PARSE_OLE2
                | clamav_sys::CL_SCAN_PARSE_PDF
                | clamav_sys::CL_SCAN_PARSE_HTML
                | clamav_sys::CL_SCAN_PARSE_SWF
                | clamav_sys::CL_SCAN_PARSE_PE
                | clamav_sys::CL_SCAN_PARSE_ELF
                | clamav_sys::CL_SCAN_PARSE_XMLDOCS;

            let path_ptr = path.as_ptr() as *const std::os::raw::c_char;
            let ret = clamav_sys::cl_scanfile(
                path_ptr,
                &mut virname,
                &mut scanned,
                engine,
                &mut scan_opts,
            );

            if ret == clamav_sys::cl_error_t::CL_VIRUS {
                let virus_name = if !virname.is_null() {
                    let len = strlen(virname);
                    let slice = std::slice::from_raw_parts(virname as *const u8, len);
                    String::from_utf8_lossy(slice).to_string()
                } else {
                    "Unknown virus".to_string()
                };
                all_threats.push(ScanResult {
                    path: path.clone(),
                    is_infected: true,
                    threat_name: Some(virus_name),
                });
            }
        }
    }

    all_threats
}

/// Scan result with severity
#[derive(Debug, Clone, serde::Serialize)]
pub struct ThreatInfo {
    pub path: String,
    pub threat: String,
    pub severity: String,
}

/// Determine severity based on threat name
pub fn get_threat_severity(threat_name: &str) -> &str {
    let threat_lower = threat_name.to_lowercase();
    if threat_lower.contains("trojan")
        || threat_lower.contains("ransomware")
        || threat_lower.contains("backdoor")
        || threat_lower.contains("rootkit")
    {
        "high"
    } else if threat_lower.contains("worm")
        || threat_lower.contains("dropper")
        || threat_lower.contains("loader")
    {
        "medium"
    } else {
        "low"
    }
}

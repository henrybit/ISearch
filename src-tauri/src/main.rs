// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use log::{info, warn, error};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::Emitter;

mod core;
mod models;

pub static DB_INIT: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
pub static IS_INDEXING: Lazy<Arc<Mutex<bool>>> = Lazy::new(|| Arc::new(Mutex::new(false)));
pub static LAST_INDEXED: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
pub static LAST_INDEX_ERRORS: Lazy<Arc<Mutex<Option<core::indexer::IndexErrors>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

// Configuration: skip hidden directories during indexing (default: true)
pub static SKIP_HIDDEN_DIRS: Lazy<Arc<Mutex<bool>>> =
    Lazy::new(|| Arc::new(Mutex::new(true)));

pub static IS_SCANNING: Lazy<Arc<Mutex<bool>>> = Lazy::new(|| Arc::new(Mutex::new(false)));

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub id: i64,
    pub path: String,
    pub filename: String,
    pub extension: Option<String>,
    pub size: u64,
    pub is_directory: bool,
    pub modified: String,
    pub created: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub files: Vec<FileEntry>,
    pub total: usize,
    pub search_time_ms: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexStatus {
    pub is_indexing: bool,
    pub file_count: usize,
    pub last_indexed: String,
    pub db_path: String,
    pub db_size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexMemoryStatus {
    pub lmdb_file_count: u64,
    pub lmdb_max_entries: u64,
    pub lmdb_max_map_size: usize,
    pub metadata_entry_count: u64,
    pub fst_size: u64,
    pub inverted_index_size: u64,
    pub estimated_memory_usage: usize,
    pub is_near_capacity: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SuggestResult {
    pub suggestions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TopLevelDir {
    pub name: String,
    pub path: String,
    pub is_indexed: bool,
    pub is_indexing: bool,
    pub file_count: usize,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecurityScanResult {
    pub threats: Vec<ThreatInfo>,
    pub scanned_count: usize,
    pub clean_count: usize,
    pub threat_count: usize,
    pub scan_time_ms: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThreatInfo {
    pub path: String,
    pub threat: String,
    pub severity: String,
}

// Initialize database
fn ensure_db_initialized() -> Result<(), String> {
    let mut initialized = DB_INIT.lock().map_err(|e| e.to_string())?;
    if !*initialized {
        core::database::init_database().map_err(|e| e.to_string())?;
        *initialized = true;
    }
    Ok(())
}

#[tauri::command]
async fn search_files(query: String, limit: Option<usize>) -> Result<SearchResult, String> {
    let start = std::time::Instant::now();
    ensure_db_initialized()?;
    info!("[search] query=\"{}\" limit={} - Search initiated", query, limit.unwrap_or(1000));
    let limit = limit.unwrap_or(1000);

    let parsed = core::search::SearchEngine::parse_query(&query);
    info!("[search] parsed query: text=\"{}\" fuzzy={} regex={:?}", parsed.text, parsed.fuzzy, parsed.regex_filter);

    // Apply limit to the parsed query
    let mut query_with_limit = parsed;
    query_with_limit.limit = limit;

    eprintln!("[DEBUG main] About to call search(), query={:?}", query_with_limit);
    info!("[DEBUG main] About to call search()");

    let results = core::search::SearchEngine::new()
        .search(&query_with_limit)
        .map_err(|e| {
            eprintln!("[DEBUG main] search() returned error: {}", e);
            e.to_string()
        })?;

    eprintln!("[DEBUG main] search() completed, results.len()={}", results.len());
    info!("[search] raw results count: {}", results.len());

    let total = results.len();
    let files: Vec<FileEntry> = results
        .into_iter()
        .take(limit)
        .map(|e| {
            let path_buf = e.path.clone();
            FileEntry {
                id: e.id,
                path: path_buf.to_string_lossy().to_string(),
                filename: e.filename.clone(),
                extension: e.extension,
                size: e.size as u64,
                is_directory: e.is_directory,
                modified: e.modified_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                created: e.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            }
        })
        .collect();

    let search_time_ms = start.elapsed().as_secs_f64() * 1000.0;
    info!("[search] query=\"{}\" result_count={} duration_ms={:.2} - Search completed", query, total, search_time_ms);

    Ok(SearchResult {
        files,
        total,
        search_time_ms,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexStartResult {
    pub message: String,
    pub total_permission_denied: usize,
    pub total_other_errors: usize,
}

#[tauri::command]
async fn start_indexing(app: tauri::AppHandle) -> Result<IndexStartResult, String> {
    info!("[start_indexing] - Indexing started manually");
    ensure_db_initialized()?;

    // Set indexing flag to true
    {
        let mut indexing = IS_INDEXING.lock().map_err(|e| e.to_string())?;
        *indexing = true;
    }

    // Run indexing in a separate thread
    let errors_clone = LAST_INDEX_ERRORS.clone();
    let app_handle = app.clone();
    let skip_hidden = *SKIP_HIDDEN_DIRS.lock().unwrap();
    std::thread::spawn(move || {
        let mut last_reported_count = 0;

        let indexer = core::native_indexer::NativeIndexer::new().with_skip_hidden(skip_hidden);
        let mut progress_cb = |count: usize, msg: &str| {
            // Report progress every 500 files
            if count - last_reported_count >= 500 {
                last_reported_count = count;
                let _ = app_handle.emit("index-progress", serde_json::json!({
                    "count": count,
                    "message": msg
                }));

                // Also emit index-status for UI updates
                let _ = app_handle.emit("index-status", serde_json::json!({
                    "file_count": count,
                    "is_indexing": true,
                    "last_indexed": "",
                    "message": format!("索引中... {} 文件", count)
                }));
            }
        };
        // Use home directory for indexing
        let index_path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        match indexer.index(&index_path, Some(&mut progress_cb)) {
            Ok((count, errors)) => {
                // Store errors for later retrieval
                let mut stored_errors = errors_clone.lock().unwrap();
                *stored_errors = Some(errors);

                // Update persistent last indexed time
                let now = chrono::Utc::now().timestamp();
                if let Err(e) = core::database::set_last_indexed_time(now) {
                    warn!("Failed to save last indexed time: {}", e);
                }

                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

                // Emit completion event
                let _ = app_handle.emit("index-complete", serde_json::json!({
                    "count": count,
                    "timestamp": timestamp
                }));

                // Emit final index-status
                let _ = app_handle.emit("index-status", serde_json::json!({
                    "file_count": count,
                    "is_indexing": false,
                    "last_indexed": timestamp,
                    "message": format!("索引完成，共 {} 个文件", count)
                }));

                // Set indexing flag to false and update in-memory last_indexed
                let mut indexing = IS_INDEXING.lock().unwrap();
                *indexing = false;
                let mut last_indexed = LAST_INDEXED.lock().unwrap();
                *last_indexed = timestamp;
            }
            Err(e) => {
                error!("Indexing failed: {}", e);

                // Emit error index-status
                let _ = app_handle.emit("index-status", serde_json::json!({
                    "file_count": 0,
                    "is_indexing": false,
                    "last_indexed": "",
                    "message": format!("索引失败: {}", e)
                }));

                let mut indexing = IS_INDEXING.lock().unwrap();
                *indexing = false;
            }
        }
    });

    Ok(IndexStartResult {
        message: "Indexing started".to_string(),
        total_permission_denied: 0,
        total_other_errors: 0,
    })
}

#[tauri::command]
async fn check_needs_reindex() -> Result<bool, String> {
    ensure_db_initialized()?;
    core::database::needs_reindex().map_err(|e| e.to_string())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexErrorsResult {
    pub permission_denied: Vec<String>,
    pub other_errors: Vec<core::indexer::IndexError>,
    pub total_errors: usize,
}

#[tauri::command]
async fn get_index_errors() -> Result<IndexErrorsResult, String> {
    let errors = LAST_INDEX_ERRORS.lock().map_err(|e| e.to_string())?;

    match &*errors {
        Some(e) => Ok(IndexErrorsResult {
            permission_denied: e.permission_denied.clone(),
            other_errors: e.other_errors.clone(),
            total_errors: e.total_errors,
        }),
        None => Ok(IndexErrorsResult {
            permission_denied: vec![],
            other_errors: vec![],
            total_errors: 0,
        }),
    }
}

#[tauri::command]
async fn get_index_status() -> Result<IndexStatus, String> {
    ensure_db_initialized()?;

    let file_count = core::database::get_file_count().map_err(|e| e.to_string())? as usize;
    let db_path = core::database::get_db_path();
    let db_size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    let is_indexing = {
        let indexing = IS_INDEXING.lock().map_err(|e| e.to_string())?;
        *indexing
    };

    let last_indexed = {
        let last = LAST_INDEXED.lock().map_err(|e| e.to_string())?;
        last.clone()
    };

    Ok(IndexStatus {
        is_indexing,
        file_count,
        last_indexed,
        db_path: db_path.to_string_lossy().to_string(),
        db_size,
    })
}

#[tauri::command]
async fn get_index_memory_status() -> Result<IndexMemoryStatus, String> {
    ensure_db_initialized()?;

    let stats = core::database::get_index_stats().map_err(|e| e.to_string())?;

    Ok(IndexMemoryStatus {
        lmdb_file_count: stats.lmdb_file_count,
        lmdb_max_entries: stats.lmdb_max_entries,
        lmdb_max_map_size: stats.lmdb_max_map_size,
        metadata_entry_count: stats.metadata_entry_count,
        fst_size: stats.fst_size,
        inverted_index_size: stats.inverted_index_size,
        estimated_memory_usage: stats.estimated_memory_usage,
        is_near_capacity: stats.is_near_capacity,
    })
}

#[tauri::command]
async fn evict_old_entries(_count: Option<usize>) -> Result<usize, String> {
    // With the ultra-simplified memory design, eviction is not needed.
    // LMDB has a fixed 10MB limit and will stop accepting new entries when near capacity.
    Ok(0)
}

#[tauri::command]
async fn get_indexed_dirs() -> Result<Vec<TopLevelDir>, String> {
    ensure_db_initialized()?;
    let dirs = core::database::get_indexed_dirs_info();
    Ok(dirs.into_iter().map(|d| TopLevelDir {
        name: d.name,
        path: d.path,
        is_indexed: d.is_indexed,
        is_indexing: d.is_indexing,
        file_count: d.file_count,
        size_bytes: d.size_bytes,
    }).collect())
}

#[tauri::command]
async fn get_unindexed_dirs() -> Result<Vec<String>, String> {
    // Return common system paths that typically require special permissions on macOS
    let system_paths = vec![
        "/System".to_string(),
        "/Library".to_string(),
        "/Applications".to_string(),
        "/Users/Shared".to_string(),
    ];

    // Check which paths exist but may have limited files
    let mut unindexable = Vec::new();
    for path in &system_paths {
        if std::path::Path::new(path).exists() {
            unindexable.push(path.clone());
        }
    }

    Ok(unindexable)
}

#[tauri::command]
async fn rebuild_indexed_dirs() -> Result<usize, String> {
    info!("[rebuild_indexed_dirs] Manual rebuild triggered");
    core::database::rebuild_indexed_dirs_from_db().map_err(|e| e.to_string())
}

#[tauri::command]
async fn rebuild_index(app: tauri::AppHandle) -> Result<IndexStartResult, String> {
    info!("[rebuild_index] - Full rebuild started");
    ensure_db_initialized()?;

    // Clear existing database first
    core::database::clear_all_files().map_err(|e| e.to_string())?;
    info!("[rebuild_index] - Database cleared");

    // Delete indexed_dirs.json to force rebuild
    let indexed_dirs_file = core::database::get_indexed_dirs_file();
    if indexed_dirs_file.exists() {
        std::fs::remove_file(&indexed_dirs_file).ok();
    }

    // Set indexing flag to true
    {
        let mut indexing = IS_INDEXING.lock().map_err(|e| e.to_string())?;
        *indexing = true;
    }

    // Emit initial index-status to signal indexing started
    let _ = app.emit("index-status", serde_json::json!({
        "file_count": 0,
        "is_indexing": true,
        "last_indexed": "",
        "message": "开始重建索引...".to_string()
    }));

    // Run indexing in a separate thread
    let errors_clone = LAST_INDEX_ERRORS.clone();
    let app_handle = app.clone();
    let skip_hidden = *SKIP_HIDDEN_DIRS.lock().unwrap();
    std::thread::spawn(move || {
        let mut last_reported_count = 0;

        info!("[rebuild_index] Starting indexing thread");

        let indexer = core::native_indexer::NativeIndexer::new().with_skip_hidden(skip_hidden);
        let mut progress_cb = |count: usize, msg: &str| {
            if count - last_reported_count >= 500 {
                last_reported_count = count;
                let _ = app_handle.emit("index-progress", serde_json::json!({
                    "count": count,
                    "message": msg
                }));

                let _ = app_handle.emit("index-status", serde_json::json!({
                    "file_count": count,
                    "is_indexing": true,
                    "last_indexed": "",
                    "message": format!("索引中... {} 文件", count)
                }));
            }
        };

        let index_path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        info!("[rebuild_index] Index path: {:?}", index_path);

        match indexer.index(&index_path, Some(&mut progress_cb)) {
            Ok((count, errors)) => {
                let mut stored_errors = errors_clone.lock().unwrap();
                *stored_errors = Some(errors);

                let now = chrono::Utc::now().timestamp();
                if let Err(e) = core::database::set_last_indexed_time(now) {
                    warn!("Failed to save last indexed time: {}", e);
                }

                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

                let _ = app_handle.emit("index-complete", serde_json::json!({
                    "count": count,
                    "timestamp": timestamp
                }));

                let _ = app_handle.emit("index-status", serde_json::json!({
                    "file_count": count,
                    "is_indexing": false,
                    "last_indexed": timestamp,
                    "message": format!("索引完成，共 {} 个文件", count)
                }));

                let mut indexing = IS_INDEXING.lock().unwrap();
                *indexing = false;
                let mut last_indexed = LAST_INDEXED.lock().unwrap();
                *last_indexed = timestamp;

                // Rebuild indexed dirs after full reindex
                info!("[rebuild_index] Rebuilding indexed dirs after full reindex...");
                if let Err(e) = core::database::rebuild_indexed_dirs_from_db() {
                    warn!("[rebuild_index] Failed to rebuild indexed dirs: {}", e);
                } else {
                    info!("[rebuild_index] Indexed dirs rebuilt successfully");
                }
            }
            Err(e) => {
                error!("[rebuild_index] Indexing failed: {}", e);

                let _ = app_handle.emit("index-status", serde_json::json!({
                    "file_count": 0,
                    "is_indexing": false,
                    "last_indexed": "",
                    "message": format!("索引失败: {}", e)
                }));

                let mut indexing = IS_INDEXING.lock().unwrap();
                *indexing = false;
            }
        }
    });

    Ok(IndexStartResult {
        message: "Rebuild started".to_string(),
        total_permission_denied: 0,
        total_other_errors: 0,
    })
}

fn get_ignored_dirs_file() -> std::path::PathBuf {
    core::database::get_data_dir().join("ignored_dirs.json")
}

fn load_ignored_dirs() -> Vec<String> {
    let path = get_ignored_dirs_file();
    if !path.exists() {
        return Vec::new();
    }
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(dirs) = serde_json::from_str(&content) {
            return dirs;
        }
    }
    Vec::new()
}

fn save_ignored_dirs(dirs: &[String]) -> Result<(), String> {
    let path = get_ignored_dirs_file();
    let content = serde_json::to_string_pretty(dirs).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn get_ignored_dirs() -> Result<Vec<String>, String> {
    info!("[get_ignored_dirs]");
    Ok(load_ignored_dirs())
}

#[tauri::command]
async fn add_ignored_dir(path: String) -> Result<(), String> {
    info!("[add_ignored_dir] path=\"{}\"", path);
    let mut dirs = load_ignored_dirs();
    if !dirs.contains(&path) {
        dirs.push(path);
        save_ignored_dirs(&dirs)?;
    }
    Ok(())
}

#[tauri::command]
async fn remove_ignored_dir(path: String) -> Result<(), String> {
    info!("[remove_ignored_dir] path=\"{}\"", path);
    let mut dirs = load_ignored_dirs();
    dirs.retain(|d| d != &path);
    save_ignored_dirs(&dirs)?;
    Ok(())
}

#[tauri::command]
async fn init_clamav() -> Result<(), String> {
    info!("[init_clamav] Initializing ClamAV scanner");
    core::clamav_scanner::init_clamav()
}

#[tauri::command]
async fn is_clamav_ready() -> Result<bool, String> {
    Ok(core::clamav_scanner::is_clamav_ready())
}

#[tauri::command]
async fn start_security_scan(app: tauri::AppHandle, scan_dir: Option<String>) -> Result<SecurityScanResult, String> {
    info!("[start_security_scan] Security scan started, dir={:?}", scan_dir);

    // Check if already scanning
    {
        let is_scanning = IS_SCANNING.lock().map_err(|e| e.to_string())?;
        if *is_scanning {
            return Err("Security scan already in progress".to_string());
        }
    }

    // Initialize ClamAV if not already
    if !core::clamav_scanner::is_clamav_ready() {
        core::clamav_scanner::init_clamav().map_err(|e| e.to_string())?;
    }

    // Set scanning flag
    {
        let mut is_scanning = IS_SCANNING.lock().map_err(|e| e.to_string())?;
        *is_scanning = true;
    }

    // Emit scan started event
    let _ = app.emit("security-scan-status", serde_json::json!({
        "is_scanning": true,
        "scanned": 0,
        "threats": 0,
        "message": "正在初始化病毒扫描..."
    }));

    let start = std::time::Instant::now();
    let mut threats = Vec::new();
    let mut scanned_count = 0;
    let mut last_reported_count = 0;

    // Collect files to scan based on whether a directory was specified
    let all_files: Vec<String> = if let Some(dir) = &scan_dir {
        // Collect files from the specified directory
        info!("[start_security_scan] Scanning directory: {}", dir);
        collect_files_from_dir(dir).unwrap_or_default()
    } else {
        // Get all files from database
        info!("[start_security_scan] Scanning all indexed files");
        core::database::get_all_file_paths().unwrap_or_default()
    };

    let total_files = all_files.len();
    info!("[start_security_scan] Total files to scan: {}", total_files);

    // Scan files in batches to emit progress
    let batch_size = 100;
    for chunk in all_files.chunks(batch_size) {
        // Check if scan was cancelled
        let is_scanning = IS_SCANNING.lock().map_err(|e| e.to_string())?;
        if !*is_scanning {
            break;
        }
        drop(is_scanning);

        let paths: Vec<String> = chunk.to_vec();
        let results = core::clamav_scanner::scan_files(&paths);

        for result in results {
            if result.is_infected {
                // Extract all values first to avoid borrow issues
                let path = result.path;
                let threat_name = result.threat_name.unwrap_or_else(|| "Unknown".to_string());
                let severity_str = core::clamav_scanner::get_threat_severity(&threat_name);
                let severity = severity_str.to_string();
                threats.push(ThreatInfo {
                    path,
                    threat: threat_name,
                    severity,
                });
            }
        }

        scanned_count += chunk.len();

        // Emit progress every batch
        if scanned_count - last_reported_count >= batch_size {
            last_reported_count = scanned_count;
            let _ = app.emit("security-scan-status", serde_json::json!({
                "is_scanning": true,
                "scanned": scanned_count,
                "total": total_files,
                "threats": threats.len(),
                "message": format!("正在扫描... {}/{}", scanned_count, total_files)
            }));
        }
    }

    let scan_time_ms = start.elapsed().as_secs_f64() * 1000.0;
    let threat_count = threats.len();
    let clean_count = scanned_count - threat_count;

    // Reset scanning flag
    {
        let mut is_scanning = IS_SCANNING.lock().map_err(|e| e.to_string())?;
        *is_scanning = false;
    }

    // Emit completion event
    let _ = app.emit("security-scan-status", serde_json::json!({
        "is_scanning": false,
        "scanned": scanned_count,
        "total": total_files,
        "threats": threat_count,
        "message": format!("扫描完成！发现 {} 个威胁", threat_count)
    }));

    info!("[start_security_scan] Scan completed: scanned={} threats={} time_ms={:.2}",
        scanned_count, threat_count, scan_time_ms);

    Ok(SecurityScanResult {
        threats,
        scanned_count,
        clean_count,
        threat_count,
        scan_time_ms,
    })
}

/// Collect file paths from a directory recursively
fn collect_files_from_dir(dir: &str) -> Option<Vec<String>> {
    use walkdir::WalkDir;

    let mut files = Vec::new();
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Some(path) = entry.path().to_str() {
                files.push(path.to_string());
            }
        }
    }
    Some(files)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileAnalysisResult {
    pub level: String,  // "low", "medium", "high"
    pub conclusion: String,
    pub has_trojan_risk: bool,
    pub reasons: Vec<String>,
    pub is_infected: bool,
    pub threat_name: Option<String>,
}

#[tauri::command]
async fn analyze_file(path: String) -> Result<FileAnalysisResult, String> {
    info!("[analyze_file] Analyzing file: {}", path);

    use std::path::Path;

    let path_obj = Path::new(&path);
    let filename = path_obj.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let ext = path_obj.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Static analysis
    let suspicious_ext = ["exe", "bat", "cmd", "scr", "vbs", "apk", "dmg", "pkg", "jar", "ps1"];
    let script_ext = ["js", "ts", "sh", "py", "rb", "php", "pl"];
    let malware_keywords = ["trojan", "virus", "keygen", "crack", "backdoor", "ransom", "payload"];

    let mut score = 0;
    let mut reasons: Vec<String> = Vec::new();

    if suspicious_ext.contains(&ext.as_str()) {
        score += 2;
        reasons.push(format!("扩展名 .{} 属于高敏感可执行类型", ext));
    } else if script_ext.contains(&ext.as_str()) {
        score += 1;
        reasons.push(format!("扩展名 .{} 为脚本文件，建议确认来源", ext));
    }

    let lowercase_name = filename.to_lowercase();
    if malware_keywords.iter().any(|k| lowercase_name.contains(k)) {
        score += 2;
        reasons.push("文件名包含常见恶意软件关键词".to_string());
    }

    // Get file size for analysis
    let is_directory = path_obj.is_dir();
    if !is_directory {
        if let Ok(metadata) = std::fs::metadata(&path) {
            if metadata.len() > 500 * 1024 * 1024 {
                score += 1;
                reasons.push("文件体积较大，建议确认用途与来源".to_string());
            }
        }
    }

    let (mut level, mut conclusion, mut has_trojan_risk) = if is_directory {
        ("low".to_string(),
         "目录本身无直接执行风险，可继续检查目录内容。".to_string(),
         false)
    } else if score >= 4 {
        ("high".to_string(),
         "存在明显风险特征，疑似木马或潜在恶意文件，建议隔离后再处理。".to_string(),
         true)
    } else if score >= 2 {
        ("medium".to_string(),
         "存在可疑特征，建议进行杀毒扫描与来源校验。".to_string(),
         false)
    } else {
        ("low".to_string(),
         "未发现明显风险特征，整体风险较低。".to_string(),
         false)
    };

    // ClamAV scan if available and file is not a directory
    let mut is_infected = false;
    let mut threat_name: Option<String> = None;

    if !is_directory {
        // Initialize ClamAV if not already initialized
        if !core::clamav_scanner::is_clamav_ready() {
            if let Err(e) = core::clamav_scanner::init_clamav() {
                info!("[analyze_file] ClamAV initialization failed: {}", e);
                // Continue with static analysis only
            }
        }

        if let Ok(scan_result) = core::clamav_scanner::scan_file(&path) {
            is_infected = scan_result.is_infected;
            if is_infected {
                threat_name = scan_result.threat_name;
                has_trojan_risk = true;
                level = "high".to_string();
                conclusion = "文件被检测为恶意文件，建议立即隔离处理。".to_string();
                reasons.push(format!("ClamAV检测到威胁: {}", threat_name.clone().unwrap_or_default()));
            }
        }
    }

    info!("[analyze_file] Result: level={} infected={}", level, is_infected);

    Ok(FileAnalysisResult {
        level,
        conclusion,
        has_trojan_risk,
        reasons,
        is_infected,
        threat_name,
    })
}

#[tauri::command]
async fn open_file(path: String) -> Result<(), String> {
    info!("[open_file] path=\"{}\" - Opening file", path);
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .output()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .output()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn open_folder(path: String) -> Result<(), String> {
    info!("[open_folder] path=\"{}\" - Opening folder", path);
    let path_buf = PathBuf::from(&path);
    if let Some(parent) = path_buf.parent() {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(parent)
                .output()
                .map_err(|e| e.to_string())?;
        }
        #[cfg(not(target_os = "macos"))]
        {
            std::process::Command::new("xdg-open")
                .arg(parent)
                .output()
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
async fn delete_file(path: String) -> Result<(), String> {
    info!("[delete_file] path=\"{}\" - Deleting file", path);
    let target = PathBuf::from(&path);

    if !target.exists() {
        return Err("File or directory does not exist".to_string());
    }

    if target.is_dir() {
        std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
    } else {
        std::fs::remove_file(&target).map_err(|e| e.to_string())?;
    }

    if let Err(e) = core::database::delete_file_by_path(&path) {
        warn!("Failed to remove deleted entry from index: {}", e);
    }

    info!("[delete_file] path=\"{}\" - File deleted successfully", path);
    Ok(())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ScanReport {
    #[serde(rename = "id")]
    id: String,
    #[serde(rename = "timestamp")]
    timestamp: String,
    #[serde(rename = "scannedDir")]
    scanned_dir: String,
    #[serde(rename = "totalFiles")]
    total_files: usize,
    #[serde(rename = "cleanCount")]
    clean_count: usize,
    #[serde(rename = "threatCount")]
    threat_count: usize,
    duration: f64,
    threats: Vec<ThreatInfo>,
}

#[tauri::command]
async fn save_scan_report(report: ScanReport) -> Result<(), String> {
    info!("[save_scan_report] Saving scan report: {}", report.timestamp);

    // Create .isearch/security directory if it doesn't exist
    let base_dir = dirs::home_dir()
        .ok_or_else(|| "Cannot find home directory".to_string())?
        .join(".isearch")
        .join("security");

    std::fs::create_dir_all(&base_dir).map_err(|e| e.to_string())?;

    // Generate filename from timestamp
    let safe_timestamp = report.timestamp
        .replace(" ", "_")
        .replace(":", "_")
        .replace("-", "_")
        .replace("年", "")
        .replace("月", "")
        .replace("日", "");
    let filename = format!("scan_{}.md", safe_timestamp);
    let filepath = base_dir.join(&filename);

    // Generate markdown content
    let mut content = format!(r#"# 安全扫描报告

**扫描时间:** {timestamp}
**扫描目录:** {scanned_dir}

## 扫描统计

| 指标 | 数值 |
|------|------|
| 扫描文件 | {total_files} |
| 安全文件 | {clean_count} |
| 可疑威胁 | {threat_count} |
| 扫描耗时 | {duration:.2}秒 |

"#, timestamp=report.timestamp, scanned_dir=report.scanned_dir, total_files=report.total_files, clean_count=report.clean_count, threat_count=report.threat_count, duration=report.duration);

    if !report.threats.is_empty() {
        content.push_str("## 威胁详情\n\n");
        content.push_str("| 文件路径 | 威胁名称 | 严重程度 |\n");
        content.push_str("|----------|----------|----------|\n");
        for threat in &report.threats {
            let severity_cn = match threat.severity.as_str() {
                "high" => "高危",
                "medium" => "中危",
                _ => "低危",
            };
            content.push_str(&format!("| {} | {} | {} |\n", threat.path, threat.threat, severity_cn));
        }
    } else {
        content.push_str("## 威胁详情\n\n未发现任何威胁文件。\n");
    }

    // Write to file
    std::fs::write(&filepath, content).map_err(|e| e.to_string())?;

    info!("[save_scan_report] Report saved to: {:?}", filepath);
    Ok(())
}

#[tauri::command]
async fn save_search_results(query: String, results: Vec<FileEntry>) -> Result<(), String> {
    info!("[save_search_results] Saving search results for: {}", query);

    // Create .isearch/search directory if it doesn't exist
    let base_dir = dirs::home_dir()
        .ok_or_else(|| "Cannot find home directory".to_string())?
        .join(".isearch")
        .join("search");

    std::fs::create_dir_all(&base_dir).map_err(|e| e.to_string())?;

    // Generate filename
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let safe_query = query
        .replace("/", "_")
        .replace("\\", "_")
        .replace(":", "_")
        .replace("*", "_")
        .replace("?", "_")
        .replace("\"", "_")
        .replace("<", "_")
        .replace(">", "_")
        .replace("|", "_");
    let filename = format!("search_{}_{}.md", safe_query, timestamp);
    let filepath = base_dir.join(&filename);

    // Generate markdown content
    let mut content = format!(r#"# 搜索结果

**搜索关键词:** {query}
**搜索时间:** {timestamp}
**结果数量:** {count} 个文件

## 文件列表

| 文件名 | 路径 | 大小 |
|--------|------|------|
"#, query=query, timestamp=timestamp, count=results.len());

    for entry in &results {
        let size = format_size(entry.size);
        content.push_str(&format!("| {} | {} | {} |\n", entry.filename, entry.path, size));
    }

    // Write to file
    std::fs::write(&filepath, content).map_err(|e| e.to_string())?;

    info!("[save_search_results] Results saved to: {:?}", filepath);
    Ok(())
}

#[tauri::command]
async fn get_log_files() -> Result<Vec<String>, String> {
    let log_dir = dirs::home_dir()
        .ok_or_else(|| "Cannot find home directory".to_string())?
        .join(".isearch")
        .join("logs");

    if !log_dir.exists() {
        return Ok(vec![]);
    }

    let mut files: Vec<String> = std::fs::read_dir(&log_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| {
            entry.ok().and_then(|e| {
                let path = e.path();
                if path.is_file() {
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
        .collect();

    files.sort_by(|a, b| b.cmp(a)); // Sort descending (newest first)
    Ok(files)
}

#[tauri::command]
async fn read_log_file(filename: String) -> Result<Vec<String>, String> {
    let log_path = dirs::home_dir()
        .ok_or_else(|| "Cannot find home directory".to_string())?
        .join(".isearch")
        .join("logs")
        .join(&filename);

    if !log_path.exists() {
        return Err("Log file not found".to_string());
    }

    let content = std::fs::read_to_string(&log_path)
        .map_err(|e| e.to_string())?;

    // Split into lines and return last 500 lines
    let lines: Vec<String> = content.lines().rev().take(500).map(|s| s.to_string()).collect();
    Ok(lines.into_iter().rev().collect())
}

fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiskStats {
    pub total: u64,
    pub used: u64,
    pub free: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtStats {
    pub extension: String,
    pub count: u64,
    pub description: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewData {
    pub disk_stats: Option<DiskStats>,
    pub total_files: u64,
    pub ext_stats: Vec<ExtStats>,
    pub last_updated: String,
}

fn get_ext_description(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "pdf" => "PDF文档",
        "doc" | "docx" => "Word文档",
        "xls" | "xlsx" => "Excel表格",
        "ppt" | "pptx" => "PowerPoint演示文稿",
        "txt" => "文本文档",
        "jpg" | "jpeg" => "JPEG图片",
        "png" => "PNG图片",
        "gif" => "GIF图片",
        "svg" => "SVG矢量图",
        "mp3" => "MP3音频",
        "mp4" => "MP4视频",
        "avi" => "AVI视频",
        "zip" => "ZIP压缩包",
        "rar" => "RAR压缩包",
        "7z" => "7-Zip压缩包",
        "tar" => "TAR归档",
        "gz" => "Gzip压缩",
        "html" | "htm" => "HTML网页",
        "css" => "CSS样式表",
        "js" => "JavaScript代码",
        "ts" => "TypeScript代码",
        "py" => "Python代码",
        "rs" => "Rust代码",
        "go" => "Go代码",
        "java" => "Java代码",
        "c" => "C代码",
        "cpp" | "cc" | "cxx" => "C++代码",
        "h" | "hpp" => "头文件",
        "json" => "JSON数据",
        "xml" => "XML数据",
        "yaml" | "yml" => "YAML配置",
        "toml" => "TOML配置",
        "md" => "Markdown文档",
        "db" | "sqlite" | "sqlite3" => "数据库文件",
        "log" => "日志文件",
        "env" => "环境变量文件",
        "gitignore" | "gitattributes" => "Git配置",
        "exe" => "Windows可执行文件",
        "dmg" => "macOS磁盘镜像",
        "app" => "macOS应用",
        "pkg" => "安装包",
        "deb" => "Debian包",
        "rpm" => "RPM包",
        "iso" => "ISO镜像",
        "torrent" => "种子文件",
        "sig" | "asc" => "数字签名",
        "pem" | "crt" | "cer" => "证书文件",
        "key" => "密钥文件",
        "csv" => "CSV数据",
        "psd" => "Photoshop设计稿",
        "ai" => "Illustrator矢量图",
        "sketch" => "Sketch设计稿",
        "fig" => "Figma设计稿",
        "xd" => "Adobe XD设计稿",
        "epub" => "EPUB电子书",
        "mobi" => "Mobi电子书",
        "azw" | "azw3" => "Kindle电子书",
        _ => "其他文件",
    }
}

#[tauri::command]
async fn get_overview_data() -> Result<OverviewData, String> {
    let overview_path = dirs::home_dir()
        .ok_or_else(|| "Cannot find home directory".to_string())?
        .join(".isearch")
        .join("overview.json");

    if !overview_path.exists() {
        return Ok(OverviewData {
            disk_stats: None,
            total_files: 0,
            ext_stats: vec![],
            last_updated: String::new(),
        });
    }

    let content = std::fs::read_to_string(&overview_path)
        .map_err(|e| e.to_string())?;

    let data: OverviewData = serde_json::from_str(&content)
        .map_err(|e| e.to_string())?;

    Ok(data)
}

#[tauri::command]
async fn refresh_overview_data() -> Result<(), String> {
    info!("[overview] Refreshing overview data");

    // Ensure database is initialized
    ensure_db_initialized()?;

    // Check if FST needs rebuild (empty but files exist in LMDB)
    if core::database::needs_fst_rebuild() {
        info!("[overview] FST needs rebuild, triggering rebuild_all_indexes...");
        core::database::rebuild_all_indexes().map_err(|e| e.to_string())?;
    }

    // Get disk stats
    let disk_stats = get_disk_stats();

    // Get total files and extension stats from database
    let (total_files, ext_stats) = get_file_stats();

    info!("[overview] File stats: total_files={}, ext_count={}", total_files, ext_stats.len());

    let data = OverviewData {
        disk_stats: Some(disk_stats),
        total_files,
        ext_stats,
        last_updated: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };

    // Save to file
    let overview_path = dirs::home_dir()
        .ok_or_else(|| "Cannot find home directory".to_string())?
        .join(".isearch")
        .join("overview.json");

    std::fs::create_dir_all(overview_path.parent().unwrap())
        .map_err(|e| e.to_string())?;

    let content = serde_json::to_string_pretty(&data)
        .map_err(|e| e.to_string())?;

    std::fs::write(&overview_path, content)
        .map_err(|e| e.to_string())?;

    info!("[overview] Overview data saved to {:?}", overview_path);
    Ok(())
}

fn get_disk_stats() -> DiskStats {
    // Query disk stats for the data directory volume, not root
    let data_dir = core::database::get_data_dir();

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let output = Command::new("df")
            .args(["-k", "-l", data_dir.to_str().unwrap_or("/")])
            .output();

        if let Ok(output) = output {
            let text = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = text.lines().collect();
            if lines.len() >= 2 {
                let parts: Vec<&str> = lines[1].split_whitespace().collect();
                // df -k on macOS returns 1024-byte (1KB) blocks
                if parts.len() >= 4 {
                    let blocks: u64 = parts[1].parse().unwrap_or(0);
                    let used_blocks: u64 = parts[2].parse().unwrap_or(0);
                    let free_blocks: u64 = parts[3].parse().unwrap_or(0);
                    return DiskStats {
                        total: blocks * 1024,
                        used: used_blocks * 1024,
                        free: free_blocks * 1024,
                    };
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let output = Command::new("df")
            .args(["-k", data_dir.to_str().unwrap_or("/")])
            .output();

        if let Ok(output) = output {
            let text = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = text.lines().collect();
            if lines.len() >= 2 {
                let parts: Vec<&str> = lines[1].split_whitespace().collect();
                if parts.len() >= 4 {
                    let total_kb: u64 = parts[1].parse().unwrap_or(0);
                    let used_kb: u64 = parts[2].parse().unwrap_or(0);
                    let free_kb: u64 = parts[3].parse().unwrap_or(0);
                    return DiskStats {
                        total: total_kb * 1024,
                        used: used_kb * 1024,
                        free: free_kb * 1024,
                    };
                }
            }
        }
    }

    DiskStats {
        total: 0,
        used: 0,
        free: 0,
    }
}

fn get_file_stats() -> (u64, Vec<ExtStats>) {
    use crate::core::database::LMDB_STORE;

    let total_files = LMDB_STORE.len().unwrap_or(0) as u64;

    // Get all paths and calculate extension stats
    let paths = LMDB_STORE.get_all_paths().unwrap_or_default();

    let mut ext_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for path in &paths {
        if let Some(ext) = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
        {
            *ext_counts.entry(ext.to_lowercase()).or_insert(0) += 1;
        }
    }

    let mut ext_stats: Vec<ExtStats> = ext_counts
        .into_iter()
        .map(|(ext, count)| {
            let desc = get_ext_description(&ext).to_string();
            ExtStats {
                extension: ext,
                count,
                description: desc,
            }
        })
        .collect();

    // Sort by count descending
    ext_stats.sort_by(|a, b| b.count.cmp(&a.count));

    // Keep top 30
    ext_stats.truncate(30);

    (total_files, ext_stats)
}

#[tauri::command]
async fn copy_to_clipboard(text: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        let mut child = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        child.wait().map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .arg(&text)
            .output()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn get_suggestions(prefix: String, limit: Option<usize>) -> Result<SuggestResult, String> {
    ensure_db_initialized()?;

    let limit = limit.unwrap_or(10);
    if prefix.is_empty() {
        return Ok(SuggestResult {
            suggestions: vec![],
        });
    }

    let suggestions = core::database::get_suggestions(&prefix, limit).map_err(|e| e.to_string())?;

    Ok(SuggestResult { suggestions })
}

#[tauri::command]
async fn get_file_by_id(id: i64) -> Result<Option<FileEntry>, String> {
    ensure_db_initialized()?;

    let result = core::database::get_file_by_id(id as u64).map_err(|e| e.to_string())?;

    Ok(result.map(|m| {
        let path = std::path::PathBuf::from(&m.path);
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let extension = path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_string());

        FileEntry {
            id: m.id as i64,
            path: path.to_string_lossy().to_string(),
            filename,
            extension,
            size: m.size,
            is_directory: m.is_directory,
            modified: chrono::DateTime::from_timestamp(m.modified_at, 0)
                .unwrap_or_default()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            created: chrono::DateTime::from_timestamp(m.created_at, 0)
                .unwrap_or_default()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        }
    }))
}

#[tauri::command]
async fn set_skip_hidden_dirs(skip: bool) -> Result<(), String> {
    let mut skip_hidden = SKIP_HIDDEN_DIRS.lock().map_err(|e| e.to_string())?;
    *skip_hidden = skip;
    info!("[config] skip_hidden_dirs set to {}", skip);
    Ok(())
}

#[tauri::command]
async fn get_skip_hidden_dirs() -> Result<bool, String> {
    let skip_hidden = SKIP_HIDDEN_DIRS.lock().map_err(|e| e.to_string())?;
    Ok(*skip_hidden)
}

fn main() {
    // Initialize database first to check if we need to reindex
    let needs_reindex = match core::database::init_database() {
        Ok(_) => {
            match core::database::needs_reindex() {
                Ok(needs) => needs,
                Err(_) => false,
            }
        }
        Err(_) => false,
    };

    // Set the initialized flag
    {
        let mut initialized = DB_INIT.lock().unwrap();
        *initialized = true;
    }

    // Set indexing flag if we need to reindex
    if needs_reindex {
        let mut indexing = IS_INDEXING.lock().unwrap();
        *indexing = true;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_log::Builder::default()
            .target(tauri_plugin_log::Target::new(
                tauri_plugin_log::TargetKind::Folder {
                    path: dirs::home_dir().unwrap().join(".isearch").join("logs"),
                    file_name: Some("isearch".to_string()),
                }
            ))
            .level(log::LevelFilter::Info)
            .build())
        .invoke_handler(tauri::generate_handler![
            search_files,
            start_indexing,
            get_index_status,
            get_index_memory_status,
            get_unindexed_dirs,
            get_index_errors,
            check_needs_reindex,
            open_file,
            open_folder,
            delete_file,
            copy_to_clipboard,
            get_suggestions,
            get_file_by_id,
            evict_old_entries,
            set_skip_hidden_dirs,
            get_skip_hidden_dirs,
            get_indexed_dirs,
            rebuild_indexed_dirs,
            rebuild_index,
            get_ignored_dirs,
            add_ignored_dir,
            remove_ignored_dir,
            init_clamav,
            is_clamav_ready,
            start_security_scan,
            analyze_file,
            save_scan_report,
            save_search_results,
            get_log_files,
            read_log_file,
            get_overview_data,
            refresh_overview_data,
        ])
        .setup(move |app| {
            // Log app startup now that logger is set up
            info!("[app_startup] version={} - iSearch application starting", env!("CARGO_PKG_VERSION"));
            info!("[db_init] status=success - Database initialized");

            // Emit current index status on startup (don't auto-index)
            let file_count = core::database::get_file_count().unwrap_or(0) as usize;
            let is_indexing = *IS_INDEXING.lock().unwrap();

            // Try to get last indexed time from database
            let last_indexed = match core::database::get_last_indexed_time() {
                Ok(Some(ts)) => {
                    // Convert timestamp to readable format
                    chrono::DateTime::from_timestamp(ts, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default()
                }
                _ => LAST_INDEXED.lock().unwrap().clone()
            };

            info!("[index_status] file_count={} is_indexing={} last_indexed={}", file_count, is_indexing, last_indexed);

            // Emit index status event
            let message = if is_indexing {
                "索引进行中...".to_string()
            } else if file_count > 0 {
                format!("已索引 {} 个文件", file_count)
            } else {
                "未索引".to_string()
            };
            let _ = app.emit("index-status", serde_json::json!({
                "file_count": file_count,
                "is_indexing": is_indexing,
                "last_indexed": last_indexed,
                "message": message
            }));

            // Rebuild indexed dirs from database if needed (for first-time setup)
            let indexed_dirs_file = core::database::get_indexed_dirs_file();
            if !indexed_dirs_file.exists() {
                info!("[startup] indexed_dirs.json not found, rebuilding from database...");
                if let Err(e) = core::database::rebuild_indexed_dirs_from_db() {
                    warn!("[startup] Failed to rebuild indexed dirs: {}", e);
                } else {
                    info!("[startup] Indexed dirs rebuilt successfully");
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

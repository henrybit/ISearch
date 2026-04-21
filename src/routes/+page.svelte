<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";

  interface FileEntry {
    id?: number;
    filename: string;
    path: string;
    is_directory: boolean;
    size: number;
    modified: string;
    created: string;
  }

  interface SearchResult {
    files: FileEntry[];
    total: number;
    search_time_ms: number;
  }

  interface IndexStatus {
    is_indexing: boolean;
    file_count: number;
    last_indexed: string;
    db_path: string;
    db_size: number;
  }

  interface IndexError {
    path: string;
    error_type: string;
    message: string;
  }

  interface IndexErrors {
    permission_denied: string[];
    other_errors: IndexError[];
    total_errors: number;
  }

  interface TopLevelDir {
    name: string;
    path: string;
    is_indexed: boolean;
    is_indexing: boolean;
    file_count: number;
    size_bytes: number;
  }

  interface SecuritySummary {
    level: "low" | "medium" | "high";
    conclusion: string;
    hasTrojanRisk: boolean;
    reasons: string[];
    isInfected?: boolean;
    threatName?: string;
  }

  // Menu state
  let activeMenu = $state<"none" | "history" | "settings" | "help" | "scanHistory">("none");
  let showLogPanel = $state(false);
  let logContent = $state<string[]>([]);
  let logFiles = $state<string[]>([]);
  let selectedLogFile = $state<string | null>(null);
  let searchHistory = $state<string[]>([]);
  let ignoredDirs = $state<string[]>([]);
  let newIgnoredDir = $state("");

  // Language state
  type Language = "zh" | "en";
  let currentLang = $state<Language>("zh");

  // Translations
  const translations = {
    zh: {
      overview: "概览",
      search: "搜索",
      securityScan: "安全扫描",
      searchPlaceholder: "输入关键词搜索文件",
      indexStatus: "已索引",
      files: "文件",
      lastUpdated: "更新时间",
      reindex: "刷新索引",
      refreshData: "刷新数据",
      diskStorage: "磁盘存储",
      total: "总容量",
      used: "已用",
      available: "可用",
      indexedFiles: "索引文件总数",
      fileTypeDistribution: "文件类型分布",
      noDataHint: "请等待索引结束后查看数据",
      history: "历史搜索",
      scanHistory: "扫描历史",
      settings: "设置",
      help: "帮助",
      logs: "日志",
      about: "关于",
      version: "版本",
      features: "主要特性",
      fastIndex: "快速索引",
      realTimeSearch: "实时搜索",
      incrementalUpdate: "增量更新",
      crossPlatform: "跨平台",
      aboutOneSolo: "关于 OneSolo",
      oneSoloDesc: "OneSolo 专注于使用先进技术制作软件，致力于为用户提供高效、可靠的产品体验。",
      close: "关闭",
      startScan: "开始扫描",
      stopScan: "停止扫描",
      scanProgress: "扫描进度",
      scanned: "已扫描",
      selectDirectory: "选择目录",
      scanAll: "扫描全部",
      clean: "正常",
      threats: "威胁",
      scanComplete: "扫描完成",
      scanTime: "耗时",
      saveReport: "保存报告",
      viewReport: "查看报告",
      noThreats: "未发现威胁文件",
      suspicious: "可疑文件",
      highRisk: "高危文件",
      lowRisk: "低危文件",
      mediumRisk: "中危文件",
      startIndexing: "开始索引",
      stopIndexing: "停止索引",
      indexing: "索引中",
      indexed: "已索引",
      noIndexDirs: "未设置索引目录",
      addDirectory: "添加目录",
      removeDirectory: "移除目录",
      ignoredDirs: "忽略目录",
      addIgnoredDir: "添加忽略目录",
      saveSettings: "保存设置",
      clearHistory: "清除历史",
      exportResults: "导出结果",
      copyPath: "复制路径",
      openFolder: "打开所在文件夹",
      fileName: "文件名",
      filePath: "路径",
      fileSize: "大小",
      modified: "修改时间",
      created: "创建时间",
      searchResults: "搜索结果",
      searchTime: "搜索耗时",
      noResults: "未找到结果",
      lastUpdate: "最后更新",
    },
    en: {
      overview: "Overview",
      search: "Search",
      securityScan: "Security Scan",
      searchPlaceholder: "Enter keywords to search files",
      indexStatus: "Indexed",
      files: "files",
      lastUpdated: "Last updated",
      reindex: "Rebuild Index",
      refreshData: "Refresh Data",
      diskStorage: "Disk Storage",
      total: "Total",
      used: "Used",
      available: "Available",
      indexedFiles: "Indexed Files",
      fileTypeDistribution: "File Type Distribution",
      noDataHint: "Please wait for indexing to complete",
      history: "History",
      scanHistory: "Scan History",
      settings: "Settings",
      help: "Help",
      logs: "Logs",
      about: "About",
      version: "Version",
      features: "Features",
      fastIndex: "Fast Indexing",
      realTimeSearch: "Real-time Search",
      incrementalUpdate: "Incremental Update",
      crossPlatform: "Cross-platform",
      aboutOneSolo: "About OneSolo",
      oneSoloDesc: "OneSolo focuses on creating software with advanced technology, committed to providing efficient and reliable products.",
      close: "Close",
      startScan: "Start Scan",
      stopScan: "Stop Scan",
      scanProgress: "Scan Progress",
      scanned: "Scanned",
      selectDirectory: "Select Directory",
      scanAll: "Scan All",
      clean: "Clean",
      threats: "Threats",
      scanComplete: "Scan Complete",
      scanTime: "Duration",
      saveReport: "Save Report",
      viewReport: "View Report",
      noThreats: "No threats found",
      suspicious: "Suspicious Files",
      highRisk: "High Risk",
      lowRisk: "Low Risk",
      mediumRisk: "Medium Risk",
      startIndexing: "Start Indexing",
      stopIndexing: "Stop Indexing",
      indexing: "Indexing",
      indexed: "Indexed",
      noIndexDirs: "No index directories set",
      addDirectory: "Add Directory",
      removeDirectory: "Remove Directory",
      ignoredDirs: "Ignored Directories",
      addIgnoredDir: "Add Ignored Directory",
      saveSettings: "Save Settings",
      clearHistory: "Clear History",
      exportResults: "Export Results",
      copyPath: "Copy Path",
      openFolder: "Open Folder",
      fileName: "File Name",
      filePath: "Path",
      fileSize: "Size",
      modified: "Modified",
      created: "Created",
      searchResults: "Search Results",
      searchTime: "Search Time",
      noResults: "No results found",
      lastUpdate: "Last Update",
    }
  };

  function t(key: keyof typeof translations.zh): string {
    return translations[currentLang][key] || key;
  }

  function toggleLanguage() {
    currentLang = currentLang === "zh" ? "en" : "zh";
  }

  // Scan history state
  interface ScanHistoryItem {
    id: string;
    timestamp: string;
    scannedDir: string;
    totalFiles: number;
    cleanCount: number;
    threatCount: number;
    duration: number;
    threats: ScanResult[];
  }
  let scanHistory = $state<ScanHistoryItem[]>([]);
  let selectedScanReport = $state<ScanHistoryItem | null>(null);

  // App mode
  type AppMode = "overview" | "search" | "security";
  let appMode = $state<AppMode>("overview");

  // Overview stats state
  interface DiskStats {
    total: number;
    used: number;
    free: number;
  }

  interface ExtStats {
    extension: string;
    count: number;
    description: string;
  }

  interface OverviewData {
    diskStats: DiskStats | null;
    totalFiles: number;
    extStats: ExtStats[];
    lastUpdated: string;
  }

  let overviewData = $state<OverviewData>({
    diskStats: null,
    totalFiles: 0,
    extStats: [],
    lastUpdated: ""
  });
  let isOverviewRefreshing = $state(false);

  // Security scan state
  interface ScanResult {
    path: string;
    threat: string;
    severity: "low" | "medium" | "high";
  }
  let isScanning = $state(false);
  let scanProgress = $state(0);
  let scanResults = $state<ScanResult[]>([]);
  let scannedFiles = $state(0);
  let totalScanned = $state(0);
  let cleanCount = $state(0);
  let scanTime = $state(0);
  let scanDir = $state(""); // User-selected directory to scan

  let query = $state("");
  let results = $state<FileEntry[]>([]);
  let total = $state(0);
  let searchTime = $state(0);
  let isLoading = $state(false);
  let hasSearched = $state(false);
  let selectedFile = $state<FileEntry | null>(null);
  let isAnalyzingFile = $state(false);
  let fileAnalysis = $state<SecuritySummary | null>(null);

  let isIndexing = $state(false);
  let indexedFiles = $state(0);
  let lastIndexed = $state("");
  let unindexedDirs = $state<string[]>([]);
  let indexedDirs = $state<TopLevelDir[]>([]);
  let showDirs = $state(false);
  let indexErrors = $state<IndexErrors | null>(null);
  let showErrorPanel = $state(false);

  const quickKeywords = ["项目", "合同", "发票", "设计稿", "日志"];
  const isMacOS =
    typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");

  onMount(async () => {
    await refreshIndexStatus();
    await checkUnindexedDirs();
    await loadIndexErrors();
    await loadIndexedDirs();
    await loadIgnoredDirs();
    loadSearchHistory();
    loadScanHistory();
    await loadOverviewData();

    await listen<{ message: string; count: number }>(
      "index-progress",
      (event) => {
        indexedFiles = event.payload.count;
      },
    );

    await listen<string>("index-complete", async () => {
      await refreshIndexStatus();
      await loadIndexedDirs();
      await refreshOverviewData();
    });

    // Listen for index-status events (emitted on startup and during indexing)
    await listen<IndexStatus>("index-status", (event) => {
      isIndexing = event.payload.is_indexing;
      indexedFiles = event.payload.file_count;
      lastIndexed = event.payload.last_indexed;
    });
  });

  async function loadOverviewData() {
    try {
      const data = await invoke<OverviewData>("get_overview_data");
      if (data && data.lastUpdated) {
        overviewData = data;
      }
    } catch (e) {
      console.error("Failed to load overview data:", e);
    }
  }

  async function refreshOverviewData() {
    try {
      isOverviewRefreshing = true;
      await invoke("refresh_overview_data");
      await loadOverviewData();
    } catch (e) {
      console.error("Failed to refresh overview data:", e);
    } finally {
      isOverviewRefreshing = false;
    }
  }

  async function refreshIndexStatus() {
    try {
      const status: IndexStatus = await invoke("get_index_status");
      isIndexing = status.is_indexing;
      indexedFiles = status.file_count;
      lastIndexed = status.last_indexed;
    } catch (e) {
      console.error("Failed to get index status:", e);
    }
  }

  async function checkUnindexedDirs() {
    try {
      unindexedDirs = await invoke("get_unindexed_dirs");
    } catch (e) {
      console.error("Failed to check unindexed dirs:", e);
    }
  }

  async function loadIndexErrors() {
    try {
      indexErrors = await invoke("get_index_errors");
    } catch (e) {
      console.error("Failed to load index errors:", e);
    }
  }

  async function loadIndexedDirs() {
    try {
      indexedDirs = await invoke("get_indexed_dirs");
    } catch (e) {
      console.error("Failed to load indexed dirs:", e);
    }
  }

  function loadSearchHistory() {
    try {
      const saved = localStorage.getItem("searchHistory");
      searchHistory = saved ? JSON.parse(saved) : [];
    } catch (e) {
      searchHistory = [];
    }
  }

  function saveSearchHistory(history: string[]) {
    try {
      localStorage.setItem("searchHistory", JSON.stringify(history));
    } catch (e) {
      console.error("Failed to save search history:", e);
    }
  }

  function addToHistory(item: string) {
    const filtered = searchHistory.filter((h) => h !== item);
    filtered.unshift(item);
    searchHistory = filtered.slice(0, 20);
    saveSearchHistory(searchHistory);
  }

  function clearHistory() {
    searchHistory = [];
    saveSearchHistory(searchHistory);
  }

  function clearScanResults() {
    scanResults = [];
    scannedFiles = 0;
    totalScanned = 0;
    cleanCount = 0;
    scanTime = 0;
    scanProgress = 0;
  }

  function useHistoryItem(item: string) {
    query = item;
    activeMenu = "none";
    doSearch();
  }

  function toggleMenu(menu: "none" | "history" | "settings" | "help" | "scanHistory") {
    activeMenu = activeMenu === menu ? "none" : menu;
    if (menu === "history") {
      loadSearchHistory();
    } else if (menu === "scanHistory") {
      loadScanHistory();
    }
  }

  function loadScanHistory() {
    try {
      const saved = localStorage.getItem("scanHistory");
      scanHistory = saved ? JSON.parse(saved) : [];
    } catch (e) {
      scanHistory = [];
    }
  }

  function saveScanHistory(history: ScanHistoryItem[]) {
    try {
      localStorage.setItem("scanHistory", JSON.stringify(history));
    } catch (e) {
      console.error("Failed to save scan history:", e);
    }
  }

  function addToScanHistory(item: ScanHistoryItem) {
    const filtered = scanHistory.filter((h) => h.id !== item.id);
    filtered.unshift(item);
    scanHistory = filtered.slice(0, 50); // Keep last 50 scans
    saveScanHistory(scanHistory);
  }

  function viewScanReport(item: ScanHistoryItem) {
    selectedScanReport = item;
  }

  function closeScanReport() {
    selectedScanReport = null;
  }

  async function loadIgnoredDirs() {
    try {
      ignoredDirs = await invoke("get_ignored_dirs");
    } catch (e) {
      console.error("Failed to load ignored dirs:", e);
    }
  }

  async function addIgnoredDir() {
    if (!newIgnoredDir.trim()) return;
    try {
      await invoke("add_ignored_dir", { path: newIgnoredDir.trim() });
      await loadIgnoredDirs();
      newIgnoredDir = "";
    } catch (e) {
      console.error("Failed to add ignored dir:", e);
    }
  }

  async function removeIgnoredDir(path: string) {
    try {
      await invoke("remove_ignored_dir", { path });
      await loadIgnoredDirs();
    } catch (e) {
      console.error("Failed to remove ignored dir:", e);
    }
  }

  async function selectScanDirectory() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择要扫描的目录"
      });
      if (selected) {
        scanDir = selected as string;
      }
    } catch (e) {
      console.error("Failed to select directory:", e);
    }
  }

  async function startSecurityScan() {
    if (isScanning) return;
    isScanning = true;
    scanResults = [];
    scanProgress = 0;
    scannedFiles = 0;
    totalScanned = 0;
    cleanCount = 0;
    scanTime = 0;

    try {
      // Initialize ClamAV first
      await invoke("init_clamav");

      // Listen for security scan status updates
      await listen("security-scan-status", (event: any) => {
        const status = event.payload;
        if (status.is_scanning) {
          scanProgress = status.total > 0 ? Math.floor((status.scanned / status.total) * 100) : 0;
          scannedFiles = status.scanned || 0;
          totalScanned = status.total || 0;
        }
      });

      // Start the security scan with optional directory
      const result: any = await invoke("start_security_scan", { scanDir: scanDir || null });

      // Update scan results with statistics
      scanResults = result.threats || [];
      scanProgress = 100;
      scannedFiles = result.scanned_count || 0;
      totalScanned = result.scanned_count || 0;
      cleanCount = result.clean_count || 0;
      scanTime = result.scan_time_ms || 0;

      // Save to scan history
      const scanItem: ScanHistoryItem = {
        id: Date.now().toString(),
        timestamp: new Date().toLocaleString("zh-CN"),
        scannedDir: scanDir || "全部文件",
        totalFiles: scannedFiles,
        cleanCount: cleanCount,
        threatCount: scanResults.length,
        duration: scanTime,
        threats: scanResults,
      };
      addToScanHistory(scanItem);

      // Also save to backend file
      try {
        await invoke("save_scan_report", { report: scanItem });
      } catch (e) {
        console.error("Failed to save scan report to file:", e);
      }

    } catch (e) {
      console.error("Security scan failed:", e);
      alert("安全扫描失败: " + e);
    } finally {
      isScanning = false;
    }
  }

  function stopSecurityScan() {
    isScanning = false;
  }

  function switchMode(mode: AppMode) {
    appMode = mode;
    activeMenu = "none";
  }

  async function startIndexing() {
    try {
      await invoke("start_indexing");
      isIndexing = true;
      // Poll for index completion and then load errors
      const pollInterval = setInterval(async () => {
        await refreshIndexStatus();
        if (!isIndexing) {
          clearInterval(pollInterval);
          await loadIndexErrors();
          if (indexErrors && indexErrors.total_errors > 0) {
            showErrorPanel = true;
          }
        }
      }, 1000);
    } catch (e) {
      console.error("Failed to start indexing:", e);
    }
  }

  async function startReindex() {
    try {
      isIndexing = true;
      await invoke("rebuild_index");
      // Poll for index completion and then load indexed dirs
      const pollInterval = setInterval(async () => {
        await refreshIndexStatus();
        if (!isIndexing) {
          clearInterval(pollInterval);
          await loadIndexedDirs();
        }
      }, 1000);
    } catch (e) {
      console.error("Failed to start reindex:", e);
    }
  }

  async function doSearch() {
    const q = query.trim();

    if (!q) {
      hasSearched = false;
      results = [];
      total = 0;
      searchTime = 0;
      selectedFile = null;
      return;
    }

    // Add to search history
    addToHistory(q);

    hasSearched = true;
    isLoading = true;

    try {
      const result: SearchResult = await invoke("search_files", {
        query: q,
        limit: 500,
      });
      results = result.files;
      total = result.total;
      searchTime = result.search_time_ms;
      selectedFile = result.files[0] ?? null;

      // Save search results to markdown file
      try {
        await invoke("save_search_results", { query: q, results });
      } catch (e) {
        console.error("Failed to save search results:", e);
      }
    } catch (e) {
      console.error("Search failed:", e);
      results = [];
      selectedFile = null;
    } finally {
      isLoading = false;
    }
  }

  function fillQuickKeyword(word: string) {
    query = word;
    doSearch();
  }

  function selectFile(entry: FileEntry) {
    selectedFile = entry;
    fileAnalysis = null;
    isAnalyzingFile = false;
  }

  async function scanSelectedFile() {
    if (!selectedFile) return;
    isAnalyzingFile = true;
    fileAnalysis = null;

    try {
      // Call ClamAV to scan the selected file
      const result = await invoke("analyze_file", { path: selectedFile.path });
      if (result.is_infected) {
        fileAnalysis = {
          level: result.threat_name?.toLowerCase().includes("trojan") ||
                 result.threat_name?.toLowerCase().includes("ransomware") ? "high" : "medium",
          conclusion: result.threat_name || "Unknown threat",
          hasTrojanRisk: true,
          reasons: [`威胁名称: ${result.threat_name || "Unknown"}`],
          isInfected: true,
          threatName: result.threat_name
        };
      } else {
        fileAnalysis = {
          level: "low",
          conclusion: currentLang === "zh" ? "文件安全" : "File is safe",
          hasTrojanRisk: false,
          reasons: [currentLang === "zh" ? "未检测到威胁" : "No threats detected"],
          isInfected: false,
          threatName: null
        };
      }
    } catch (e) {
      console.error("Failed to scan file:", e);
      fileAnalysis = {
        level: "low",
        conclusion: currentLang === "zh" ? "扫描失败" : "Scan failed",
        hasTrojanRisk: false,
        reasons: [`${e}`],
        isInfected: false,
        threatName: null
      };
    } finally {
      isAnalyzingFile = false;
    }
  }

  function closeDetail() {
    selectedFile = null;
    fileAnalysis = null;
  }

  async function loadLogFiles() {
    try {
      logFiles = await invoke("get_log_files");
      if (logFiles.length > 0 && !selectedLogFile) {
        selectedLogFile = logFiles[0];
        await loadLogContent();
      }
    } catch (e) {
      console.error("Failed to load log files:", e);
    }
  }

  async function loadLogContent() {
    if (!selectedLogFile) return;
    try {
      const content: string[] = await invoke("read_log_file", { filename: selectedLogFile });
      logContent = content;
    } catch (e) {
      console.error("Failed to load log content:", e);
      logContent = ["加载日志失败: " + e];
    }
  }

  let logPollTimer: ReturnType<typeof setInterval> | null = null;
  let logPanelWidth = $state(500);
  let isResizing = $state(false);

  function startResize(e: MouseEvent) {
    isResizing = true;
    const startX = e.clientX;
    const startWidth = logPanelWidth;

    const onMouseMove = (e: MouseEvent) => {
      if (!isResizing) return;
      const delta = startX - e.clientX;
      logPanelWidth = Math.max(300, Math.min(800, startWidth + delta));
    };

    const onMouseUp = () => {
      isResizing = false;
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', onMouseUp);
    };

    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
  }

  async function toggleLogPanel() {
    showLogPanel = !showLogPanel;
    if (showLogPanel) {
      await loadLogFiles();
      // Start polling for real-time updates
      if (logPollTimer) clearInterval(logPollTimer);
      logPollTimer = setInterval(loadLogContent, 2000);
    } else {
      // Stop polling
      if (logPollTimer) {
        clearInterval(logPollTimer);
        logPollTimer = null;
      }
    }
  }

  function formatSize(bytes: number): string {
    if (!bytes || bytes <= 0) return "-";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
  }

  function formatDate(dateStr: string): string {
    if (!dateStr) return "-";
    const date = new Date(dateStr);
    return `${date.toLocaleDateString("zh-CN")} ${date.toLocaleTimeString("zh-CN", { hour12: false })}`;
  }

  function getFileType(entry: FileEntry): string {
    if (entry.is_directory) return "文件夹";
    const parts = entry.filename.split(".");
    if (parts.length < 2) return "未知类型";
    return `${parts.pop()?.toUpperCase()} 文件`;
  }

  function getSecuritySummary(entry: FileEntry): SecuritySummary {
    const lowercaseName = entry.filename.toLowerCase();
    const ext = lowercaseName.includes(".")
      ? (lowercaseName.split(".").pop() ?? "")
      : "";

    const suspiciousExt = new Set([
      "exe",
      "bat",
      "cmd",
      "scr",
      "vbs",
      "apk",
      "dmg",
      "pkg",
      "jar",
      "ps1",
    ]);
    const scriptExt = new Set(["js", "ts", "sh", "py"]);
    const malwareKeywords = [
      "trojan",
      "virus",
      "keygen",
      "crack",
      "backdoor",
      "ransom",
      "payload",
    ];

    const reasons: string[] = [];
    let score = 0;

    if (suspiciousExt.has(ext)) {
      score += 2;
      reasons.push(`扩展名 .${ext || "unknown"} 属于高敏感可执行类型`);
    } else if (scriptExt.has(ext)) {
      score += 1;
      reasons.push(`扩展名 .${ext} 为脚本文件，建议确认来源`);
    }

    if (malwareKeywords.some((k) => lowercaseName.includes(k))) {
      score += 2;
      reasons.push("文件名包含常见恶意软件关键词");
    }

    if (entry.size > 500 * 1024 * 1024) {
      score += 1;
      reasons.push("文件体积较大，建议确认用途与来源");
    }

    if (entry.is_directory) {
      return {
        level: "low",
        conclusion: "目录本身无直接执行风险，可继续检查目录内容。",
        hasTrojanRisk: false,
        reasons: ["当前对象是目录，需结合内部文件综合判断"],
      };
    }

    if (score >= 4) {
      return {
        level: "high",
        conclusion:
          "存在明显风险特征，疑似木马或潜在恶意文件，建议隔离后再处理。",
        hasTrojanRisk: true,
        reasons,
      };
    }

    if (score >= 2) {
      return {
        level: "medium",
        conclusion: "存在可疑特征，建议进行杀毒扫描与来源校验。",
        hasTrojanRisk: false,
        reasons,
      };
    }

    return {
      level: "low",
      conclusion: "未发现明显风险特征，整体风险较低。",
      hasTrojanRisk: false,
      reasons: reasons.length > 0 ? reasons : ["未命中可疑扩展名及高危关键词"],
    };
  }

  async function openFile(path: string) {
    try {
      await invoke("open_file", { path });
    } catch (e) {
      console.error("Failed to open file:", e);
    }
  }

  async function openFolder(path: string) {
    try {
      await invoke("open_folder", { path });
    } catch (e) {
      console.error("Failed to open folder:", e);
    }
  }

  async function deleteFile(path: string) {
    try {
      await invoke("delete_file", { path });
      results = results.filter((item) => item.path !== path);
      total = Math.max(0, total - 1);
      selectedFile = null;
    } catch (e) {
      console.error("Failed to delete file:", e);
    }
  }
</script>

<div class="page" class:compact={hasSearched}>
  <!-- Menu Bar -->
  <div class="menu-bar">
    <!-- Language Toggle -->
    <button class="lang-toggle" onclick={toggleLanguage}>
      {currentLang === "zh" ? "EN" : "中"}
    </button>
    <!-- Mode Switcher -->
    <div class="mode-switcher">
      <button
        class="mode-btn"
        class:active={appMode === "overview"}
        onclick={() => switchMode("overview")}
      >
        {t("overview")}
      </button>
      <button
        class="mode-btn"
        class:active={appMode === "search"}
        onclick={() => switchMode("search")}
      >
        {t("search")}
      </button>
      <button
        class="mode-btn"
        class:active={appMode === "security"}
        onclick={() => switchMode("security")}
      >
        {t("securityScan")}
      </button>
    </div>
    <div class="menu-spacer"></div>
    <button
      class="menu-item"
      class:active={activeMenu === "history"}
      onclick={() => toggleMenu("history")}
    >
      {t("history")}
    </button>
    <button
      class="menu-item"
      class:active={activeMenu === "scanHistory"}
      onclick={() => toggleMenu("scanHistory")}
    >
      {t("scanHistory")}
    </button>
    <button
      class="menu-item"
      class:active={activeMenu === "settings"}
      onclick={() => toggleMenu("settings")}
    >
      {t("settings")}
    </button>
    <button
      class="menu-item"
      class:active={activeMenu === "help"}
      onclick={() => toggleMenu("help")}
    >
      {t("help")}
    </button>
  </div>

  <!-- Overview Panel -->
  {#if appMode === "overview"}
    <div class="overview-panel">
      <div class="overview-header">
        <h2>{t("overview")}</h2>
        <button class="refresh-btn" onclick={refreshOverviewData} disabled={isOverviewRefreshing}>
          {#if isOverviewRefreshing}
            <span class="refresh-spinner"></span>
            {currentLang === "zh" ? "刷新中..." : "Refreshing..."}
          {:else}
            {t("refreshData")}
          {/if}
        </button>
      </div>

      {#if isOverviewRefreshing}
        <div class="overview-loading">
          <div class="loading-bar">
            <div class="loading-bar-fill"></div>
          </div>
          <p>{currentLang === "zh" ? "正在分析文件系统..." : "Analyzing file system..."}</p>
        </div>
      {:else if overviewData.lastUpdated}
        <!-- Disk Stats -->
        <div class="overview-section">
          <h3>{t("diskStorage")}</h3>
          {#if overviewData.diskStats}
            <div class="disk-stats">
              <div class="disk-item">
                <span class="disk-label">{t("total")}</span>
                <span class="disk-value">{formatSize(overviewData.diskStats.total)}</span>
              </div>
              <div class="disk-item">
                <span class="disk-label">{t("used")}</span>
                <span class="disk-value">{formatSize(overviewData.diskStats.used)}</span>
              </div>
              <div class="disk-item">
                <span class="disk-label">{t("available")}</span>
                <span class="disk-value">{formatSize(overviewData.diskStats.free)}</span>
              </div>
            </div>
            <div class="disk-bar">
              <div class="disk-used" style="width: {overviewData.diskStats.total > 0 ? (overviewData.diskStats.used / overviewData.diskStats.total * 100) : 0}%"></div>
            </div>
          {/if}
          <div class="stat-item">
            <span class="stat-label">{t("indexedFiles")}</span>
            <span class="stat-value">{overviewData.totalFiles.toLocaleString()}</span>
          </div>
        </div>

        <!-- Extension Stats -->
        <div class="overview-section">
          <h3>{t("fileTypeDistribution")}</h3>
          <div class="ext-list">
            {#each overviewData.extStats as ext}
              <div class="ext-item">
                <div class="ext-info">
                  <span class="ext-name">.{ext.extension}</span>
                  <span class="ext-desc">{ext.description}</span>
                </div>
                <span class="ext-count">{ext.count.toLocaleString()}</span>
              </div>
            {/each}
          </div>
        </div>

        <div class="overview-footer">
          <span>{t("lastUpdate")}: {overviewData.lastUpdated}</span>
        </div>
      {:else if !isOverviewRefreshing}
        <div class="overview-empty">
          <p>{t("noDataHint")}</p>
          <p class="hint">{t("noDataHint")}</p>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Security Scan Panel -->
  {#if appMode === "security"}
    <div class="security-panel">
      <div class="security-header">
        <h2>{t("securityScan")}</h2>
        <p class="security-desc">{currentLang === "zh" ? "选择目录扫描潜在威胁文件" : "Select a directory to scan for potential threats"}</p>
      </div>

      <!-- Directory Selector -->
      <div class="scan-dir-selector">
        <div class="dir-input-group">
          <input
            type="text"
            class="dir-input"
            placeholder={currentLang === "zh" ? "选择要扫描的目录（留空扫描全部）" : "Select directory to scan (leave empty to scan all)"}
            bind:value={scanDir}
            readonly
          />
          <button class="dir-btn" onclick={selectScanDirectory}>
            {t("selectDirectory")}
          </button>
        </div>
        {#if scanDir}
          <div class="dir-selected">
            {currentLang === "zh" ? "已选择" : "Selected"}: <span class="dir-path">{scanDir}</span>
            <button class="dir-clear" onclick={() => scanDir = ""}>{currentLang === "zh" ? "清除" : "Clear"}</button>
          </div>
        {/if}
      </div>

      {#if !isScanning && scanResults.length === 0}
        <div class="security-start">
          <button class="scan-btn-circle" onclick={startSecurityScan}>
            <span class="scan-icon">▶</span>
            <span class="scan-text">{t("startScan")}</span>
          </button>
        </div>
      {/if}

      {#if isScanning}
        <div class="scan-progress">
          <div class="progress-bar">
            <div class="progress-fill" style="width: {scanProgress}%"></div>
          </div>
          <div class="progress-info">
            <span>{t("scanProgress")}: {scanProgress}%</span>
            <span>{t("scanned")}: {scannedFiles.toLocaleString()} {t("files")}</span>
          </div>
          <div class="stop-btn-center">
            <button class="stop-btn-circle" onclick={stopSecurityScan}>
              <span class="scan-icon">■</span>
              <span class="scan-text">{t("stopScan")}</span>
            </button>
          </div>
        </div>
      {/if}

      <!-- Scan Results (always show after scan completes) -->
      {#if !isScanning && (scanResults.length > 0 || scannedFiles > 0)}
        <div class="scan-results">
          <div class="results-header">
            <h3>{scanResults.length > 0 ? (currentLang === "zh" ? "扫描结果" : "Scan Results") : (currentLang === "zh" ? "扫描完成" : "Scan Complete")}</h3>
            {#if scanResults.length > 0}
              <span class="threat-count">{scanResults.length} {t("threats")}</span>
            {/if}
            <button class="clear-results-btn" onclick={clearScanResults}>{currentLang === "zh" ? "清空结果" : "Clear Results"}</button>
          </div>
          <!-- Scan Statistics -->
          <div class="scan-stats">
            <div class="stat-item">
              <span class="stat-value">{totalScanned.toLocaleString()}</span>
              <span class="stat-label">{currentLang === "zh" ? "扫描文件" : "Scanned Files"}</span>
            </div>
            <div class="stat-item clean">
              <span class="stat-value">{cleanCount.toLocaleString()}</span>
              <span class="stat-label">{t("clean")}</span>
            </div>
            <div class="stat-item" class:has-threats={scanResults.length > 0}>
              <span class="stat-value">{scanResults.length}</span>
              <span class="stat-label">{t("suspicious")}</span>
            </div>
            <div class="stat-item">
              <span class="stat-value">{(scanTime / 1000).toFixed(1)}s</span>
              <span class="stat-label">{t("scanTime")}</span>
            </div>
          </div>
          <!-- Threat severity breakdown -->
          {#if scanResults.length > 0}
            {@const highCount = scanResults.filter(r => r.severity === "high").length}
            {@const mediumCount = scanResults.filter(r => r.severity === "medium").length}
            {@const lowCount = scanResults.filter(r => r.severity === "low").length}
            <div class="severity-breakdown">
              {#if highCount > 0}<span class="breakdown-item high">{t("highRisk")}: {highCount}</span>{/if}
              {#if mediumCount > 0}<span class="breakdown-item medium">{t("mediumRisk")}: {mediumCount}</span>{/if}
              {#if lowCount > 0}<span class="breakdown-item low">{t("lowRisk")}: {lowCount}</span>{/if}
            </div>
          {/if}
          <!-- Threat list -->
          {#each scanResults as result}
            <div class="threat-item" class:high={result.severity === "high"} class:medium={result.severity === "medium"}>
              <div class="threat-info">
                <span class="threat-path">{result.path}</span>
                <span class="threat-name">{result.threat}</span>
              </div>
              <span class="severity-badge" class:high={result.severity === "high"} class:medium={result.severity === "medium"}>
                {result.severity === "high" ? t("highRisk") : result.severity === "medium" ? t("mediumRisk") : t("lowRisk")}
              </span>
            </div>
          {/each}
        </div>
      {/if}

      {#if !isScanning && scanResults.length > 0}
        <div class="scan-actions">
          <button class="rescan-btn" onclick={startSecurityScan}>{currentLang === "zh" ? "重新扫描" : "Rescan"}</button>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Menu Panels (Slide-out Drawers) -->
  {#if activeMenu !== "none"}
    <div class="menu-overlay open" onclick={() => activeMenu = "none"}></div>
  {/if}

  {#if activeMenu === "history"}
    <div class="menu-panel open">
      <div class="panel-header">
        <span>{t("history")}</span>
        <button class="panel-close" onclick={() => activeMenu = "none"}>×</button>
      </div>
      {#if searchHistory.length === 0}
        <div class="empty-state">{currentLang === "zh" ? "暂无搜索历史" : "No search history"}</div>
      {:else}
        <div class="history-list">
          {#each searchHistory as item}
            <button class="history-item" onclick={() => useHistoryItem(item)}>
              <span class="history-icon">&#128269;</span>
              <span class="history-text">{item}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  {#if activeMenu === "scanHistory"}
    <div class="menu-panel open">
      <div class="panel-header">
        <span>{t("scanHistory")}</span>
        <button class="panel-close" onclick={() => activeMenu = "none"}>×</button>
      </div>
      {#if scanHistory.length === 0}
        <div class="empty-state">{currentLang === "zh" ? "暂无扫描记录" : "No scan history"}</div>
      {:else}
        <div class="scan-history-list">
          {#each scanHistory as item}
            <button class="scan-history-item" onclick={() => viewScanReport(item)}>
              <div class="scan-history-info">
                <span class="scan-history-time">{item.timestamp}</span>
                <span class="scan-history-dir">{item.scannedDir}</span>
              </div>
              <div class="scan-history-stats">
                <span class="scan-stat files">{item.totalFiles} {t("files")}</span>
                <span class="scan-stat threats" class:has-threats={item.threatCount > 0}>{item.threatCount} {t("threats")}</span>
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  {#if activeMenu === "settings"}
    <div class="menu-panel open">
      <div class="panel-header">
        <span>{t("ignoredDirs")}</span>
        <button class="panel-close" onclick={() => activeMenu = "none"}>×</button>
      </div>
      <div class="settings-section">
        <p class="settings-desc">{currentLang === "zh" ? "以下目录将在索引时被忽略：" : "The following directories will be ignored during indexing:"}</p>
        <div class="ignored-list">
          {#each ignoredDirs as dir}
            <div class="ignored-item">
              <span class="ignored-path">{dir}</span>
              <button class="remove-btn" onclick={() => removeIgnoredDir(dir)}>{currentLang === "zh" ? "删除" : "Remove"}</button>
            </div>
          {/each}
        </div>
        <div class="add-ignored">
          <input
            type="text"
            class="ignored-input"
            placeholder={currentLang === "zh" ? "输入要忽略的目录路径" : "Enter directory path to ignore"}
            bind:value={newIgnoredDir}
            onkeydown={(e) => e.key === "Enter" && addIgnoredDir()}
          />
          <button class="add-btn" onclick={addIgnoredDir}>{currentLang === "zh" ? "添加" : "Add"}</button>
        </div>
      </div>
    </div>
  {/if}

  {#if activeMenu === "help"}
    <div class="menu-panel open">
      <div class="panel-header">
        <button class="log-btn" onclick={toggleLogPanel}>{t("logs")}</button>
        <span>{t("about")} iSearch</span>
        <button class="panel-close" onclick={() => activeMenu = "none"}>×</button>
      </div>
      <div class="help-content">
        <h3>iSearch {currentLang === "zh" ? "文件搜索工具" : "File Search Tool"}</h3>
        <p class="version">{t("version")} 0.1.0</p>
        <p class="description">
          {currentLang === "zh" ? "iSearch 是一款高效的本地文件搜索工具，采用先进的索引技术，支持快速、准确的文件搜索功能。" : "iSearch is an efficient local file search tool using advanced indexing technology for fast and accurate file search."}
        </p>
        <div class="features">
          <h4>{t("features")}:</h4>
          <ul>
            <li>{t("fastIndex")}: {currentLang === "zh" ? "使用 WalkDir 高效遍历文件" : "Efficient file traversal using WalkDir"}</li>
            <li>{t("realTimeSearch")}: {currentLang === "zh" ? "支持模糊匹配和精确搜索" : "Supports fuzzy and exact search"}</li>
            <li>{t("incrementalUpdate")}: {currentLang === "zh" ? "自动检测文件变化" : "Auto-detect file changes"}</li>
            <li>{t("crossPlatform")}: {currentLang === "zh" ? "支持 macOS、Windows、Linux" : "Supports macOS, Windows, Linux"}</li>
          </ul>
        </div>
        <div class="team-info">
          <h4>{t("aboutOneSolo")}</h4>
          <p>{t("oneSoloDesc")}</p>
        </div>
      </div>
    </div>
  {/if}

  <!-- Log Viewer Panel -->
  {#if showLogPanel}
    <div class="log-panel" style="width: {logPanelWidth}px">
      <div class="resize-handle" onmousedown={startResize}></div>
      <div class="panel-header">
        <span>{currentLang === "zh" ? "日志查看" : "Log Viewer"}</span>
        <button class="panel-close" onclick={() => showLogPanel = false}>×</button>
      </div>
      <div class="log-content">
        <div class="log-file-list">
          <h4>{currentLang === "zh" ? "日志文件" : "Log Files"}</h4>
          {#each logFiles as file}
            <button
              class="log-file-item"
              class:active={selectedLogFile === file}
              onclick={() => { selectedLogFile = file; loadLogContent(); }}
            >
              {file}
            </button>
          {/each}
        </div>
        <div class="log-entries">
          {#each logContent as line}
            <div class="log-line">{line}</div>
          {/each}
        </div>
      </div>
    </div>
  {/if}

  <!-- Scan Report Modal -->
  {#if selectedScanReport}
    <div class="menu-overlay open" onclick={closeScanReport}></div>
    <div class="scan-report-modal">
      <div class="modal-header">
        <h3>{currentLang === "zh" ? "扫描报告详情" : "Scan Report Details"}</h3>
        <button class="panel-close" onclick={closeScanReport}>×</button>
      </div>
      <div class="modal-content">
        <div class="report-summary">
          <div class="report-info">
            <p><strong>{currentLang === "zh" ? "扫描时间" : "Scan Time"}：</strong>{selectedScanReport.timestamp}</p>
            <p><strong>{currentLang === "zh" ? "扫描目录" : "Scan Directory"}：</strong>{selectedScanReport.scannedDir}</p>
          </div>
          <div class="report-stats">
            <div class="stat-item">
              <span class="stat-value">{selectedScanReport.totalFiles.toLocaleString()}</span>
              <span class="stat-label">{currentLang === "zh" ? "扫描文件" : "Scanned Files"}</span>
            </div>
            <div class="stat-item clean">
              <span class="stat-value">{selectedScanReport.cleanCount.toLocaleString()}</span>
              <span class="stat-label">{t("clean")}</span>
            </div>
            <div class="stat-item" class:has-threats={selectedScanReport.threatCount > 0}>
              <span class="stat-value">{selectedScanReport.threatCount}</span>
              <span class="stat-label">{t("suspicious")}</span>
            </div>
            <div class="stat-item">
              <span class="stat-value">{(selectedScanReport.duration / 1000).toFixed(1)}s</span>
              <span class="stat-label">{t("scanTime")}</span>
            </div>
          </div>
        </div>
        {#if selectedScanReport.threats.length > 0}
          <div class="report-threats">
            <h4>{currentLang === "zh" ? "威胁详情" : "Threat Details"}</h4>
            {#each selectedScanReport.threats as threat}
              <div class="threat-item" class:high={threat.severity === "high"} class:medium={threat.severity === "medium"}>
                <div class="threat-info">
                  <span class="threat-path">{threat.path}</span>
                  <span class="threat-name">{threat.threat}</span>
                </div>
                <span class="severity-badge" class:high={threat.severity === "high"} class:medium={threat.severity === "medium"}>
                  {threat.severity === "high" ? t("highRisk") : threat.severity === "medium" ? t("mediumRisk") : t("lowRisk")}
                </span>
              </div>
            {/each}
          </div>
        {:else}
          <div class="no-threats">
            <p>{t("noThreats")}</p>
          </div>
        {/if}
      </div>
    </div>
  {/if}

  <!-- Search Mode UI -->
  {#if appMode === "search"}
  <header class="search-header" class:compact={hasSearched}>
    <div class="search-card" class:compact={hasSearched}>
      <div class="input-wrap">
        <input
          class="search-input"
          type="text"
          bind:value={query}
          onkeydown={(e) => e.key === "Enter" && doSearch()}
          placeholder={t("searchPlaceholder")}
          spellcheck="false"
        />
        {#if isLoading}
          <span class="loading-dot"></span>
        {/if}
      </div>
      <button class="search-btn" onclick={doSearch}>{t("search")}</button>
    </div>

    <div class="index-status-bar">
      {#if isIndexing}
        <span class="status-text indexing">{t("indexing")} · {indexedFiles.toLocaleString()} {t("files")}</span>
      {:else if indexedFiles > 0}
        <span class="status-text">{t("indexed")} {indexedFiles.toLocaleString()} {t("files")}{#if lastIndexed} · {t("lastUpdated")} {lastIndexed}{/if}</span>
        <button class="reindex-btn" onclick={startIndexing}>{t("reindex")}</button>
      {:else}
        <span class="status-text">{currentLang === "zh" ? "未索引" : "Not indexed"}</span>
        <button class="reindex-btn" onclick={startIndexing}>{t("startIndexing")}</button>
      {/if}
      <button class="rebuild-btn" onclick={startReindex}>{currentLang === "zh" ? "重建索引" : "Rebuild Index"}</button>
      {#if indexedDirs.length > 0}
        <button class="dirs-toggle" onclick={() => (showDirs = !showDirs)}>
          {showDirs ? (currentLang === "zh" ? "收起目录" : "Hide Dirs") : (currentLang === "zh" ? "查看目录" : "Show Dirs")}
        </button>
      {/if}
    </div>

    {#if showDirs && indexedDirs.length > 0}
      <div class="dirs-panel">
        {#each indexedDirs as dir}
          <div class="dir-item" class:indexed={dir.is_indexed}>
            <span class="dir-name">{dir.name}</span>
            <span class="dir-status">
              {#if dir.is_indexed}
                <span class="badge success">{t("indexed")}</span>
                <span class="dir-count">{dir.file_count.toLocaleString()} {t("files")}</span>
              {:else if dir.is_indexing}
                <span class="badge indexing">{t("indexing")}</span>
              {:else}
                <span class="badge">{currentLang === "zh" ? "未索引" : "Not indexed"}</span>
              {/if}
            </span>
          </div>
        {/each}
      </div>
    {/if}

    {#if hasSearched}
      <div class="meta-row">
        <div class="left-meta"></div>
        <div class="right-meta">
          {total.toLocaleString()} {currentLang === "zh" ? "条结果" : "results"} · {searchTime.toFixed(1)}ms
        </div>
      </div>
    {/if}

    {#if isIndexing}
      <div class="index-progress-bar">
        <div class="index-progress-track">
          <div
            class="index-progress-fill"
            style="width: {Math.min(100, (indexedFiles / 100000) * 100)}%"
          ></div>
        </div>
        <div class="index-progress-text">
          {currentLang === "zh" ? "正在索引文件，已找到" : "Indexing files, found"} {indexedFiles.toLocaleString()} {currentLang === "zh" ? "个文件..." : "files..."}
        </div>
      </div>
    {/if}

    {#if indexErrors && indexErrors.total_errors > 0}
      <div class="error-panel">
        <div class="error-header">
          <span class="error-title">⚠️ {currentLang === "zh" ? "索引警告" : "Index Warning"}</span>
          <span class="error-count"
            >{indexErrors.total_errors} {currentLang === "zh" ? "个路径因权限不足未能索引" : "paths could not be indexed due to permission"}</span
          >
          <button
            class="error-toggle"
            onclick={() => (showErrorPanel = !showErrorPanel)}
          >
            {showErrorPanel ? (currentLang === "zh" ? "收起" : "Hide") : (currentLang === "zh" ? "查看详情" : "Show Details")}
          </button>
        </div>
        {#if showErrorPanel}
          <div class="error-body">
            {#if isMacOS}
              <div class="fda-hint">
                <strong>💡 macOS Full Disk Access Tip:</strong>
                {currentLang === "zh" ? "在 macOS 上，即使应用已有完全磁盘访问权限，某些系统路径（如 /System、/Library）仍受 SIP 保护，无法被索引。这些路径的文件不会被搜索到，但不影响正常使用。" : "On macOS, even with Full Disk Access, some system paths (like /System, /Library) are protected by SIP and cannot be indexed. Files in these paths will not appear in search results, but this is normal behavior."}
              </div>
            {/if}
            {#if indexErrors.permission_denied.length > 0}
              <div class="error-section">
                <div class="error-section-title">
                  {currentLang === "zh" ? "权限不足的路径" : "Paths with permission denied"}（{indexErrors.permission_denied.length}）：
                </div>
                <ul class="error-list">
                  {#each indexErrors.permission_denied.slice(0, 20) as path}
                    <li class="error-item">{path}</li>
                  {/each}
                  {#if indexErrors.permission_denied.length > 20}
                    <li class="error-more">
                      ... {currentLang === "zh" ? "还有" : "and"} {indexErrors.permission_denied.length - 20} {currentLang === "zh" ? "个路径" : "more paths"}
                    </li>
                  {/if}
                </ul>
              </div>
            {/if}
            {#if indexErrors.other_errors.length > 0}
              <div class="error-section">
                <div class="error-section-title">
                  {currentLang === "zh" ? "其他错误" : "Other errors"}（{indexErrors.other_errors.length}）：
                </div>
                <ul class="error-list">
                  {#each indexErrors.other_errors.slice(0, 10) as error}
                    <li class="error-item">
                      <span class="error-path">{error.path}</span>
                      <span class="error-msg">({error.message})</span>
                    </li>
                  {/each}
                  {#if indexErrors.other_errors.length > 10}
                    <li class="error-more">
                      ... {currentLang === "zh" ? "还有" : "and"} {indexErrors.other_errors.length - 10} {currentLang === "zh" ? "个错误" : "more errors"}
                    </li>
                  {/if}
                </ul>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </header>

  <main class="content">
    {#if !hasSearched}
      <section class="center-hint">
        <h2>{currentLang === "zh" ? "快速搜索本地文件" : "Quick Local File Search"}</h2>
        <p>{currentLang === "zh" ? "输入文件名、路径片段或关键词，结果会按行展示。" : "Enter filename, path, or keyword to search. Results are displayed in rows."}</p>
        <div class="quick-tags">
          {#each quickKeywords as word}
            <button class="tag" onclick={() => fillQuickKeyword(word)}
              >{word}</button
            >
          {/each}
        </div>
      </section>
    {:else}
      <section class="search-layout">
        <section class="list-panel">
          <div class="list-header">
            <span class="col-name">{t("fileName")}</span>
            <span class="col-size">{t("fileSize")}</span>
            <span class="col-created">{t("created")}</span>
            <span class="col-action">{currentLang === "zh" ? "详情" : "Details"}</span>
          </div>

          {#if isLoading}
            <div class="state">{currentLang === "zh" ? "正在搜索..." : "Searching..."}</div>
          {:else if results.length === 0}
            <div class="state">{currentLang === "zh" ? "未找到匹配文件，请尝试更换关键词。" : "No matching files found. Try different keywords."}</div>
          {:else}
            <div class="list-body">
              {#each results as entry}
                <button
                  class="row"
                  class:selected={selectedFile?.path === entry.path}
                  onclick={() => selectFile(entry)}
                >
                  <span class="col-name" title={entry.filename}
                    >{entry.filename}</span
                  >
                  <span class="col-size"
                    >{entry.is_directory ? "-" : formatSize(entry.size)}</span
                  >
                  <span class="col-created">{formatDate(entry.created)}</span>
                  <span class="col-action">{currentLang === "zh" ? "查看详情" : "View"}</span>
                </button>
              {/each}
            </div>
          {/if}
        </section>

        {#if selectedFile}
          <aside class="detail-drawer">
            <div class="drawer-head">
              <h3>{currentLang === "zh" ? "文件详情" : "File Details"}</h3>
              <button class="close-btn" onclick={closeDetail}>{t("close")}</button>
            </div>

            <div class="drawer-body">
              <div class="info-group">
                <div class="info-item">
                  <span>{t("fileName")}</span><strong>{selectedFile.filename}</strong>
                </div>
                <div class="info-item">
                  <span>{t("filePath")}</span><strong title={selectedFile.path}
                    >{selectedFile.path}</strong
                  >
                </div>
                <div class="info-item">
                  <span>{t("modified")}</span><strong
                    >{formatDate(selectedFile.modified)}</strong
                  >
                </div>
                <div class="info-item">
                  <span>{currentLang === "zh" ? "文件类型" : "File Type"}</span><strong
                    >{getFileType(selectedFile)}</strong
                  >
                </div>
              </div>

              {#if isAnalyzingFile}
                <div class="security-loading">
                  <div class="analyze-spinner"></div>
                  <p>{currentLang === "zh" ? "正在分析文件安全..." : "Analyzing file security..."}</p>
                </div>
              {:else if fileAnalysis}
                <div
                  class="security-box"
                  class:high={fileAnalysis.level === "high"}
                  class:medium={fileAnalysis.level === "medium"}
                >
                  <h4>{currentLang === "zh" ? "安全分析结果" : "Security Analysis"}</h4>
                  <p>{fileAnalysis.conclusion}</p>
                  <ul>
                    {#each fileAnalysis.reasons as reason}
                      <li>{reason}</li>
                    {/each}
                  </ul>
                  <div class="risk-line">
                    <span
                      >{currentLang === "zh" ? "风险等级" : "Risk Level"}：{fileAnalysis.level === "high"
                        ? (currentLang === "zh" ? "高风险" : "High")
                        : fileAnalysis.level === "medium"
                          ? (currentLang === "zh" ? "中风险" : "Medium")
                          : (currentLang === "zh" ? "低风险" : "Low")}</span
                    >
                    <span
                      >{currentLang === "zh" ? "木马倾向" : "Trojan Risk"}：{fileAnalysis.hasTrojanRisk
                        ? (currentLang === "zh" ? "疑似存在" : "Suspected")
                        : (currentLang === "zh" ? "未发现明显迹象" : "No obvious signs")}</span
                    >
                  </div>
                </div>
              {:else}
                <div class="security-box">
                  <p>{currentLang === "zh" ? "暂无" : "None"}</p>
                </div>
              {/if}
            </div>

            <div class="drawer-actions">
              <button
                class="action primary"
                onclick={() => scanSelectedFile()}
                >{currentLang === "zh" ? "安全扫描" : "Security Scan"}</button
              >
              <button
                class="action"
                onclick={() => selectedFile && openFolder(selectedFile.path)}
                >{t("openFolder")}</button
              >
              <button
                class="action danger"
                onclick={() => selectedFile && deleteFile(selectedFile.path)}
                >{currentLang === "zh" ? "删除文件" : "Delete File"}</button
              >
            </div>
          </aside>
        {/if}
      </section>
    {/if}
  </main>
  {/if}
</div>

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
      "Helvetica Neue", Arial, sans-serif;
    background: linear-gradient(180deg, #f7f9fc 0%, #f3f6fb 100%);
    color: #1e2432;
  }

  /* Menu Bar */
  .menu-bar {
    display: flex;
    gap: 4px;
    padding: 8px 16px;
    background: rgba(255, 255, 255, 0.9);
    border-bottom: 1px solid #e3e9f4;
  }

  .menu-item {
    padding: 6px 16px;
    border: none;
    background: transparent;
    color: #5d6a82;
    cursor: pointer;
    border-radius: 6px;
    font-size: 13px;
    transition: all 0.15s ease;
  }

  .mode-switcher {
    display: flex;
    gap: 2px;
    background: #e3e9f4;
    padding: 3px;
    border-radius: 8px;
  }

  .mode-btn {
    padding: 5px 14px;
    border: none;
    background: transparent;
    color: #5d6a82;
    cursor: pointer;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 500;
    transition: all 0.15s ease;
  }

  .mode-btn:hover {
    color: #4a72ff;
  }

  .mode-btn.active {
    background: white;
    color: #4a72ff;
    box-shadow: 0 1px 3px rgba(0,0,0,0.1);
  }

  .lang-toggle {
    padding: 4px 10px;
    border: 1px solid #ddd;
    background: white;
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
    font-weight: 600;
    color: #4a72ff;
    margin-right: 8px;
  }

  .lang-toggle:hover {
    background: #4a72ff;
    color: white;
    border-color: #4a72ff;
  }

  .menu-spacer {
    flex: 1;
  }

  .menu-item:hover {
    background: #f0f4ff;
    color: #4a72ff;
  }

  .menu-item.active {
    background: #4a72ff;
    color: white;
  }

  .menu-panel {
    position: fixed;
    top: 0;
    right: 0;
    width: 320px;
    height: 100vh;
    background: white;
    box-shadow: -2px 0 20px rgba(0, 0, 0, 0.15);
    padding: 20px;
    overflow-y: auto;
    z-index: 1000;
    transform: translateX(100%);
    transition: transform 0.3s ease;
  }

  .menu-panel.open {
    transform: translateX(0);
  }

  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
    font-weight: 600;
    color: #1f2636;
    padding-bottom: 12px;
    border-bottom: 1px solid #e3e9f4;
  }

  .panel-close {
    border: none;
    background: transparent;
    color: #9ca3af;
    cursor: pointer;
    font-size: 20px;
    padding: 4px 8px;
    line-height: 1;
  }

  .panel-close:hover {
    color: #1f2636;
  }

  /* Overlay behind menu panel */
  .menu-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.3);
    z-index: 999;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.3s ease;
  }

  .menu-overlay.open {
    opacity: 1;
    pointer-events: auto;
  }

  /* Scan History Panel */
  .scan-history-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .scan-history-item {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px;
    background: #f7f9fc;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    text-align: left;
    width: 100%;
  }

  .scan-history-item:hover {
    background: #eef1f7;
  }

  .scan-history-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .scan-history-time {
    font-size: 13px;
    font-weight: 500;
    color: #1f2636;
  }

  .scan-history-dir {
    font-size: 11px;
    color: #5d6a82;
    word-break: break-all;
  }

  .scan-history-stats {
    display: flex;
    gap: 12px;
  }

  .scan-stat {
    font-size: 11px;
    color: #5d6a82;
  }

  .scan-stat.threats.has-threats {
    color: #ef4444;
    font-weight: 600;
  }

  /* Log Panel */
  .resize-handle {
    position: absolute;
    top: 0;
    left: 0;
    width: 6px;
    height: 100%;
    cursor: ew-resize;
    z-index: 1003;
    background: transparent;
  }

  .resize-handle:hover {
    background: #4a72ff;
  }

  .log-btn {
    padding: 4px 12px;
    border: 1px solid #ddd;
    background: white;
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
  }

  .log-btn:hover {
    background: #f5f5f5;
  }

  .log-panel {
    position: fixed;
    top: 0;
    right: 0;
    width: 500px;
    height: 100vh;
    background: white;
    box-shadow: -4px 0 20px rgba(0, 0, 0, 0.15);
    z-index: 1002;
    display: flex;
    flex-direction: column;
  }

  .log-panel .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid #eee;
  }

  .log-content {
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  .log-file-list {
    width: 150px;
    border-right: 1px solid #eee;
    padding: 8px;
    overflow-y: auto;
  }

  .log-file-list h4 {
    font-size: 12px;
    color: #666;
    margin-bottom: 8px;
  }

  .log-file-item {
    display: block;
    width: 100%;
    padding: 6px 8px;
    margin-bottom: 4px;
    border: none;
    background: transparent;
    text-align: left;
    font-size: 11px;
    border-radius: 4px;
    cursor: pointer;
  }

  .log-file-item:hover {
    background: #f5f5f5;
  }

  .log-file-item.active {
    background: #4a72ff;
    color: white;
  }

  .log-entries {
    flex: 1;
    padding: 8px;
    overflow-y: auto;
    font-family: monospace;
    font-size: 11px;
    background: #1e1e1e;
    color: #d4d4d4;
  }

  .log-line {
    padding: 2px 0;
    white-space: pre-wrap;
    word-break: break-all;
  }

  /* Scan Report Modal */
  .scan-report-modal {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 90%;
    max-width: 600px;
    max-height: 80vh;
    background: white;
    border-radius: 12px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3);
    z-index: 1001;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 16px 20px;
    border-bottom: 1px solid #e3e9f4;
  }

  .modal-header h3 {
    margin: 0;
    font-size: 16px;
    color: #1f2636;
  }

  .modal-content {
    padding: 20px;
    overflow-y: auto;
  }

  .report-summary {
    margin-bottom: 20px;
  }

  .report-info p {
    margin: 4px 0;
    font-size: 13px;
    color: #5d6a82;
  }

  .report-stats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 12px;
    margin-top: 16px;
    padding: 12px;
    background: #f7f9fc;
    border-radius: 8px;
  }

  .report-stats .stat-item {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  .report-stats .stat-value {
    font-size: 18px;
    font-weight: 700;
    color: #1f2636;
  }

  .report-stats .stat-item.clean .stat-value {
    color: #22c55e;
  }

  .report-stats .stat-item.has-threats .stat-value {
    color: #ef4444;
  }

  .report-stats .stat-label {
    font-size: 11px;
    color: #5d6a82;
    margin-top: 2px;
  }

  .report-threats h4 {
    margin: 0 0 12px 0;
    font-size: 14px;
    color: #1f2636;
  }

  .no-threats {
    text-align: center;
    padding: 20px;
    color: #22c55e;
    font-size: 14px;
  }

  .clear-btn {
    border: none;
    background: transparent;
    color: #9ca3af;
    cursor: pointer;
    font-size: 12px;
  }

  .clear-btn:hover {
    color: #ef4444;
  }

  .empty-state {
    color: #9ca3af;
    font-size: 13px;
    text-align: center;
    padding: 20px;
  }

  .history-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .history-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border: none;
    background: #f7f9fc;
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s ease;
  }

  .history-item:hover {
    background: #eef2ff;
  }

  .history-icon {
    font-size: 14px;
    opacity: 0.6;
  }

  .history-text {
    color: #1f2636;
    font-size: 13px;
  }

  .settings-section {
    padding: 0 4px;
  }

  .settings-desc {
    color: #5d6a82;
    font-size: 12px;
    margin: 0 0 12px 0;
  }

  .ignored-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 12px;
  }

  .ignored-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    background: #f7f9fc;
    border-radius: 6px;
  }

  .ignored-path {
    color: #1f2636;
    font-size: 13px;
    font-family: monospace;
  }

  .remove-btn {
    border: none;
    background: transparent;
    color: #ef4444;
    cursor: pointer;
    font-size: 12px;
    padding: 4px 8px;
  }

  .remove-btn:hover {
    text-decoration: underline;
  }

  .add-ignored {
    display: flex;
    gap: 8px;
  }

  .ignored-input {
    flex: 1;
    padding: 8px 12px;
    border: 1px solid #e3e9f4;
    border-radius: 6px;
    font-size: 13px;
    font-family: monospace;
  }

  .ignored-input:focus {
    outline: none;
    border-color: #4a72ff;
  }

  .add-btn {
    padding: 8px 16px;
    border: none;
    background: #4a72ff;
    color: white;
    border-radius: 6px;
    cursor: pointer;
    font-size: 13px;
  }

  .add-btn:hover {
    background: #3b5ce4;
  }

  /* Overview Panel */
  .overview-panel {
    background: white;
    border-bottom: 1px solid #e3e9f4;
    padding: 24px;
    max-height: calc(100vh - 120px);
    overflow-y: auto;
  }

  .overview-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 20px;
  }

  .overview-header h2 {
    margin: 0;
    color: #1f2636;
    font-size: 18px;
  }

  .refresh-btn {
    padding: 6px 16px;
    border: 1px solid #ddd;
    background: white;
    border-radius: 6px;
    cursor: pointer;
    font-size: 13px;
  }

  .refresh-btn:hover {
    background: #f5f5f5;
  }

  .overview-section {
    margin-bottom: 24px;
  }

  .overview-section h3 {
    color: #1f2636;
    font-size: 14px;
    margin: 0 0 12px 0;
  }

  .disk-stats {
    display: flex;
    gap: 32px;
    margin-bottom: 12px;
  }

  .disk-item {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .disk-label {
    font-size: 12px;
    color: #666;
  }

  .disk-value {
    font-size: 16px;
    font-weight: 600;
    color: #1f2636;
  }

  .disk-bar {
    height: 8px;
    background: #e3e9f4;
    border-radius: 4px;
    overflow: hidden;
    margin-bottom: 16px;
  }

  .disk-used {
    height: 100%;
    background: linear-gradient(135deg, #4a72ff 0%, #3b5ce4 100%);
    border-radius: 4px;
    transition: width 0.3s ease;
  }

  .stat-item {
    display: flex;
    justify-content: space-between;
    padding: 8px 0;
    border-bottom: 1px solid #eee;
  }

  .stat-label {
    font-size: 13px;
    color: #5d6a82;
  }

  .stat-value {
    font-size: 14px;
    font-weight: 600;
    color: #1f2636;
  }

  .ext-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .ext-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 12px;
    background: #f8f9fb;
    border-radius: 6px;
  }

  .ext-info {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .ext-name {
    font-family: monospace;
    font-size: 13px;
    font-weight: 600;
    color: #4a72ff;
    background: #eef2ff;
    padding: 2px 8px;
    border-radius: 4px;
  }

  .ext-desc {
    font-size: 12px;
    color: #666;
  }

  .ext-count {
    font-size: 14px;
    font-weight: 600;
    color: #1f2636;
  }

  .overview-footer {
    text-align: right;
    font-size: 11px;
    color: #999;
    margin-top: 16px;
    padding-top: 12px;
    border-top: 1px solid #eee;
  }

  .overview-empty {
    text-align: center;
    padding: 60px 20px;
    color: #666;
  }

  .overview-empty p {
    margin: 8px 0;
  }

  .overview-empty .hint {
    font-size: 13px;
    color: #999;
  }

  .overview-loading {
    text-align: center;
    padding: 40px 20px;
  }

  .overview-loading .loading-bar {
    height: 4px;
    background: #e0e8f5;
    border-radius: 2px;
    overflow: hidden;
    margin-bottom: 16px;
  }

  .overview-loading .loading-bar-fill {
    height: 100%;
    background: linear-gradient(90deg, #4a72ff 0%, #6b8fff 100%);
    border-radius: 2px;
    animation: loading-progress 1.5s ease-in-out infinite;
  }

  @keyframes loading-progress {
    0% { width: 0%; }
    50% { width: 70%; }
    100% { width: 100%; }
  }

  .overview-loading p {
    color: #666;
    font-size: 14px;
    margin: 0;
  }

  .refresh-spinner {
    display: inline-block;
    width: 14px;
    height: 14px;
    border: 2px solid #e0e8f5;
    border-top-color: #4a72ff;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    margin-right: 6px;
    vertical-align: middle;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .refresh-btn:disabled {
    opacity: 0.7;
    cursor: not-allowed;
  }

  /* Security Panel */
  .security-panel {
    background: white;
    border-bottom: 1px solid #e3e9f4;
    padding: 24px;
    max-height: calc(100vh - 120px);
    overflow-y: auto;
  }

  .security-header {
    margin-bottom: 20px;
  }

  .security-header h2 {
    margin: 0 0 4px 0;
    color: #1f2636;
    font-size: 18px;
  }

  .security-desc {
    margin: 0;
    color: #5d6a82;
    font-size: 13px;
  }

  /* Directory Selector */
  .scan-dir-selector {
    background: #f7f9fc;
    border-radius: 8px;
    padding: 16px;
    margin-bottom: 16px;
  }

  .dir-input-group {
    display: flex;
    gap: 8px;
  }

  .dir-input {
    flex: 1;
    padding: 10px 12px;
    border: 1px solid #e3e9f4;
    border-radius: 6px;
    font-size: 13px;
    color: #1f2636;
    background: white;
  }

  .dir-input:focus {
    outline: none;
    border-color: #3b5ce4;
  }

  .dir-btn {
    padding: 10px 16px;
    background: #3b5ce4;
    color: white;
    border: none;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
  }

  .dir-btn:hover {
    background: #2d4ae0;
  }

  .dir-selected {
    margin-top: 10px;
    font-size: 12px;
    color: #5d6a82;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .dir-path {
    color: #1f2636;
    font-family: monospace;
    word-break: break-all;
  }

  .dir-clear {
    padding: 4px 8px;
    background: transparent;
    border: 1px solid #e3e9f4;
    border-radius: 4px;
    font-size: 11px;
    color: #5d6a82;
    cursor: pointer;
  }

  .dir-clear:hover {
    background: #fee2e2;
    border-color: #ef4444;
    color: #ef4444;
  }

  .security-start {
    display: flex;
    justify-content: center;
    padding: 20px;
  }

  .scan-btn {
    padding: 12px 32px;
    border: none;
    background: linear-gradient(135deg, #4a72ff 0%, #3b5ce4 100%);
    color: white;
    border-radius: 8px;
    cursor: pointer;
    font-size: 14px;
    font-weight: 600;
    box-shadow: 0 4px 12px rgba(74, 114, 255, 0.3);
    transition: all 0.2s ease;
  }

  .scan-btn:hover {
    transform: translateY(-1px);
    box-shadow: 0 6px 16px rgba(74, 114, 255, 0.4);
  }

  .scan-btn-circle {
    width: 120px;
    height: 120px;
    border-radius: 50%;
    border: none;
    background: linear-gradient(135deg, #4a72ff 0%, #3b5ce4 100%);
    color: white;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    box-shadow: 0 6px 24px rgba(74, 114, 255, 0.4);
    transition: all 0.2s ease;
  }

  .scan-btn-circle:hover {
    transform: scale(1.05);
    box-shadow: 0 8px 32px rgba(74, 114, 255, 0.5);
  }

  .scan-btn-circle .scan-icon {
    font-size: 28px;
  }

  .scan-btn-circle .scan-text {
    font-size: 14px;
    font-weight: 600;
  }

  .stop-btn-center {
    display: flex;
    justify-content: center;
    margin-top: 20px;
  }

  .stop-btn-circle {
    width: 100px;
    height: 100px;
    border-radius: 50%;
    border: none;
    background: linear-gradient(135deg, #ff4a4a 0%, #e43b3b 100%);
    color: white;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    box-shadow: 0 4px 16px rgba(255, 74, 74, 0.4);
    transition: all 0.2s ease;
  }

  .stop-btn-circle:hover {
    transform: scale(1.05);
    box-shadow: 0 6px 24px rgba(255, 74, 74, 0.5);
  }

  .stop-btn-circle .scan-icon {
    font-size: 24px;
  }

  .stop-btn-circle .scan-text {
    font-size: 12px;
    font-weight: 600;
  }

  .scan-progress {
    margin-bottom: 16px;
  }

  .progress-bar {
    height: 8px;
    background: #e3e9f4;
    border-radius: 4px;
    overflow: hidden;
    margin-bottom: 8px;
  }

  .progress-fill {
    height: 100%;
    background: linear-gradient(90deg, #4a72ff 0%, #6b8fff 100%);
    border-radius: 4px;
    transition: width 0.3s ease;
  }

  .progress-info {
    display: flex;
    justify-content: space-between;
    color: #5d6a82;
    font-size: 12px;
  }

  .scan-results {
    background: #f7f9fc;
    border-radius: 8px;
    padding: 16px;
    max-height: 400px;
    overflow-y: auto;
  }

  .results-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 12px;
  }

  .results-header h3 {
    margin: 0;
    color: #1f2636;
    font-size: 14px;
  }

  .threat-count {
    color: #ef4444;
    font-size: 12px;
    font-weight: 600;
  }

  .clear-results-btn {
    margin-left: auto;
    padding: 4px 12px;
    background: #f1f5f9;
    border: 1px solid #e3e9f4;
    border-radius: 4px;
    font-size: 11px;
    color: #5d6a82;
    cursor: pointer;
  }

  .clear-results-btn:hover {
    background: #fee2e2;
    border-color: #ef4444;
    color: #ef4444;
  }

  .scan-stats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 12px;
    margin-bottom: 16px;
    padding: 12px;
    background: white;
    border-radius: 8px;
  }

  .stat-item {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  .stat-value {
    font-size: 20px;
    font-weight: 700;
    color: #1f2636;
  }

  .stat-item.clean .stat-value {
    color: #22c55e;
  }

  .stat-item.has-threats .stat-value {
    color: #ef4444;
  }

  .stat-label {
    font-size: 11px;
    color: #5d6a82;
    margin-top: 2px;
  }

  .severity-breakdown {
    display: flex;
    gap: 12px;
    margin-bottom: 12px;
    padding: 8px 12px;
    background: #f1f5f9;
    border-radius: 6px;
  }

  .breakdown-item {
    font-size: 12px;
    font-weight: 500;
  }

  .breakdown-item.high {
    color: #ef4444;
  }

  .breakdown-item.medium {
    color: #f59e0b;
  }

  .breakdown-item.low {
    color: #22c55e;
  }

  .threat-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 12px;
    background: white;
    border-radius: 6px;
    margin-bottom: 8px;
    border-left: 3px solid #fbbf24;
  }

  .threat-item.high {
    border-left-color: #ef4444;
  }

  .threat-item.medium {
    border-left-color: #fbbf24;
  }

  .threat-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .threat-path {
    color: #1f2636;
    font-size: 13px;
    font-family: monospace;
  }

  .threat-name {
    color: #5d6a82;
    font-size: 11px;
  }

  .severity-badge {
    padding: 4px 10px;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 600;
    background: #fef3c7;
    color: #92400e;
  }

  .severity-badge.high {
    background: #fee2e2;
    color: #991b1b;
  }

  .scan-actions {
    display: flex;
    justify-content: center;
    margin-top: 16px;
  }

  .rescan-btn {
    padding: 8px 20px;
    border: 1px solid #e3e9f4;
    background: white;
    color: #5d6a82;
    border-radius: 6px;
    cursor: pointer;
    font-size: 13px;
  }

  .rescan-btn:hover {
    border-color: #4a72ff;
    color: #4a72ff;
  }

  .help-content {
    padding: 0 4px;
  }

  .help-content h3 {
    margin: 0 0 4px 0;
    color: #1f2636;
    font-size: 16px;
  }

  .version {
    color: #9ca3af;
    font-size: 12px;
    margin: 0 0 16px 0;
  }

  .description {
    color: #5d6a82;
    font-size: 13px;
    line-height: 1.6;
    margin: 0 0 16px 0;
  }

  .features, .team-info {
    margin-bottom: 16px;
  }

  .features h4, .team-info h4 {
    color: #1f2636;
    font-size: 13px;
    margin: 0 0 8px 0;
  }

  .features ul, .team-info ul {
    margin: 0;
    padding-left: 20px;
    color: #5d6a82;
    font-size: 12px;
    line-height: 1.8;
  }

  .team-info p {
    color: #5d6a82;
    font-size: 13px;
    line-height: 1.6;
    margin: 0;
  }

  .page {
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .search-header {
    width: min(980px, calc(100vw - 48px));
    margin: 0 auto;
    padding-top: 27vh;
    transition: padding-top 0.24s ease;
  }

  .search-header.compact {
    padding-top: 18px;
  }

  .search-card {
    display: grid;
    grid-template-columns: 1fr 88px;
    gap: 12px;
    background: rgba(255, 255, 255, 0.9);
    border: 1px solid #e3e9f4;
    border-radius: 16px;
    padding: 10px;
    box-shadow: 0 8px 30px rgba(24, 39, 75, 0.08);
    backdrop-filter: blur(8px);
  }

  .input-wrap {
    position: relative;
    padding-right: 60px;
  }

  .search-input {
    width: 100%;
    height: 42px;
    border: 1px solid #d7e0ef;
    border-radius: 10px;
    padding: 0 38px 0 14px;
    font-size: 15px;
    outline: none;
    transition: border-color 0.2s;
  }

  .search-input:focus {
    border-color: #4a72ff;
    box-shadow: 0 0 0 3px rgba(74, 114, 255, 0.12);
  }

  .loading-dot {
    position: absolute;
    right: 12px;
    top: 50%;
    transform: translateY(-50%);
    width: 14px;
    height: 14px;
    border: 2px solid #d4def2;
    border-top-color: #4a72ff;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .search-btn {
    height: 42px;
    border: none;
    border-radius: 10px;
    background: linear-gradient(135deg, #4d77ff 0%, #4268e8 100%);
    color: #fff;
    font-size: 14px;
    cursor: pointer;
  }

  .search-btn:hover {
    filter: brightness(0.98);
  }

  .meta-row {
    margin-top: 10px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: #5d6a82;
    font-size: 13px;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.2s;
  }

  .meta-row.show {
    opacity: 1;
    pointer-events: auto;
  }

  .link-btn {
    margin-left: 10px;
    border: none;
    background: transparent;
    color: #4a72ff;
    cursor: pointer;
  }

  .reindex-btn {
    margin-left: 8px;
    border: none;
    background: transparent;
    color: #9ca3af;
    cursor: pointer;
    font-size: 12px;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .reindex-btn:hover {
    color: #6b7280;
  }

  .index-status-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    font-size: 12px;
    color: #5d6a82;
  }

  .status-text {
    color: #5d6a82;
  }

  .status-text.indexing {
    color: #4a72ff;
  }

  .dirs-toggle {
    margin-left: 8px;
    border: none;
    background: transparent;
    color: #4a72ff;
    cursor: pointer;
    font-size: 12px;
  }

  .dirs-toggle:hover {
    text-decoration: underline;
  }

  .rebuild-btn {
    margin-left: 8px;
    border: 1px solid #e3e9f4;
    background: #f7f9ff;
    color: #4a72ff;
    cursor: pointer;
    font-size: 12px;
    padding: 4px 12px;
    border-radius: 6px;
  }

  .rebuild-btn:hover {
    background: #eef2ff;
    border-color: #c7d2fe;
  }

  .dirs-panel {
    margin-top: 10px;
    background: rgba(255, 255, 255, 0.9);
    border: 1px solid #e3e9f4;
    border-radius: 10px;
    padding: 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .dir-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 8px;
    border-radius: 6px;
  }

  .dir-item:hover {
    background: #f4f8ff;
  }

  .dir-item.indexed {
    background: #f7faff;
  }

  .dir-name {
    font-size: 13px;
    color: #1f2636;
    font-weight: 500;
  }

  .dir-status {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .badge {
    font-size: 11px;
    padding: 2px 6px;
    border-radius: 4px;
    background: #e8eef9;
    color: #5d6a82;
  }

  .badge.success {
    background: #d4edda;
    color: #155724;
  }

  .badge.indexing {
    background: #cce5ff;
    color: #004085;
  }

  .dir-count {
    font-size: 11px;
    color: #9ca3af;
  }

  .index-progress-bar {
    margin-top: 12px;
    padding: 10px 14px;
    background: #f0f7ff;
    border: 1px solid #c4d9f8;
    border-radius: 10px;
  }

  .index-progress-track {
    height: 6px;
    background: #e0e8f5;
    border-radius: 3px;
    overflow: hidden;
  }

  .index-progress-fill {
    height: 100%;
    background: linear-gradient(90deg, #4a72ff 0%, #6b8fff 100%);
    border-radius: 3px;
    transition: width 0.3s ease;
  }

  .index-progress-text {
    margin-top: 6px;
    font-size: 12px;
    color: #4a72ff;
  }

  .error-panel {
    margin-top: 12px;
    background: #fff8f0;
    border: 1px solid #ffd49a;
    border-radius: 10px;
    overflow: hidden;
  }

  .error-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    background: #fff3e6;
    border-bottom: 1px solid #ffe4cc;
  }

  .error-title {
    font-weight: 600;
    color: #8b5a2b;
  }

  .error-count {
    flex: 1;
    font-size: 13px;
    color: #a67c52;
  }

  .error-toggle {
    border: none;
    background: transparent;
    color: #4a72ff;
    cursor: pointer;
    font-size: 12px;
  }

  .error-body {
    padding: 12px 14px;
  }

  .fda-hint {
    background: #e8f4ff;
    border: 1px solid #b8d4f0;
    border-radius: 8px;
    padding: 10px 12px;
    font-size: 12px;
    color: #3a5a7c;
    margin-bottom: 10px;
  }

  .error-section {
    margin-top: 8px;
  }

  .error-section-title {
    font-size: 12px;
    color: #8b5a2b;
    margin-bottom: 4px;
  }

  .error-list {
    margin: 0;
    padding-left: 20px;
    font-size: 11px;
    color: #a67c52;
  }

  .error-item {
    margin: 2px 0;
    word-break: break-all;
  }

  .error-path {
    font-family: monospace;
  }

  .error-msg {
    color: #c4885a;
  }

  .error-more {
    font-style: italic;
    color: #c4885a;
  }

  .content {
    width: min(1220px, calc(100vw - 48px));
    margin: 14px auto 20px;
    flex: 1;
    min-height: 0;
    position: relative;
  }

  .search-layout {
    height: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 360px;
    gap: 16px;
    width: 100%;
  }

  .search-layout .list-panel {
    min-width: 0;
  }

  .center-hint {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-direction: column;
    gap: 10px;
    color: #627089;
  }

  .center-hint h2 {
    margin: 0;
    color: #1e2433;
    font-size: 30px;
    font-weight: 650;
    letter-spacing: 0.2px;
  }

  .center-hint p {
    margin: 0;
    font-size: 14px;
  }

  .quick-tags {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  .tag {
    border: 1px solid #d7dfef;
    border-radius: 999px;
    background: #fff;
    color: #4f5d78;
    padding: 6px 12px;
    cursor: pointer;
  }

  .list-panel {
    height: 100%;
    width: 100%;
    border: 1px solid #e3e9f4;
    background: rgba(255, 255, 255, 0.88);
    border-radius: 14px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    box-shadow: 0 10px 28px rgba(24, 39, 75, 0.08);
    backdrop-filter: blur(8px);
  }

  .list-header,
  .row {
    display: grid;
    grid-template-columns: 1fr 140px 210px 100px;
    align-items: center;
    gap: 10px;
    padding: 0 14px;
    width: 100%;
    box-sizing: border-box;
  }

  .list-header {
    height: 44px;
    background: #f4f7fd;
    border-bottom: 1px solid #e7edf8;
    font-size: 12px;
    color: #67758f;
  }

  .list-body {
    flex: 1;
    overflow: auto;
    width: 100%;
  }

  .row {
    height: 48px;
    border: none;
    border-bottom: 1px solid #edf1f8;
    background: transparent;
    text-align: left;
    cursor: pointer;
    color: #1f2636;
    font-size: 13px;
  }

  .row:hover {
    background: #f4f8ff;
  }

  .row.selected {
    background: #eaf1ff;
  }

  .col-name,
  .col-created {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .col-size,
  .col-action {
    color: #5a6782;
    font-size: 12px;
  }

  .state {
    padding: 30px;
    text-align: center;
    color: #68758f;
  }

  .detail-drawer {
    border: 1px solid #e3e9f4;
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.9);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: 0 12px 30px rgba(24, 39, 75, 0.1);
    min-height: 0;
    backdrop-filter: blur(8px);
  }

  .drawer-head {
    height: 54px;
    padding: 0 14px;
    border-bottom: 1px solid #ebf0fa;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .drawer-head h3 {
    margin: 0;
    font-size: 15px;
  }

  .close-btn {
    border: none;
    background: #f0f4ff;
    color: #485878;
    border-radius: 8px;
    height: 30px;
    padding: 0 10px;
    cursor: pointer;
  }

  .drawer-body {
    flex: 1;
    overflow: auto;
    padding: 14px;
  }

  .info-group {
    border: 1px solid #e8eef9;
    border-radius: 10px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .info-item {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .info-item span {
    font-size: 12px;
    color: #69758d;
  }

  .info-item strong {
    font-size: 13px;
    color: #1f2839;
    word-break: break-all;
    overflow-wrap: break-word;
  }

  .security-box {
    margin-top: 12px;
    border-radius: 10px;
    padding: 12px;
    border: 1px solid #dce6fa;
    background: #f7faff;
  }

  .security-box.medium {
    border-color: #ffe1a8;
    background: #fff9ef;
  }

  .security-box.high {
    border-color: #ffc7c7;
    background: #fff3f3;
  }

  .security-box h4 {
    margin: 0 0 8px;
    font-size: 14px;
  }

  .security-box p {
    margin: 0;
    color: #3a465f;
    font-size: 13px;
    line-height: 1.45;
  }

  .security-box ul {
    margin: 10px 0 0;
    padding-left: 18px;
    color: #4f5c78;
    font-size: 12px;
    line-height: 1.45;
  }

  .risk-line {
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: #3f4b64;
  }

  .security-loading {
    margin-top: 12px;
    border-radius: 10px;
    padding: 20px;
    border: 1px solid #d9e2f4;
    background: #f7faff;
    text-align: center;
  }

  .security-loading p {
    margin: 10px 0 0;
    color: #666;
    font-size: 13px;
  }

  .analyze-spinner {
    display: inline-block;
    width: 24px;
    height: 24px;
    border: 3px solid #e0e8f5;
    border-top-color: #4a72ff;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .drawer-actions {
    border-top: 1px solid #ebf0fa;
    padding: 12px;
    display: grid;
    grid-template-columns: 1fr;
    gap: 8px;
    background: rgba(255, 255, 255, 0.95);
    position: sticky;
    bottom: 0;
  }

  .action {
    height: 36px;
    border-radius: 8px;
    border: 1px solid #d9e2f4;
    background: #fff;
    color: #33425a;
    cursor: pointer;
  }

  .action.primary {
    border: none;
    background: linear-gradient(135deg, #4d77ff 0%, #4268e8 100%);
    color: #fff;
  }

  .action.danger {
    border-color: #ffbcbc;
    color: #b33a3a;
    background: #fff5f5;
  }

  @keyframes spin {
    to {
      transform: translateY(-50%) rotate(360deg);
    }
  }

  @media (max-width: 1120px) {
    .search-layout {
      grid-template-columns: 1fr;
    }

    .detail-drawer {
      min-height: 360px;
    }
  }
</style>

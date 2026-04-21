# isearch 接口设计文档

## 1. 概述

isearch 使用 Tauri 2 框架，通过 IPC（进程间通信）直接调用 Rust 后端函数。前端使用 SvelteKit，通过 `@tauri-apps/api/tauri` 调用后端命令。

## 2. Tauri Commands

所有命令通过 `invoke()` 从前端调用：

```javascript
import { invoke } from "@tauri-apps/api/tauri";

// 调用示例
const result = await invoke("command_name", { param1: "value" });
```

### 2.1 搜索命令

#### search_files

执行文件搜索

**参数**:
```typescript
{
  query: string,        // 搜索关键词
  limit?: number,      // 返回结果数量（默认 50）
  offset?: number,     // 结果偏移量（默认 0）
  fuzzy?: boolean,     // 启用模糊搜索（默认 true）
  paths?: string[],    // 限定搜索路径
}
```

**返回**:
```typescript
{
  total: number,        // 总结果数
  results: {
    id: number,
    filename: string,
    filepath: string,
    filesize: number,
    modified_at: string,
    extension: string,
    score: number,
  }[],
  search_time_ms: number,
}
```

**前端调用**:
```javascript
const result = await invoke("search_files", {
  query: "document",
  options: {
    limit: 20,
    fuzzy: true,
  }
});
```

### 2.2 索引命令

#### start_indexing

开始索引指定目录

**参数**:
```typescript
{
  paths?: string[],    // 要索引的目录路径（可选，为空则使用配置中的路径）
}
```

**返回**:
```typescript
{
  status: "started",
  message: string,
}
```

#### get_index_status

获取当前索引状态

**参数**: 无

**返回**:
```typescript
{
  is_indexing: boolean,
  total_files: number,
  indexed_files: number,
  last_updated: string,
  indexed_paths: string[],
}
```

#### rebuild_all_indexes

重建所有索引

**参数**: 无

**返回**:
```typescript
{
  status: "completed",
  indexed_files: number,
  duration_ms: number,
}
```

### 2.3 文件详情命令

#### get_file_details

根据路径获取文件详情

**参数**:
```typescript
{
  path: string,        // 文件路径
}
```

**返回**:
```typescript
{
  id: number,
  filename: string,
  filepath: string,
  filesize: number,
  modified_at: string,
  created_at: string,
  extension: string,
}
```

### 2.4 安全扫描命令

#### init_clamav

初始化 ClamAV 引擎

**参数**: 无

**返回**:
```typescript
{
  status: "initialized",
  signature_count: number,
}
```

#### start_security_scan

开始安全扫描

**参数**:
```typescript
{
  scanDir?: string,    // 要扫描的目录（可选，为空则扫描所有已索引文件）
}
```

**返回**:
```typescript
{
  threats: {
    path: string,
    threat: string,
    severity: "low" | "medium" | "high",
  }[],
  scanned_count: number,
  clean_count: number,
  threat_count: number,
  scan_time_ms: number,
}
```

#### save_scan_report

保存扫描报告

**参数**:
```typescript
{
  report: {
    id: string,
    timestamp: string,
    scannedDir: string,
    totalFiles: number,
    cleanCount: number,
    threatCount: number,
    duration: number,
    threats: {
      path: string,
      threat: string,
      severity: string,
    }[],
  }
}
```

**返回**:
```typescript
{
  status: "saved",
  path: string,
}
```

### 2.5 配置命令

#### get_indexed_directories

获取已索引的目录列表

**参数**: 无

**返回**:
```typescript
{
  directories: string[],
}
```

#### add_indexed_directory

添加索引目录

**参数**:
```typescript
{
  path: string,        // 目录路径
}
```

**返回**:
```typescript
{
  status: "added",
  path: string,
}
```

#### remove_indexed_directory

移除索引目录

**参数**:
```typescript
{
  path: string,         // 目录路径
}
```

**返回**:
```typescript
{
  status: "removed",
  path: string,
}
```

#### get_ignored_patterns

获取忽略的文件模式

**参数**: 无

**返回**:
```typescript
{
  patterns: string[],
}
```

## 3. 事件推送 (Event Emitter)

Tauri 使用事件系统进行服务端推送，前端通过 `listen()` 订阅。

### 3.1 索引进度事件

**事件名**: `index-status`

**推送内容**:
```typescript
{
  is_indexing: boolean,
  total: number,
  indexed: number,
  current_path: string,
}
```

**前端订阅**:
```javascript
import { listen } from "@tauri-apps/api/event";

await listen("index-status", (event) => {
  console.log("Index progress:", event.payload);
});
```

### 3.2 安全扫描进度事件

**事件名**: `security-scan-status`

**推送内容**:
```typescript
{
  is_scanning: boolean,
  scanned: number,
  total: number,
  threats: number,
  message: string,
}
```

**前端订阅**:
```javascript
await listen("security-scan-status", (event) => {
  const { is_scanning, scanned, total, threats } = event.payload;
  const progress = total > 0 ? Math.floor((scanned / total) * 100) : 0;
  console.log(`Scan progress: ${progress}%`);
});
```

## 4. 数据模型

### 4.1 FileEntry

```typescript
interface FileEntry {
  id: number;
  filename: string;
  filepath: string;
  filesize: number;
  modified_at: string;
  extension: string;
  is_directory: boolean;
}
```

### 4.2 SearchOptions

```typescript
interface SearchOptions {
  limit?: number;
  offset?: number;
  fuzzy?: boolean;
  paths?: string[];
}
```

### 4.3 ScanResult

```typescript
interface ScanResult {
  path: string;
  threat: string;
  severity: "low" | "medium" | "high";
}
```

### 4.4 ThreatInfo

```typescript
interface ThreatInfo {
  path: string;
  threat: string;
  severity: string;
}
```

## 5. 错误处理

所有命令返回 `Result<T, String>`，错误以字符串形式返回。

**错误码定义**:

| 错误码 | 说明 |
|--------|------|
| "INDEX_NOT_READY" | 索引尚未构建完成 |
| "INVALID_PATH" | 指定路径不存在或无权访问 |
| "SCAN_FAILED" | 安全扫描失败 |
| "CLAMAV_NOT_INITIALIZED" | ClamAV 未初始化 |
| "DATABASE_ERROR" | 数据库操作失败 |
| "INTERNAL_ERROR" | 服务器内部错误 |

**前端错误处理**:
```javascript
try {
  const result = await invoke("start_security_scan", { scanDir: "/path" });
} catch (e) {
  if (e === "INDEX_NOT_READY") {
    console.log("索引未就绪，请先建立索引");
  } else {
    console.error("扫描失败:", e);
  }
}
```

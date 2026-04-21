# isearch 技术架构文档

## 1. 架构概览

isearch 采用分层架构设计，分为四个核心模块：

```
┌─────────────────────────────────────────────┐
│              GUI (SvelteKit + Tauri)         │  表现层
├─────────────────────────────────────────────┤
│              Tauri Commands (IPC)             │  接口层
├─────────────────────────────────────────────┤
│  Search Engine │ Index Manager │ File Watcher│  核心引擎层
├─────────────────────────────────────────────┤
│     LMDB Store  │  FST Index  │ Metadata    │  数据层
├─────────────────────────────────────────────┤
│              File System (OS)                │  系统层
└─────────────────────────────────────────────┘
```

## 2. 模块设计

### 2.1 表现层 (Presentation)

#### GUI 客户端
- 基于 Tauri + SvelteKit 的跨平台桌面应用
- 搜索输入框 + 结果列表 + 详情面板
- 快捷键支持（Cmd/Ctrl+K 唤起搜索）
- 支持暗黑/明亮主题

### 2.2 接口层 (API Layer)

#### Tauri Commands
- 通过 IPC 直接调用 Rust 后端命令
- 支持实时事件推送（进度、安全扫描状态等）
- 主要命令：
  - `search_files` - 执行搜索
  - `start_indexing` - 开始索引
  - `get_index_status` - 获取索引状态
  - `start_security_scan` - 执行安全扫描

### 2.3 核心引擎层 (Core Engine)

#### Search Engine（搜索引擎）
- **负责**: 执行搜索查询
- **核心算法**:
  - 基于编辑距离的模糊匹配（FST Levenshtein）
  - 前缀树（Trie）加速前缀匹配
  - 倒排索引加速全文搜索
- **接口**:
  ```rust
  pub trait SearchEngine {
      fn search(&self, query: &str, limit: usize) -> Vec<SearchResult>;
      fn suggest(&self, prefix: &str) -> Vec<String>;
  }
  ```

#### Index Manager（索引管理器）
- **负责**: 管理文件索引的创建、更新、删除
- **核心功能**:
  - 增量索引构建
  - 索引持久化（LMDB）
  - 文件变化监听（notify）
- **接口**:
  ```rust
  pub trait IndexManager {
      fn build_index(&self, paths: &[PathBuf]) -> IndexResult;
      fn update_index(&self, file: &PathBuf) -> IndexResult;
      fn remove_from_index(&self, file: &PathBuf);
  }
  ```

### 2.4 数据层 (Data Layer)

#### LMDB Store（LMDB 存储引擎）
- **负责**: 高性能键值存储
- **特点**:
  - 零配置、嵌入式
  - 多版本并发控制（MVCC）
  - 内存映射文件
- **索引数据结构**:
  ```
  FileIndex {
    file_id: u64,
    filename: String,
    filepath: String,
    filesize: u64,
    modified_at: i64,
    extension: String,
    indexed_at: DateTime,
  }
  ```

#### FST Index（有限状态转换器索引）
- **负责**: 快速前缀搜索和模糊匹配
- **特点**:
  - 基于 Levenshtein 距离的模糊搜索
  - 常量时间复杂度的前缀查询
  - 内存高效表示

#### Metadata Store（元数据存储）
- **负责**: 存储文件元信息和索引路径
- **数据结构**: JSON 文件 + LMDB

#### File Watcher（文件监听器）
- **负责**: 监听文件系统变化，触发增量索引
- **实现**: 基于 `notify` crate（跨平台）
- **事件类型**: CREATE, MODIFY, DELETE, RENAME

## 3. 技术栈选择

### 3.1 后端技术栈

| 组件 | 技术选型 | 理由 |
|------|----------|------|
| 编程语言 | Rust | 高性能、低内存、安全 |
| 桌面框架 | Tauri 2 | 轻量、安全、跨平台 (macOS/Windows/Linux) |
| 存储引擎 | LMDB (heed) | 高性能、嵌入式、事务支持 |
| 搜索索引 | FST (fst crate) | 快速前缀匹配、模糊搜索 |
| 文件监听 | notify | 跨平台、成熟稳定 |
| 病毒扫描 | ClamAV (clamav-sys) | 开源、免费、安全 |

### 3.2 前端技术栈

| 组件 | 技术选型 | 理由 |
|------|----------|------|
| 桌面框架 | Tauri 2 | 轻量、安全、跨平台 |
| 前端框架 | SvelteKit | 轻量、响应式、简单易用 |
| 构建工具 | Vite | 快速热更新 |
| 语言 | TypeScript | 类型安全 |

### 3.3 CLI 技术栈

| 组件 | 技术选型 | 理由 |
|------|----------|------|
| 命令行解析 | clap | 声明式、类型安全 |
| 交互式界面 | ratatui | 跨平台 TUI 库 |

## 4. 数据流设计

### 4.1 索引构建流程

```
1. 用户指定要索引的目录
2. File Watcher 扫描目录树
3. 对每个文件提取元信息（文件名、路径、大小、修改时间）
4. Index Manager 更新 LMDB + FST 索引
5. Search Engine 加载索引到内存
```

### 4.2 搜索流程

```
1. 用户输入搜索关键词
2. GUI 调用 Tauri Command
3. Search Engine 执行查询（FST + LMDB）
4. 返回排序后的结果列表
5. 展示给用户
```

### 4.3 安全扫描流程

```
1. 用户选择要扫描的目录（或全部文件）
2. File Watcher 收集文件列表
3. ClamAV 扫描每个文件
4. 实时推送扫描进度
5. 返回威胁文件列表
```

## 5. 配置文件

```toml
# isearch.toml

[search]
max_results = 100
fuzzy_threshold = 0.6

[index]
paths = [
    "/Users/aaron/Documents",
    "/Users/aaron/Projects",
]
exclude_patterns = [
    "**/.git/**",
    "**/node_modules/**",
    "**/target/**",
    "**/.DS_Store",
]
incremental = true

[clamav]
# ClamAV 数据库路径（留空则自动检测）
db_path = ""

[ui]
theme = "system"
shortcut = "CmdOrCtrl+K"
```

## 6. 目录结构

```
isearch/
├── src-tauri/                # Tauri 后端 (Rust)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs           # 入口文件
│       ├── core/             # 核心引擎
│       │   ├── mod.rs
│       │   ├── database.rs   # 数据库初始化
│       │   ├── lmdb_store.rs # LMDB 存储
│       │   ├── metadata_store.rs
│       │   ├── fst_index.rs  # FST 索引
│       │   ├── sharded_fst.rs
│       │   ├── inverted_index.rs
│       │   ├── indexer.rs    # 索引管理
│       │   ├── fs_indexer.rs # 文件系统索引
│       │   ├── native_indexer.rs
│       │   ├── watcher.rs    # 文件监听
│       │   ├── clamav_scanner.rs # 病毒扫描
│       │   └── channel_writer.rs
│       └── lib.rs
├── src/                      # SvelteKit 前端
│   ├── routes/
│   │   └── +page.svelte      # 主页面
│   ├── app.html
│   ├── app.css
│   └── lib/
├── docs/                     # 文档
├── build.sh                  # 构建脚本
└── README.md
```

## 7. 扩展性设计

### 插件系统
- 插件接口定义搜索过滤器、内容解析器
- 支持第三方插件扩展

### 存储后端可插拔
- 通过 trait 定义存储接口
- 可切换 LMDB / RocksDB / 自定义后端

## 8. 安全设计

### ClamAV 集成
- 使用原生 libclamav 进行文件扫描
- 自动检测多平台数据库路径
- 支持增量扫描（只扫描变更文件）

### 数据安全
- 所有数据存储在用户本地
- 不上传任何文件内容或元数据
- 索引文件加密存储（可选）

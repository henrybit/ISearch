# isearch

A fast, secure file search and security scanning application for macOS, Windows, and Linux.

## Features | 功能特性

- **快速文件搜索** - 基于 FST 和 LMDB 的高性能全文索引，支持模糊匹配和前缀搜索
- **安全扫描** - 集成 ClamAV 杀毒引擎，支持文件/目录安全扫描
- **实时监控** - 文件系统监控，自动增量更新索引
- **跨平台** - 支持 macOS、Windows、Linux

## Requirements | 系统要求

- macOS 10.15+ / Windows 10+ / Linux (Ubuntu 20.04+)
- [Rust 1.77+](https://www.rust-lang.org/) (for development)
- [Node.js 18+](https://nodejs.org/) (for development)

## Installation | 安装

### Pre-built | 预构建版本

Download the latest release from [GitHub Releases](https://github.com/your-repo/isearch/releases).

### Build from source | 从源码构建

```bash
# Clone the repository
git clone https://github.com/your-repo/isearch.git
cd isearch

# Install dependencies
brew install clamav    # macOS
# sudo apt install clamav  # Linux

# Build
./build.sh
```

### ClamAV Database | ClamAV 病毒库

首次扫描时会自动下载病毒库（约 85MB），或手动下载：

```bash
# macOS
brew install clamav
mkdir -p ~/.isearch/clamav_db
cd ~/.isearch/clamav_db
curl -fsSL -A ClamAV/1.0 -o main.cvd https://database.clamav.net/main.cvd
```

## Usage | 使用方法

1. **启动应用**
   ```bash
   # Run in development mode
   cd src-tauri && cargo run

   # Or run the built app
   open isearch.app  # macOS
   ```

2. **建立索引**
   - 首次使用需要选择要索引的目录
   - 点击"开始索引"构建文件索引

3. **搜索文件**
   - 输入关键词进行搜索
   - 支持模糊匹配（自动开启）
   - 点击结果查看文件详情

4. **安全扫描**
   - 在搜索结果中点击文件，选择"安全扫描"
   - 或在"安全扫描"页面扫描整个目录

## Tech Stack | 技术栈

| Component | Technology |
|-----------|------------|
| Backend | Rust + Tauri 2 |
| Frontend | SvelteKit + TypeScript |
| Database | LMDB |
| Search Index | FST (Finite State Transducer) |
| Antivirus | ClamAV |

## Project Structure | 项目结构

```
isearch/
├── src/                    # Frontend (SvelteKit)
│   └── routes/
│       └── +page.svelte   # Main UI
├── src-tauri/             # Backend (Rust)
│   └── src/
│       ├── main.rs        # Entry & Tauri commands
│       └── core/          # Core modules
│           ├── database.rs
│           ├── lmdb_store.rs
│           ├── fst_index.rs
│           ├── clamav_scanner.rs
│           └── watcher.rs
├── docs/                  # Documentation
└── build.sh               # Build script
```

## License | 许可证

MIT

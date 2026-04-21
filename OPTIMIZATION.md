# isearch 架构优化记录

## 问题描述

2026-04-17: 索引文件数量达到千万级别后，内存占用高达 33GB。

## 问题根因分析

### 优化后架构

```
┌─────────────────────────────────────────────────────┐
│  LMDB (400MB fixed map)                            │
│    path → FileMetadata (包含完整路径)               │
│    id → path (按需查询，不全部加载到内存)            │
├─────────────────────────────────────────────────────┤
│  InvertedIndex (mmap，二分搜索)                      │
│    token → [path_id, path_id, ...] (排序存储)      │
│    无内存 path mapping - 按需从 LMDB 查询           │
├─────────────────────────────────────────────────────┤
│  Sharded FST (mmap)                                │
│    prefix → [path, path, ...]                      │
└─────────────────────────────────────────────────────┘
```

### 内存爆炸原因

| 问题 | 原因 |
|------|------|
| InvertedIndex 膨胀 | 每个 token 存完整的 path_id 列表，1000万文件 × 平均50个token × 8字节 = ~4GB |
| Path Mapping 膨胀 | HashMap<u64, String>，1000万文件 × 平均路径100字节 = ~10GB |
| Search 时爆炸 | 搜索高频 token（如 "file"）需要遍历数百万 postings |

## 已实施的优化 (2026-04-17)

### 1. 高频 Token 过滤

```rust
const STOP_TOKENS: &[&str] = &[
    "the", "and", "for", "are", "file", "data", "doc", "txt", "pdf",
    "tmp", "temp", "cache", "system", "desktop", "documents", ...
    "0", "1", "2", ..., "2020", "2021", ...  // 纯数字
];

const MAX_DOC_FREQUENCY: f32 = 0.01;  // 出现率 >1% 的 token 被跳过
const MIN_TOKEN_LEN: usize = 2;       // 太短的 token 跳过
const MAX_TOKEN_LEN: usize = 50;      // 太长的 token 跳过
```

### 2. Postings 数量限制

```rust
const MAX_POSTINGS_PER_TOKEN: usize = 50_000;  // 每个 token 最多保留 5 万个路径
```

### 3. Path ID 代替完整路径

```rust
// 使用 LMDB 的 FileMetadata.id (u64) 代替 String 路径
// 存储从 100+ 字节/路径 → 8 字节/路径
```

### 4. 二进制文件存储 + Memory-Mapped

```rust
// inverted_index.bin - 二进制格式，mmap 读取 (排序存储，支持二分搜索)
// 注意: 不再创建 path_mapping.bin，路径通过 LMDB 按需查询
```

### 内存估算对比

| 项目 | 优化前 | 优化后 |
|------|--------|--------|
| 1000万文件 | ~33GB | ~1-2GB |
| Token存储 | String (100+ bytes each) | u64 (8 bytes) |
| postings | 无限制 | 最多 5万/token |
| 高频token | 全保留 | 跳过 >1% 文档频率 |

---

## 传统文件搜索工具对比 (Everything, Spotlight, DocFetcher)

### 核心设计原则

| 工具 | 索引内容 | 内存效率 |
|------|----------|----------|
| Everything | 文件名 + MFT 引用 | ~20-50 字节/文件 |
| Spotlight | 文件名 + 部分元数据 | 极小 |
| DocFetcher | 文件名 + 内容片段 | 紧凑 |

### Everything 的秘密

```
1. 读取 NTFS 的 MFT (Master File Table) 直接获取所有文件名
2. USN 日志追踪文件变化（增量更新，不需要重建）
3. 索引本身极小（只存文件名 + MFT 位置）
4. 搜索使用 B-tree，时间复杂度 O(1)
```

### 关键区别

```
传统工具架构:
┌─────────────────────────────────────────────────────┐
│                    操作系统                           │
│  ┌─────────────┐    ┌─────────────────────────────┐│
│  │  MFT/Catalog │    │    USN Journal (变更追踪)   ││
│  └─────────────┘    └─────────────────────────────┘│
│         ↑                    ↑                        │
│         │ 只读，不占搜索内存   │ 增量通知               │
└─────────│────────────────────│───────────────────────┘
          │                    │
    ┌─────▼─────┐        ┌─────▼─────┐
    │  极小索引   │        │ 增量更新   │
    │  (B-tree)  │        │  不重建    │
    └─────────────┘        └───────────┘
```

## 待优化项

### 高优先级

- [x] ~~**Path Mapping 内存问题**：当前仍需加载完整 path_id → path 映射到内存~~ ✅ 已完成
  - 使用 LMDB 存储映射，需要时查询而非全部加载

- [x] ~~**Search 效率问题**：分词搜索需要遍历 postings~~ ✅ 已完成
  - 改用二分搜索，O(log n) 复杂度

### 中优先级

- [x] ~~**增量更新支持**：当前每次重建索引需要 O(n)~~ ✅ 已完成 (基础实现)
  - FileWatcher 已实现文件系统事件监听
  - DeltaTracker 记录增量变化

- [ ] **分层索引**：热门 token 内存 + 冷数据磁盘
  - 当前实现已部分支持（mmap），但 search 时仍需遍历

### 低优先级

- [ ] **FST 替代 InvertedIndex**
  - FST 本身就是为倒排索引设计的
  - 内存紧凑 + 前缀友好

---

## 已完成优化 (详见 FINISHED.md)

## 相关文件

- `src-tauri/src/core/inverted_index.rs` - 倒排索引实现
- `src-tauri/src/core/sharded_fst.rs` - 分片 FST 实现
- `src-tauri/src/core/lmdb_store.rs` - LMDB 存储
- `src-tauri/src/core/database.rs` - 数据库层整合

## 参考资料

- [Everything Search Engine Architecture](https://www.voidtools.com/support/everything/)
- [USN Journal](https://docs.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_query-usn-journal)
- [FST Library](https://docs.rs/fst/latest/fst/)

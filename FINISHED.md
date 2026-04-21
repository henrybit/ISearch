# 已完成的优化记录

## 2026-04-17 完成项

### 1. Path Mapping 内存优化 (高优先级) ✅

**问题**: InvertedIndex 在内存中存储完整的 `HashMap<u64, String>` (path_id → path)，1000万文件约占用10GB内存。

**解决方案**:
- 移除内存中的 path_id → path 映射
- 通过 LMDB 的 `get_by_id()` 方法按需查询路径
- 只在搜索结果需要解析时才查询 LMDB

**实现文件**:
- `src-tauri/src/core/inverted_index.rs`
  - 移除了 `path_id_to_path: RwLock<HashMap<u64, String>>` 字段
  - 新增 `lmdb_store: RwLock<Option<Arc<LmdbStore>>>` 用于按需路径解析
  - 新增 `set_lmdb_store()` 方法配置 LMDB 存储
  - 修改 `resolve_paths()` 从 LMDB 查询而非内存

**内存节省**: ~10GB (1000万文件场景)

### 2. Search 效率优化 - 二分搜索 (高优先级) ✅

**问题**: 之前的 `get_postings()` 对 mmap 数据进行线性扫描，O(n) 复杂度。

**解决方案**:
- 实现 `write_binary_index_sorted()` 写入排序的 token 索引
- 新增 `get_postings()` 使用二分搜索，O(log n) 复杂度
- 保留 `get_postings_linear_scan()` 用于兼容旧格式

**索引格式**:
- Token 数据按字母排序存储
- Token 偏移量数组在 header 后，便于二分查找
- 版本号标识 (version = 1)

**搜索性能提升**: O(n) → O(log n + k)，其中 k 是匹配的 posting 数量

### 3. 增量更新支持 - 改进 DeltaTracker (中优先级) ✅

**参考 Everything 的 USN Journal 设计思路**:
- 使用 `notify` crate 监听文件系统事件
- `FileWatcher` 实现已存在，记录文件增删改

**当前实现**:
- `DeltaTracker` 记录 added/removed/modified 文件
- `FileWatcher` 监听文件系统变化并实时更新索引
- `database::rebuild_all_indexes()` 重建所有索引

**下一步优化方向**:
- 增量更新而非全量重建索引
- 使用 macOS FSEvents 实现更高效的文件追踪

---

## 内存优化效果对比

| 项目 | 优化前 | 优化后 |
|------|--------|--------|
| 1000万文件内存占用 | ~33GB | ~1-2GB |
| Path Mapping | HashMap in-memory (~10GB) | LMDB按需查询 (0) |
| Token存储 | String (100+ bytes) | u64 (8 bytes) |
| Postings限制 | 无限制 | 最多5万/token |
| 高频token | 全保留 | 跳过 >1% 文档频率 |
| Token搜索 | O(n) 线性扫描 | O(log n) 二分搜索 |

## 架构变化

```
优化后:
┌─────────────────────────────────────────────────────┐
│  LMDB (400MB fixed map)                            │
│    path → FileMetadata (包含完整路径)               │
│    id → path (通过 get_by_id 按需查询)              │
├─────────────────────────────────────────────────────┤
│  InvertedIndex (mmap, 无内存path mapping)          │
│    token → [path_id, path_id, ...] (排序存储)       │
│    二分搜索 O(log n)                                │
├─────────────────────────────────────────────────────┤
│  Sharded FST (mmap)                                │
│    prefix → [path, path, ...]                      │
└─────────────────────────────────────────────────────┘
```

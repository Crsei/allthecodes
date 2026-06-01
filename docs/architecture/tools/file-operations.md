---
title: "文件操作工具 - Read、Write、Edit 三大工具源码解析"
description: "源码级剖析 FileRead、FileEdit、FileWrite 三大工具的完整执行链路：编码自动检测、原子性安全写入、缩进自动适配、模糊匹配引擎的实现细节。"
keywords: ["文件操作", "FileRead", "FileEdit", "FileWrite", "Rust 实现", "safe_write"]
---

## 三大工具的职责分化

allthecodes 将文件操作拆分为三个独立工具——这不是功能划分，而是**风险分级**：

| 工具 | 权限级别 | 源码路径 | 关键属性 |
|------|---------|---------|---------|
| **Read** | 只读（免审批） | `fs/file_read.rs` | `is_read_only() → true`, `is_concurrency_safe() → true` |
| **Edit** | 写入（需确认） | `fs/file_edit.rs` | `is_destructive() → true`, 使用 `safe_write_text` |
| **Write** | 写入（需确认） | `fs/file_write.rs` | `is_destructive() → true`, 使用 `safe_write_text` |

## FileRead：多格式自动检测读取引擎

`allthecodes-tools/src/fs/file_read.rs` 中的 `FileReadTool` 实现了完整的文件读取能力。

### 编码自动检测

Read 工具在 `decode_text_bytes()` 中实现了四级编码检测，无需第三方库：

1. **UTF-8 BOM**（`0xEF 0xBB 0xBF`）→ `utf-8-bom`
2. **UTF-16LE BOM**（`0xFF 0xFE`）→ `utf-16le-bom`
3. **UTF-16BE BOM**（`0xFE 0xFF`）→ `utf-16be-bom`
4. **无 BOM UTF-16 启发式检测**：统计偶数和奇数位置的 NUL 字节比例，当某一侧占比超 30% 且另一侧低于 5% 时判定为 UTF-16

最终兜底使用 `String::from_utf8_lossy()`，这确保了任意文本文件都能被读取。

### 二进制文件检测

```rust
fn is_binary(content: &[u8]) -> bool {
    let check_len = content.len().min(8192);
    content[..check_len].contains(&0)
}
```

通过检查前 8192 字节中是否包含 NUL 字节来判定是否为二进制。纯文本路径在检测后返回明确错误。

### 四路分发

`call()` 方法按文件扩展名分发到四条处理路径：

```
路径分发:
  .ipynb        → read_notebook()   → JSON cell 解析 → 带行号的细胞提取
  .png/.jpg/.gif/.webp/.bmp/.svg → read_image() → base64 编码（SVG 返回文本）
  .pdf          → read_pdf()        → pdftotext 子进程提取
  其他          → read_text()       → 编码检测 + 分页读取
```

**图片路径**的 SVG 特殊处理：直接返回文本内容（SVG 本质是 XML）。其他图片使用 `base64::engine::general_purpose::STANDARD` 编码为 base64。

**PDF 路径**依赖系统安装的 `pdftotext`（poppler-utils），缺失时返回明确的安装提示。

### 分页读取机制

`format_text_window()` 实现了自动分页：

- 默认行数上限：`DEFAULT_TEXT_LINE_LIMIT = 2000` 行
- 超过上限时自动截断，返回 `next_offset` 指示继续位置
- 行号格式为 `linenumber\tcontent`（与 `cat -n` 兼容）
- 字符级上限通过 `max_result_size_chars()` 控制（默认 100,000 字符）

### 符号链接追踪

`resolve_read_target()` 利用 `tokio::fs::symlink_metadata()` 检测符号链接，在结果中返回 `symlink_resolved` 字段。所有路径键（原始路径、读取路径、解析路径）都会被缓存到 `FileStateCache`。

## FileEdit：精确字符串替换引擎

`allthecodes-tools/src/fs/file_edit.rs` 中的 `FileEditTool` 实现了原子性精确编辑。

### 缩进自动适配

当 `old_string` 在文件中找不到时，工具启动缩进自动适配机制：

```rust
fn find_indentation_adjusted_edit(content, old_string, new_string) -> Option<...>
```

算法步骤：
1. 将 `old_string` 按行分割，去除每行的前导空白得到"trimmed 行"
2. 在文件内容中滑动窗口寻找 trimmed 行序列的唯一匹配
3. 建立缩进映射（`old_indent → actual_indent`）
4. 将 `new_string` 中的缩进按映射关系替换

这解决了 AI 输出缩进与文件实际缩进不一致的问题（如 AI 用 2 空格但文件用 4 空格）。

### 模糊匹配建议

当精确匹配和缩进适配都失败时，`find_best_fuzzy_match()` 使用 `similar::TextDiff` 进行滑动窗口相似度匹配：

```rust
fn find_best_fuzzy_match(content: &str, old_string: &str) -> Option<FuzzyMatch> {
    // 使用 chars 级别的 diff 计算相似度
    let diff = TextDiff::from_chars(&needle_text, &window_text);
    let ratio = diff.ratio();
    // 相似度 > 0.6 时返回建议
}
```

如果相似度超过 60%，工具会返回"你是否想找..."的建议，包含匹配到的实际文本和行号范围。

### 原子性写入

Edit 工具在 `call()` 中使用 `safe_write_text`（来自 `fs/safe_write.rs`）实现原子写入：

```
1. 校验缓存状态（文件必须被读取过且未被外部修改）
2. 统计 occurrences → 唯一性检查
3. 执行字符串替换
4. spawn_blocking 调用 safe_write_text
5. 记录新状态到 FileStateCache
6. 生成 unified diff（使用 similar::TextDiff）
7. 触发 FileChanged hook
```

步骤 3-4 的关键约束：替换必须在异步上下文中正确同步，通过 `spawn_blocking` 将阻塞的文件 I/O 移到线程池。

### 防覆写校验

```rust
fn validate_cached_read(ctx, file_path, path, content) -> Result<(), &'static str> {
    let entry = cached_entry(ctx, file_path, path)?;
    let current_hash = FileStateCache::hash_content(content.as_bytes());
    if current_hash != entry.content_hash {
        return Err(FILE_UNEXPECTEDLY_MODIFIED_ERROR);
    }
    Ok(())
}
```

编辑前必须通过 Read 工具读取文件，且内容哈希必须匹配。文件权限检查也会提前进行（readonly 文件直接拒绝）。

### Unified Diff 生成

每次编辑后使用 `TextDiff::from_lines()` 生成 unified diff（context radius = 3 行）：

```rust
fn unified_hunk_lines(path: &str, old_content: &str, new_content: &str) -> Vec<String> {
    TextDiff::from_lines(old_content, new_content)
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{}", path), &format!("b/{}", path))
        .to_string()
        .lines()
        .map(str::to_string)
        .collect()
}
```

## FileWrite：全量写入与创建

`allthecodes-tools/src/fs/file_write.rs` 中的 `FileWriteTool` 相对简单——它是全量写入操作。

### 安全写入

与 Edit 共享 `safe_write_text` 基础设施：

```rust
let safe_options = SafeWriteOptions {
    session_id: Some(ctx.session_id.clone()),
    ..Default::default()
};
let report = safe_write_text(file_path, &content, &safe_options);
```

写入后返回丰富的诊断信息：字节数、行数、是否创建新文件、备份路径、权限保留情况、符号链接解析情况。

### 输入校验

`validate_input()` 通过 `validate_write_request()` 执行两个关键检查：

1. **二进制检测**：内容中不能包含 NUL 字节
2. **大小限制**：不能超过 `DEFAULT_MAX_WRITE_BYTES`

### FileChanged Hook

写入后如果用户配置了 `FileChanged` Hook，会自动触发：

```rust
let configs = allthecodes_types::hooks::load_hook_configs(&app_state.hooks, "FileChanged");
if !configs.is_empty() {
    let payload = json!({ "file_path": ..., "operation": "write", ... });
    let _ = crate::hooks::run_event_hooks("FileChanged", &payload, &configs).await;
}
```

## Safe Write 机制

`fs/safe_write.rs` 实现了原子性安全写入：

- 创建临时文件 → 写入内容 → 重命名为目标文件
- 保留原有文件权限
- 创建编辑历史备份
- 支持符号链接目标解析

三个工具（Read/Edit/Write）通过 `FileStateCache` 共享状态：Read 写入缓存，Edit/Write 消费缓存并在修改后刷新。

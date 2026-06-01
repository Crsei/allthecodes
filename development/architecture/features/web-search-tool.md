# Web Search Tool — 网页搜索工具

> 门控：始终启用（无 feature flag）
> 实现状态：完整实现
> 提供商：Tavily Search API / Brave Search API
> 特性：查询缓存 / 域名过滤 / 结果格式化

## 一、功能概述

WebSearchTool 让模型可以搜索互联网获取最新信息。与 TypeScript 参考实现支持 API 服务端搜索 + Bing/Brave HTML 解析不同，Rust 移植使用商业搜索 API（Tavily / Brave）作为后端，提供更稳定、更快速的结构化搜索结果。

## 二、实现架构

### 2.1 Crate 结构

`web_search` 模块位于 `allthecodes-tools` crate 内：

| 模块 | 文件 | 职责 |
|------|------|------|
| 工具入口 | `tool.rs` | WebSearchTool 实现（Tool trait） |
| 提供商 | `providers.rs` | Tavily / Brave API 调用 |
| 模块根 | `mod.rs` | 缓存、类型定义、域名过滤、结果格式化 |
| 测试 | `tests.rs` | 集成测试 |

### 2.2 数据流

```
模型调用 WebSearch(query, max_results, allowed_domains, blocked_domains)
       │
       ▼
  validate_input()
       │  query 长度 2-400 字符
       │  max_results 不超过 20
       ▼
  detect_provider()
       │  1. TAVILY_API_KEY → Tavily
       │  2. BRAVE_SEARCH_API_KEY → Brave
       │  3. 两者都无 → 错误
       ▼
  构建缓存 key: "{query}|{max_results}|{provider}"
       │
       ├── 缓存命中 → 直接返回缓存结果
       └── 缓存未命中 → 调用 API
              │
              ├── Tavily: POST /search
              │     api_key + query + max_results
              │
              └── Brave: GET /res/v1/web/search
                    X-Subscription-Token + q + count
              │
              ▼
          解析 API 响应 → SearchResultEntry[]
              │
              ▼
          filter_results_unified()
              │  域名白名单 / 黑名单过滤
              ▼
          format_results_text()
              │  格式化为 Markdown 列表
              ▼
          ToolResult 返回给模型
```

### 2.3 搜索结果格式

返回给模型的文本格式：

```
1. **标题**
   https://example.com
   描述文本
   (1 week ago)

2. **标题2**
   https://example2.com
   描述文本
```

### 2.4 查询缓存

内存缓存，key 为 `"{query}|{max_results}|{provider_name}"`：

| 参数 | 值 |
|------|-----|
| 默认 TTL | 300 秒（5 分钟） |
| 最大缓存条目 | 128 |
| 环境变量覆盖 | `ALLTHECODES_SEARCH_CACHE_TTL` |
| 缓存位置 | 域名过滤前（不同过滤共享缓存） |

缓存使用 `parking_lot::Mutex` 保证并发安全，采用惰性过期 + 满容淘汰策略。

## 三、搜索提供商

### 3.1 Tavily Search API

| 属性 | 值 |
|------|-----|
| 端点 | `https://api.tavily.com/search` |
| 方法 | POST |
| 环境变量 | `TAVILY_API_KEY` |
| 优先级 | 最高（优先选择） |
| 请求格式 | JSON body: api_key, query, max_results, search_depth |
| 响应字段 | title, url, content, published_date |

### 3.2 Brave Search API

| 属性 | 值 |
|------|-----|
| 端点 | `https://api.search.brave.com/res/v1/web/search` |
| 方法 | GET |
| 环境变量 | `BRAVE_SEARCH_API_KEY` |
| 优先级 | 备选（Tavily 不可用时） |
| 认证 | `X-Subscription-Token` 请求头 |
| 请求参数 | q, count |
| 响应字段 | web.results[].title, url, description, age |

### 3.3 提供商探测逻辑

```rust
fn detect_provider() -> Option<SearchProvider> {
    match (tavily_key, brave_key) {
        (Some(key), _) if !key.is_empty() => Some(Tavily(key)),
        (_, Some(key)) if !key.is_empty() => Some(Brave(key)),
        _ => None,
    }
}
```

至少需要配置一个 API key，否则工具在验证阶段会报错。

## 四、域名过滤

客户端侧实现，支持子域名匹配：

**白名单模式（allowed_domains）：**

```rust
// 结果域名必须匹配白名单中的某项（含子域名）
matches_domain("https://docs.rust-lang.org", "rust-lang.org") // true
matches_domain("https://example.com", "rust-lang.org")        // false
```

**黑名单模式（blocked_domains）：**

```rust
// 结果域名匹配黑名单的将被过滤
matches_domain("https://spam.com/page", "spam.com") // 被过滤
```

**注意**：`allowed_domains` 和 `blocked_domains` 不能同时使用。

## 五、常量与限制

| 常量 | 值 |
|------|-----|
| `DEFAULT_MAX_RESULTS` | 5 |
| `MAX_RESULTS_CAP` | 20 |
| `MAX_QUERY_LENGTH` | 400 字符 |
| `SEARCH_TIMEOUT` | 30 秒 |
| 缓存 TTL（默认） | 300 秒 |
| 缓存最大条目 | 128 |

## 六、关键设计决策

1. **商业 API 优先**：选择 Tavily/Brave 而非 Bing HTML 抓取，避免反爬检测和维护成本
2. **Tavily 优先**：Tavily 专为 LLM 搜索设计，返回结构化 content 字段
3. **内存缓存**：减少重复 API 调用，支持自定义 TTL
4. **客户端域名过滤**：结果返回后本地过滤，减少 API 调用次数
5. **单一 Tool trait**：与 TypeScript 多种工具注册方式不同，Rust 版本通过统一的 Tool trait 注册

## 七、测试覆盖

| 测试 | 覆盖内容 |
|------|---------|
| 提供商检测 | detect_provider_from_keys 单元测试 |
| Brave 响应解析 | 完整/缺失/空结果/可选字段序列化 |
| Tavily 响应解析 | 结果解析 |
| 域名过滤 | filter_results_unified 白名单/黑名单 |
| 结果格式化 | format_results_text 空/完整结果 |
| 缓存 | 缓存命中/过期 |
| 集成测试 | tests.rs 中的真实 HTTP 请求测试 |

## 八、使用方式

```bash
# 配置搜索 API key
export TAVILY_API_KEY="your-key-here"

# 或使用 Brave
export BRAVE_SEARCH_API_KEY="your-key-here"

# 可选：自定义缓存 TTL
export ALLTHECODES_SEARCH_CACHE_TTL=600
```

## 九、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-tools/src/web_search/mod.rs` | 类型定义、缓存、域名过滤、格式化 |
| `crates/allthecodes-tools/src/web_search/tool.rs` | WebSearchTool 实现 |
| `crates/allthecodes-tools/src/web_search/providers.rs` | Tavily / Brave API 调用 |
| `crates/allthecodes-tools/src/web_search/tests.rs` | 集成测试 |

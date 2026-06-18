一般开源 Rust 项目不会简单地“一刀切删掉所有 `unwrap/expect/panic`”，而是按场景分级处理：

## 1. 核心原则

`unwrap()` / `expect()` 本质上都是“取值失败就 panic”。区别只是 `expect()` 能给出自定义信息，`unwrap()` 只给泛化信息；`panic!()` 则是显式让当前线程进入不可恢复错误。Rust 官方建议：函数可能失败且调用方可以处理时，默认返回 `Result`；只有原型、示例、测试，或者确实违反内部不变量时，panic 才更合适。([Rust文档][1])

最常见处理顺序是：

```rust
// 不推荐：运行时失败直接崩
let config = std::fs::read_to_string(path).unwrap();

// 推荐：把错误向上传播
let config = std::fs::read_to_string(path)?;

// 更推荐：给错误加上下文
let config = std::fs::read_to_string(path)
    .with_context(|| format!("failed to read config: {}", path.display()))?;
```

对于应用层项目，`anyhow::Result` + `?` + `context/with_context` 很常见；`anyhow` 官方文档也强调它适合应用代码，并支持给低层错误添加上下文。对于库 crate，更常见的是定义清晰的错误类型，例如用 `thiserror` 派生 `std::error::Error`，避免把随意字符串错误暴露成公共 API。([Docs.rs][2])

## 2. 开源项目里的真实做法

| 项目/来源                       | 做法                                                                                                                                                                             | 可借鉴点                                          |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------- |
| ICU4X                       | 在生产代码中通过 `cfg_attr(not(test), deny(...))` 禁用 `clippy::unwrap_used`、`clippy::expect_used`、`clippy::panic`，同时考虑迁移到 Cargo workspace lints；测试代码用 Clippy 的 allow 配置放宽。([GitHub][3]) | 生产代码严控 panic，测试代码允许更直接失败。                     |
| EigenSDK Rust               | 在 workspace 层设置 `clippy.unwrap_used = "warn"`、`clippy.expect_used = "warn"`、`clippy.panic = "warn"`、`clippy.panic_in_result_fn = "warn"`。([GitHub][4])                         | 先设为 warn，逐步清理，适合已有项目。                         |
| Kanidm                      | 在 `clippy.toml` 中设置 `allow-expect-in-tests = true`、`allow-unwrap-in-tests = true`、`allow-panic-in-tests = true`。([GitHub][5])                                                  | 测试中允许 `unwrap/expect/panic`，但生产代码仍可严格。        |
| HAProxy Protocol Rust crate | 同样允许测试中的 `expect/unwrap/panic/dbg`。([GitHub][6])                                                                                                                               | 小型库也采用“测试放宽、生产严格”的模式。                         |
| Clippy 官方                   | `unwrap_used`、`expect_used`、`panic` 都是 restriction lint，默认是 allow，需要项目主动启用。Clippy 也明确建议优先处理 `None/Err`，或用 `?` 向上传播。([Rust语言][7])                                               | 不建议直接开整个 `clippy::restriction`，而是按需启用这些 lint。 |

需要注意：`allow-unwrap-in-tests = true` 对 `#[test]` / `cfg(test)` 有效，但 Clippy 项目里有人指出它不一定覆盖 `tests/` 目录下的 integration tests、examples、benches，因为这些目标并不总是 `cfg(test)`。所以 CI 里最好单独决定哪些 target 检查严格。([GitHub][8])

## 3. 实际重构规则

### A. IO、网络、解析、配置读取：不要 `unwrap`

这些都是“预期可能失败”的情况，应返回 `Result`。

```rust
// before
let data = std::fs::read_to_string(path).unwrap();
let value: Config = toml::from_str(&data).unwrap();

// after
let data = std::fs::read_to_string(path)
    .with_context(|| format!("read config file: {}", path.display()))?;

let value: Config = toml::from_str(&data)
    .context("parse config file")?;
```

### B. `Option::unwrap()`：优先改成 `ok_or_else`

```rust
// before
let home = std::env::var_os("HOME").unwrap();

// after
let home = std::env::var_os("HOME")
    .ok_or_else(|| anyhow::anyhow!("HOME environment variable is not set"))?;
```

### C. 有默认值的情况：用 `unwrap_or` / `unwrap_or_else` / `unwrap_or_default`

```rust
let timeout = config.timeout.unwrap_or(Duration::from_secs(30));
let name = maybe_name.unwrap_or_else(|| "default".to_string());
let items = maybe_items.unwrap_or_default();
```

这类不是“吞错误”，而是业务上明确允许缺省值时才用。

### D. 内部不变量：可以 `expect`，但消息必须写“为什么不可能失败”

```rust
let ip: IpAddr = "127.0.0.1"
    .parse()
    .expect("hardcoded loopback IP address must be valid");
```

Rust 官方书也给了类似例子：当人能确认某个失败逻辑上不可能，但编译器无法证明时，`expect` 可以接受，并且消息应说明这个假设。([Rust文档][1])

### E. 测试代码：`unwrap/expect` 可以接受，但更推荐 `expect` 或 `?`

测试里 `unwrap()` 的问题不大，因为失败就是测试失败。官方也认为测试中调用 `unwrap/expect` 是合理的。([Rust文档][1])

```rust
#[test]
fn parses_config() -> anyhow::Result<()> {
    let config = parse_config("timeout = 30")?;
    assert_eq!(config.timeout, 30);
    Ok(())
}
```

这种写法比到处 `.unwrap()` 更干净。

### F. 公共 API 里会 panic：要么改成 `Result`，要么写清楚 `# Panics`

Clippy 有 `missing_panics_doc`，用于检查公开函数可能 panic 但没有 `# Panics` 文档；它的目的就是让调用方知道哪些条件会导致 panic。([Rust语言][7])

```rust
/// # Panics
///
/// Panics if `index >= self.len()`.
pub fn get_unchecked_logic(&self, index: usize) -> &Item {
    &self.items[index]
}
```

能返回错误时更推荐：

```rust
pub fn get_item(&self, index: usize) -> Result<&Item, Error> {
    self.items.get(index).ok_or(Error::IndexOutOfBounds { index })
}
```

## 4. 推荐你在项目里这样落地

### 第一步：先扫描

```bash
rg '\.(unwrap|expect)\(|panic!\(' crates src tests examples benches
```

### 第二步：开 Clippy 警告，不要一开始 deny

已有项目建议先 warn：

```toml
[workspace.lints]
clippy.unwrap_used = "warn"
clippy.expect_used = "warn"
clippy.panic = "warn"
clippy.panic_in_result_fn = "warn"
```

Cargo 官方支持 workspace lints，让 workspace 成员继承统一 lint 配置。([Rust文档][9])

### 第三步：测试里放宽

```toml
# clippy.toml
allow-expect-in-tests = true
allow-unwrap-in-tests = true
allow-panic-in-tests = true
```

这和 Kanidm、haproxy-protocol 的做法一致。([GitHub][5])

### 第四步：CI 中逐步收紧

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Clippy 官方 README 也建议在 CI 中用 `--all-targets --all-features -- -D warnings` 来让 warning 变成失败。([GitHub][10])

## 5. 适合你的项目的判断标准

对于 coding agent / sandbox / executor 这种项目，我建议规则更严格一些：

生产路径中，用户输入、模型输出、文件系统、网络、JSON/TOML/YAML 解析、路径转换、权限判断、进程执行结果，都不要 `unwrap()`，统一返回 `Result` 并加上下文。

可以保留的情况只有这些：

```rust
// 1. 测试代码
let value = parse(input).expect("test fixture should parse");

// 2. 硬编码常量，逻辑上不可能失败
let re = Regex::new(r"^[a-z_]+$").expect("hardcoded regex must be valid");

// 3. 内部不变量被破坏，继续运行更危险
panic!("policy invariant violated: sandbox id missing after allocation");
```

其他大多数 `unwrap()` 都可以按这个顺序改：`?` → `context()` → `ok_or_else()` → `match` → 明确默认值。

[1]: https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html "To panic! or Not to panic! - The Rust Programming Language"
[2]: https://docs.rs/anyhow "anyhow - Rust"
[3]: https://github.com/unicode-org/icu4x/issues/5974 "Move to using Cargo.toml [lints] · Issue #5974 · unicode-org/icu4x · GitHub"
[4]: https://github.com/Layr-Labs/eigensdk-rs/blob/dev/Cargo.toml "eigensdk-rs/Cargo.toml at dev · Layr-Labs/eigensdk-rs · GitHub"
[5]: https://github.com/kanidm/kanidm/blob/master/clippy.toml "kanidm/clippy.toml at master · kanidm/kanidm · GitHub"
[6]: https://github.com/kanidm/haproxy-protocol/blob/main/clippy.toml "haproxy-protocol/clippy.toml at main · kanidm/haproxy-protocol · GitHub"
[7]: https://rust-lang.github.io/rust-clippy/master/index.html "Clippy Lints"
[8]: https://github.com/rust-lang/rust-clippy/issues/13981 "allow-unwrap-in-tests (etc) do not work for integration tests / examples / benches · Issue #13981 · rust-lang/rust-clippy · GitHub"
[9]: https://doc.rust-lang.org/cargo/reference/workspaces.html "Workspaces - The Cargo Book"
[10]: https://github.com/rust-lang/rust-clippy "GitHub - rust-lang/rust-clippy: A bunch of lints to catch common mistakes and improve your Rust code. Book: https://doc.rust-lang.org/clippy/ · GitHub"

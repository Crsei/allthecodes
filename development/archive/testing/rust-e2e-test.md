用 Rust 做 CLI 的 E2E 测试，assert_cmd 是最顺手的方案：直接启动你的二进制，断言退出码、输出和文件副作用。

依赖

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
assert_fs = "1"
assert_cmd 负责跑命令，predicates 负责断言输出，assert_fs 负责临时目录和文件断言。

目录结构

src/
  main.rs
tests/
  cli.rs
假设你的包名叫 mycli，那么 Command::cargo_bin("mycli") 会直接运行这个 CLI。

最小示例

src/main.rs

use std::{env, fs, process};

fn main() {
    let mut args = env::args().skip(1);

    match args.next().as_deref() {
        Some("hello") => {
            println!("hello");
        }
        Some("init") => {
            let name = args.next().unwrap_or_else(|| {
                eprintln!("missing name");
                process::exit(2);
            });

            fs::write(format!("{name}.txt"), "created").unwrap();
            println!("created {name}.txt");
        }
        _ => {
            eprintln!("usage: mycli <hello|init>");
            process::exit(2);
        }
    }
}
tests/cli.rs

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

#[test]
fn hello_prints_expected_output() {
    let mut cmd = Command::cargo_bin("mycli").unwrap();

    cmd.arg("hello")
        .assert()
        .success()
        .stdout(predicate::str::contains("hello\n"));
}

#[test]
fn invalid_args_return_nonzero() {
    let mut cmd = Command::cargo_bin("mycli").unwrap();

    cmd.assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("usage:"));
}

#[test]
fn init_creates_file_in_working_dir() {
    let temp = assert_fs::TempDir::new().unwrap();

    let mut cmd = Command::cargo_bin("mycli").unwrap();
    cmd.current_dir(temp.path())
        .args(["init", "demo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("created demo.txt"));

    temp.child("demo.txt").assert("created");
}
你通常要测的点

成功路径
命令成功执行，stdout 正确，退出码是 0。

错误路径
缺参数、非法参数、文件不存在时，退出码非 0，stderr 有明确信息。

文件副作用
用临时目录跑命令，验证生成/修改/删除的文件。

环境变量
对 CLI 很常见：

cmd.env("NO_COLOR", "1")
   .env("MYCLI_CONFIG", "test");
交互输入
如果 CLI 会从 stdin 读内容：
#[test]
fn reads_from_stdin() {
    let mut cmd = Command::cargo_bin("mycli").unwrap();

    cmd.write_stdin("Alice\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Alice"));
}
实用建议

E2E 测试放在 tests/ 下，按黑盒方式测，不要直接调内部函数。
每个测试用独立临时目录，不要污染仓库。
输出断言尽量匹配关键片段，不要整段死匹配。
如果 CLI 会调用网络、git、外部程序，尽量做可注入替身，不要依赖真实环境。
运行

cargo test
如果你愿意，我可以直接给你一套更贴近实际项目的模板，比如：

clap CLI 的 E2E 测试
带子命令的测试结构
交互式 CLI 的测试写法
包含 fixture 输入文件的完整示例


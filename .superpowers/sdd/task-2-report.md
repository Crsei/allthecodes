status: DONE

changed files:
- crates/allthecodes/src/ui/app.rs
- crates/allthecodes/src/ui/app/input.rs
- crates/allthecodes/src/ui/app/render.rs
- crates/allthecodes/src/ui/app/overlays.rs
- crates/allthecodes/src/ui/app/tests.rs
- .superpowers/sdd/task-2-report.md

commit sha(s):
- 9cace825 refactor(tui): centralize overlay dispatch

tests run with results:
- RED: `env CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target PATH=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin cargo test -p allthecodes ui::app::overlays` -> exit 101, expected compile failure: undeclared `OverlayState` and `ActiveOverlay`.
- GREEN: `env CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target PATH=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin cargo test -p allthecodes ui::app::overlays` -> exit 0, 1 passed, 0 failed, 798 filtered out.
- `env CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target PATH=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin cargo test -p allthecodes agent_tree_dialog_navigation_select_and_close` -> exit 0, 1 passed, 0 failed, 798 filtered out.
- `env CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target PATH=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin cargo test -p allthecodes question_dialog_collects_answer` -> exit 0, 1 passed, 0 failed, 798 filtered out.
- `env CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target PATH=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin cargo test -p allthecodes permission_dialog_can_render_above_prompt` -> exit 0, 1 passed, 0 failed, 798 filtered out.
- `git diff --check` -> exit 0.

self-review notes:
- `OverlayState::active_overlay()` is now the single overlay priority source used by input dispatch.
- Runtime priority is preserved exactly as required: BypassPermissions -> Question -> Permission -> AgentTree -> HistorySearch -> CommandSurface.
- Rendering still uses the previous draw stack order while reading overlay state through `self.overlays`.
- Existing tests that directly inspected moved fields were updated to the new `app.overlays.*` paths only.

concerns:
- None.

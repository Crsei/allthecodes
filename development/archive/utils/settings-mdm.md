# 设置系统与 MDM 移植缺口

对应: `claude-code-bun/src/utils/settings/`
Rust 对应: `cc-config/`

## Bun 端规模

| 文件 | 行数 | 说明 |
|------|------|------|
| settings.ts | 1,003 | 核心设置管理 |
| types.ts | 1,196 | Zod 类型定义 |
| constants.ts | 202 | 来源定义 |
| changeDetector.ts | 488 | 变更检测 |
| validation.ts | 267 | 通用验证 |
| permissionValidation.ts | 411 | 权限规则验证 |
| validationTips.ts | 164 | 验证建议 |
| settingsCache.ts | 80 | 内存缓存 |
| applySettingsChange.ts | 92 | 变更应用 |
| internalWrites.ts | 37 | 内部写入检测 |
| managedPath.ts | 34 | 路径解析 |
| toolValidationConfig.ts | 103 | 工具验证配置 |
| validateEditTool.ts | 45 | 编辑工具验证 |
| schemaOutput.ts | 8 | Schema 输出 |
| pluginOnlyPolicy.ts | 60 | 插件策略 |
| allErrors.ts | 30 | 错误聚合 |
| **mdm/** | | |
| mdm/settings.ts | 316 | MDM 企业设置 |
| mdm/rawRead.ts | 129 | 原始策略读取 |
| mdm/constants.ts | 81 | MDM 常量 |
| **合计** | **~4,521** | |

## Rust 已实现

### cc-config/ (设置主干与 MDM 诊断已成型)
- `crates/cc-config/src/settings.rs`: `RawSettings` / `EffectiveSettings`、source map、JSON schema、atomic write + backup、多源加载。
- `crates/cc-config/src/settings.rs`: 已有 `SettingsSource::Managed`，启动加载顺序包含 managed/user/project/local/env；managed 文件路径支持 `CC_RUST_MANAGED_SETTINGS`、Windows `%ProgramData%\cc-rust\settings.json`、非 Windows `/etc/cc-rust/managed-settings.json`。
- `crates/cc-config/src/mdm/`: 企业 MDM 策略管理和读取，覆盖 `mdm/settings.ts`、`mdm/rawRead.ts`、`mdm/constants.ts` 的文件型策略读取与规范化。
- `crates/cc-config/src/change_detector.rs`: settings 文件变更检测。
- `crates/cc-config/src/permission_validation.rs`: settings 权限规则验证与 shadowed rule 诊断。
- `crates/cc-config/src/validation_tips.rs`: 常见配置问题的结构化建议。
- `crates/cc-config/src/internal_writes.rs`: 内部写入标记，避免 watcher 自触发反馈循环。
- `crates/cc-commands/src/doctor.rs`: `/doctor` 展示 managed policy、权限验证和 shadowed rule。
- `crates/cc-commands/src/permissions_cmd.rs`: `/permissions show` 展示被 managed policy 覆盖的规则。

## 仍需补齐

### 1. 工具验证配置 (103+45 行)
- 还没有完全等价的集中式 `toolValidationConfig.ts`。
- Edit 工具已有局部校验，但缺少 settings 驱动的统一适配层。

### 2. 通用变更应用 (92 行)
- `/config set` 和 `/permissions` 已覆盖常用写入路径。
- 仍缺少覆盖全部 settings key 的通用 `applySettingsChange` 入口、nested object/array patch、失败回滚和 watcher 联动。

### 3. 平台级 MDM 源
- 当前 MDM 读取以文件型 managed settings 为主。
- macOS 配置描述文件、Windows policy store、设备管理原始策略读取仍需继续对齐上游。

## 关键差异总结

| 功能 | Bun | Rust | 差距 |
|------|-----|------|------|
| 多源设置 | 完整 (5 来源) | settings.rs | ✅ 已移植 |
| 设置类型 | Zod schemas | Rust structs | ✅ 已移植 |
| Feature flags | features.rs | features.rs | ✅ 已移植 |
| CLAUDE.md | claude_md.rs | claude_md.rs | ✅ 已移植 |
| 路径管理 | paths.rs | paths.rs | ✅ 已移植 |
| 运行时设置 | runtime_settings.rs | runtime_settings.rs | ✅ 已移植 |
| 验证 | validation.rs | validation.rs | ✅ 已移植 |
| MDM 文件型策略 | mdm/ | mdm/ | ✅ 已移植 |
| 变更检测 | changeDetector.ts | change_detector.rs | ✅ 已移植 |
| 权限验证 | permissionValidation.ts | permission_validation.rs | ✅ 已移植 |
| 验证建议 | validationTips.ts | validation_tips.rs | ✅ 已移植 |
| 内部写入检测 | internalWrites.ts | internal_writes.rs | ✅ 已移植 |
| 工具验证配置 | toolValidationConfig.ts | 分散在工具 validate_input | 部分实现 |
| 通用变更应用 | applySettingsChange.ts | /config 与 /permissions 局部写入 | 部分实现 |
| 平台 MDM 源 | macOS/Windows policy | 文件型 managed settings | 部分实现 |

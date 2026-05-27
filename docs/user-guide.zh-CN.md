# 用户指南

[English](user-guide.md)

这份指南说明如何把 `any-switch` 当作日常的本机应用 profile 切换工具使用。它尽量避开实现细节；如果需要架构和安全模型，请看 `docs/design.md`。

## 核心概念

**App** 是一个可以由 `any-switch` 管理本地状态的工具。构建产物可以包含内置 app definition，用户也可以在 `apps.d/*.yaml` 下添加更多定义。

**Profile** 是一组具名的应用状态，例如：

- `codex-personal`
- `codex-work`
- `claude-anthropic`
- `claude-proxy`

**Target** 是应用读写的本地位置，例如 JSON 文件、TOML 子树、普通文件、Keychain 项或环境片段。

`any-switch` 不会替你登录远端服务。你先用目标应用自己的方式完成登录或配置，再用 `any-switch` 保存和回放这些本地状态。

## 第一次使用

当前版本是 macOS-evidenced stage release。macOS Claude OAuth import 有真实本机证据；更完整的重启检查，以及 Linux 和 Windows 的真实应用证据仍作为后续工作跟踪。在这些完成前，项目不会声明完整覆盖 `docs/design.md` 第 13 节。

`any-switch` 是源码构建型 CLI。安装命令会在你的机器上编译 Rust 二进制，而不是下载未签名的 macOS 或 Windows 二进制文件。

先安装 Rust 工具链：

```bash
rustup toolchain install 1.95.0
```

如果你还没有安装 `rustup`，请先从 <https://rustup.rs> 安装 Rust。

对大多数习惯 npm CLI 的用户：

```bash
npm install -g any-switch
any-switch --version
```

第一次 npm 安装可能会花一点时间，因为它会运行一次 Cargo，并把编译好的二进制放进 npm 包目录。之后直接运行 `any-switch` 即可。

对 Rust 用户：

```bash
cargo install any-switch --locked
any-switch --version
```

`npx any-switch --version` 可以用于快速试用，但首次使用可能会编译；如果 npm cache 被清理，也可能再次编译。日常使用建议全局安装。

从 checkout 做本地开发时：

```bash
cargo install --path .
```

查看当前构建知道哪些 app：

```bash
any-switch apps
```

查看 active `profiles.yaml` 路径，并检查本地安全诊断：

```bash
any-switch config path
any-switch doctor
```

`config path` 会打印主 profile registry 的可编辑路径。`doctor` 会打印 any-switch home 目录、`profiles.yaml` 路径、权限检查、已知云同步风险，以及 app-specific 诊断。

默认情况下，profile 和 capture 保存在 `~/.any-switch`。这个目录可能包含静态 secret、OAuth capture 和防御性备份。不要把它放进 iCloud Drive、Dropbox、OneDrive、Google Drive 等云同步目录；`doctor` 会在检测到已知同步目录时报警。

如果测试时想使用单独的状态目录，请选择 home 目录下的绝对路径：

```bash
export ANY_SWITCH_HOME="$HOME/.any-switch-test"
```

## 保存当前状态

当应用已经配置好或已经登录后，使用 `import-current`：

```bash
any-switch import-current <app> personal
```

OAuth 状态导入前建议先关闭目标应用。如果应用确实已经关闭，但进程检测误报，再使用 `--assume-app-stopped` 并确认提示。脚本和 CI 中可以为这个 escape hatch 加上 `--yes`。

内置 Claude OAuth 的典型首次捕获：

```bash
any-switch import-current claude personal --kind oauth_capture
```

只在进程检测误报时使用 process-probe escape hatch。OAuth token 可能在应用运行时旋转，所以从运行中的应用导入 live state 可能保存到不完整或过期的 capture。

内置 Codex 的典型首次捕获：

```bash
any-switch import-current codex personal
```

## 添加静态 profile

如果目标状态可以由 API key、endpoint、provider、model 或环境值等显式字段构成，使用 `add`。可用字段来自所选 app definition 和 profile kind。

```bash
any-switch add <app> work --kind <kind> --field key=value
```

内置 Codex API key 状态示例：

```bash
any-switch add codex openai --kind file_template \
  --secret-field api_key=@prompt \
  --field model=gpt-5-codex \
  --field model_provider=openai
```

Claude 风格环境注入示例：

```bash
any-switch add claude proxy \
  --kind env_injection \
  --field base_url=https://example.test/api \
  --field models.default=example-model \
  --secret-field auth_token=@env:ANTHROPIC_AUTH_TOKEN
```

Secret 字段可以从交互式隐藏输入、stdin、环境变量或本地文件读取：

```bash
--secret-field api_key=@prompt
--secret-field api_key=@stdin
--secret-field api_key=@env:OPENAI_API_KEY
--secret-field api_key=@file:~/secrets/openai-api-key
```

日常交互配置建议使用 `@prompt`。脚本里可以使用 `@env:NAME`、`@stdin` 或 `@file:PATH`。避免把 secret 值直接写进 shell 命令。

## 切换 profile

先预览：

```bash
any-switch use <profile-id> --dry-run
```

应用 profile：

```bash
any-switch use <profile-id>
```

在交互式终端中，根据提示输入 `yes` 确认写入。脚本或 CI 中没有终端提示时使用 `--yes`。

对于动态 OAuth profile，`use` 会先把当前 active profile 对应的最新 live capture 写回，但前提是 live identity 仍然匹配 active profile。这样可以避免把一个账号的 credential 保存进另一个账号的 profile。

## 检查当前状态

快速比较：

```bash
any-switch status <app>
```

查看更详细诊断：

```bash
any-switch doctor <app>
```

这些命令都会脱敏 secret 值。

## 理解安全参数

`--yes` 用来非交互确认高风险动作，例如 `use`、`restore-target`、`remove` 或 `--assume-app-stopped`。交互式终端可以不传 `--yes`，在提示中输入 `yes`。两种确认方式都不会跳过身份检查、备份、锁、路径校验或 secret 脱敏。`add` 和普通 `import-current` 不接受 `--yes`，因为它们创建或捕获状态，而不是覆盖目标应用。`import-current --yes` 只在和 `--assume-app-stopped` 一起使用时有效。

`--allow-running` 只适用于静态、非 OAuth 写入，表示你有意接受在应用运行时编辑本地文件。

`--assume-app-stopped` 只用于进程敏感操作，且应用确实已经停止但进程检测错误。OAuth import、writeback 和 restore 流程使用这个参数，而不是 `--allow-running`。脚本中配合 `--yes`，交互式终端中输入 `yes`。如果没有检测到匹配进程，请去掉这个参数并重新运行命令。

## 处理常见错误

### DriftBeforeWriteback

live app identity 已经不再匹配 `any-switch` 当前认为 active 的 profile。切换会被阻断，避免把错误的 live state 写回旧 profile。

检查漂移：

```bash
any-switch status <app>
any-switch doctor <app>
```

如果 live state 是有价值的新 profile：

```bash
any-switch import-current <app> <new-name>
```

如果你想丢弃 live state 并恢复已保存的 profile：

```bash
any-switch detach <app>
any-switch use <profile-id>
```

### IdentityMissing

当前应用状态缺少 app definition 要求的身份字段。确认应用已经登录或配置好，然后运行：

```bash
any-switch doctor <app>
```

如果应用不是 OAuth 状态，使用正确的 `--kind` 重新 import，或者用 `add` 创建静态 profile。

### TargetMissing

应用没有当前 kind 所需的完整可导入状态。运行：

```bash
any-switch doctor <app>
```

对于 OAuth profile，查看 `definition_capture_source` 行。它们会显示当前平台 credential 来源，例如 Keychain 项或 credential 文件，是 `exists`、`missing`，还是因为无法确认而显示 `warning`。如果该行包含 `hint:`，先按对应提示处理。文件来源的 hint 通常意味着检查应用配置目录；macOS Keychain warning 建议在本机桌面终端运行 `doctor`，除非你明确需要显示 credential，否则不要使用带 `-w` 的 `security find-generic-password`。

### ImportAmbiguous

超过一条 import rule 匹配了当前应用状态。显式选择目标 kind：

```bash
any-switch import-current <app> <name> --kind <kind>
```

也可以清理应用 live config，让当前只剩一种状态。

### AppRunning

关闭目标应用后重试。OAuth 或进程敏感操作中，只有在进程检测误报时才使用 `--assume-app-stopped`。非交互运行中加 `--yes`，交互式终端中按提示输入 `yes`。

## 备份和恢复

写入受管理 target 前，`any-switch` 会创建备份。

列出备份：

```bash
any-switch backup list
```

从备份恢复应用：

```bash
any-switch restore-target <app> <backup-id>
```

`restore-target` 会恢复目标应用的 live state，但不会把某个 profile 标记为 active。恢复后运行 `any-switch status <app>`，检查恢复出的状态是否匹配 active profile。交互式终端中输入 `yes` 确认，脚本和 CI 中加 `--yes`。对于 OAuth 或进程敏感 target，restore 遵循和 switching 相同的 stop-app 规则。

删除不再需要的 profile：

```bash
any-switch remove <profile-id>
```

`remove` 会删除 profile 和它的 any-switch capture 文件。它不会清空或恢复目标应用当前的 live state。交互式终端中输入 `yes` 确认，脚本和 CI 中加 `--yes`。

## 编辑 profile

用编辑器打开已保存的 profile：

```bash
any-switch edit <profile-id>
```

`any-switch` 会依次使用 `$VISUAL`、`$EDITOR`，然后回退到构建目标平台的默认编辑器。保存前会校验修改后的 profile。

## 添加更多应用

用户应用定义放在：

```text
~/.any-switch/apps.d/*.yaml
```

常用命令：

```bash
any-switch apps validate <path>
any-switch apps show <app>
any-switch apps export <app> --source system
any-switch apps export <app> --source resolved
any-switch apps export <app> --as override --output ~/.any-switch/overrides.d/<app>.yaml
```

定义应该以声明式方式描述本地状态，并复用可信 handler，而不是要求核心 CLI 增加 app-specific 代码。

`--source system` 导出编译进 binary 的内置定义。`--source resolved` 导出用户定义和 override 应用后的最终定义。`--as override` 写出一个较窄的 override 起点，而不是完整替换定义。

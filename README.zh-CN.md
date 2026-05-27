# any-switch

[English](README.md)

`any-switch` 用来切换本机应用的 profile 和本地状态。

当一个应用有多套本地配置，而你不想手动编辑文件、复制 token、记住哪些配置项属于同一套状态时，可以使用它。

典型场景：

- 在 Claude Code 的个人账号和工作账号之间切换；
- 在 Codex 的 ChatGPT OAuth 和 API key provider 之间切换；
- 在任意受支持的本地工具之间切换不同 endpoint、模型、账号、workspace 或其他文件状态。

当前内置应用是 Claude Code 和 OpenAI Codex。工具本身并不只面向 AI CLI：应用定义描述哪些本地状态可以被捕获和恢复，`any-switch` 负责围绕这些状态做备份、脱敏、漂移检查和写入保护。

## 它做什么

`any-switch` 在你的机器上保存命名 profile。一个 profile 是某个应用要使用的一组本地状态，例如：

- 账号身份和 OAuth credential 状态；
- API key 和 provider 设置；
- 模型、endpoint 和环境设置；
- 应用定义声明的 JSON、TOML、文件、Keychain 或环境片段。

切换 profile 时，`any-switch` 会展示执行计划、创建备份、只写入声明过的目标，并避免在输出中打印 secret。

## 安装

`any-switch` 是源码构建型 CLI。安装命令会在你的机器上编译 Rust 二进制，而不是下载未签名的 macOS 或 Windows 二进制文件。

当前版本是 macOS-evidenced stage release：macOS Claude OAuth import 有真实本机证据；更完整的重启检查，以及 Linux 和 Windows 的真实应用证据仍作为后续工作跟踪。本版本不声明已经完整覆盖 `docs/design.md` 第 13 节。

### 1. 安装 Rust

```bash
rustup toolchain install 1.95.0
```

如果你还没有安装 `rustup`，请先从 <https://rustup.rs> 安装 Rust。

### 2. 安装 any-switch

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

检查安装结果：

```bash
any-switch --version
any-switch doctor
```

## 快速开始

查看当前构建内置或加载到的应用：

```bash
any-switch apps
```

把某个应用当前登录或配置状态捕获成 profile：

```bash
any-switch import-current <app> personal
```

列出已保存的 profile：

```bash
any-switch list
```

切换到某个 profile：

```bash
any-switch use <profile-id> --dry-run
any-switch use <profile-id>
```

检查当前状态：

```bash
any-switch status <app>
any-switch doctor <app>
```

内置应用示例：

```bash
any-switch import-current codex personal
any-switch import-current claude work --kind oauth_capture
any-switch use codex-personal
```

## 常见流程

### 保存当前应用状态

先用目标应用自己的方式完成登录或配置，然后运行：

```bash
any-switch import-current <app> personal
```

OAuth 状态应该走这个流程，因为真正的登录过程归目标应用所有。`any-switch` 只在登录完成后捕获本地状态，不替你登录。内置应用的例子：

```bash
any-switch import-current codex personal
any-switch import-current claude work --kind oauth_capture
```

### 添加静态 profile

当 profile 可以通过 API key、模型、provider 或 base URL 等字段描述时，使用 `add`。字段名由应用定义和 profile kind 决定：

```bash
any-switch add <app> work --kind <kind> --field key=value
```

内置 Codex 示例：

```bash
any-switch add codex openai --kind file_template \
  --secret-field api_key=@prompt \
  --field model=gpt-5-codex \
  --field model_provider=openai
```

Secret 字段可以从交互式隐藏输入、stdin、环境变量或本地文件读取：

```bash
--secret-field api_key=@prompt
--secret-field api_key=@stdin
--secret-field api_key=@env:OPENAI_API_KEY
--secret-field api_key=@file:~/secrets/openai-api-key
```

日常交互配置建议使用 `@prompt`。脚本里可以使用 `@env:NAME`、`@stdin` 或 `@file:PATH`。

### 写入前预览

使用 `--dry-run` 查看切换计划，不修改本机文件：

```bash
any-switch use <profile-id> --dry-run
```

### 从备份恢复目标

覆盖受管理目标之前，`any-switch` 会先创建备份。查看备份：

```bash
any-switch backup list
```

从指定备份恢复某个应用：

```bash
any-switch restore-target <app> <backup-id>
```

`restore-target` 会恢复目标应用的 live state，但不会把某个 profile 标记为 active。恢复后请运行 `any-switch status <app>` 检查结果。在交互式终端中输入 `yes` 确认；脚本或 CI 中使用 `--yes`。

## 安全说明

- Profile 默认存放在 `~/.any-switch`。这个目录可能包含静态 secret、OAuth capture 和防御性备份。不要把它放进 iCloud Drive、Dropbox、OneDrive、Google Drive 等云同步目录；`doctor` 会在检测到已知同步目录时报警。如果需要单独的状态目录，可以把 `ANY_SWITCH_HOME` 设置为 home 目录下的绝对路径。
- 普通命令输出和 JSON 输出会脱敏 secret 值。
- 不要提交 `~/.any-switch` 或任何生成的 profile/capture 文件。
- OAuth 或进程敏感操作前，请先退出目标应用。应用运行时 OAuth credential 可能旋转，所以 `--allow-running` 不适用于这些操作。
- `--assume-app-stopped` 只用于应用确实已经停止，但进程检测误报的情况；脚本中配合 `--yes`，交互式终端中也可以输入 `yes`。不要预先默认加这个参数：如果没有检测到匹配进程，`any-switch` 会拒绝这个参数并要求去掉后重试。
- 静态文件或环境 profile 可以使用 `--allow-running`，但先停止应用仍然更安全，因为应用可能会重写自己的配置文件。
- `--yes` 用来非交互确认高风险动作，例如 `use`、`restore-target`、`remove` 或 `--assume-app-stopped`。交互式终端可以不传 `--yes`，在提示中输入 `yes`。两种确认方式都不会跳过身份检查、备份检查、路径检查、锁、schema 校验或 secret 脱敏。`add` 和普通 `import-current` 不接受 `--yes`，因为它们是创建或捕获状态，不是覆盖目标应用。`import-current --yes` 只在和 `--assume-app-stopped` 一起使用时有效。

## 排查问题

先运行：

```bash
any-switch doctor
any-switch doctor <app>
any-switch status <app>
```

常见错误：

- `IdentityMissing`：当前应用状态缺少该 profile kind 要求的身份字段。确认应用已登录，然后再次运行 `doctor <app>`。
- `TargetMissing`：运行 `doctor <app>`，查看 `definition_capture_source` 行。它会显示当前平台的 credential 来源，例如 Keychain 项或 credential 文件，是 `exists`、`missing`，还是无法确认的 `warning`。如果 warning 行带有 `hint:`，先按提示处理。macOS Keychain 检查中，除非你明确要显示 credential，否则不要使用 `security find-generic-password -w`。
- `DriftBeforeWriteback`：live app state 已经不再匹配 active profile。运行 `status <app>` 检查。如果 live state 有价值，先把它 import 成新 profile，再切换走。
- `AppRunning`：退出目标应用后重试。只有在进程检测误报时才使用 `--assume-app-stopped`，并用 `--yes` 或交互式提示确认。
- `ImportAmbiguous`：传入 `--kind <kind>`，或清理应用当前 auth 文件，让只有一条 import rule 匹配。

## 自定义应用

`any-switch` 可以通过 `apps.d/*.yaml` 扩展应用定义。定义声明应用使用哪些本地目标，以及哪些可信 handler 可以捕获或写入它们。这样新应用可以复用同一套安全模型，而不需要在核心 CLI 中增加 app-specific 分支。

查看完整模型请读
[docs/design.md](https://github.com/riverscn/any-switch/blob/main/docs/design.md)。

查看或自定义应用定义：

```bash
any-switch apps show <app>
any-switch apps export <app> --source system
any-switch apps export <app> --source resolved
any-switch apps export <app> --as override --output ~/.any-switch/overrides.d/<app>.yaml
any-switch apps validate ~/.any-switch/overrides.d/<app>.yaml
```

## 更多文档

- [docs/user-guide.zh-CN.md](https://github.com/riverscn/any-switch/blob/main/docs/user-guide.zh-CN.md)：中文用户指南，包含常见流程、安全参数和排查说明。
- [docs/user-guide.md](https://github.com/riverscn/any-switch/blob/main/docs/user-guide.md)：英文用户指南。
- [docs/design.md](https://github.com/riverscn/any-switch/blob/main/docs/design.md)：架构和安全模型。
- [docs/manual-verification.md](https://github.com/riverscn/any-switch/blob/main/docs/manual-verification.md)：无法完全在 CI 中证明的真实应用检查。
- [docs/acceptance.md](https://github.com/riverscn/any-switch/blob/main/docs/acceptance.md)：验收覆盖。
- [docs/evidence-followups.md](https://github.com/riverscn/any-switch/blob/main/docs/evidence-followups.md)：完整声明第 13 节覆盖前的后续证据跟踪。
- [docs/release.md](https://github.com/riverscn/any-switch/blob/main/docs/release.md)：发布、打包和签名策略。
- [CHANGELOG.md](CHANGELOG.md)：面向用户的 release notes。
- [CONTRIBUTING.md](https://github.com/riverscn/any-switch/blob/main/CONTRIBUTING.md)：开发和贡献规则。
- [CODE_OF_CONDUCT.md](https://github.com/riverscn/any-switch/blob/main/CODE_OF_CONDUCT.md)：社区规范。
- [SECURITY.md](SECURITY.md)：漏洞报告。

## 开发

打开 PR 前运行本地验证：

```bash
scripts/verify-local.sh
scripts/verify-packages.sh
```

## License

MIT。见 [LICENSE](LICENSE)。

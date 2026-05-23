# switch-cli 设计文档

## 1. 项目定位

`switch-cli` 是一个本地命令行工具，用来快速切换 Codex 和 Claude Code 的登录账号。

它的实现方式不是调用第三方登录接口，而是管理这些 App 在用户目录下的配置文件：把不同账号对应的配置文件保存成具名快照，切换时用目标快照替换 App 当前使用的配置文件。

MVP 只解决一个明确问题：

```text
我已经在 Codex / Claude Code 中登录过多个账号，希望用一条命令在这些账号之间切换。
```

示例：

```bash
switch save codex work
switch save codex personal
switch use codex work

switch save claude work
switch use claude personal
switch status
```

## 2. 设计目标

### 2.1 MVP 目标

- 支持 Codex 和 Claude Code 两个内置 App。
- Codex 和 Claude Code 都通过内部 Account Module 实现，而不是写死在 core 里。
- 支持把当前登录状态保存为账号快照。
- 支持列出账号、查看当前账号、切换账号。
- 切换前能预览将要修改的文件。
- 切换前自动备份当前配置。
- 切换失败时尽量回滚到切换前状态。
- 默认不解析、不展示、不改写 token 或 credential 字段。
- 所有数据只保存在本机。

### 2.2 非目标

以下能力不进入 MVP：

- 通用状态管理框架。
- 系统代理、Git 身份、Shell 环境变量、服务进程等非账号切换能力。
- 运行时插件系统、外部模块安装和动态加载协议。
- 项目目录配置、trust / allow 机制和 shell hook。
- 远程同步、多机器同步、云备份。
- Secret backend、Keychain、1Password、pass 等凭证解析能力。
- 自动登录、刷新 token、校验账号是否仍可用。
- 图形界面和 TUI。

这些能力可以作为后续演进方向，但不能影响 MVP 的实现复杂度。

## 3. 设计原则

### 3.1 账号快照优先

工具管理的基本单元是 `account snapshot`，不是单个 token 字段。

Codex / Claude Code 的配置文件格式可能变化。MVP 不假设内部 schema 稳定，只把配置文件作为 opaque bytes 处理。

### 3.2 文件系统操作必须可预期

所有写操作必须满足：

- 写入前展示计划。
- 写入前备份当前配置。
- 写入时使用临时文件或临时目录。
- 尽量使用原子替换。
- 出错时明确提示已完成和未完成的动作。

### 3.3 默认不接触明文凭证语义

MVP 可以复制含凭证的配置文件，但不解析凭证、不打印凭证、不把凭证拆成字段管理。

日志、history、status、diff 只能展示路径、大小、mtime、hash 前缀和账号名，不能展示文件内容。

### 3.4 内部 Account Module 先行

MVP 实现内部 Account Module 边界，但不实现外部模块协议。

第一版内置两个 Account Module：

- `codex`
- `claude`

Account Module 负责声明需要管理哪些用户目录下的配置路径，以及提供 App 专属的 doctor 提示。core 只负责通用的保存快照、备份、替换、回滚和展示。

这条边界的目的有两个：

- 避免 core 写死 Codex / Claude Code 的分支逻辑。
- 降低开源贡献新 App 的门槛，贡献者只需要新增一个内部 Account Module。

### 3.5 按业务分 module，按能力分 core capability

Module 按业务状态域划分，例如：

- `codex`
- `claude`
- 后续可能的 `gemini`、`cursor`、`windsurf`

底层能力不作为 module，而是由 core 统一提供 capability：

- snapshot store
- backup store
- file replace
- directory replace
- lock
- hash
- permission handling
- redacted output

这样开源贡献者面对的是“我要支持哪个 App”，而不是“我要组合哪些底层能力”。同时，文件替换、权限、备份、回滚和脱敏这些安全边界保持在 core 内，避免每个业务模块各自实现一套。

### 3.6 后续扩展保留但不预支

设计允许以后新增 App，但 MVP 不实现插件协议。新增 App 的第一步是通过 PR 增加新的内部 Account Module，并补齐模块测试；当内置模块数量和维护成本明显上升后，再考虑 manifest、外部模块协议或插件生态。

## 4. 核心概念

### 4.1 App

一个可被切换账号的应用。

MVP 固定支持：

```text
codex
claude
```

### 4.2 Managed Path

App 当前登录状态依赖的配置路径。它可以是文件，也可以是目录。

示例结构：

```yaml
apps:
  codex:
    managed_paths:
      - path: ~/.codex/auth.json
        kind: file
        required: false

  claude:
    managed_paths:
      - path: ~/.claude.json
        kind: file
        required: false
      - path: ~/.claude/
        kind: dir
        required: false
```

上面的路径是 Account Module 配置示例，不是协议承诺。具体默认路径由内置 Account Module 维护，并允许用户在配置文件中覆盖。这样可以应对 Codex / Claude Code 未来调整配置文件位置。

### 4.3 Account

用户给某个登录状态起的名字，例如：

```text
work
personal
oss
```

同一个账号名只在某个 App 内唯一。`codex/work` 和 `claude/work` 是两个独立快照。

### 4.4 Snapshot

某个账号对应的一组配置文件副本。

快照必须包含 manifest：

```json
{
  "schema_version": 1,
  "app": "codex",
  "account": "work",
  "created_at": "2026-05-23T10:00:00Z",
  "source": "save-current",
  "files": [
    {
      "path": "~/.codex/auth.json",
      "kind": "file",
      "stored_as": "files/auth.json",
      "sha256": "..."
    }
  ]
}
```

### 4.5 Active Config

App 当前实际读取的用户目录配置文件。

`switch use <app> <account>` 的本质是：

```text
account snapshot -> active config
```

### 4.6 Backup

切换前从 active config 复制出来的安全副本。

backup 不是账号快照，不应该出现在账号列表里。它只用于恢复上一次切换前的状态。

### 4.7 Plan

一次命令将要执行的文件操作。

Plan 是只读预览，不包含文件内容：

```text
App: codex
Target account: work

Actions:
  backup ~/.codex/auth.json
  replace ~/.codex/auth.json from accounts/codex/work/files/auth.json
```

## 5. 本地数据布局

默认目录：

```text
config: ~/.config/switch-cli/config.yaml
data:   ~/.local/share/switch-cli/
state:  ~/.local/state/switch-cli/
```

macOS 可以继续使用这些 XDG 风格路径。Windows 后续再映射到 `%APPDATA%` / `%LOCALAPPDATA%`，不影响 MVP 的核心模型。

数据目录：

```text
~/.local/share/switch-cli/
  accounts/
    codex/
      work/
        manifest.json
        files/
      personal/
        manifest.json
        files/
    claude/
      work/
        manifest.json
        files/
  backups/
    codex/
      20260523T100000Z/
        manifest.json
        files/
  locks/
```

状态目录：

```text
~/.local/state/switch-cli/
  history.jsonl
```

`history.jsonl` 只记录操作元数据：

```json
{
  "time": "2026-05-23T10:00:00Z",
  "operation": "use",
  "app": "codex",
  "account": "work",
  "backup_id": "20260523T100000Z",
  "ok": true
}
```

## 6. 配置模型

MVP 配置只包含两个部分：

- 内部 Account Module 的 managed path 覆盖。
- CLI 行为偏好。

示例：

```yaml
version: 1

apps:
  codex:
    managed_paths:
      - path: ~/.codex/auth.json
        kind: file
        required: false
      - path: ~/.codex/config.toml
        kind: file
        required: false

  claude:
    managed_paths:
      - path: ~/.claude.json
        kind: file
        required: false
      - path: ~/.claude/
        kind: dir
        required: false
        exclude:
          - logs/**
          - cache/**

behavior:
  confirm_before_switch: true
  keep_backups: 20
```

合并规则：

- 内置默认值先加载。
- 用户配置覆盖内置默认值。
- 命令行参数覆盖用户配置。

MVP 不支持项目级配置和环境变量批量覆盖。

## 7. 命令设计

### 7.1 MVP 命令

| 命令 | 说明 |
|------|------|
| `switch apps` | 列出支持的 App |
| `switch accounts [<app>]` | 列出已保存的账号快照 |
| `switch status [<app>]` | 展示当前配置匹配哪个账号 |
| `switch save <app> <account>` | 把 App 当前登录状态保存为账号快照 |
| `switch use <app> <account>` | 切换到指定账号 |
| `switch use <app> <account> --dry-run` | 只展示计划，不修改文件 |
| `switch backup list [<app>]` | 查看自动备份 |
| `switch restore <app> <backup-id>` | 从自动备份恢复 |
| `switch doctor [<app>]` | 检查路径、权限、快照完整性 |
| `switch config path` | 输出配置文件路径 |

### 7.2 暂缓命令

以下命令不进入 MVP：

- `switch on/off/toggle`
- `switch set`
- `switch diff`
- `switch plan`
- `switch apply`
- `switch module ...`
- `switch plugin ...`
- `switch trust ...`
- `switch import <archive>`，以后如需从外部快照包导入再引入

如果后续需要通用状态模型，再引入 `plan/apply/diff` 这一套命令。账号切换 MVP 只需要 `use --dry-run` 即可覆盖预览需求。

`import` 不作为 MVP 命令名，因为它更像“从外部文件导入”。MVP 的实际动作是保存当前 App 的已登录配置，所以使用 `save`。

### 7.3 输出格式

默认输出面向人读：

```text
codex
  current: work
  managed files: 2
  drift: no

claude
  current: personal
  managed files: 3
  drift: yes
```

所有 MVP 命令支持 `--json`：

```bash
switch status --json
switch use codex work --dry-run --json
```

JSON 输出同样不能包含配置文件内容。

## 8. 执行流程

### 8.1 `switch save <app> <account>`

```text
load config
resolve account module
acquire app lock
detect managed paths
validate readable files
copy active config into temp snapshot directory
write snapshot manifest
atomically publish snapshot
append history
release lock
```

规则：

- 如果账号快照已存在，默认失败。
- 使用 `--force` 才允许覆盖已有快照。
- 保存时不修改 App 当前配置。
- 不存在且 `required: false` 的路径跳过并记录在 manifest 中。
- 不存在且 `required: true` 的路径导致保存失败。

### 8.2 `switch status [<app>]`

```text
load config
resolve account module
detect active config
hash active config
compare with known snapshots
print matched account or drift
```

状态结果：

| 状态 | 含义 |
|------|------|
| `matched` | 当前配置与某个账号快照完全一致 |
| `drifted` | 当前配置接近某个快照但文件 hash 不一致 |
| `unknown` | 当前配置不匹配任何已保存账号 |
| `missing` | App 配置文件不存在或不完整 |

MVP 的 `drifted` 可以先实现为 `unknown`。后续再做更细的相似度判断。

### 8.3 `switch use <app> <account>`

```text
load config
resolve account module
load target snapshot
acquire app lock
detect current active config
build plan
if dry-run: print plan and exit
ask confirmation unless --yes
create backup from active config
stage target files in temp location
replace active config
verify active config hash
append history
release lock
```

替换成功的判断：

- 所有目标 managed path 都存在。
- active config 的 hash 与目标 snapshot manifest 一致。
- 失败时能报告具体 path 和操作阶段。

### 8.4 `switch restore <app> <backup-id>`

`restore` 和 `use` 使用同一套替换流程，只是来源从账号快照换成 backup。

restore 前也要再创建一个新的 backup，避免恢复操作覆盖当前状态后无法反悔。

## 9. 文件操作协议

### 9.1 锁

每个 App 使用独立锁：

```text
~/.local/share/switch-cli/locks/<app>.lock
```

`save`、`use`、`restore` 必须持有锁。`status` 可以无锁读取，但如果读到不一致状态，应提示用户重试。

### 9.2 文件替换

文件替换流程：

```text
write target content to temp file in same directory
fsync temp file
preserve or set file permissions
rename temp file over active file
fsync parent directory when platform supports it
```

敏感文件默认权限：

```text
file: 0600
dir:  0700
```

如果 active file 已存在，应尽量继承原文件权限。

### 9.3 目录替换

目录替换比文件替换更难。MVP 采用保守策略：

```text
copy snapshot dir to temp dir under same parent
move current active dir to backup staging path
move temp dir to active path
verify
if verify fails, move backup staging path back
```

目录 managed path 应尽量只覆盖确认为登录状态所需的目录。对缓存、日志、临时文件应使用 `exclude` 排除。

### 9.4 Symlink

默认不跟随指向用户目录外部的 symlink。

规则：

- managed path 本身是 symlink 时，`doctor` 必须展示真实路径。
- 真实路径在用户 home 外时，MVP 默认拒绝写入。
- 用户可以通过后续配置显式允许外部路径，但这不进入第一版。

### 9.5 Hash

hash 用于判断快照和当前配置是否一致。

规则：

- 文件 hash 使用 SHA-256。
- 目录 hash 包含相对路径、文件类型和文件内容 hash。
- hash 不包含 mtime。
- status 只展示 hash 前缀，例如前 8 位。

## 10. Account Module

### 10.1 模块职责

内部 Account Module 是编译进二进制的静态模块，不是运行时插件。

Account Module 按业务划分，一个模块对应一个可切换账号的 App。它回答的是“管理哪个 App 的账号状态”，而不是“如何执行文件操作”。

模块只负责声明：

- App id。
- 展示名。
- 默认 managed paths。
- 哪些路径是 required。
- 目录路径的 exclude 规则。
- doctor 检查提示。

模块不负责：

- 解析 token。
- 调登录接口。
- 刷新凭证。
- 判断账号在服务端是否有效。
- 执行保存、切换、备份和恢复。

这些文件操作由 core 统一实现。

### 10.2 Core capability 职责

以下能力属于 core capability，不属于 Account Module：

- 读取 active config。
- 保存 account snapshot。
- 创建 backup。
- 文件原子替换。
- 目录替换。
- 文件锁。
- hash 计算。
- 权限继承和敏感文件默认权限。
- history 写入。
- 输出脱敏。

这些能力必须集中实现，原因是它们决定了账号切换的安全性和一致性。业务模块不能绕过 core 直接修改用户配置文件。

模块和 core 的关系：

```text
Account Module -> declares managed paths / excludes / doctor hints
Core           -> performs save / use / status / backup / restore safely
```

内部接口应保持窄而稳定，概念上类似：

```rust
trait AccountModule {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn default_managed_paths(&self) -> Vec<ManagedPath>;
    fn doctor(&self, ctx: &Context) -> DoctorResult;
}
```

MVP 使用静态 registry：

```text
registry.register(CodexModule)
registry.register(ClaudeModule)
```

不做动态发现、安装、卸载、版本协商或子进程协议。

### 10.3 Codex Module

MVP 设计要求：

- 默认管理 Codex 在用户目录下的认证和必要配置文件。
- 允许用户覆盖 managed paths。
- `doctor` 输出当前检测到的 Codex 配置路径。
- 未检测到默认路径时，提示先登录 Codex 或手动配置 managed paths。

文档不把某个具体 Codex 配置路径写成永久协议。Codex 的实际文件布局由实现中的 Codex Module 维护。

### 10.4 Claude Code Module

MVP 设计要求：

- 默认管理 Claude Code 在用户目录下的认证和必要配置文件。
- 支持文件和目录两种 managed path。
- 对目录 managed path 默认排除缓存、日志和临时文件。
- `doctor` 输出当前检测到的 Claude Code 配置路径。

文档不把某个具体 Claude Code 配置路径写成永久协议。Claude Code 的实际文件布局由实现中的 Claude Code Module 维护。

### 10.5 开源贡献边界

开源后，新增 App 的主要贡献路径是提交一个新的内部 Account Module。

建议源码组织：

```text
src/
  account_modules/
    mod.rs
    codex.rs
    claude.rs
```

每个模块文件只放该 App 的路径声明、exclude 规则和 doctor 提示，不放通用文件复制逻辑。

一个新模块至少需要包含：

- 默认 managed paths。
- 默认 exclude 规则。
- doctor 检查。
- 路径展开和 home 目录边界测试。
- fixture hash 测试。
- 不输出 managed file 内容的测试。

MVP 不要求贡献者理解插件生命周期、外部协议、安装路径或权限模型。等内部模块数量足够多、发布节奏开始受影响时，再把这套内部接口外化为 manifest 或外部模块协议。

## 11. 安全和隐私

### 11.1 不展示文件内容

任何命令默认都不能打印 managed path 的文件内容。

允许展示：

- path
- kind
- exists
- size
- mtime
- sha256 prefix
- snapshot name
- backup id

不允许展示：

- token
- API key
- session
- cookie
- 完整配置文件内容

### 11.2 不解析 credential 字段

MVP 不维护 secret schema。所有 credential 都只是 opaque file content。

这降低功能复杂度，也避免工具承担凭证解析和脱敏的额外责任。

### 11.3 备份保留

默认保留最近 20 个 backup。

清理 backup 必须满足：

- 只清理 `switch-cli` 自己创建的 backup。
- 不清理 account snapshot。
- 清理动作可通过 `doctor` 或后续 `backup prune` 展示。

MVP 可以先不自动清理，只在超过阈值时提示。

### 11.4 并发

同一 App 的写操作互斥。不同 App 可以并行切换。

如果 Codex 或 Claude Code 正在运行，MVP 不强制终止进程。`doctor` 可以提示用户：运行中的 App 可能缓存旧账号状态，切换后可能需要重启 App。

## 12. 错误类型

| 错误 | 含义 |
|------|------|
| `AppNotFound` | 不支持的 App |
| `AccountNotFound` | 指定账号快照不存在 |
| `AccountExists` | 保存目标账号已存在 |
| `ManagedPathMissing` | required managed path 不存在 |
| `PermissionDenied` | 配置文件不可读或不可写 |
| `SnapshotCorrupt` | 快照 manifest 和文件不一致 |
| `BackupFailed` | 切换前备份失败 |
| `ReplaceFailed` | 替换 active config 失败 |
| `VerifyFailed` | 替换后 hash 校验失败 |
| `LockBusy` | 另一个写操作正在执行 |

错误输出应包含下一步建议：

```text
codex: switch failed at replace step
path: ~/.codex/auth.json
reason: permission denied
backup: 20260523T100000Z
next: switch restore codex 20260523T100000Z
```

## 13. MVP 验收标准

MVP 完成的最低标准：

1. 能保存当前 Codex 配置为 `codex/<account>`。
2. 能保存当前 Claude Code 配置为 `claude/<account>`。
3. 能在两个已保存账号之间切换。
4. `status` 能识别当前配置匹配的账号。
5. `use --dry-run` 能展示将要修改的路径。
6. 每次 `use` 前都会创建 backup。
7. `restore` 能从 backup 恢复。
8. 日志和输出不会包含配置文件内容。
9. 权限不足、路径不存在、快照损坏时有明确错误。
10. 文档中明确说明：切换后可能需要重启 Codex / Claude Code。
11. core 通过 Account Module registry 查找 App，不包含 Codex / Claude Code 专属分支。

## 14. 后续演进

只有当 MVP 稳定后，才考虑以下方向：

### Phase 2: 更多 App

- 增加 Gemini CLI、OpenAI CLI 或其他本地账号配置切换。
- 通过 PR 增加新的内部 Account Module。
- 增加模块测试模板和 fixture 约定。
- 继续使用静态 registry，不引入运行时安装。

### Phase 3: 体验增强

- `switch rename <app> <old> <new>`
- `switch remove <app> <account>`
- `switch backup prune`
- shell completion
- 更好的 drift 展示

### Phase 4: 同步和加密

- 可选加密快照。
- 可选跨机器同步。
- 与系统 Keychain 集成。

### Phase 5: 通用状态模型

如果账号切换场景之外出现明确需求，再考虑引入通用状态管理能力：

- state / desired / plan / apply
- 外部模块协议
- 插件系统
- 项目 scope
- trust / allow 机制

外部模块协议只有在以下条件同时成立时才值得设计：

- 内部 Account Module 数量已经足够多。
- 第三方贡献者需要在不改 core 仓库的情况下发布模块。
- 安装、权限、版本兼容和 contract test 的维护成本有明确收益。

这些能力不应反向污染账号切换 MVP。

## 15. 当前设计结论

`switch-cli` 第一版应该是一个小而可靠的账号配置切换器，而不是通用自动化框架。

最重要的边界是：

1. 核心只做本地文件快照和切换。
2. Codex / Claude Code 作为内部 Account Module 静态注册。
3. 配置文件内容按 opaque bytes 处理。
4. 每次写入前备份，每次切换后校验。
5. 后续扩展先通过 PR 新增内部模块，不在 MVP 引入插件和外部模块生态。

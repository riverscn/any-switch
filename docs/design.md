# switch-cli 设计文档

## 1. 项目定位

`switch-cli` 是一个本地命令行工具，用来管理 Codex 和 Claude Code 等 AI CLI 工具的 profile，并在它们之间快速切换。第一阶段的 profile 主要承载账号、凭据、服务端点和模型配置。

工具不接入任何第三方登录接口。它把每个 profile 建模为一条**结构化记录**：

- 对于通过环境变量或配置文件投递的 API key / 第三方代理：记录里直接存语义字段。
- 对于通过 OAuth 登录的 Anthropic 官方账号、ChatGPT 官方账号：记录里存身份指纹 + 动态凭据 capture，并在每次切换时**双向同步**，避免 refresh token 旋转把存档作废。

MVP 解决一个明确问题：

```text
我在 Codex / Claude Code 中需要在多个账号、服务端点或模型 profile 之间切换，
希望一条命令搞定，不用反复手动改 settings.json 或重新登录。
```

示例：

```bash
# 把当前 Claude Code 的 Anthropic OAuth 登录捕获为一条 profile (oauth_capture)
switch import-current claude anthropic-work

# 添加一个 GLM 第三方代理 profile (env_injection)
switch add claude glm \
  --kind env_injection \
  --base-url https://open.bigmodel.cn/api/anthropic \
  --auth-token sk-xxx

# 切到 GLM 代理
switch use claude-glm

# 切回 Anthropic 官方 OAuth（不再需要重新登录）
switch use claude-anthropic-work

# Codex 当前是 ChatGPT OAuth 登录，捕获为 profile (oauth_capture)
switch import-current codex chatgpt-work

switch list
switch status

# 长期未切到该 profile、refresh token 被服务端旋转作废时，重新登录并回灌
switch reauth claude-anthropic-work
```

## 2. 设计目标

### 2.1 MVP 目标

- 支持 Claude Code 和 Codex 两个内置 App。
- Profile 建模为结构化记录，可由编辑器手编，也可由 `import-current` 自动捕获。
- 支持四种凭据投递方式（kind）：
  - `env_injection`：写入目标 JSON 文件的 env 块（Claude Code 第三方代理场景）。
  - `file_template`：把语义字段渲染成一个或多个目标文件（Codex API-key 登录）。
  - `oauth_capture`：复合源 OAuth 凭据 + 身份指纹的捕获与回放（Claude 官方、Codex ChatGPT 登录）。
  - `opaque_capture`：纯不透明 blob 捕获（schema 保留，MVP 暂无实例）。
- `oauth_capture` 在切换时执行**双向写回**——切换前先把当前活动 profile 的最新 token 回写到它自己的 capture 中，再加载目标 profile，避免 refresh token 旋转造成静默失效。
- 切换前能预览将要修改的内容（不含明文 secret）。
- 切换前对将被覆盖的目标文件做防御性备份。
- 切换失败时尽量回滚到操作前状态。
- 默认不打印任何 secret 字段，不输出文件原文。
- 提供 `switch reauth <id>` 命令处理 server-side refresh 失效的情况。
- 所有数据只保存在本机。

### 2.2 非目标

以下能力不进入 MVP：

- 通用状态管理框架。
- 系统代理、Git 身份、Shell 环境变量、服务进程等非 AI CLI profile 切换能力。
- 运行时插件系统、外部模块安装和动态加载协议。
- 项目目录配置、trust / allow 机制和 shell hook。
- 远程同步、多机器同步、云备份。
- 通用 secret backend（1Password / pass / Bitwarden 等）集成。
- 解析 OAuth blob 内部结构（JWT 签名校验、scopes 语义判断等）——MVP 只 base64-decode JWT payload 取 `exp` 用作过期提示。
- 自动登录、刷新 token、调用 Anthropic / OpenAI 任何业务 API。
- 校验账号凭据在服务端是否仍可用——`reauth` 走 App 自己的 `/login` 流程，不绕过它。
- GUI 和 TUI。
- Linux Secret Service、Windows Credential Manager backend。MVP 仅实现 macOS Keychain backend 和 Linux 文件源。

## 3. 设计原则

### 3.1 Profile 是结构化记录，不是文件快照

工具管理的基本单元是 `profile record`：一个有 `id` / `app` / `kind` / `fields` 或 `identity + capture` 的结构化对象。第一阶段的 profile 主要表示账号和凭据组合，但这个名字为后续承载模型、provider、endpoint 等 App 级配置留下空间。结构化记录的好处：

- 用户可以直接 `vim` 编辑 profile。
- 加密 / 解密、字段升级、批量改 model 等未来需求都有锚点。
- 不同 App 的差异显式表达在 `kind` 和 `fields` 上，core 不写死分支。
- OAuth 这类动态凭据可以在 record 里同时存"不变的身份"（identity）和"会变的凭据"（capture），两者独立处理。

### 3.2 四种 kind 覆盖现实世界的凭据形态

| kind | 适用场景 | 凭据语义 | 切换原子性 |
|------|----------|----------|------------|
| `env_injection` | 通过环境变量配置（Claude Code 走 `settings.json.env`）| 静态语义字段 | 单文件 JSON 子树合并 |
| `file_template` | 读取专属配置文件、字段可识别（Codex API-key 模式）| 静态语义字段 | 多文件渲染 + 原子替换 |
| `oauth_capture` | 可刷新的 OAuth 登录态（Claude 官方、Codex ChatGPT）| 身份指纹 + 动态 capture，切换时双向写回 | 多源原子切换 |
| `opaque_capture` | 无刷新、无身份语义的纯 blob | 单一 opaque blob | 整体替换；MVP 暂无实例 |

每个 App Module 声明自己支持哪几种 kind，并提供"profile 字段 / capture → 目标文件内容"的转换器。

### 3.3 文件系统操作必须可预期

所有写操作满足：

- 写入前展示计划。
- 写入前对所有将被覆盖的目标位置做防御性备份。
- 写入时使用临时文件 + 原子 rename。
- 替换后用 hash 校验；OAuth kind 额外校验身份指纹。
- 出错时明确提示已完成和未完成的动作。

### 3.4 OAuth 凭据视作动态资产

`oauth_capture` kind 的核心约束：

- **写回先行**：切换前先把当前 Keychain / 凭据文件的最新内容回写到当前活动 profile 的 capture，再写入目标 profile。
- **进程互斥强制**：OAuth kind 切换前如果检测到目标 App 在运行，**拒绝**执行——`--allow-running` 不对 OAuth kind 生效。这是因为 App 运行时会刷新 token，原子性无法保证。
- **身份校验**：切换后从恢复的状态里读出 `account_uuid` / `organization_uuid`，与 capture 的 `identity` 比对，不一致即视为失败回滚。
- **过期感知**：capture 记录 `expires_at`（从 JWT 解出）和 `captured_at`，切换时如果检测到 capture 已远超合理时长，警告用户可能需要 reauth。
- **失效降级**：refresh 失败时不假装成功，引导用户运行 `switch reauth <id>` 走 App 自身 `/login` 流程后重新捕获。

### 3.5 默认不打印 secret 内容

工具复制和写入含凭据的数据，但**默认不打印任何 secret 字段值**。允许打印：

- profile id / name / app / kind / created_at
- 非敏感字段（model 名、model_provider、reasoning_effort 等）
- `identity` 块（account_uuid / organization_name / email 等——这些不是 secret）
- 目标位置 path / size / mtime / sha256 prefix
- capture source 的 metadata（type / path / sha256 前缀）

敏感字段（字段名匹配 `*token*` / `*key*` / `*secret*` / `*password*`，或 App Module 标 `sensitive: true`，以及所有 capture blob 的内容）默认脱敏为 `***`。

### 3.6 按业务分 module，按能力分 core capability

App Module 按业务状态域划分（claude / codex / 未来 gemini / cursor），声明：

- 支持的 kind 列表。
- 各 kind 的 field schema 和 capture source spec。
- `managed_env_keys`（env_injection 用）、`managed_json_subtrees`（oauth_capture 用）等"管理边界"声明。
- "记录 → 目标"渲染逻辑、"目标 → 记录" import 逻辑。
- 身份指纹提取逻辑（oauth_capture 用）。
- reauth 引导（oauth_capture 用）。
- doctor 检查与进程探测。

Core 提供：

- config.yaml 加载、迁移、保存。
- captures/ 目录管理。
- 防御性备份与恢复。
- 文件原子替换、JSON 子树合并、Keychain backend、锁、hash、权限。
- 输出脱敏。

### 3.7 多轴可扩展，但 MVP 不预支

设计在多个轴上预留扩展位（顶层 / 单记录双层 `schema_version`、`extensions` 自由字段、`kind` 可枚举、`capture.source.type` 可枚举、`backend` 可枚举），但 MVP 只实现最小必要集合。新增 App、kind 或 backend 通过 PR 增加内置 Module，不引入运行时插件。

## 4. 核心概念

### 4.1 App

一个可被切换 profile 的应用。MVP 固定支持：

```text
codex
claude
```

### 4.2 Profile

用户管理的一条 profile 记录。至少包含 `id` / `app` / `kind` / `name`，其余字段随 kind 变化。profile id 全局唯一。

### 4.3 Kind

profile 的投递方式。第一阶段主要是凭据投递方式。MVP 四种：`env_injection`、`file_template`、`oauth_capture`、`opaque_capture`（保留）。

每个 App Module 声明它支持哪些 kind。例如：

- Claude 支持 `env_injection`（第三方代理）和 `oauth_capture`（官方账号）。
- Codex 支持 `file_template`（API-key 模式）和 `oauth_capture`（ChatGPT 登录）。

### 4.4 Fields

`env_injection` 和 `file_template` 类 profile 的语义字段集合：

```yaml
fields:
  base_url: "https://api.anthropic.com"
  auth_token: "sk-ant-..."
  models:
    default: claude-sonnet-4-6
```

字段名是 switch-cli 内部约定，由 App Module 渲染成目标格式的实际字段名（如 `ANTHROPIC_AUTH_TOKEN`）。

### 4.5 Identity（oauth_capture 专用）

`oauth_capture` profile 的不变身份指纹。**不含 secret，用于校验和展示**：

```yaml
identity:
  account_uuid: "5f3e..."
  organization_uuid: "a1b2..."
  organization_name: "Personal"
  email: "work@example.com"
  subscription_type: "pro"
```

切换后从恢复的状态读出对应字段，与 identity 比对，不一致即回滚。identity 还用于：

- `list` / `show` 输出时给用户辨识用。
- `import_current` 时去重（同 `account_uuid` 已存在 → 提示用户更新现有记录而不是新建）。

### 4.6 Capture

`oauth_capture` 和 `opaque_capture` profile 的 blob 引用。每个 capture 描述若干 source，每个 source 标注类型、存储位置、sha256、可选的平台限定。

```yaml
capture:
  sources:
    - type: secret_entry
      backend: macos_keychain
      service: "Claude Code-credentials"
      account: "${MACOS_USER}"
      stored_as: captures/claude-anthropic-work/keychain.json
      sha256: 7a3f...
      platforms: [macos]
    - type: file
      path: ~/.claude/.credentials.json
      stored_as: captures/claude-anthropic-work/credentials.json
      sha256: 7a3f...
      platforms: [linux]
    - type: json_subtree
      path: ~/.claude.json
      json_path: $.oauthAccount
      stored_as: captures/claude-anthropic-work/oauthAccount.json
      sha256: c1d2...
    - type: json_subtree
      path: ~/.claude.json
      json_path: $.userID
      stored_as: captures/claude-anthropic-work/userID.txt
      sha256: e4f5...
```

支持的 source type：

| type | 含义 | MVP 实现 |
|------|------|----------|
| `file` | 整个文件 | ✓ |
| `secret_entry` | 系统 secret store 中一条具名条目 | ✓（仅 macOS Keychain）|
| `json_subtree` | 某个 JSON 文件中某个 JSONPath 子树（部分写入）| ✓ |

blob 内容不内联到 config.yaml，而在 `~/.local/share/switch-cli/captures/<id>/` 下，目录 `0700`、文件 `0600`。

### 4.7 Freshness（oauth_capture 专用）

```yaml
freshness:
  captured_at: "2026-05-23T10:00:00Z"
  expires_at: "2026-05-23T11:00:00Z"          # 从 accessToken JWT exp 字段解出
  last_writeback_at: "2026-05-23T15:30:00Z"   # 最近一次切走该 profile 时的 writeback 时间
```

`expires_at` 只用于切换前的过期提示，**不**用于判断该 profile 能否切（access_token 过期不代表 refresh_token 失效）。`last_writeback_at` 用于审计：如果一个 profile 从来没被切换过、`captured_at` 已经很久远，refresh_token 大概率已被 server 端旋转作废，切过去会失败。

### 4.8 Target

App Module 为某个 kind 声明的"目标位置"，是切换时被改写的对象。

- Claude `env_injection` target：`~/.claude/settings.json` 的 `$.env` 子树。
- Claude `oauth_capture` target：Keychain entry + `~/.claude.json` 的 `$.oauthAccount` 与 `$.userID` 子树。
- Codex `file_template` target：`~/.codex/auth.json` + `~/.codex/config.toml`。
- Codex `oauth_capture` target：`~/.codex/auth.json`（+ 可选 `~/.codex/config.toml`）。

### 4.9 Defensive Backup

切换前对所有将被改写的目标位置自动建立的备份。它是防御性的，不是 profile 备份——目的是用户在目标文件里的手工改动（MCP 配置、自定义 Codex profile 等）丢失后可以恢复。

backup 不出现在 profile 列表里。Keychain entry 和 JSON 子树同样要进 backup（前者以 JSON 文件形式落地，后者以原值落地）。

### 4.10 Plan

一次命令将要执行的操作的只读预览。Plan 不包含 secret 字段值和 capture blob 内容：

```text
App: claude
Profile: claude-anthropic-work  (oauth_capture)
Identity: work@example.com  org=Personal  uuid=5f3e...

Writeback (current active profile: claude-anthropic-personal):
  refresh capture from:
    macOS Keychain  service="Claude Code-credentials"
    ~/.claude.json  $.oauthAccount, $.userID

Targets:
  macOS Keychain  service="Claude Code-credentials"
    write ***  (sha256 7a3f... from capture)
  ~/.claude.json
    write $.oauthAccount (5f3e..., Personal, work@example.com)
    write $.userID
  defensive backup: backups/claude/20260523T100000Z/

Post-write verify: account_uuid must equal 5f3e...
```

## 5. 本地数据布局

```text
config: ~/.config/switch-cli/config.yaml   # 用户配置 + profile 记录（source of truth）
data:   ~/.local/share/switch-cli/         # 捕获 blob 和防御性备份
state:  ~/.local/state/switch-cli/         # 活动 profile 指针和历史
```

数据目录：

```text
~/.local/share/switch-cli/
  captures/
    claude-anthropic-work/
      keychain.json          # 或 credentials.json on Linux
      oauthAccount.json
      userID.txt
    codex-chatgpt-work/
      auth.json
      config.toml
  backups/
    claude/
      20260523T100000Z/
        settings.json
        keychain.json
        oauthAccount.json
        userID.txt
        manifest.json
    codex/
      20260523T101500Z/
        auth.json
        config.toml
        manifest.json
  locks/
    claude.lock
    codex.lock
    config.lock
```

状态目录：

```text
~/.local/state/switch-cli/
  active.json
  history.jsonl
```

`active.json`：

```json
{
  "schema_version": 1,
  "active_profiles": {
    "claude": "claude-anthropic-work",
    "codex": "codex-chatgpt-work"
  }
}
```

`history.jsonl` 每行一条记录，只含元数据：

```json
{
  "time": "2026-05-23T10:00:00Z",
  "operation": "use",
  "app": "claude",
  "from_profile": "claude-anthropic-personal",
  "to_profile": "claude-anthropic-work",
  "kind": "oauth_capture",
  "writeback_ok": true,
  "verify_ok": true,
  "backup_id": "20260523T100000Z",
  "ok": true
}
```

## 6. 配置模型

`config.yaml` 是 profile 记录的 source of truth，同时承载 CLI 行为偏好。

```yaml
schema_version: 1

preferences:
  default_app: claude
  confirm_before_switch: true
  keep_backups: 20
  redact_secrets: true
  oauth_stale_warn_days: 30        # capture 超过这个天数未刷新时切换前警告

profiles:
  - id: claude-anthropic-work
    app: claude
    kind: oauth_capture
    schema_version: 1
    name: "Anthropic Pro (work)"
    notes: ""
    created_at: 2026-05-23T10:00:00Z
    identity:
      account_uuid: "5f3e..."
      organization_uuid: "a1b2..."
      organization_name: "Personal"
      email: "work@example.com"
      subscription_type: "pro"
    capture:
      sources:
        - type: secret_entry
          backend: macos_keychain
          service: "Claude Code-credentials"
          account: "${MACOS_USER}"
          stored_as: captures/claude-anthropic-work/keychain.json
          sha256: 7a3f...
          platforms: [macos]
        - type: file
          path: ~/.claude/.credentials.json
          stored_as: captures/claude-anthropic-work/credentials.json
          sha256: 7a3f...
          platforms: [linux]
        - type: json_subtree
          path: ~/.claude.json
          json_path: $.oauthAccount
          stored_as: captures/claude-anthropic-work/oauthAccount.json
          sha256: c1d2...
        - type: json_subtree
          path: ~/.claude.json
          json_path: $.userID
          stored_as: captures/claude-anthropic-work/userID.txt
          sha256: e4f5...
    freshness:
      captured_at: "2026-05-23T10:00:00Z"
      expires_at: "2026-05-23T11:00:00Z"
      last_writeback_at: null
    extensions: {}

  - id: claude-anthropic-personal
    app: claude
    kind: oauth_capture
    schema_version: 1
    name: "Anthropic Free (personal)"
    identity:
      account_uuid: "8d2a..."
      organization_uuid: "b3c4..."
      organization_name: "Personal"
      email: "personal@example.com"
      subscription_type: "free"
    capture:
      sources:
        - type: secret_entry
          backend: macos_keychain
          service: "Claude Code-credentials"
          account: "${MACOS_USER}"
          stored_as: captures/claude-anthropic-personal/keychain.json
          sha256: 9b1e...
          platforms: [macos]
        - type: file
          path: ~/.claude/.credentials.json
          stored_as: captures/claude-anthropic-personal/credentials.json
          sha256: 9b1e...
          platforms: [linux]
        - type: json_subtree
          path: ~/.claude.json
          json_path: $.oauthAccount
          stored_as: captures/claude-anthropic-personal/oauthAccount.json
          sha256: d4e5...
        - type: json_subtree
          path: ~/.claude.json
          json_path: $.userID
          stored_as: captures/claude-anthropic-personal/userID.txt
          sha256: f6a7...
    freshness:
      captured_at: "2026-05-15T08:00:00Z"
      expires_at: null
      last_writeback_at: "2026-05-23T09:55:00Z"
    extensions: {}

  - id: claude-glm
    app: claude
    kind: env_injection
    schema_version: 1
    name: "GLM (智谱代理)"
    fields:
      base_url: "https://open.bigmodel.cn/api/anthropic"
      auth_token: "..."
      models:
        default: glm-4.6
    extensions: {}

  - id: codex-openai-personal
    app: codex
    kind: file_template
    schema_version: 1
    name: "OpenAI 官方 (personal)"
    fields:
      api_key: "sk-..."
      model: gpt-5-codex
      model_provider: openai
    extensions: {}

  - id: codex-chatgpt-work
    app: codex
    kind: oauth_capture
    schema_version: 1
    name: "ChatGPT 登录 (work)"
    identity:
      account_uuid: "..."
      email: "work@example.com"
    capture:
      sources:
        - type: file
          path: ~/.codex/auth.json
          stored_as: captures/codex-chatgpt-work/auth.json
          sha256: 7a3f...
        - type: file
          path: ~/.codex/config.toml
          stored_as: captures/codex-chatgpt-work/config.toml
          sha256: c1d2...
          required: false
    freshness:
      captured_at: "2026-05-23T10:00:00Z"
      expires_at: null
      last_writeback_at: null
    extensions: {}
```

### 6.1 Schema 约束

- 顶层 `schema_version` 标识整体配置文件版本。Bump 仅在出现破坏性改动时；新增字段不 bump。
- 每条 profile 的 `schema_version` 标识该 `app + kind` 组合的字段 schema 版本，可独立演进。
- 读到比当前二进制更高的 `schema_version` → 拒绝写入（read-only 模式），并提示用户升级 CLI。
- 读到旧 `schema_version` → 自动迁移到新版本；迁移失败时备份原 config.yaml 并以只读模式运行。
- 未识别的 `extensions` 字段在迁移期间必须保留。

### 6.2 字段规范

各 App / kind 的字段 schema 由对应 App Module 维护。所有 kind 共享的约定：

- `id` 必须满足 `^[a-z0-9][a-z0-9-]{0,63}$`。`switch add` / `import-current` 自动生成的 id 形如 `<app>-<slug(name)>`，可用 `--id` 显式指定覆盖。
- `slug(s)` 的定义：转小写 → 把任何不在 `[a-z0-9]` 范围的字符替换为 `-` → 合并连续 `-` → 去掉首尾 `-`。最终 id（含 `<app>-` 前缀）总长必须 ≤ 64 字符，超长则截断 slug 部分并保留前缀。slug 结果为空时（如全中文 name）报错要求用户显式 `--id`。
- `name` 是任意 UTF-8 字符串，仅用于展示。
- `notes` 可选，多行字符串。
- `created_at` 由 CLI 自动写入。
- `extensions` 是开放对象。
- `oauth_capture` 必须含 `identity` 和 `capture.sources`，可选 `freshness`。
- `${MACOS_USER}` 这类占位符在加载时展开（仅支持 `MACOS_USER`，从 `getpwuid(getuid())` 取），不信任 `$USER` 环境变量。

### 6.3 命令行覆盖

命令行参数覆盖 `preferences`。没有项目级配置和环境变量覆盖。

### 6.4 文件权限

`config.yaml` 强制 `0600`。父目录 `~/.config/switch-cli/` 强制 `0700`。启动时若权限被改宽，doctor 警告并提示修复。

## 7. 命令设计

### 7.1 MVP 命令

| 命令 | 说明 |
|------|------|
| `switch apps` | 列出支持的 App 及其支持的 kind |
| `switch list [<app>]` | 列出已注册 profile（脱敏） |
| `switch show <id>` | 查看单个 profile 的元数据、identity、非敏感字段 |
| `switch add <app> <name> [--id <id>] [--kind <kind>] [--field k=v ...]` | 手动添加 env_injection / file_template profile |
| `switch edit <id>` | 用 `$EDITOR` 打开 profile 的 yaml 片段编辑 |
| `switch remove <id>` | 删除 profile；同时清理 captures/<id>/ |
| `switch import-current <app> <name> [--id <id>]` | 从 App 当前状态自动捕获一条 profile |
| `switch use <id>` | 切换到指定 profile（oauth_capture 会先写回当前活动 profile）|
| `switch use <id> --dry-run` | 只打印 plan，不写入 |
| `switch reauth <id>` | 引导用户重新登录 App 并把新凭据回灌到该 oauth_capture profile |
| `switch status [<app>]` | 显示每个 App 的活动 profile、是否 drift |
| `switch backup list [<app>]` | 列出防御性备份 |
| `switch restore-target <app> <backup-id>` | 从防御性备份恢复目标位置 |
| `switch doctor [<app>]` | 检查路径、权限、字段完整性、进程状态、Keychain 可访问性 |
| `switch config path` | 打印 config.yaml 路径 |

### 7.2 命令选项约定

- `--yes` / `-y`：跳过交互确认。
- `--json`：以 JSON 格式输出（同样脱敏）。
- `--allow-running`：在目标 App 进程运行时仍允许切换。**对 `oauth_capture` 不生效**——切 OAuth profile 永远要求 App 退出。
- `--force`：仅在 `add` 同 id 覆盖、`remove` 跳过确认时使用。
- 默认所有写命令在交互式 TTY 下请求确认；非交互式必须显式 `--yes`。

### 7.3 暂缓命令

不进入 MVP：

- `switch rename`（用 `remove` + `add` 替代）。
- `switch diff` / `plan` / `apply` 通用三件套。
- `switch backup prune`（MVP 自动按 `keep_backups` 修剪）。
- `switch export` / `import` 跨机器迁移。
- shell completion 脚本。
- `switch module install`、`switch plugin`、`switch trust` 等插件 / 沙箱概念。

### 7.4 输出格式

默认人类可读：

```text
claude
  active: claude-anthropic-work  (oauth_capture)  work@example.com / Personal
  status: matched

  profiles:
    claude-anthropic-work     oauth_capture   work@example.com / Personal     ← active
    claude-anthropic-personal oauth_capture   personal@example.com / Personal
    claude-glm                env_injection   "GLM (智谱代理)"

codex
  active: codex-chatgpt-work  (oauth_capture)
  status: matched

  profiles:
    codex-openai-personal  file_template   "OpenAI 官方 (personal)"
    codex-chatgpt-work     oauth_capture   work@example.com               ← active
```

`--json` 输出包含同样字段，secret / blob 部分以 `"***"` 占位。

## 8. 执行流程

### 8.1 `switch add`

```
load config.yaml
validate app supports requested kind
reject oauth_capture (oauth_capture 只能通过 import-current 或 reauth 创建)
fill default fields, prompt missing required ones (TTY) or fail (non-TTY)
resolve id:
  if --id <explicit>: use it as-is (must match id regex)
  else:               id = "<app>-" + slug(name)
on id collision: refuse unless --force
append profile to config.yaml (atomic write)
```

id 自动生成示例：

- `switch add claude glm` → `claude-glm`
- `switch add claude "GLM 智谱"` → `claude-glm`（中文字符在 slug 中被丢弃，详见 §6.2）
- `switch add codex personal --id corp-prod` → `corp-prod`（显式 `--id` 覆盖自动规则）

不修改任何目标位置。

### 8.2 `switch import-current <app> <name>`

```
load config.yaml + app module
acquire app lock
module probes current state:
  - settings.json $.env populated (Claude)        -> draft env_injection
  - auth.json API-key shape (Codex)               -> draft file_template
  - OAuth indicators detected                     -> draft oauth_capture
  - multiple modes co-exist                       -> ImportAmbiguous, ask user
for oauth_capture drafts:
  read all sources (Keychain entry + json_subtree + relevant files)
  extract identity fields from oauthAccount / auth.json
  extract expires_at by base64-decoding the access_token JWT payload
  check identity.account_uuid against existing profiles: if match, ask user
    whether to refresh that profile's capture instead of creating new
resolve id (same rule as 8.1: --id 优先，否则 "<app>-" + slug(name))
show summary (sanitized), ask for confirmation, allow user to edit name / id
on confirm:
  - copy bytes into captures/<id>/ (0600/0700)
  - write profile to config.yaml (atomic)
release lock
```

特例：Claude 在 macOS 上检测到 Keychain `Claude Code-credentials` 时，必须把它和 `~/.claude.json` 的 `$.oauthAccount` / `$.userID` 一起捕获——只捕获其中一个会让记录处于不一致状态。

### 8.3 `switch use <id>`

```
load config.yaml + app module + state/active.json
resolve target_profile by id
acquire app lock
detect target app process running:
  - if target_profile.kind == oauth_capture: refuse, ignore --allow-running
  - else: refuse unless --allow-running

# Step A: writeback current active (oauth_capture only)
if state.active_profiles[app] exists:
    previous = load profile(state.active_profiles[app])
    if previous.kind == oauth_capture:
        for each source in previous.capture.sources (current platform):
            read current bytes from source
            write to captures/<previous.id>/<stored_as>
            update sha256 in config.yaml
        update previous.freshness.last_writeback_at = now
        atomic write config.yaml
        if writeback fails -> abort entire switch, no targets touched

# Step B: build plan
build plan from target_profile (kind-specific render or load from captures/)
if --dry-run: print plan, release lock, exit
ask confirmation unless --yes

# Step C: defensive backup of ALL target locations
for each target (files, Keychain entries, json subtrees):
    read current bytes
    write to backups/<app>/<timestamp>/<file>
write backup manifest

# Step D: apply
for each target:
    stage new bytes (atomic file replace / Keychain write / json_subtree merge)

# Step E: verify
for kind != oauth_capture:
    sha256 of each written target equals expected
for kind == oauth_capture:
    re-read $.oauthAccount.accountUuid (or codex equivalent)
    must equal target_profile.identity.account_uuid
    if mismatch -> rollback from defensive backup, error IdentityMismatch

# Step F: bookkeeping
update state/active.json
append history.jsonl
prune old backups beyond keep_backups
release lock

# Step G: post-switch advisory (not blocking)
if target_profile.kind == oauth_capture and freshness.captured_at is older than
   oauth_stale_warn_days, warn user that refresh might fail and suggest
   running `claude` (or `codex`) briefly to trigger refresh; if it returns
   401, prompt to run `switch reauth <id>`.
```

任一步失败的处理：

- writeback 失败 → 完全中止，目标位置不动，错误信息说明哪个 source 写回失败。
- defensive backup 失败 → 中止，不动任何 target。
- apply 中途失败 → 用刚才的 backup 自动恢复已替换的 target，标记本次失败。
- verify 失败（hash 或 identity）→ 同 apply 失败处理。
- 错误信息明确指出失败步骤、target、可用 backup id。

### 8.4 `switch reauth <id>`

仅对 `oauth_capture` profile 有效。

```
load config.yaml + app module
resolve profile by id
acquire app lock
require: target app process NOT running
guide user:
  1. print: "switch-cli 即将启动 App 的登录流程。完成后请退出 App，按回车继续。"
  2. spawn `claude /login` 或 `codex login` (by app module)
  3. wait for the app process to exit
read all sources, build a refreshed capture
verify identity.account_uuid matches the existing record
  - if mismatch: ask user whether this is a different profile (suggest add as new)
  - if match: update capture/* bytes and sha256, update freshness.captured_at
atomic write config.yaml
state.active_profiles[app] = <id>  (reauth implies activation)
release lock
```

### 8.5 `switch status [<app>]`

```
load config + state
for each app:
  read active profile id from state/active.json
  if no active profile                      -> no-active
  resolve profile
  read actual bytes from each target
  for env_injection / file_template:
    render expected bytes from fields
    compare actual vs expected -> matched / drifted
  for oauth_capture:
    read identity from actual ~/.claude.json $.oauthAccount
    compare to profile.identity -> matched / drifted
    (不比对 Keychain 内容——token 可能刚被 App 刷新，bytes 必然变；
     身份指纹才是稳定不变量)
  if any required target missing -> missing
```

MVP 状态集合：

| 状态 | 含义 |
|------|------|
| `matched` | env/file kind 渲染结果一致 / oauth identity 一致 |
| `drifted` | 活动 profile id 已知，但实际状态与预期不符 |
| `missing` | required target 不存在 |
| `no-active` | state 里没有该 App 的活动 profile |

### 8.6 `switch restore-target <app> <backup-id>`

从防御性备份恢复指定 App 的所有目标位置。来源 bytes 来自 `backups/<app>/<backup-id>/`，操作不改 state/active.json，恢复前再生成一份新备份。

对包含 Keychain entry / json_subtree 的 backup，恢复时按对应机制写回（不是简单 file copy）。

`switch list` / `show` / `edit` / `remove` 的流程平凡，略。

## 9. 文件操作协议

### 9.1 路径解析

- 所有 managed 路径中的 `~` 在配置加载时一次性展开为 `getpwuid(getuid())` 得到的真实 home 目录。
- **不信任 `$HOME` / `$USER` 环境变量**——`${MACOS_USER}` 也从 getpwuid 解出。
- 展开后路径必须落在 home 之下；否则 `doctor` 标红，`use` 拒绝写入。

### 9.2 锁

```text
~/.local/share/switch-cli/locks/<app>.lock     # 单 App 的状态写操作
~/.local/share/switch-cli/locks/config.lock    # config.yaml 写操作
```

`use` / `import-current` / `reauth` / `restore-target` 持有 App 锁。`add` / `edit` / `remove` 持有 config 锁。`list` / `show` / `status` / `doctor` 无锁读取。

注意：锁只在 switch-cli 自身实例之间生效。OAuth kind 已通过"拒绝目标 App 运行时切换"额外保护。

### 9.3 文件原子替换

```text
write target content to temp file in same directory
fsync temp file
preserve or set file permissions
rename temp file over active file
fsync parent directory when platform supports it
```

敏感目标文件默认权限：

| 文件 | 权限 |
|------|------|
| `settings.json` / `auth.json` / `.credentials.json` / `config.toml` / `claude.json` | 0600 |
| 父目录（如需新建） | 0700 |

若目标文件已存在，继承原文件权限。

### 9.4 JSON 子树合并

两种 JSON 子树场景：

**`managed_env_keys`（env_injection 用）**

1. 读现有 `settings.json`（不存在则创建空对象）。
2. 删除上一活动 profile `managed_env_keys` 中的所有键。
3. 写入当前 profile 渲染出的 env 键。
4. 其他键（`mcpServers` 等）保持不变。
5. 序列化（保持原文件 indent；新建用 2 空格）。
6. 走原子替换流程。

**`managed_json_subtrees`（oauth_capture 用）**

1. 读现有 JSON 文件（`~/.claude.json`）。
2. 按 App Module 声明的 `managed_json_subtrees`（如 `[$.oauthAccount, $.userID]`），用 capture 中的对应值整体替换。
3. 其他键（projects、tipsHistory 等运行时状态）保持不变。
4. 序列化并原子替换。

两种场景都必须保持 JSON 文件的字段顺序稳定（用保序的反序列化器，如 Rust 的 `serde_json::Value` 配合 `preserve_order` feature），避免每次切换都产生大量无意义 diff。

### 9.5 Keychain backend（macOS）

- 读：`SecKeychainFindGenericPassword` 或 `security find-generic-password -s <service> -a <account> -w`。
- 写：先写一个临时 service 名（`<service>.switch-cli.tmp.<random>`），成功后通过 `SecItemUpdate` 改名到目标 service（macOS Keychain 支持 attribute update）。失败保留旧值。
- 备份：把读出的 bytes 写入 `backups/.../<file>.json`，权限 `0600`。
- 删除：先确认临时项存在 → 删除主项 → rename 临时项到主项名 → 删除临时项备份。

MVP 仅实现 generic password 类型，仅访问当前用户的 login keychain。

### 9.6 Symlink

- 目标位置是 symlink 时，沿链跟随到真实路径写入。
- 真实路径在 home 外时，MVP 默认拒绝写入；`doctor` 标红。
- 外部路径允许开关不在 MVP。

### 9.7 Hash

- 文件 / blob hash 使用 SHA-256。
- JSON subtree 的 hash 基于"序列化后的规范字节"（保序、UTF-8、无尾空格）。
- hash 不包含 mtime / 权限位。
- 输出场景默认只展示 hash 前 8 个 hex 字符。

## 10. 实证记录与 App Module

### 10.0 Claude Code 凭据存储实证记录

以下基于 Claude Code 2.1.150（macOS）的逆向调研结论，作为 Module 实现的依据：

**OAuth 凭据载体**：

- **macOS**：单条 Keychain entry。`service = "Claude Code-credentials"`，`account = <macOS 用户名>`。value 是 JSON：
  ```json
  {
    "claudeAiOauth": {
      "accessToken": "<JWT>",
      "refreshToken": "<opaque>",
      "expiresAt": <ms epoch>,
      "scopes": ["..."],
      "subscriptionType": "...",
      "rateLimitTier": "..."
    }
  }
  ```
  注意 `account` 是 macOS 用户名而不是 Anthropic 账号——同一台机器上的多个 Anthropic 账号**共用**这条 entry，必须串行切换（不能并存）。
- **Linux**：`~/.claude/.credentials.json`，结构相同。
- **Windows**：当前未验证，结构推测同 Linux。

**账号身份载体**（在 `~/.claude.json` 顶层）：

- `oauthAccount` 对象：`accountUuid` / `organizationUuid` / `organizationName` / `organizationRole` / `emailAddress` / `subscriptionType` / `subscriptionCreatedAt` / `billingType` / `accountCreatedAt` / `hasExtraUsageEnabled` / `claudeCodeTrialEndsAt` / `ccOnboardingFlags` 等 18 个子键。
- 顶层 `userID`（64 字符 hash，关联 `accountUuid`）。

切换 OAuth 账号必须**同时**改 Keychain 和这两个 JSON subtree。仅改 Keychain 会导致 UI 显示与实际身份不一致的中间态。

**Token 刷新行为**：

- 刷新端点：`POST <anthropic-domain>/v1/oauth/token`。
- access_token 过期时自动触发，**refresh_token 大概率会被旋转**（从二进制 mutate 集合包含 `refreshToken` 推断）。
- 这意味着 capture 越久没回写，refresh 失败概率越高——是 writeback 机制的根本理由。
- 实测前提：实现 Claude Module 前必须做一次"登录 → 触发刷新 → 比对 refresh_token 是否旋转 → 用旧 capture 尝试恢复"实验，把结论写入模块 README。

**Env 覆盖**：`CLAUDE_CODE_OAUTH_TOKEN` 走 setup-token 长期凭据，仅 inference-only，不适合日常 profile 切换，MVP 不纳入管理。

**Codex 待实证**：实现 Codex Module 前必须先 `cat ~/.codex/auth.json` 看实际 schema，识别 OAuth blob 的可识别字段（如 `tokens.access_token` / `tokens.refresh_token` / `tokens.id_token` 等），并实测 refresh token 是否旋转。

### 10.1 模块职责

App Module 是编译进二进制的静态模块。每个模块对应一个 App，声明：

- App id、display name。
- 支持的 kind 列表。
- 各 kind 的 field schema（env_injection / file_template）。
- 各 kind 的 target spec 和 capture source spec（按平台过滤）。
- `managed_env_keys` / `managed_json_subtrees`：声明该 App 在各 kind 下管理的"键边界"，core 用于先清后写。
- 从 fields 渲染出目标内容的逻辑（env_injection / file_template）。
- 从 capture 还原目标内容的逻辑（oauth_capture / opaque_capture）。
- identity 提取逻辑（oauth_capture 用，从 oauthAccount 或 auth.json 提取）。
- import_current：从当前状态推断 profile 草稿（可能返回多个候选）。
- reauth：spawn 用户登录流程，登录后重新捕获。
- doctor 检查与进程探测。

Module 不负责：

- 解析 OAuth blob 内部结构（除了 base64-decode JWT 的 exp 字段以提示过期）。
- 调登录接口 / 直接刷新 token。
- 判断账号凭据在服务端是否有效。
- 文件原子写、JSON 合并、Keychain 操作、锁、备份、hash —— 这些是 core。

### 10.2 接口骨架

```rust
pub trait AppModule {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn supported_kinds(&self) -> &[Kind];

    fn field_schema(&self, kind: Kind) -> Option<FieldSchema>;
    fn target_spec(&self, kind: Kind, platform: Platform) -> TargetSpec;

    fn managed_env_keys(&self, kind: Kind) -> &[&'static str];
    fn managed_json_subtrees(&self, kind: Kind) -> &[JsonSubtreeSpec];

    fn render(&self, profile: &Profile, current: &TargetState)
        -> Result<RenderedTargets, RenderError>;
    fn restore(&self, profile: &Profile, captures: &CaptureBytes)
        -> Result<RenderedTargets, RenderError>;

    fn extract_identity(&self, captures: &CaptureBytes)
        -> Result<Identity, ExtractError>;
    fn extract_freshness(&self, captures: &CaptureBytes)
        -> Result<Freshness, ExtractError>;

    fn import_current(&self, ctx: &Ctx) -> Result<Vec<ProfileDraft>, ImportError>;

    fn spawn_login(&self, ctx: &Ctx) -> Result<LoginHandle, ReauthError>;

    fn doctor(&self, ctx: &Ctx) -> DoctorReport;
    fn detect_processes(&self) -> ProcessProbe;
}
```

`RenderedTargets` 是 `target_location → bytes` 的映射；`target_location` 可以是文件路径、Keychain entry 标识、或 JSON subtree 引用。

### 10.3 Claude Module

支持的 kind：

- `env_injection`
- `oauth_capture`

**`env_injection` 渲染规则**

- 目标：`~/.claude/settings.json` 的 `$.env`。
- `managed_env_keys`：
  - `ANTHROPIC_BASE_URL`
  - `ANTHROPIC_AUTH_TOKEN`
  - `ANTHROPIC_MODEL`
  - `ANTHROPIC_DEFAULT_HAIKU_MODEL`
  - `ANTHROPIC_DEFAULT_SONNET_MODEL`
  - `ANTHROPIC_DEFAULT_OPUS_MODEL`
- 字段映射：
  - `fields.base_url`        → `ANTHROPIC_BASE_URL`
  - `fields.auth_token`      → `ANTHROPIC_AUTH_TOKEN` (sensitive)
  - `fields.models.default`  → `ANTHROPIC_MODEL`
  - `fields.models.haiku`    → `ANTHROPIC_DEFAULT_HAIKU_MODEL`（缺省继承 default）
  - `fields.models.sonnet`   → `ANTHROPIC_DEFAULT_SONNET_MODEL`（缺省继承 default）
  - `fields.models.opus`     → `ANTHROPIC_DEFAULT_OPUS_MODEL`（缺省继承 default）
- 不纳入默认 managed：`~/.claude.json`、`~/.claude/` 目录。

**`oauth_capture` 规则**

- target / capture sources：
  - macOS：Keychain entry `service = "Claude Code-credentials"`, `account = ${MACOS_USER}`
  - Linux：`~/.claude/.credentials.json`
  - 所有平台：`~/.claude.json` 的 `$.oauthAccount` 和 `$.userID`（json_subtree）
- `managed_json_subtrees`：`[$.oauthAccount, $.userID]` on `~/.claude.json`。
- identity 提取：从 capture 中 `oauthAccount.json` 读出 `accountUuid` / `organizationUuid` / `organizationName` / `emailAddress` / `subscriptionType`。
- freshness 提取：base64-decode 凭据 JSON 中 `claudeAiOauth.accessToken` 的 JWT payload，取 `exp` 作为 `expires_at`；`captured_at` = 当前时间。
- verify：切换后从 `~/.claude.json` 读 `$.oauthAccount.accountUuid`，必须等于 `profile.identity.account_uuid`。
- spawn_login：`claude /login`（具体命令实测确定，可能是子命令或交互菜单项）；模块负责检测 App 退出。
- 不能与 env_injection 同时活动：如果 `settings.json.env` 设了 `ANTHROPIC_AUTH_TOKEN`，会**覆盖** OAuth 走 API key。切到 oauth_capture 时如果发现 env 里有这些键，`use` 默认报错并提示用户先用 `switch use <env-injection-id>` 把它切走，或在 plan 里展示"将同时清空这些 env 键"并要求确认。

**`import_current` 行为**

1. 探测 macOS Keychain `Claude Code-credentials` 是否存在；存在 → 候选 oauth_capture。
2. 探测 Linux `~/.claude/.credentials.json` 是否存在；存在 → 候选 oauth_capture。
3. 探测 `~/.claude/settings.json` `$.env.ANTHROPIC_AUTH_TOKEN` 是否非空；非空 → 候选 env_injection。
4. 两者都存在 → 报告"Anthropic OAuth 与第三方代理 env 同时配置，请选择捕获哪一项"。
5. 仅 oauth_capture 候选时，自动捕获 oauthAccount + userID，提取 identity，提示用户确认 name。

**doctor 输出**：settings.json 存在性、`managed_env_keys` 填充状态、`~/.claude.json` 中 `oauthAccount.accountUuid` / `email`、Keychain entry 是否可读（不读值，只测 existence）、是否检测到 Claude Code 进程。

### 10.4 Codex Module

支持的 kind：

- `file_template`：API-key 登录。
- `oauth_capture`：ChatGPT OAuth 登录。

**`file_template` 渲染规则**

- 目标文件：
  - `~/.codex/auth.json`：含 API key 的 JSON（具体 schema 由 Module 维护并通过 fixture 测试锁定）。
  - `~/.codex/config.toml`：渲染当前 profile 对应的 `[model_providers.<name>]` 段及活动 model 字段。
- 字段：
  - `fields.api_key` (sensitive, required)
  - `fields.base_url` (optional)
  - `fields.model`
  - `fields.model_provider`
  - `fields.model_reasoning_effort` (optional)

**`oauth_capture` 规则**

- sources：
  - `~/.codex/auth.json`（必需）
  - `~/.codex/config.toml`（可选，`required: false`）
- identity 提取：从 `auth.json` 中识别 OAuth blob 字段（如 `tokens.id_token` 的 JWT payload `sub` / `email`）；具体字段实证后写入模块。
- freshness 提取：从 OAuth blob 中 `tokens.access_token` 或 `expires_at` 字段读取。
- verify：切换后从恢复的 `auth.json` 读出 identity 字段比对。
- spawn_login：`codex login`（实测确定）。

**`import_current` 行为**

- 读 `auth.json`：
  - 形态 `{"OPENAI_API_KEY": "..."}` 且没有 OAuth 字段 → draft `file_template`。
  - 含 OAuth 字段（如 `tokens` / `refresh_token`）→ draft `oauth_capture`。
  - 两种字段共存（不太可能但需防御）→ 报错。
- 模块实现前必须实测当前 Codex 版本的 auth.json 实际 schema，把识别规则写入模块代码（不放文档），避免文档与实现脱节。

**doctor 输出**：auth.json / config.toml 存在性、当前形态识别结果、识别到的 identity 字段、是否检测到 Codex 进程。

### 10.5 开源贡献边界

新增 App 的主要贡献路径是提交一个新的内部 Module。源码组织建议：

```text
src/
  app_modules/
    mod.rs
    claude.rs
    codex.rs
    common/
      field_schema.rs
      render_helpers.rs
      jwt_parser.rs
      keychain_macos.rs
```

新模块至少需要包含：

- 至少一种 kind 的实现。
- `import_current` 实现（哪怕仅返回 NotImplemented）。
- 渲染 fixture 测试（fields → bytes）。
- import_current fixture 测试（bytes → fields）。
- 对 oauth_capture 模块：identity / freshness 提取 fixture 测试。
- 不输出 secret 字段值的脱敏测试。
- 路径展开和 home 边界测试。

oauth_capture 模块必须在 PR 描述里附上实测记录：
- 该 App 的 refresh_token 是否旋转。
- writeback 与 restore 是否真的能成功切换。
- spawn_login 的实际命令。

## 11. 安全和隐私

### 11.1 不打印 secret / blob 内容

默认不打印明文：

- `env_injection` / `file_template` 中带敏感语义的字段（字段名匹配 `*token*` / `*key*` / `*secret*` / `*password*`，或 schema 标 `sensitive: true`）。
- 所有 capture blob（Keychain 内容、credentials.json、auth.json OAuth 段等）。

允许打印：profile id / name / app / kind / created_at、非敏感字段、`identity` 块、目标位置 metadata、capture source 的 metadata、sha256 前缀。

### 11.2 文件权限强制

- `~/.config/switch-cli/config.yaml`：`0600`。
- `~/.local/share/switch-cli/captures/`：目录 `0700`，文件 `0600`。
- `~/.local/share/switch-cli/backups/`：目录 `0700`，文件 `0600`。
- 目标文件继承现有权限，新建默认 `0600`。
- Keychain entry：依赖 macOS Keychain 自身权限模型；switch-cli 只用当前用户 login keychain。

启动时若发现 config / captures / backups 权限被改宽，doctor 警告。

### 11.3 防御性备份保留

默认 `keep_backups: 20`（每 App 独立计数）。

MVP 自动清理：在 `use` / `restore-target` / `reauth` 成功后、释放锁前执行。按 mtime 倒序保留最近 N 份。失败不影响主流程结果，但写入 history 的 `warnings`。

`doctor` 展示每个 App 的备份数量、最旧备份时间。

### 11.4 并发与进程检测

同 App 写操作互斥（文件锁）。不同 App 可并行切换。

`use` / `reauth` / `restore-target` 执行前通过进程名匹配（由 App Module 声明）粗粒度检测目标 App 是否运行：

- env_injection / file_template kind：命中默认拒绝，可 `--allow-running` 跳过。
- **oauth_capture kind：命中强制拒绝，`--allow-running` 不生效**。理由：App 运行时会刷新 token，原子性无法保证，且我们 writeback 的瞬间 App 也可能在写。

### 11.5 OAuth refresh token rotation

`oauth_capture` 假设 App 后端**可能**旋转 refresh_token。两条机制对抗这个风险：

1. **切换时写回**：切走当前活动 oauth_capture profile 前，先把最新 Keychain / 凭据文件内容回灌到该 profile 的 capture。这样只要用户在多个 profile 之间循环切换，每个 capture 都保持最新。
2. **失效降级**：如果 capture 长期不被切到（即 refresh_token 已老化），用户切过去时 App 会在首次 refresh 时失败。CLI 通过 `freshness.captured_at` 提前警告，且提供 `switch reauth <id>` 让用户重新登录后回灌新 capture。

`oauth_stale_warn_days` 默认 30 天。这个值不代表 server 端的 token 失效阈值（那是 Anthropic 内部决定），只是一个保守的提示线，**实测后**可调整。

### 11.6 Schema 升级兼容性

- 读到比当前二进制更高的 `schema_version` → 拒绝写入，仅 read-only 命令可用，提示升级。
- 读到旧 `schema_version` → 自动迁移；失败时备份原 config.yaml 为 `config.yaml.bak.<timestamp>` 并只读运行。
- 未识别的 `extensions` 字段在迁移期间保留。

## 12. 错误类型

| 错误 | 含义 |
|------|------|
| `AppNotFound` | 不支持的 App |
| `ProfileNotFound` | 指定 id 的 profile 不存在 |
| `ProfileExists` | `add` 时 id 已被占用 |
| `KindNotSupported` | App 不支持指定 kind |
| `FieldMissing` | 必需字段缺失或无效 |
| `TargetMissing` | required target 不存在 |
| `CaptureMissing` | capture source bytes 在 captures/ 下找不到 |
| `PermissionDenied` | 目标 / config / captures / Keychain 不可读或不可写 |
| `KeychainUnavailable` | macOS Keychain 不可用或用户拒绝授权 |
| `RenderFailed` | App Module 渲染过程出错 |
| `WritebackFailed` | oauth_capture 切换前的写回当前活动 profile 失败 |
| `BackupFailed` | 切换前防御性备份失败 |
| `ReplaceFailed` | 替换目标位置失败 |
| `VerifyFailed` | 替换后 hash 与预期不一致 |
| `IdentityMismatch` | 替换后 identity 与 capture 中的 identity 不一致（oauth_capture）|
| `ImportAmbiguous` | `import_current` 检测到多种可能 kind |
| `LockBusy` | 另一个写操作正在执行 |
| `AppRunning` | 目标 App 进程在运行；oauth_capture 直接拒绝，其他 kind 默认拒绝 |
| `RefreshLikelyStale` | freshness 显示 capture 已超过保守阈值，建议先 reauth |
| `SchemaTooNew` | config.yaml 的 schema_version 高于当前二进制 |

错误输出包含下一步建议：

```text
claude: switch failed at verify step
target: ~/.claude.json $.oauthAccount.accountUuid
reason: identity mismatch
  expected: 5f3e...
  actual:   a1b2...
backup: 20260523T100000Z
next: switch restore-target claude 20260523T100000Z
hint: target profile's capture may be from a different Anthropic account.
      run `switch reauth claude-anthropic-work` to refresh.
```

## 13. MVP 验收标准

1. 能用 `add` 手动创建 Claude `env_injection` profile、Codex `file_template` profile。
2. 能用 `import-current` 在 macOS 上从当前 Keychain + `~/.claude.json` 自动捕获一条 Claude `oauth_capture` profile；identity 字段（accountUuid / email / organizationName）正确提取。
3. 能用 `import-current` 在 Linux 上从 `~/.claude/.credentials.json` + `~/.claude.json` 自动捕获 Claude `oauth_capture` profile。
4. 能用 `import-current` 从 Codex `~/.codex/auth.json` 识别 API-key 模式 → `file_template`，或 OAuth 模式 → `oauth_capture`。
5. 能在已注册 profile 之间切换：
   - env_injection ↔ env_injection
   - oauth_capture ↔ oauth_capture（同一 App 内）
   - env_injection ↔ oauth_capture（同一 App 内；切到 oauth_capture 时清掉 env 中的 managed 键并要求用户确认）
   - 切换后重启 App 实测确认 profile 生效（macOS 与 Linux 各做一次）。
6. `switch use` 切到 oauth_capture B 之前，能成功把当前活动 oauth_capture A 的最新 Keychain / 凭据文件 / json_subtree 写回 A 的 capture（实测：触发一次 App 内 refresh 改变 Keychain mdat → 切走 → 验证 captures/A/* 已更新）。
7. `status` 能正确报告 `matched` / `drifted` / `missing` / `no-active`；oauth_capture 的 matched 基于 identity 比对而非 capture bytes。
8. `use --dry-run` 输出的 plan 不包含 secret 字段值的明文，也不打印 capture blob 内容；但能展示 identity 块。
9. 每次 `use` 前自动建立防御性备份（覆盖 file / Keychain / json_subtree 三类目标）；`use` / `restore-target` / `reauth` 成功后按 `keep_backups` 自动修剪。
10. `restore-target` 能从备份恢复所有类型的目标位置，恢复前再生成一份新备份。
11. `use` 在检测到目标 App 进程运行时默认拒绝；env_injection / file_template 可 `--allow-running` 跳过；**oauth_capture 不可跳过**。
12. `reauth` 能成功 spawn App 的 login 流程、等待 App 退出、重新捕获并验证 identity 仍匹配（不匹配时提示用户）。
13. `remove` 能删除 profile；同时清理 `captures/<id>/`。
14. 所有命令输出（人类格式和 `--json`）都不打印 secret 字段明文，也不打印 capture blob 内容。
15. config.yaml / captures/ / backups/ 权限不正确时启动有警告。
16. 配置加载时 `~` 和 `${MACOS_USER}` 都从 `getpwuid` 展开，不信任 `$HOME` / `$USER`。
17. core 不包含 `claude` / `codex` 专属分支；所有 App 特定逻辑在 App Module 内。
18. 文档明确：oauth_capture 切换前必须退出目标 App；其他 kind 也建议退出。

**前置实测（在实现 Module 前必须完成并记录结论）**：

A. **Claude refresh_token rotation 实测**：登录 → 等触发刷新 → 比对旋转前后 → 用 capture 中的旧 refresh_token 尝试恢复 → 看是否能续期。
B. **Claude oauthAccount 不一致容忍度实测**：仅改 Keychain 不改 `~/.claude.json.oauthAccount` → 启动 Claude Code → 观察行为（自动修正 / UI 不一致 / 报错）。
C. **Claude 并发写 `~/.claude.json` 实测**：Claude Code 运行时观察 `~/.claude.json` 的写频率与字段，识别哪些字段会和 `$.oauthAccount` / `$.userID` 同一次写入。
D. **Codex auth.json schema 实证**：登录两种模式各一次，cat 文件，记录实际字段。
E. **Codex spawn_login 命令**：实测 `codex login` 或对应命令、退出条件。

## 14. 后续演进

### Phase 2: 更多 App、kind、backend

- Gemini CLI / OpenCode / Cursor / Windsurf 等 App Module。
- Linux Secret Service / Windows Credential Manager backend。
- 新 kind：`dotenv_file`（Gemini 风格）、`composite`（多 kind 组合，例如 OAuth 加额外环境变量）。
- `opaque_capture` 的首个实例（如果出现没有刷新语义的纯 blob 凭据场景）。

### Phase 3: 体验增强

- `switch rename` / `switch tag` / shell completion。
- 更细的 drift 展示：diff 出具体哪些键 / 字段被外部改了。
- `switch export` / `import`：跨机器迁移（明文导出需 `--unsafe-export`）。
- 字段级 secret 加密（passphrase-based）。
- `daemon` 模式：常驻监听 Keychain / credentials.json 变化，实时回写当前 active profile 的 capture，进一步降低旋转失效风险。

### Phase 4: 凭据管理器集成

- 与 1Password / pass / Bitwarden 集成。
- 项目级 scope（按工作目录绑定 profile）。
- 加密快照、跨机器同步。

### Phase 5: 通用状态模型

如果 AI CLI profile 切换之外出现明确需求，再考虑通用 state / plan / apply 模型、外部模块协议、插件系统、trust / allow 机制。这些能力不应反向污染第一阶段 MVP。

## 15. 当前设计结论

`switch-cli` 第一版是一个**AI CLI profile 管理与切换工具**，其中最重要的 profile 类型是账号和凭据配置。它特别认真地对待 OAuth 这类动态凭据。

最重要的边界：

1. Profile 是结构化记录（`id` / `app` / `kind` / `fields` 或 `identity + capture`），不是 opaque 文件快照。
2. 四种 kind（`env_injection` / `file_template` / `oauth_capture` / `opaque_capture`）覆盖第一阶段现实世界的主要凭据投递方式。
3. OAuth 凭据视为动态资产：切换时双向写回、身份指纹校验、过期感知、reauth 降级路径。
4. 用户可以手编 config.yaml（除 `oauth_capture` 外），也可以 `import-current` 自动捕获当前状态。
5. App 专属逻辑封装在 App Module 内；core 提供文件原子写、JSON 合并、Keychain backend、锁、防御性备份、hash、脱敏。
6. 每次切换前对所有目标位置做防御性备份，每次切换后做 hash 校验和（对 oauth_capture）identity 校验。
7. oauth_capture 切换前**强制要求**目标 App 退出，不接受 `--allow-running`。
8. 后续扩展通过 PR 增加内置 Module / kind / backend，不在 MVP 引入插件和外部模块生态。

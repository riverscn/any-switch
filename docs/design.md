# switch-cli 设计文档

## 1. 项目定位

`switch-cli` 是一个本地 profile / state 切换工具。它把某个本地 App 或 CLI 的一组可管理状态建模为 profile，并通过声明式 App Definition，把 profile 捕获、渲染或回放到具体 target，例如 JSON 子树、TOML 路径、文件、Keychain entry 或环境变量块。

MVP 阶段先聚焦 AI CLI 工具，内置 Claude Code 和 Codex 的 App Definition，用来解决账号、凭据、服务端点和模型配置的快速切换问题。但这些 AI CLI 账号场景只是第一组内置用例，不是 core 的边界。

工具不接入任何第三方登录接口。它把每个 profile 建模为一条**结构化记录**，并由 kind 决定这条记录如何作用到目标状态：

- 对于可由环境变量、JSON、TOML 或文件模板表达的状态：记录里直接存语义字段。
- 对于会被目标 App 动态改写的状态：记录里存身份指纹 + 动态 capture，并在每次切换时按需写回，避免本地存档落后于 App 的最新状态。

MVP 解决一个明确的起点问题：

```text
我在 Codex / Claude Code 中需要在多个账号、服务端点或模型 profile 之间切换，
希望一条命令搞定，不用反复手动改 settings.json 或重新登录。
```

这个问题同时验证 switch-cli 的核心抽象：声明式 target、受信任 handler、防御性备份、失败回滚、secret 脱敏和动态状态写回。

示例：

```bash
# 把当前 Claude Code 的 Anthropic OAuth 登录捕获为一条 profile (oauth_capture)
switch import-current claude anthropic-work

# 添加一个 GLM 第三方代理 profile (env_injection)
switch add claude glm \
  --kind env_injection \
  --base-url https://open.bigmodel.cn/api/anthropic \
  --auth-token-stdin
# 在 TTY 中粘贴 token；或从本机 secret 命令 pipe 到 stdin，避免 token 进入 shell history / argv

# 切到 GLM 代理
switch use claude-glm

# 切回 Anthropic 官方 OAuth（不再需要重新登录）
switch use claude-anthropic-work

# Codex 当前是 ChatGPT OAuth 登录，捕获为 profile (oauth_capture)
switch import-current codex chatgpt-work

switch list
switch status
```

## 2. 设计目标

### 2.1 MVP 目标

- 随二进制提供 Claude Code 和 Codex 两个系统预置 App Definition。
- 支持从用户配置目录加载声明式 App Definition / override 文件，用已有 core handler 扩展新的 App、CLI 或状态切换用法。
- Profile 建模为结构化记录，可由编辑器手编，也可由 `import-current` 自动捕获。
- 支持四种 profile 应用方式（kind）。这些 kind 在 MVP 中主要承载凭据和账号状态，但本质上描述的是 profile 如何作用到目标 App 的本地状态：
  - `env_injection`：写入目标 JSON 文件的 env 块（Claude Code 第三方代理场景）。
  - `file_template`：把语义字段渲染到一个或多个受管理文件 / 文件子区域（Codex API-key 登录）。
  - `oauth_capture`：复合源 OAuth 凭据 + 身份指纹的捕获与回放（Claude 官方、Codex ChatGPT 登录）。
  - `opaque_capture`：纯不透明 blob 捕获（schema 保留，MVP 暂无实例）。
- `oauth_capture` 在切换时执行**双向写回**——切换前先把当前活动 profile 的最新 token 回写到它自己的 capture 中，再加载目标 profile，避免 refresh token 旋转造成静默失效。
- `oauth_capture` 写回前必须先校验 live target 的身份仍等于当前活动 profile，避免 drift 状态污染错误 profile 的 capture。
- 切换前能预览将要修改的内容（不含明文 secret）。
- 切换前对将被覆盖的目标文件做防御性备份。
- 切换失败时尽量回滚到操作前状态。
- 默认不打印任何 secret 字段，不输出文件原文。
- 默认不接受 secret 明文作为命令行参数值；敏感字段通过 masked prompt、stdin、环境变量名或本地文件引用读入。
- 所有数据只保存在本机。

### 2.2 非目标

以下能力不进入 MVP：

- 通用状态管理框架。
- 系统代理、Git 身份、Shell 环境变量、服务进程等非 AI CLI 的系统级集成。core 的抽象不排斥这些场景，但 MVP 不提供内置 Definition、专用 handler 或安全策略。
- 可执行代码形式的运行时插件系统、外部模块安装和动态加载协议。MVP 只支持声明式配置扩展。
- 项目目录配置、trust / allow 机制和 shell hook。
- 远程同步、多机器同步、云备份。
- 通用 secret backend（1Password / pass / Bitwarden 等）集成。
- 解析 OAuth blob 的授权语义（JWT 签名校验、scopes 判断、access token 过期时间驱动刷新等）。MVP 只允许为 identity 展示 / 校验而解码本地 JWT payload 的非敏感 claims。
- 自动登录、刷新 token、调用 Anthropic / OpenAI 任何业务 API。
- 登录、重新登录、引导登录或修复登录状态。凭据失效时，用户在目标 App 外部自行处理，switch-cli 只负责重新捕获当前本地状态。
- 校验账号凭据在服务端是否仍可用。
- GUI 和 TUI。
- Linux Secret Service、Windows Credential Manager backend。MVP 仅实现 macOS Keychain backend 和 Linux 文件源。

## 3. 设计原则

### 3.1 Profile 是结构化记录，不是文件快照

工具管理的基本单元是 `profile record`：一个有 `id` / `app` / `kind` / `fields` 或 `identity + capture` 的结构化对象。profile 表示"目标 App 的一组期望本地状态"，而不是目标文件快照。MVP 内置 profile 主要表示账号、凭据、模型、provider 和 endpoint 组合，但 core 不理解也不绑定这些业务概念。结构化记录的好处：

- 用户可以直接 `vim` 编辑 profile。
- 加密 / 解密、字段升级、批量改 model 等未来需求都有锚点。
- 不同 App 的差异显式表达在 `kind` 和 `fields` 上，core 不写死分支。
- OAuth 这类动态凭据可以在 record 里同时存"不变的身份"（identity）和"会变的凭据"（capture），两者独立处理。

### 3.2 四种 kind 覆盖 MVP 的状态应用形态

| kind | 适用场景 | 状态语义 | 切换原子性 |
|------|----------|----------|------------|
| `env_injection` | 通过环境变量配置（Claude Code 走 `settings.json.env`）| 静态语义字段 | 单文件 JSON 子树合并 |
| `file_template` | 写入专属配置文件或声明式 managed paths（Codex API-key 模式）| 静态语义字段 | 多文件 / 子区域渲染 + 原子替换 |
| `oauth_capture` | 可刷新的 OAuth 登录态（Claude 官方、Codex ChatGPT）| 身份指纹 + 动态 capture，切换时双向写回 | 多源原子切换 |
| `opaque_capture` | 无刷新、无身份语义的纯 blob | 单一 opaque blob | 整体替换；MVP 仅保留 kind 名称作为前向兼容，不实现代码路径 |

每个 App Definition 声明自己支持哪几种 kind，并把"profile 字段 / capture → 目标状态"绑定到 core 提供的受信任 handler。新增业务域优先复用这些 kind 和 handler；只有现有状态应用形态表达不了时，才增加新的 kind 或 handler。

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
- **写回身份门禁**：写回当前活动 profile 前，必须先从 live target 提取 verification identity，并与 `state/active.json` 指向的 profile identity 比对。比对失败说明目标状态已 drift 或上次操作在提交 bookkeeping 前中断，此时拒绝 writeback，避免把错误账号的 live bytes 写入当前 profile 的 capture。
- **进程互斥强制**：OAuth kind 切换前如果检测到目标 App 在运行，**拒绝**执行——`--allow-running` 不对 OAuth kind 生效。这是因为 App 运行时会刷新 token，原子性无法保证。
- **提交前后校验**：切换后先校验每个写入 target 的 hash，再从恢复的状态里读出 `account_uuid` / `organization_uuid`，与 capture 的 `identity` 比对，不一致即视为失败回滚。
- **中断恢复**：写入目标前记录 pending switch journal。若进程在 apply 和 bookkeeping 之间崩溃，下一次写命令必须先完成提交、回滚或拒绝并提示用户恢复，不能盲目信任旧的 active 指针。
- **陈旧感知**：capture manifest 记录 `captured_at` 和 `last_writeback_at`，切换时如果检测到 capture 长期没有被写回，警告用户该 capture 可能已经失效。
- **失效边界**：refresh 失败时不假装成功，也不提供登录修复流程。用户应在目标 App 外部恢复可用状态后，再用 `switch import-current` 捕获或更新 profile。

### 3.5 默认不打印 secret 内容

工具复制和写入含凭据的数据，但**默认不打印任何 secret 字段值**。允许打印：

- profile id / name / app / kind / created_at
- 非敏感字段（model 名、model_provider、reasoning_effort 等）
- `identity` 块（account_uuid / organization_name / email 等——这些不是 secret）
- 目标位置 path / size / mtime / sha256 prefix
- capture source 的 metadata（type / path / manifest 中的 sha256 前缀）

敏感字段（字段名匹配 `*token*` / `*key*` / `*secret*` / `*password*`，或 App Definition 标 `sensitive: true`，以及所有 capture blob 的内容）默认脱敏为 `***`。App Definition 可在字段 schema 中显式声明 `sensitive: false` 取消名称模式匹配（例如 `key_id` / `model_provider_key` 这类字面含 key 但语义非 secret 的字段）。`sensitive` 字段为三态：`true` 强制 redact，`false` 强制明文打印，未设置则按名称模式判定。

### 3.6 按产品分 definition，按能力分 core handler

App Definition 按业务状态域划分（claude / codex / 未来 gemini / cursor）。Definition 是声明式配置，可以由系统预置，也可以由用户在配置目录中扩展。它声明：

- 支持的 kind 列表。
- 各 kind 的 field schema 和 capture source spec。
- `managed_env_keys`（env_injection 用）、`managed_json_subtrees`（oauth_capture 用）等"管理边界"声明。
- 使用哪些 core handler 完成"记录 → 目标"渲染、"目标 → 记录"导入和身份提取。
- 进程探测规则、doctor 检查项。

Core handler 是编译进二进制的受信任能力，包括：

- JSON 子树合并、TOML 模板渲染、整文件捕获 / 回放、Keychain entry 读写。
- 字段 schema 校验、字段映射、secret 脱敏。
- 进程探测、doctor 检查项执行。

Core 不执行用户配置里的任意脚本，不动态加载用户代码。

Core 提供：

- profiles.yaml 加载、校验和迁移；只有 profile 管理命令会保存 profiles.yaml。
- 系统预置 App Definition 和用户扩展 Definition 的加载、校验、合并。
- captures/ 目录和 capture manifest 管理。
- 防御性备份与恢复。
- 文件原子替换、JSON 子树合并、Keychain backend、锁、hash、权限。
- 输出脱敏。

### 3.7 多轴可扩展，但 MVP 不预支

设计在多个轴上预留扩展位（顶层 / 单记录双层 `schema_version`、`extensions` 自由字段、`kind` 可枚举、`capture.source.type` 可枚举、`backend` 可枚举、`handler` 可枚举），但 MVP 只实现最小必要集合。

新增 App 或新增 provider 用法优先通过声明式 App Definition / override 完成。新增 core handler、kind、backend 或需要产品专属实证逻辑时，仍通过 PR 增加受信任实现。MVP 不引入可执行代码插件、脚本 hook、trust / allow 机制。

## 4. 核心概念

### 4.1 App

一个可被切换 profile 的应用。App 由 App Definition 声明。MVP 随二进制预置：

```text
codex
claude
```

用户可以通过 `~/.switch-cli/apps.d/*.yaml` 增加新的 App Definition，也可以通过 override 文件调整系统预置 App 的默认字段、模型和 provider 模板。App id 全局唯一，必须满足与 profile id 相同的 slug 规则。

### 4.2 App Definition

App Definition 是 switch-cli 对某个本地 App / CLI 状态面的产品知识声明。它不保存用户 profile 数据，也不保存 secret。它只描述：

- 支持哪些 kind。
- 每个 kind 的字段 schema、敏感字段、默认值和展示名。
- 每个 kind 使用哪些 target、capture source 和 core handler。
- 字段如何映射到 JSON / TOML / env / file 目标。
- oauth_capture 的 identity 从哪些路径提取。
- 进程探测、doctor 检查项。

Definition 来源按优先级合并：

1. 系统预置 Definition（随二进制发布，默认只读）。
2. 用户 App Definition：`~/.switch-cli/apps.d/*.yaml`。
3. 用户 override：`~/.switch-cli/overrides.d/*.yaml`。

同一个 app id 的 override 只能修改允许覆盖的声明式字段，不能替换 handler 为未知值，不能声明执行脚本，不能扩大到 home 目录外的写入目标。

系统预置 Definition 本身也使用同一套 YAML schema。源码中放在 `src/app_definitions/builtin/claude.yaml` 和 `src/app_definitions/builtin/codex.yaml`，构建时校验并嵌入二进制。发布包可以额外安装一份只读副本到资源目录（例如 macOS app bundle resources、Homebrew cellar 或 Linux `/usr/share/switch-cli/app_definitions/`），仅用于审计和调试；运行时以二进制内嵌版本为准，避免资源文件缺失导致内置 App 不可用。

用户想调整内置 Definition 时，不直接修改只读副本，而是在 `overrides.d/` 写局部 override。为了让用户从内置内容开始，CLI 提供导出命令：

```bash
# 查看完整 resolved definition
switch apps show claude

# 导出系统内置原始 definition 到 stdout
switch apps export claude --source system

# 生成一个可编辑的 override 起点
switch apps export claude --as override --output ~/.switch-cli/overrides.d/claude.yaml
```

`--source system` 输出二进制内嵌的系统 Definition，不包含用户 override；`switch apps show` 输出合并后的 resolved Definition；`--as override` 只输出允许覆盖字段的骨架和注释，避免用户复制整份系统 Definition 后误以为可以替换 handler、target path 或声明脚本。

### 4.3 Profile

用户管理的一条 profile 记录。至少包含 `id` / `app` / `kind` / `name`，其余字段随 kind 变化。profile id 全局唯一。

### 4.4 Kind

profile 的应用方式：它决定结构化记录如何被捕获、渲染或回放到目标状态。MVP 四种：`env_injection`、`file_template`、`oauth_capture`、`opaque_capture`（保留）。这些名称来自 MVP 的实际 target 形态，其中 `oauth_capture` 明确服务于 OAuth 动态状态；在其他 App Definition 中，`env_injection`、`file_template` 和未来的 `opaque_capture` 也可以表达非账号类配置状态。

每个 App Definition 声明它支持哪些 kind。例如：

- Claude 支持 `env_injection`（第三方代理）和 `oauth_capture`（官方账号）。
- Codex 支持 `file_template`（API-key 模式）和 `oauth_capture`（ChatGPT 登录）。

MVP 实现三种 kind：`env_injection` / `file_template` / `oauth_capture`。`opaque_capture` 仅作为 schema 中的 kind 名称保留，不进入 MVP 代码路径——在出现真实场景（无 refresh、无 identity 的纯 blob 凭据）前不实现，避免 dead code。

用户扩展 Definition 在 MVP 中优先支持 `env_injection` 和 `file_template`。`oauth_capture` 可以声明 capture source 和 verification identity path，但只能使用 core 已提供的 source / extractor handler；如果某个产品需要额外实证逻辑或特殊恢复步骤，应新增受信任 core handler。

### 4.5 Fields

`env_injection` 和 `file_template` 类 profile 的语义字段集合：

```yaml
fields:
  base_url: "https://api.anthropic.com"
  auth_token: "sk-ant-..."
  models:
    default: claude-sonnet-4-6
```

字段名由 App Definition 声明，由 core handler 渲染成目标格式的实际字段名（如 `ANTHROPIC_AUTH_TOKEN`）。

### 4.6 Identity / Verification Fingerprint（oauth_capture 专用）

`oauth_capture` profile 的稳定校验指纹。它在 MVP 中通常是账号身份，但概念上只是"恢复后应满足的非敏感不变量"。**不含 secret，用于校验和展示**：

```yaml
identity:
  account_uuid: "5f3e..."
  organization_uuid: "a1b2..."
  organization_name: "Personal"
  email: "work@example.com"
  subscription_type: "pro"
```

切换后从恢复的状态读出对应字段，与 identity / verification fingerprint 比对，不一致即回滚。identity 还用于：

- `list` / `show` 输出时给用户辨识用。
- `import_current` 时去重（同 `account_uuid` 已存在 → 提示用户更新现有记录而不是新建）。

### 4.7 Capture

`oauth_capture` 和 `opaque_capture` profile 的 blob 引用。profiles.yaml 里的 capture 只描述稳定 source：类型、目标位置、`stored_as` 和可选的平台限定。`stored_as` 是相对 `captures/<profile-id>/` 的受限路径，不包含 `captures/<id>/` 前缀。`sha256`、写回时间等动态元数据不写入 profiles.yaml，而写入 `captures/<id>/manifest.json`。

```yaml
capture:
  sources:
    - type: secret_entry
      backend: macos_keychain
      service: "Claude Code-credentials"
      account: "${MACOS_USER}"
      stored_as: keychain.json
      platforms: [macos]
    - type: file
      path: "${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json"
      stored_as: credentials.json
      platforms: [linux]
    - type: json_subtree
      path: ~/.claude.json
      json_path: $.oauthAccount
      stored_as: oauthAccount.json
    - type: json_subtree
      path: ~/.claude.json
      json_path: $.userID
      stored_as: userID.txt
```

支持的 source type：

| type | 含义 | MVP 实现 |
|------|------|----------|
| `file` | 整个文件 | ✓ |
| `secret_entry` | 系统 secret store 中一条具名条目 | ✓（仅 macOS Keychain）|
| `json_subtree` | 某个 JSON 文件中某个 JSONPath 子树（部分写入）| ✓ |

blob 内容不内联到 profiles.yaml，而在 `~/.switch-cli/captures/<id>/` 下，目录 `0700`、文件 `0600`。

**Capture 不跨平台 portable**：同一账号的 macOS Keychain entry value 与 Linux `${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json` 内容结构相近但语义来源不同（系统 secret store vs. 应用文件），MVP 不做平台间字节迁移。把 profile 复制到新平台后，必须重新运行 `switch import-current` 在该平台上重新捕获 capture——identity（accountUuid 等）可用于在新机器上确认捕获到的是同一账号。

### 4.8 Capture Manifest（oauth_capture 专用）

`captures/<id>/manifest.json` 由 switch-cli 自动维护，记录 capture blob 的 hash 和审计时间。它是运行时状态，不是用户 profile 配置。

```json
{
  "schema_version": 1,
  "profile_id": "claude-anthropic-work",
  "captured_at": "2026-05-23T10:00:00Z",
  "last_writeback_at": "2026-05-23T15:30:00Z",
  "sources": [
    {
      "stored_as": "keychain.json",
      "sha256": "7a3f..."
    },
    {
      "stored_as": "oauthAccount.json",
      "sha256": "c1d2..."
    }
  ]
}
```

capture manifest 只用于本地审计、hash 校验和陈旧提示，不用于判断该 profile 能否切。判断依据是 `last_writeback_at ?? captured_at`：如果一个 profile 从来没被切换过、`captured_at` 已经很久远，或最近一次写回很久远，refresh_token 大概率已被 server 端旋转作废，切过去可能失败。

### 4.9 Target

App Definition 为某个 kind 声明的"目标位置"，是切换时被改写的对象。

- Claude `env_injection` target：`~/.claude/settings.json` 的 `$.env` 子树。
- Claude `oauth_capture` target：Keychain entry + `~/.claude.json` 的 `$.oauthAccount` 与 `$.userID` 子树；如果 `~/.claude/settings.json` 中存在会覆盖 OAuth 的 managed env keys，同一次切换还要清理这些键。
- Codex `file_template` target：`${CODEX_HOME:-~/.codex}/auth.json` + `${CODEX_HOME:-~/.codex}/config.toml` 中声明的 managed TOML paths。
- Codex `oauth_capture` target：`${CODEX_HOME:-~/.codex}/auth.json`（仅 file-backed credential store；+ 可选 managed TOML paths）。

### 4.10 Defensive Backup

切换前对所有将被改写的目标位置自动建立的备份。它是防御性的，不是 profile 备份——目的是用户在目标文件里的手工改动（MCP 配置、自定义 Codex profile 等）丢失后可以恢复。

backup 不出现在 profile 列表里。Keychain entry 和 JSON 子树同样要进 backup（前者以 JSON 文件形式落地，后者以原值落地）。

### 4.11 Plan

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
    write ***  (sha256 7a3f... from capture manifest)
  ~/.claude.json
    write $.oauthAccount (5f3e..., Personal, work@example.com)
    write $.userID
  defensive backup: backups/claude/20260523T100000Z/

Post-write verify: account_uuid must equal 5f3e...
```

## 5. 本地数据布局

switch-cli 的所有文件统一放在 `~/.switch-cli/` 下，跨 Linux / macOS / Windows 走同一路径，不走 XDG 三段。可通过 `SWITCH_CLI_HOME` 环境变量重定向根目录（必须为绝对路径且展开后落在用户 home 内），主要用途是自动化测试以及希望走 XDG 的 Linux 用户软链到 `$XDG_CONFIG_HOME/switch-cli`。MVP 不做更精细的 XDG split。

系统预置 App Definition 随二进制发布。权威版本在源码的 `src/app_definitions/builtin/*.yaml`，构建时嵌入二进制；安装包可同时放一份只读资源副本供用户查看。它们不落在 `~/.switch-cli/` 下。用户目录只保存扩展、覆盖、profile、capture、备份和运行时状态。

```text
~/.switch-cli/
  profiles.yaml                    # 用户主入口：profile 注册表 + preferences；只由 profile 管理命令写入
  apps.d/                          # 用户新增的 App Definition
    opencode.yaml
    gemini.yaml
  overrides.d/                     # 用户对系统预置 Definition 的局部覆盖
    claude.yaml
    codex.yaml

  captures/                        # oauth_capture profile 的 blob，工具自动维护
    claude-anthropic-work/
      manifest.json                # sha256 / captured_at / last_writeback_at，工具自动维护
      keychain.json                # macOS Keychain entry 内容；Linux 为 .credentials.json
      oauthAccount.json
      userID.txt
    codex-chatgpt-work/
      manifest.json
      auth.json
      config.toml
  backups/                         # 防御性备份，工具自动维护
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

  state/                           # 运行时状态，工具自动维护，不应手编
    active.json                    # 每个 App 当前活动 profile id
    pending-switch/                # apply/bookkeeping 之间的崩溃恢复 journal；正常情况下为空
      claude.json
    history.jsonl                  # 操作历史
  locks/                           # 文件锁
    claude.lock
    codex.lock
    profiles.lock
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

`state/pending-switch/<app>.json` 只在 `use` / `restore-target` 已经完成防御性备份、准备改写目标位置时短暂存在。它记录 operation_id、app、from_profile、to_profile、backup_id、目标 target 的 expected hash / identity 和当前阶段。正常成功路径在更新 `active.json` 与 `history.jsonl` 后删除该文件；如果进程崩溃，下次该 App 的任意写命令在获取 App 锁后必须先读取它：

- 目标状态已经与 `to_profile` 的 hash / identity 匹配 → 补写 `active.json` / history（按 operation_id 去重），删除 pending journal。
- 目标状态仍与 `from_profile` 匹配 → 删除 pending journal，报告上次操作未生效。
- 目标状态两边都不匹配，但 backup 完整 → 提示或自动回滚到 backup（非交互式默认拒绝，要求显式 `restore-target`）。
- backup 不完整或恢复失败 → 拒绝新的写操作，报 `InterruptedSwitch`，提示用户手工检查目标文件。

`history.jsonl` 每行一条记录，只含元数据：

```json
{
  "operation_id": "01JY1H8Y8W7Q7K3Y7A5W4M3D2P",
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

MVP 不实现 `history.jsonl` 的 rotate；文件长期 append-only。Phase 2 引入按行数或大小的 rotate 策略。

## 6. 配置模型

### 6.1 配置文件家族

switch-cli 把配置和状态分散在若干文件 / 目录里，分别承担清晰不同的职责：

| 文件 / 目录 | 作用 | 维护者 |
|-------------|------|--------|
| `src/app_definitions/builtin/*.yaml`（源码）/ 二进制内嵌 Definition | 系统预置 App 的 schema、handler 绑定、进程探测和 doctor 检查项等 | 源码维护，构建时校验并嵌入二进制，随二进制版本一起更新 |
| `~/.switch-cli/apps.d/*.yaml` | 用户新增的 App Definition（系统未预置的 App）| 用户手编 |
| `~/.switch-cli/overrides.d/*.yaml` | 对已有 App Definition 的局部覆盖（白名单字段）| 用户手编 |
| `~/.switch-cli/profiles.yaml` | **profile 注册表 + CLI 偏好** | 用户手编；switch-cli 只在 add / edit / remove / import-current 等 profile 管理命令中写入 |
| `~/.switch-cli/captures/<id>/` | oauth_capture profile 的 blob 内容和 manifest | switch-cli 自动维护（import-current / writeback）|
| `~/.switch-cli/backups/<app>/<ts>/` | 防御性备份 | switch-cli 在写操作前自动创建 |
| `~/.switch-cli/state/active.json` | 每个 App 当前活动 profile id | switch-cli 在 use / restore 成功后更新 |
| `~/.switch-cli/state/pending-switch/<app>.json` | apply 与 bookkeeping 之间的崩溃恢复 journal | switch-cli 在写目标前创建，成功提交或恢复后删除 |
| `~/.switch-cli/state/history.jsonl` | 操作历史元数据（append-only）| switch-cli 追加写 |
| `~/.switch-cli/locks/` | 文件锁 | switch-cli 临时持有 |

#### profiles.yaml 的职责和写入边界

profiles.yaml 是用户可手编的稳定配置。switch-cli 可以读取它，但只有明确修改 profile 注册表的命令可以写它：`switch add`、`switch edit`、`switch remove`、`switch import-current`（以及未来同类 profile 管理命令）。`switch use`、`switch status`、`switch doctor`、`switch restore-target` 和 OAuth writeback 都不得修改 profiles.yaml。

1. **Profile 注册表**：记录这台机器上有哪些 profile（id / app / kind / name / created_at 等）。无论什么 kind 都需要这层 metadata；`switch list` / `switch use <id>` / `switch status` 都从这里读起。

2. **静态凭据的内联存储**（env_injection / file_template）：secret 字段（`auth_token` / `api_key` 等）**直接明文写在** `fields` 块里。理由：这些 token 是用户从代理服务商页面复制的静态字符串，性质上就是配置，inline 存读写都顺手，也方便手编 / 对比 / 修改 model 等批量操作。

3. **动态凭据的清单**（oauth_capture）：secret blob **不**在 profiles.yaml 里。这里只放：
   - `identity`：非敏感的身份指纹（accountUuid / email / organizationName 等）
   - `capture.sources`：每个 source 的 type / 目标位置 / `stored_as` / platform / required 等稳定声明

   实际的 OAuth blob 在 `~/.switch-cli/captures/<id>/` 下作为独立文件存。`sha256`、`captured_at`、`last_writeback_at` 等动态元数据写入 `captures/<id>/manifest.json`，因此 oauth_capture 切换和 writeback 不会污染用户可编辑的 profiles.yaml。

4. **CLI 偏好**：`preferences` 块（`default_app` / `confirm_before_switch` / `keep_backups` / `redact_secrets` / `oauth_stale_warn_days`）。

#### 不在 profiles.yaml 里的内容

App Definition 与 override **不写入 profiles.yaml**，独立放在 `apps.d/` 和 `overrides.d/` 下。系统预置 Definition 由二进制提供；用户扩展 / 覆盖文件在加载时与系统预置合并成运行时的 resolved definition registry。这样拆分的理由：

- 编辑权责分离：Definition 是"这个 App 的哪些本地状态可被切换、如何切换"的协议，profile 是"我要切到哪组状态"的数据。混在一起会让普通用户在新增 profile 时不慎修改 Definition 字段。
- 升级路径不同：系统 Definition 随二进制升级；profile 由用户长期持有，schema 演进策略不同。

OAuth blob、capture manifest、防御性备份、活动 profile 指针、操作历史等都属于工具自动维护的运行时状态，分别放在 `captures/` / `backups/` / `state/` / `locks/` 子目录下，不与 `profiles.yaml` 混在同一文件里。

#### 关于"静态明文 secret 与动态 blob 分离"的取舍

env_injection / file_template 的 secret 明文存在 profiles.yaml 里、oauth_capture 的 secret 存在 captures/ 里——这是**有意的不对称**：

- 静态字段的心理模型就是"一段配置"；对 MVP 的 API key / 代理 token 来说，明文 inline 存读写、diff、手编都直观，多 profile 场景下用户能在一个文件里看完整张表。
- OAuth blob 是 App 自己持续 mutate 的状态（refresh 时被改写）。若把它写进 profiles.yaml，每次刷新都会污染用户可编辑的配置文件，把"用户意图"和"工具/App 自动维护的动态状态"混在一起。
- 代价是 profiles.yaml 通常含明文 secret，**不能直接 git commit 或公开分享**。§6.7 / §11.2 强制 `0600` 权限。如果用户有跨机器同步需求，留给 Phase 3 的 `switch export --unsafe-export` 提供脱敏选项。
- 同理，`~/.switch-cli/captures/<id>/manifest.json` 是工具维护的运行时索引，用户不应手编；丢失时可由 capture 文件重新计算 hash，但 `captured_at` / `last_writeback_at` 审计信息会丢失。

### 6.2 App Definition 文件

App Definition 文件只允许声明数据，不允许声明可执行脚本。MVP schema 摘要：

```yaml
schema_version: 1
app:
  id: claude
  display_name: "Claude Code"
  definition_version: 1

process_probe:
  names: ["claude", "Claude"]

kinds:
  env_injection:
    field_schema:
      base_url:
        type: string
        required: true
      auth_token:
        type: string
        required: true
        sensitive: true
      models:
        type: object
        fields:
          default:
            type: string
            required: false
    targets:
      - handler: json_env_merge
        path: ~/.claude/settings.json
        json_path: $.env
        managed_keys:
          - ANTHROPIC_BASE_URL
          - ANTHROPIC_AUTH_TOKEN
          - ANTHROPIC_MODEL
        mapping:
          ANTHROPIC_BASE_URL: "{{ fields.base_url }}"
          ANTHROPIC_AUTH_TOKEN: "{{ fields.auth_token }}"
          ANTHROPIC_MODEL: "{{ fields.models.default }}"

  oauth_capture:
    sources:
      - handler: macos_keychain_entry
        service: "Claude Code-credentials"
        account: "${MACOS_USER}"
        platforms: [macos]
      - handler: json_subtree
        path: ~/.claude.json
        json_path: $.oauthAccount
    identity:
      handler: json_paths
      fields:
        account_uuid: "$.oauthAccount.accountUuid"
        email: "$.oauthAccount.emailAddress"
```

加载规则：

- 系统预置 Definition 先从二进制内嵌 registry 加载，用户 `apps.d/*.yaml` 后加载，最后加载 `overrides.d/*.yaml`。
- `apps.d/*.yaml` 只能声明新的 app id；与系统预置或其他用户 Definition 重名时拒绝加载。
- `overrides.d/*.yaml` 只能覆盖已存在 app id，且只允许修改 schema 默认值、字段展示名、provider 模板、managed keys 的追加项、doctor 展示项等白名单字段。
- handler 名称必须来自二进制内置 registry；未知 handler 直接拒绝加载。
- Definition 不支持 `login.command`、`reauth` 或任何登录相关字段，也不支持任何可执行命令字段。
- target path 默认必须在当前用户 home 内，且经过 `~` / `${MACOS_USER}` / 内置白名单路径环境变量展开后再做边界检查。
- Definition 加载失败时，写命令拒绝执行；只读命令显示错误并继续展示已成功加载的 profile。
- Definition 的来源（system / user / override）由 loader 根据内嵌 registry 或文件位置计算，不从 YAML 字段读取。
- 只读资源目录中的系统 Definition 副本不参与运行时加载；`switch apps show <app>` 可显示 resolved Definition，并标明 system 部分来自二进制内嵌 registry，必要时显示资源副本路径供用户审计。
- `switch apps export <app> --as override` 生成的文件必须能被 `switch apps validate` 直接通过；如果目标文件已存在，默认拒绝覆盖，除非显式 `--force`。

### 6.3 Profile 配置

```yaml
schema_version: 1

preferences:
  default_app: claude
  confirm_before_switch: true
  keep_backups: 20
  redact_secrets: true
  oauth_stale_warn_days: 30        # capture 超过这个天数未写回时切换前警告

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
          stored_as: keychain.json
          platforms: [macos]
        - type: file
          path: "${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json"
          stored_as: credentials.json
          platforms: [linux]
        - type: json_subtree
          path: ~/.claude.json
          json_path: $.oauthAccount
          stored_as: oauthAccount.json
        - type: json_subtree
          path: ~/.claude.json
          json_path: $.userID
          stored_as: userID.txt
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
          stored_as: keychain.json
          platforms: [macos]
        - type: file
          path: "${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json"
          stored_as: credentials.json
          platforms: [linux]
        - type: json_subtree
          path: ~/.claude.json
          json_path: $.oauthAccount
          stored_as: oauthAccount.json
        - type: json_subtree
          path: ~/.claude.json
          json_path: $.userID
          stored_as: userID.txt
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
          path: "${CODEX_HOME:-~/.codex}/auth.json"
          stored_as: auth.json
        - type: file
          path: "${CODEX_HOME:-~/.codex}/config.toml"
          stored_as: config.toml
          required: false
    extensions: {}
```

### 6.4 Schema 约束

MVP 把所有 `schema_version` / `definition_version` **锁定为 `1`**，不引入 v1 → v2 迁移代码路径。仅保留以下不变量：

- 顶层 `schema_version` 标识整体 profiles.yaml 版本；每条 profile 的 `schema_version` 标识该 `app + kind` 组合的字段 schema 版本；App Definition 的 `definition_version` 标识该 definition 自身 schema。三者命名空间预留，独立演进。
- 读到比当前二进制更高的任一 `schema_version` → 拒绝写入，仅 read-only 命令可用，提示升级 CLI。
- 未识别的 `extensions` 字段在 profile 管理命令写回期间必须保留。

完整的旧版本迁移、迁移失败降级、profiles.yaml.bak 等策略推迟到 Phase 2 文档；MVP 出现需要 bump 时再补设计。

### 6.5 字段规范

各 App / kind 的字段 schema 由 resolved App Definition 维护。所有 kind 共享的约定：

- `id` 必须满足 `^[a-z0-9][a-z0-9-]{0,63}$`。`switch add` / `import-current` 自动生成的 id 形如 `<app>-<slug(name)>`，可用 `--id` 显式指定覆盖。
- `slug(s)` 的定义：转小写 → 把任何不在 `[a-z0-9]` 范围的字符替换为 `-` → 合并连续 `-` → 去掉首尾 `-`。最终 id（含 `<app>-` 前缀）总长必须 ≤ 64 字符，超长则截断 slug 部分并保留前缀。slug 结果为空时（如全中文 name）报错要求用户显式 `--id`。
- `name` 是任意 UTF-8 字符串，仅用于展示。
- `notes` 可选，多行字符串。
- `created_at` 由 CLI 自动写入。
- `extensions` 是开放对象。
- `oauth_capture` 必须含 `identity` 和 `capture.sources`；动态 hash 和时间戳由 `captures/<id>/manifest.json` 维护。
- `${MACOS_USER}` 从 `getpwuid(getuid())` 取，不信任 `$USER` 环境变量。路径模板还支持内置 Definition 白名单中的 `${CLAUDE_CONFIG_DIR:-~/.claude}` 和 `${CODEX_HOME:-~/.codex}`；二者按 §9.1 做 home 边界、绝对路径和 symlink 检查。

### 6.6 命令行覆盖

命令行参数覆盖 `preferences`。没有项目级配置，也没有用环境变量覆盖 CLI 偏好的机制；§9.1 中的 `CLAUDE_CONFIG_DIR` / `CODEX_HOME` 仅用于解析目标 App 自己的配置目录，`--secret-field @env:NAME` 仅用于读取用户显式指定的 secret 值。

### 6.7 文件权限

`~/.switch-cli/` 根目录及所有子目录强制 `0700`；`profiles.yaml`、用户 App Definition、override 文件、captures / backups / state 下的所有文件强制 `0600`。完整权限矩阵见 §11.2。启动时若权限被改宽，doctor 警告并提示修复。

## 7. 命令设计

### 7.1 MVP 命令

| 命令 | 说明 |
|------|------|
| `switch apps` | 列出已加载 App Definition、来源、支持的 kind |
| `switch apps show <app>` | 展示 resolved App Definition（脱敏，不含 profile 数据）|
| `switch apps export <app> [--source system\|resolved] [--as override] [--output <path>]` | 导出内置 / resolved Definition，或生成 override 起点 |
| `switch apps validate [<path>]` | 校验用户 App Definition / override 文件 |
| `switch list [<app>]` | 列出已注册 profile（脱敏） |
| `switch show <id>` | 查看单个 profile 的元数据、verification identity、非敏感字段 |
| `switch add <app> <name> [--id <id>] [--kind <kind>] [--field k=v ...] [--secret-field k=@stdin\|@prompt\|@env:NAME\|@file:PATH ...]` | 手动添加 env_injection / file_template profile |
| `switch edit <id>` | 用 `$EDITOR` 打开 profile 的 yaml 片段编辑 |
| `switch remove <id>` | 删除 profile；同时清理 captures/<id>/；若该 profile 是某 App 的活动 profile，将该 App 在 state/active.json 中置为 null（不动目标文件）|
| `switch import-current <app> <name> [--id <id>] [--kind auto\|<kind>]` | 从 App 当前状态自动识别 kind 并捕获 profile |
| `switch use <id>` | 切换到指定 profile（oauth_capture 会先写回当前活动 profile 的 capture，不修改 profiles.yaml）|
| `switch use <id> --dry-run` | 只打印 plan，不写入 |
| `switch status [<app>]` | 显示每个 App 的活动 profile、是否 drift |
| `switch backup list [<app>]` | 列出防御性备份 |
| `switch restore-target <app> <backup-id>` | 从防御性备份恢复目标位置 |
| `switch doctor [<app>]` | 检查路径、权限、字段完整性、进程状态、Keychain 可访问性 |
| `switch config path` | 打印 profiles.yaml 路径 |

### 7.2 命令选项约定

- `--yes` / `-y`：跳过交互确认。
- `--json`：以 JSON 格式输出（同样脱敏）。`--dry-run` 与 `--json` 组合时，plan 以 JSON 输出（字段集合与人类格式一致，secret / blob 字段同样以 `"***"` 占位），便于脚本化校验和 MVP 验收。
- `--allow-running`：在目标 App 进程运行时仍允许切换。**对 `oauth_capture` 不生效**——切 OAuth profile 永远要求 App 退出。
- `--force`：仅在 `add` 同 id 覆盖、`remove` 跳过确认时使用。
- `--field k=v` 只允许传非敏感字段。若字段 schema 标 `sensitive: true`，或字段名按敏感模式命中，CLI 必须拒绝明文 argv 输入并报 `UnsafeSecretArgument`。
- `--secret-field k=@stdin|@prompt|@env:NAME|@file:PATH` 用于敏感字段：`@prompt` 为 TTY masked prompt，`@stdin` 从标准输入读取一次，`@env:NAME` 只把环境变量名放进 argv、值从进程环境读取，`@file:PATH` 从本机文件读取且同样做 home 边界和权限检查。内置便捷参数（如 `--auth-token-stdin`、`--api-key-stdin`）只是对应 `--secret-field` 的别名。
- 默认所有写命令在交互式 TTY 下请求确认；非交互式必须显式 `--yes`。

### 7.3 暂缓命令

不进入 MVP：

- `switch rename`（用 `remove` + `add` 替代）。
- `switch diff` / `plan` / `apply` 通用三件套。
- `switch backup prune`（MVP 自动按 `keep_backups` 修剪）。
- `switch export` / `import` 跨机器迁移。
- shell completion 脚本。
- `switch module install`、`switch plugin`、`switch trust` 等可执行插件 / 沙箱概念。

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
load profiles.yaml + resolved App Definition registry
validate app definition supports requested kind
reject oauth_capture (oauth_capture 只能通过 import-current 创建)
reject sensitive fields supplied by --field k=v; require --secret-field or TTY prompt
fill default fields, prompt missing required ones (TTY) or fail (non-TTY)
resolve id:
  if --id <explicit>: use it as-is (must match id regex)
  else:               id = "<app>-" + slug(name)
on id collision: refuse unless --force
append profile to profiles.yaml (atomic write)
```

id 自动生成示例：

- `switch add claude glm` → `claude-glm`
- `switch add claude "GLM 智谱"` → `claude-glm`（中文字符在 slug 中被丢弃，详见 §6.5）
- `switch add codex personal --id corp-prod` → `corp-prod`（显式 `--id` 覆盖自动规则）

不修改任何目标位置。

### 8.2 `switch import-current <app> <name>`

`import-current` 默认 `--kind auto`。它不要求用户预先知道当前 App 是 API-key、OAuth 还是 env 配置；CLI 根据 resolved App Definition 的 `import_current` 探测规则读取当前本地状态，生成一个或多个 profile draft。

可选 `--kind <kind>` 用于约束探测范围：

- 如果指定 kind，只运行该 kind 的探测和导入逻辑；当前状态不匹配则失败。
- 如果未指定或 `--kind auto`，单一候选自动采用，多候选进入 `ImportAmbiguous`。
- 交互式 TTY 下 `ImportAmbiguous` 展示候选摘要并让用户选择；非交互式必须显式 `--kind <kind>` 或失败。

```
load profiles.yaml + resolved App Definition
acquire profiles lock, then app lock
definition-driven import probes current state:
  - settings.json $.env populated (Claude)        -> draft env_injection
  - auth.json API-key shape (Codex)               -> draft file_template
  - OAuth indicators detected                     -> draft oauth_capture
  - exactly one candidate                         -> use that kind automatically
  - multiple modes co-exist                       -> ImportAmbiguous, ask user or require --kind
  - no candidate                                  -> TargetMissing / KindNotSupported with doctor hint
for oauth_capture drafts:
  read all sources (Keychain entry + json_subtree + relevant files)
  extract identity fields from oauthAccount / auth.json
  check identity.account_uuid against existing profiles: if match, ask user
    whether to refresh that profile's capture instead of creating new
  on refresh:
    - capture bytes 全部用最新读到的内容覆盖
    - identity 字段全部用最新值覆盖（包括 organization_name / email /
      subscription_type 等可能在 web 后台被改的字段，verify 仍只比对
      稳定的 account_uuid）
    - profile 的 name / notes / extensions 保留用户当前值不动
    - 如果 identity 与 profiles.yaml 中已有值不同，必须把 profile metadata
      原子写回 profiles.yaml；只有 identity 完全不变的纯 capture 刷新可以
      不修改 profiles.yaml
resolve id (same rule as 8.1: --id 优先，否则 "<app>-" + slug(name))
show summary (sanitized), ask for confirmation, allow user to edit name / id
on confirm:
  - copy bytes into captures/<id>/ (0600/0700)
  - write captures/<id>/manifest.json with sha256 + captured_at
  - if creating profile or changing profile metadata / identity, write profile to profiles.yaml (atomic)
  - if only refreshing an existing profile's capture and identity is unchanged, leave profiles.yaml unchanged
release locks
```

特例：Claude 在 macOS 上检测到 Keychain `Claude Code-credentials` 时，必须把它和 `~/.claude.json` 的 `$.oauthAccount` / `$.userID` 一起捕获——只捕获其中一个会让记录处于不一致状态。

### 8.3 `switch use <id>`

```
load profiles.yaml + resolved App Definition + state/active.json
resolve target_profile by id
acquire app lock
reload profiles.yaml + state/active.json and re-resolve target_profile
if target_profile no longer exists -> abort ProfileNotFound
if state/pending-switch/<app>.json exists:
    run pending-switch recovery first:
      - if live targets already match pending.to_profile -> finish bookkeeping and delete journal
      - if live targets still match pending.from_profile -> delete journal and continue
      - otherwise refuse with InterruptedSwitch unless user explicitly runs restore-target
detect target app process running:
  - if target_profile.kind == oauth_capture: refuse, ignore --allow-running
  - else: refuse unless --allow-running

# Short-circuit: 切到当前活动 profile 自身
if target_profile.id == state.active_profiles[app]:
    if target_profile.kind == oauth_capture:
        # 这是保护当前 capture 的非破坏性时机：只跑 Step B writeback；
        # writeback 仍必须通过 live identity 门禁
        build writeback-only plan
        if --dry-run: print plan, release lock, exit
        ask confirmation unless --yes
        run Step B only
        update history (operation=use, no_op_apply=true, writeback_ok=...)
        release lock; exit
    else:
        # env_injection / file_template 切到自身真正是 no-op
        update history (operation=use, no_op=true)
        release lock; exit

# Step A: build plan
build plan from target_profile (kind-specific render or load from captures/)
if current active profile is oauth_capture:
    include writeback actions and live identity precheck for its current-platform sources
if target_profile.kind == oauth_capture:
    load captures/<target.id>/manifest.json
    include stale warning if manifest.last_writeback_at ?? manifest.captured_at is too old
if --dry-run: print plan, release lock, exit
ask confirmation unless --yes

# Step B: writeback current active (oauth_capture only)
# manifest.json 是 source of truth：只有 manifest 原子替换成功后，
# 整个 writeback 才视为已提交
if state.active_profiles[app] exists:
    previous = load profile(state.active_profiles[app])
    if previous missing -> abort ProfileNotFound, no writeback or target apply
    if previous.kind == oauth_capture:
        # B0. 身份门禁：先从 live target 提取 verification identity
        #     （Claude 读 ~/.claude.json $.oauthAccount；Codex 读 auth.json tokens.account_id/id_token）
        live_identity = extract identity from live target
        if live_identity does not match previous.identity stable fields:
            abort entire switch, no captures touched
            error DriftBeforeWriteback with hint:
              - current target state is not the profile recorded in active.json
              - run status / import-current / restore-target before switching

        # B1. 再把所有 source 当前 bytes 全部读入内存
        for each source in previous.capture.sources (current platform):
            read current bytes from live target
        if any read fails -> abort entire switch, no captures touched

        # B2. 计算 sha256，写每个 source 到 .tmp + fsync + rename 到最终路径
        for each source:
            write bytes to captures/<previous.id>/<stored_as>.tmp
            fsync; rename .tmp -> <stored_as>

        # B3. 构造新的 manifest（含全部新 sha256 + last_writeback_at = now），
        #     原子替换 captures/<previous.id>/manifest.json —— 这是 commit point
        write captures/<previous.id>/manifest.json.tmp
        fsync; rename manifest.json.tmp -> manifest.json

        # 失败模型
        # - B1 失败                : 没有任何 capture 文件被改动，abort
        # - B2 中途失败            : 部分 source 文件可能已更新，但 manifest 仍指向旧 sha256；
        #                            doctor 会检测出 sha256 mismatch，下次切换的 writeback
        #                            会用最新 live bytes 覆盖整组 source，自我修复
        # - B3 失败                : 同上；manifest 未提交意味着此次 writeback 整体未生效
        # 任一步失败 -> abort entire switch, 不进入 Step C

# Step C: defensive backup of ALL target locations
for each target (files, Keychain entries, json subtrees):
    read current bytes
    write to backups/<app>/<timestamp>/<file>
write backup manifest
write state/pending-switch/<app>.json with:
  operation_id, app, from_profile, to_profile, backup_id, target list,
  expected hashes, expected oauth identity, stage="applying"

# Step D: apply
for each target:
    stage new bytes (atomic file replace / Keychain write / json_subtree merge)
update state/pending-switch/<app>.json stage="verifying"

# Step E: verify
sha256 of each written file / subtree / Keychain entry equals expected
for kind == oauth_capture:
    re-read $.oauthAccount.accountUuid (or codex equivalent)
    must equal target_profile.identity.account_uuid
    if mismatch -> rollback from defensive backup, error IdentityMismatch
update state/pending-switch/<app>.json stage="bookkeeping"

# Step F: bookkeeping
update state/active.json
append history.jsonl with operation_id (skip if already present during recovery)
delete state/pending-switch/<app>.json
prune old backups beyond keep_backups
release lock
```

任一步失败的处理：

- writeback 失败 → 完全中止，目标位置不动，错误信息说明哪个 source 写回失败。
- defensive backup 失败 → 中止，不动任何 target。
- pending journal 写入失败 → 中止，不动任何 target。
- apply 中途失败 → 用刚才的 backup 自动恢复已替换的 target，标记本次失败，删除或更新 pending journal。
- verify 失败（hash 或 identity）→ 同 apply 失败处理。
- apply 成功但 bookkeeping 前崩溃 → 下一次写命令根据 pending journal 完成提交或要求恢复，不允许直接开始新切换。
- 错误信息明确指出失败步骤、target、可用 backup id。

### 8.4 `switch status [<app>]`

```
load config + state
for each app:
  if state/pending-switch/<app>.json exists -> interrupted (show recovery hint; continue next app)
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
  check high-priority external overrides declared by App Definition
    if managed target matched but override exists -> matched-with-overrides
  if any required target missing -> missing
```

MVP 状态集合：

| 状态 | 含义 |
|------|------|
| `matched` | env/file kind 渲染结果一致 / oauth identity 一致 |
| `matched-with-overrides` | switch-cli 管理的 target 与 active profile 一致，但检测到更高优先级的外部配置、环境变量或 managed policy 可能覆盖实际运行认证 |
| `drifted` | 活动 profile id 已知，但实际状态与预期不符 |
| `missing` | required target 不存在 |
| `no-active` | state 里没有该 App 的活动 profile |
| `interrupted` | 存在 pending-switch journal，上次写操作需要恢复或人工处理 |

### 8.5 `switch restore-target <app> <backup-id>`

从防御性备份恢复指定 App 的所有目标位置。来源 bytes 来自 `backups/<app>/<backup-id>/`，操作不改 state/active.json，恢复前再生成一份新备份。

对包含 Keychain entry / json_subtree 的 backup，恢复时按对应机制写回（不是简单 file copy）。

`restore-target` 与 `use` 使用同一套 App 锁、防御性备份、pending journal、apply、verify 和失败回滚协议；区别是它不更新 `active.json`，恢复完成后 `status` 可能显示 `drifted` 或 `no-active`，这是预期行为。

`switch list` / `show` / `edit` 的流程平凡，略。

### 8.6 `switch remove <id>`

```
acquire profiles lock
load profiles.yaml + state/active.json
resolve profile by id
acquire app lock for profile.app before mutating active state or deleting captures
reload state/active.json after app lock
if state/pending-switch/<profile.app>.json exists:
    refuse with InterruptedSwitch; user must complete recovery / restore first
for each app in state.active_profiles:
    if state.active_profiles[app] == id:
        set state.active_profiles[app] = null
        write state/active.json atomically
        # 不动目标文件；下一次 status 将报 no-active
delete profile entry from profiles.yaml (atomic write)
delete captures/<id>/ recursively (if exists)
append history (operation=remove)
release app lock, then profiles lock
```

注意：remove 只清理 switch-cli 自身的注册表和 capture，不还原目标 App 当前的本地状态——用户如果希望"删掉 profile 同时把目标文件清空到无配置状态"，应先 `switch restore-target` 到一个早期备份，再 `switch remove`。

## 9. 文件操作协议

### 9.1 路径解析

- 所有 managed 路径中的 `~` 在配置加载时一次性展开为 `getpwuid(getuid())` 得到的真实 home 目录。
- **不信任 `$HOME` / `$USER` 环境变量**——`${MACOS_USER}` 也从 getpwuid 解出。
- App Definition 可以声明受信任的路径根环境变量，但必须显式列在 Definition schema 中，不能任意展开用户环境。MVP 内置仅允许：
  - Claude Linux / Windows credentials 目录：`CLAUDE_CONFIG_DIR`（未设置时回退到 `~/.claude`）。
  - Codex home：`CODEX_HOME`（未设置时回退到 `~/.codex`）。
- 这些环境变量的值必须是绝对路径或 `~/` 前缀路径；展开、规范化和 symlink 解析后仍必须落在当前用户 home 内。plan / doctor 必须展示最终 resolved path，而不是只展示模板路径。
- 展开后路径必须落在 home 之下；否则 `doctor` 标红，`use` 拒绝写入。
- `capture.sources[].stored_as` 不是任意文件路径，只能是相对 `captures/<profile-id>/` 的普通文件名或子路径；禁止绝对路径、空路径、`.` / `..` 片段、symlink 逃逸和与 `manifest.json` 冲突。删除 `captures/<id>/` 时也必须在规范化后确认路径仍位于该 profile 的 capture 目录内。

### 9.2 锁

```text
~/.switch-cli/locks/<app>.lock     # 单 App 的状态写操作
~/.switch-cli/locks/profiles.lock    # profiles.yaml 写操作
```

`use` / `restore-target` 持有 App 锁，不持有 profiles lock，也不写 profiles.yaml；获取 App 锁后必须重新读取 profiles.yaml 和 state，避免与 `remove` 竞态。`add` / `edit` 只持有 profiles lock。`remove` 会删除 capture 并可能修改 `state/active.json`，因此按 profiles lock → profile.app 的 App lock 顺序持有两把锁。`import-current` 同时写 profiles.yaml 和 captures，按固定顺序持有 profiles lock → App lock，避免锁反转。`list` / `show` / `status` / `doctor` 无锁读取。

锁实现使用 POSIX advisory lock（`flock(2)` 或 `fcntl(F_SETLK)`），持有者状态由 OS 维护——switch-cli 进程崩溃、被 kill 或异常退出时，OS 自动释放锁。锁文件本身允许残留为空文件，不需要 stale lock 清理逻辑，也不在锁文件中写 PID。

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

若目标文件已存在，保留比 `0600` 更严格的权限；如果现有权限比 `0600` 更宽，写入 secret 前必须收紧到 `0600`，或在无法 chmod 时拒绝写入并提示用户修复权限。非 secret 目标可继承原权限。

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
2. 按 App Definition 声明的 `managed_json_subtrees`（如 `[$.oauthAccount, $.userID]`），用 capture 中的对应值整体替换。
3. 其他键（projects、tipsHistory 等运行时状态）保持不变。
4. 序列化并原子替换。

两种场景都必须保持 JSON 文件的字段顺序稳定（用保序的反序列化器，如 Rust 的 `serde_json::Value` 配合 `preserve_order` feature），避免每次切换都产生大量无意义 diff。

### 9.5 TOML managed paths

Codex `config.toml` 不做整文件模板覆盖。App Definition 必须声明可管理的 TOML path，例如：

```text
model
model_provider
model_reasoning_effort
model_providers.<profile-provider-id>
```

写入规则：

1. 读现有 TOML，保留未知 key、未知 table、注释以外的可保留结构。
2. 只删除 / 替换 Definition 声明的 managed paths。
3. 不删除 `mcp_servers`、`projects`、`plugins`、`features`、`desktop`、`tui` 等非 managed table。
4. 输出前做脱敏 plan；写入走临时文件 + rename。

**实现层强制要求**：MVP 必须使用 lossless TOML 库（Rust 生态推荐 `toml_edit`，而不是 `toml`），保证未知 key / table 与字段顺序在写回后**位精确保留**——这是正确性依赖。注释和空行保留是 best effort（依赖库的能力），不作为正确性依赖。

### 9.6 Keychain backend（macOS）

- 读：`SecKeychainFindGenericPassword` / Security.framework API；CLI fallback 可用 `security find-generic-password -s <service> -a <account> -w`。
- 写：MVP 不假设 Keychain 有文件系统式原子 rename。先做防御性备份，再用 `SecItemUpdate` 更新既有 generic password 的 secret bytes；目标不存在时用 `SecItemAdd` 创建。若 Keychain 写失败，旧值必须保持可恢复；若后续 target 失败，按 backup 用同一机制回滚。
- 备份：把读出的 bytes 写入 `backups/.../<file>.json`，权限 `0600`。
- 删除：只在 `remove` 清理临时 / capture 资源时使用，不删除目标 App 的当前 Keychain entry。

MVP 仅实现 generic password 类型，仅访问当前用户的 login keychain。Keychain 写入不是跨 target 原子事务，因此 `use` 的整体原子性语义是"逐 target 备份 + 失败回滚"，不是 OS 级事务。

### 9.7 Symlink

- 目标位置是 symlink 时，沿链跟随到真实路径写入。
- 真实路径在 home 外时，MVP 默认拒绝写入；`doctor` 标红。
- 外部路径允许开关不在 MVP。

### 9.8 Hash

- 文件 / blob hash 使用 SHA-256。
- JSON subtree 的 hash 基于"序列化后的规范字节"（保序、UTF-8、无尾空格）。
- hash 不包含 mtime / 权限位。
- 输出场景默认只展示 hash 前 8 个 hex 字符。

## 10. 实证记录与 App Definition

### 10.0 Claude Code 凭据存储实证记录

以下基于 Claude Code 2.1.150（macOS）的逆向调研结论，作为系统预置 Claude Definition 和相关 handler 配置的依据：

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
- **Linux**：`${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json`，结构相同。
- **Windows**：官方文档说明默认在 `%USERPROFILE%\.claude\.credentials.json`，并且设置 `CLAUDE_CONFIG_DIR` 时 credentials 文件位于该目录下；MVP 暂不实现 Windows backend。

**账号身份载体**（在 `~/.claude.json` 顶层）：

- `oauthAccount` 对象：`accountUuid` / `organizationUuid` / `organizationName` / `organizationRole` / `emailAddress` / `subscriptionType` / `subscriptionCreatedAt` / `billingType` / `accountCreatedAt` / `hasExtraUsageEnabled` / `claudeCodeTrialEndsAt` / `ccOnboardingFlags` 等 18 个子键。
- 顶层 `userID`（64 字符 hash，关联 `accountUuid`）。

切换 OAuth 账号必须**同时**改 Keychain 和这两个 JSON subtree。仅改 Keychain 会导致 UI 显示与实际身份不一致的中间态。

**Token 刷新行为**：

- 刷新端点：`POST <anthropic-domain>/v1/oauth/token`。
- access_token 过期时自动触发，**refresh_token 大概率会被旋转**（从二进制 mutate 集合包含 `refreshToken` 推断）。
- 这意味着 capture 越久没回写，refresh 失败概率越高——是 writeback 机制的根本理由。
- 实测前提：固化 Claude 系统预置 Definition 前必须做一次"登录 → 触发刷新 → 比对 refresh_token 是否旋转 → 用旧 capture 尝试恢复"实验，把结论写入 Definition 注释或实现记录。

**Env 覆盖**：`CLAUDE_CODE_OAUTH_TOKEN` 走 setup-token 长期凭据，仅 inference-only，不适合日常 profile 切换，MVP 不纳入管理。

**Codex 实证记录**：Codex CLI 0.133.0（macOS）在 file-backed ChatGPT OAuth 模式下使用 `${CODEX_HOME:-~/.codex}/auth.json`，根字段包含 `OPENAI_API_KEY: null`、`auth_mode: "chatgpt"`、`last_refresh` 和 `tokens`；`tokens` 下包含 `access_token` / `account_id` / `id_token` / `refresh_token`。`codex login --with-api-key` 写出的 API-key 模式为根字段 `auth_mode: "apikey"` + `OPENAI_API_KEY` 两个字符串。官方文档同时说明 Codex 可使用 OS credential store；MVP 仅管理 file-backed `auth.json`，其他 store 报 `CredentialStoreUnsupported`。

### 10.1 Definition 与 handler 职责

App Definition 是产品知识的声明层。它负责声明：

- App id、display name、definition_version。
- 支持的 kind 列表。
- 各 kind 的 field schema（env_injection / file_template）。
- 各 kind 的 target spec 和 capture source spec（按平台过滤）。
- `managed_env_keys` / `managed_json_subtrees`：声明该 App 在各 kind 下管理的"键边界"，core 用于先清后写。
- fields 到目标内容的映射模板。
- capture 还原目标内容时使用的 source / restore handler。
- identity 提取路径。
- import_current 探测规则。
- doctor 检查项、进程探测规则。

Core handler 是受信任执行层。它负责：

- 读取和写入文件、JSON subtree、TOML 文件、Keychain entry。
- 根据 Definition 做字段校验、模板渲染、capture 复制和 identity 提取。
- import_current 的通用探测和 profile draft 构造。
- 进程探测、doctor 检查、路径边界检查、权限检查、锁、备份、hash。

Definition 和 handler 都不负责：

- 调 Anthropic / OpenAI 登录接口、引导登录或直接刷新 token。
- 判断目标状态在远端服务端是否有效（例如账号凭据是否仍可用）。
- 执行用户脚本、加载动态库或运行外部插件。

### 10.2 内置 handler registry

MVP 内置 handler 名称固定，可被系统预置 Definition 和用户 Definition 引用：

| handler | 用途 |
|---------|------|
| `json_env_merge` | 将 profile fields 映射到目标 JSON 文件的 env 子树，按 managed keys 先清后写 |
| `json_subtree` | 捕获 / 恢复某个 JSONPath 子树 |
| `file_capture` | 捕获 / 恢复整个文件 |
| `toml_managed_paths` | 只写 App Definition 声明的 TOML key / table，保留其他 TOML 配置 |
| `macos_keychain_entry` | 捕获 / 恢复 macOS generic password entry |
| `json_paths` | 从 capture 或目标状态按 JSONPath 提取 identity 字段 |
| `jwt_payload_json_paths` | base64url 解码 JWT payload 后按 JSONPath 提取非敏感 identity 字段；不校验签名、不据此做授权判断 |
| `process_name` | 按进程名粗粒度检测目标 App 是否运行 |

handler registry 是二进制的一部分。用户 Definition 只能引用 registry 中存在的 handler，不能声明新的 handler 代码。

### 10.3 Claude 系统预置 Definition

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
  - Linux：`${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json`（resolved path 必须落在 home 内）
  - 所有平台：`~/.claude.json` 的 `$.oauthAccount` 和 `$.userID`（json_subtree）
  - 所有平台：`~/.claude/settings.json` 的 `$.env` managed keys 清理（仅作为切到 OAuth 时的 target，不作为 OAuth capture source）
- `managed_json_subtrees`：`[$.oauthAccount, $.userID]` on `~/.claude.json`。
- identity 提取：从 capture 中 `oauthAccount.json` 读出 `accountUuid` / `organizationUuid` / `organizationName` / `emailAddress` / `subscriptionType`。
- verify：切换后从 `~/.claude.json` 读 `$.oauthAccount.accountUuid`，必须等于 `profile.identity.account_uuid`。
- 不能与 env_injection 同时活动：如果 `settings.json.env` 设了 `ANTHROPIC_AUTH_TOKEN`，会**覆盖** OAuth 走 API key。切到 oauth_capture 时如果发现 Claude managed env keys，plan 必须展示"将清空这些 env 键"，并把 `settings.json` 纳入 defensive backup / apply / rollback；交互模式要求用户确认，非交互模式要求 `--yes`。

**`import_current` 行为**

1. 探测 macOS Keychain `Claude Code-credentials` 是否存在；存在 → 候选 oauth_capture。
2. 探测 Linux `${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json` 是否存在；存在 → 候选 oauth_capture。
3. 探测 `~/.claude/settings.json` `$.env.ANTHROPIC_AUTH_TOKEN` 是否非空；非空 → 候选 env_injection。
4. 两者都存在 → `ImportAmbiguous`；交互式展示"Anthropic OAuth 与第三方代理 env 同时配置，请选择捕获哪一项"，非交互式要求 `--kind`。
5. 仅 oauth_capture 候选时，自动捕获 oauthAccount + userID，提取 identity，提示用户确认 name。

**doctor 输出**：settings.json 存在性、`managed_env_keys` 填充状态、`~/.claude.json` 中 `oauthAccount.accountUuid` / `email`、Keychain entry 是否可读（不读值，只测 existence）、是否检测到 Claude Code 进程。

doctor / status 还必须检测可能覆盖目标 profile 的高优先级认证来源：当前进程环境中的 `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY`、Claude settings scopes 中的相关 env 键、`apiKeyHelper`、以及 managed policy 强制项。MVP 不尝试修改项目级、local 或 managed scope，但如果这些来源存在，`status` 不能简单报告 `matched`；应报告 `matched-with-overrides` 或 `drifted` 并提示实际运行时可能不会使用目标 OAuth / env profile。

**Claude Code 升级时的字段倒退提示**：`json_subtree` 的应用语义是"用 capture 中的子树**整体替换**目标位置的子树"。如果 Claude Code 升级后在 `$.oauthAccount` 引入新字段（例如新增 `subscriptionRegion`），用 v2.1 时期捕获的旧 capture 恢复，会让这些新字段消失——目标 App 一般会在下一次启动时回填，但部分字段可能引发短暂的 UI 不一致。doctor 在检测到当前 `~/.claude.json.$.oauthAccount` 的字段集合超出最近一次实测固化在 Definition 注释中的 schema 时，给出"建议对所有 oauth_capture profile 重新 `switch import-current` 以同步新字段"的提示。MVP 不在 use 时阻塞，只 doctor 提示。

### 10.4 Codex 系统预置 Definition

以下基于 Codex CLI 0.133.0（macOS）本机实测，并与 OpenAI Codex 官方文档交叉确认：Codex CLI 支持 ChatGPT 登录和 API key 登录；`${CODEX_HOME:-~/.codex}/auth.json` 是 file-backed credential store 的凭据文件，但 Codex 也可能配置为 OS credential store。MVP 只管理 file-backed `auth.json`。如果 `config.toml` 中 `cli_auth_credentials_store` 明确设置为非 `file`，或当前平台没有 `auth.json`，Codex `oauth_capture` / `file_template` 写命令应拒绝，并提示用户先在 Codex 外部切到 file store 后重新 `import-current`。如果未显式设置但 `auth.json` 存在，则按 file-backed 处理。

支持的 kind：

- `file_template`：API-key 登录。
- `oauth_capture`：ChatGPT OAuth 登录（仅 file-backed credential store）。

**`file_template` 渲染规则**

- 目标文件：
  - `${CODEX_HOME:-~/.codex}/auth.json`：API-key 模式写入 `auth_mode: "apikey"` + `OPENAI_API_KEY` 两个字符串字段。已知 ChatGPT auth 形态会在根对象保留 `OPENAI_API_KEY: null`，因此判断 API-key 模式不能只看 key 是否存在，必须看值是否为字符串以及 `auth_mode`。
  - `${CODEX_HOME:-~/.codex}/config.toml`：仅写 Definition 声明的 managed TOML paths（如 `model`、`model_reasoning_effort`、当前 profile 对应的 provider 子表）；保留 `mcp_servers`、`projects`、`plugins`、`features` 等用户配置。
- 字段：
  - `fields.api_key` (sensitive, required)
  - `fields.base_url` (optional)
  - `fields.model`
  - `fields.model_provider`
  - `fields.model_reasoning_effort` (optional)
  - `fields.provider_id` (optional；缺省从 profile id 派生，用于 `[model_providers.<provider_id>]`)

**`oauth_capture` 规则**

- sources：
  - `${CODEX_HOME:-~/.codex}/auth.json`（必需）
  - `${CODEX_HOME:-~/.codex}/config.toml` 中的 managed TOML paths（可选，`required: false`；不捕获整文件）
- ChatGPT OAuth `auth.json` 实测形态：
  - 根字段：`OPENAI_API_KEY: null`、`auth_mode: "chatgpt"`、`last_refresh: <timestamp>`、`tokens: {...}`
  - `tokens` 字段：`access_token`、`account_id`、`id_token`、`refresh_token`
  - `id_token` payload 可解码出 `sub`、`email`、`name`、`auth_provider`、`https://api.openai.com/auth` 等 claims；MVP 只解码 payload 用于 identity，不校验 JWT 签名。
- identity 提取：
  - `account_id`：`$.tokens.account_id`
  - `subject`：`jwt_payload($.tokens.id_token).sub`
  - `email`：`jwt_payload($.tokens.id_token).email`
  - `name`：`jwt_payload($.tokens.id_token).name`（可选）
- verify：切换后从恢复的 `auth.json` 读 `auth_mode == "chatgpt"`，并比对 `tokens.account_id` 与 `profile.identity.account_id`；如果 profile 有 `subject`，同时比对 id_token payload 的 `sub`。
- refresh token 是否旋转：按动态凭据处理，`use` 切走当前 active Codex OAuth profile 前必须 writeback 整个 `auth.json` 并更新 capture manifest。

**`import_current` 行为**

- 读 `auth.json`：
  - `auth_mode == "chatgpt"` 且 `tokens.refresh_token` / `tokens.id_token` 为字符串 → draft `oauth_capture`。
  - `OPENAI_API_KEY` 为非空字符串，且 `auth_mode == "apikey"` 或缺失 OAuth `tokens` → draft `file_template`。
  - `auth_mode == "chatgpt"` 但 `OPENAI_API_KEY` 同时为字符串 → `ImportAmbiguous`，提示用户先在 Codex 外部整理登录状态。
  - credential store 不是 file-backed 或 `auth.json` 不存在 → 报 `TargetMissing` / `CredentialStoreUnsupported`（后者可作为 `DefinitionLoadFailed` 的细分错误或新增错误）。

**doctor 输出**：auth.json / config.toml 存在性、当前形态识别结果、识别到的 identity 字段、是否检测到 Codex 进程。

### 10.5 开源贡献边界

新增 App 的首选路径是新增声明式 App Definition。如果现有 handler 足够表达，可以只贡献 YAML 和 fixture；如果需要新的 source、target、extractor 或特殊实证逻辑，再贡献新的 core handler。源码组织建议：

```text
src/
  app_definitions/
    builtin/
      claude.yaml
      codex.yaml
    loader.rs
    schema.rs
  core_handlers/
    mod.rs
    json_env_merge.rs
    json_subtree.rs
    file_capture.rs
    toml_managed_paths.rs
    jwt_parser.rs
    keychain_macos.rs
```

新增 Definition 至少需要包含：

- 至少一种 kind 的实现。
- `import_current` 探测规则（哪怕仅声明 NotImplemented）。
- 渲染 fixture 测试（fields → bytes / target plan）。
- import_current fixture 测试（current state bytes → profile draft）。
- 对 oauth_capture Definition：identity 提取和 capture manifest 更新 fixture 测试。
- 不输出 secret 字段值的脱敏测试。
- 路径展开和 home 边界测试。

新增 core handler 必须包含：

- handler 输入 / 输出 schema。
- 正常路径和错误路径测试。
- secret 脱敏测试。
- home 边界、symlink 和权限测试（涉及文件系统时）。

oauth_capture Definition 或 handler 必须在 PR 描述里附上实测记录：

- 该 App 的 refresh_token 是否旋转。
- writeback 与 restore 是否真的能成功切换。
- 用户在 App 外部恢复当前状态后，`import-current` 是否能正确捕获或更新 profile。

## 11. 安全和隐私

### 11.1 不打印 secret / blob 内容

默认不打印明文：

- `env_injection` / `file_template` 中带敏感语义的字段（字段名匹配 `*token*` / `*key*` / `*secret*` / `*password*`，或 schema 标 `sensitive: true`；schema 显式 `sensitive: false` 可取消名称模式匹配，三态行为详见 §3.5）。
- 所有 capture blob（Keychain 内容、credentials.json、auth.json OAuth 段等）。

允许打印：profile id / name / app / kind / created_at、非敏感字段、`identity` 块、目标位置 metadata、capture source 的 metadata、manifest 中的 sha256 前缀。

同样默认不把 secret 接受为命令行参数值。CLI 生成或修改静态 secret 字段时必须使用 masked prompt、stdin、环境变量名或本机文件引用读取 secret；如果用户尝试 `--field api_key=sk-...` 或类似敏感字段 argv 输入，命令失败并提示改用 `--secret-field` / `--api-key-stdin`。这是为了避免 shell history、进程列表和终端日志泄漏。

macOS OAuth capture 的 Keychain entry 会被复制到 `~/.switch-cli/captures/` 和 `backups/` 的 `0600` 文件中。MVP 明确接受这个本地明文存储取舍以换取离线切换能力，但输出、plan、history 不得打印其内容；Phase 3/4 再引入字段级加密或 Keychain-per-profile 存储。

### 11.2 文件权限强制

- `~/.switch-cli/` 根目录：`0700`。
- `~/.switch-cli/profiles.yaml`：`0600`。
- `~/.switch-cli/apps.d/*.yaml`、`~/.switch-cli/overrides.d/*.yaml`：`0600`。
- `~/.switch-cli/apps.d/`、`~/.switch-cli/overrides.d/`：目录 `0700`。
- `~/.switch-cli/captures/`：目录 `0700`，文件 `0600`。
- `~/.switch-cli/backups/`：目录 `0700`，文件 `0600`。
- `~/.switch-cli/state/`：目录 `0700`，文件 `0600`。
- `~/.switch-cli/locks/`：目录 `0700`。
- 目标文件继承现有权限，新建默认 `0600`。
- Keychain entry：依赖 macOS Keychain 自身权限模型；switch-cli 只用当前用户 login keychain。

启动时若发现 `~/.switch-cli/` 任一文件 / 目录权限被改宽，doctor 警告并提示修复。

### 11.3 防御性备份保留

默认 `keep_backups: 20`（每 App 独立计数）。

MVP 自动清理：在 `use` / `restore-target` 成功后、释放锁前执行。按 mtime 倒序保留最近 N 份。失败不影响主流程结果，但写入 history 的 `warnings`。

`doctor` 展示每个 App 的备份数量、最旧备份时间。

### 11.4 并发与进程检测

同 App 写操作互斥（文件锁）。不同 App 可并行切换。

`use` / `restore-target` 执行前通过进程名匹配（由 App Definition 声明）粗粒度检测目标 App 是否运行：

- env_injection / file_template kind：命中默认拒绝，可 `--allow-running` 跳过。
- **oauth_capture kind：命中强制拒绝，`--allow-running` 不生效**。理由：App 运行时会刷新 token，原子性无法保证，且我们 writeback 的瞬间 App 也可能在写。

### 11.5 OAuth refresh token rotation

`oauth_capture` 假设 App 后端**可能**旋转 refresh_token。两条机制对抗这个风险：

1. **切换时写回**：切走当前活动 oauth_capture profile 前，先把最新 Keychain / 凭据文件内容回灌到该 profile 的 capture。这样只要用户在多个 profile 之间循环切换，每个 capture 都保持最新。
2. **失效边界**：如果 capture 长期不被切到（即 refresh_token 已老化），用户切过去时 App 会在首次 refresh 时失败。CLI 基于 capture manifest 的 `last_writeback_at ?? captured_at` 提前警告，但不提供登录修复能力；用户在目标 App 外部恢复可用状态后，再运行 `switch import-current` 捕获或更新 profile。

`oauth_stale_warn_days` 默认 30 天。这个值不代表 server 端的 token 失效阈值（那是 Anthropic 内部决定），只是一个保守的提示线，**实测后**可调整。

### 11.6 Schema 升级兼容性

详见 §6.4。MVP 锁定所有 `schema_version = 1`，不实现迁移路径；仅保留两条硬约束：

- 读到更高 `schema_version` → 拒绝写入，进入 read-only 模式，提示升级。
- `extensions` 字段在所有写入路径中必须保留。

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
| `CredentialStoreUnsupported` | 目标 App 当前使用的 credential store 不是 MVP 支持的 backend（如 Codex keyring 而非 file-backed auth.json） |
| `DefinitionLoadFailed` | App Definition 或 override 加载 / 合并失败 |
| `UnknownHandler` | App Definition 引用了二进制不支持的 handler |
| `RenderFailed` | App Definition + core handler 渲染过程出错 |
| `WritebackFailed` | oauth_capture 切换前的写回当前活动 profile 失败 |
| `DriftBeforeWriteback` | active profile 指向 OAuth profile，但 live target identity 与该 profile 不一致，拒绝写回以避免污染 capture |
| `BackupFailed` | 切换前防御性备份失败 |
| `ReplaceFailed` | 替换目标位置失败 |
| `VerifyFailed` | 替换后 hash 与预期不一致 |
| `IdentityMismatch` | 替换后 identity 与 capture 中的 identity 不一致（oauth_capture）|
| `InterruptedSwitch` | 发现上次写操作留下 pending-switch journal，必须先完成提交、回滚或人工恢复 |
| `ImportAmbiguous` | `import_current --kind auto` 检测到多种可能 kind；交互式可选择，非交互式需显式 `--kind` |
| `LockBusy` | 另一个写操作正在执行 |
| `AppRunning` | 目标 App 进程在运行；oauth_capture 直接拒绝，其他 kind 默认拒绝 |
| `CaptureLikelyStale` | capture manifest 显示 capture 已超过保守阈值，提示该 capture 可能需要外部恢复后重新 import-current |
| `UnsafeSecretArgument` | 用户把敏感字段明文作为 argv 值传入；应改用 prompt / stdin / env name / file 引用 |
| `SchemaTooNew` | profiles.yaml 的 schema_version 高于当前二进制 |

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
      fix the current App state outside switch-cli, then run
      `switch import-current claude <name>` to capture or update it.
```

## 13. MVP 验收标准

1. 能用 `add` 手动创建 Claude `env_injection` profile、Codex `file_template` profile；敏感字段不能通过 `--field k=v` 明文 argv 传入，必须使用 prompt / stdin / env name / file 引用。
2. 能用 `import-current --kind auto` 在 macOS 上从当前 Keychain + `~/.claude.json` 自动识别并捕获一条 Claude `oauth_capture` profile；identity 字段（accountUuid / email / organizationName）正确提取。
3. 能用 `import-current --kind auto` 在 Linux 上从 `${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json` + `~/.claude.json` 自动识别并捕获 Claude `oauth_capture` profile。
4. 能用 `import-current --kind auto` 从 file-backed Codex `${CODEX_HOME:-~/.codex}/auth.json` 识别 API-key 模式 → `file_template`，或 ChatGPT OAuth 模式（`auth_mode == "chatgpt"` + `tokens.refresh_token`）→ `oauth_capture`；Codex keyring credential store 在 MVP 中明确报 `CredentialStoreUnsupported`。
5. 能在已注册 profile 之间切换：
   - env_injection ↔ env_injection
   - oauth_capture ↔ oauth_capture（同一 App 内）
   - env_injection ↔ oauth_capture（同一 App 内；切到 oauth_capture 时清掉 env 中的 managed 键并要求用户确认）
   - 切换后重启 App 实测确认 profile 生效（macOS 与 Linux 各做一次）。
6. `switch use` 切到 oauth_capture B 之前，先校验 live target identity 仍等于当前活动 oauth_capture A，再把 A 的最新 Keychain / 凭据文件 / json_subtree 写回 A 的 capture，并更新 `captures/A/manifest.json`；identity 不匹配时必须拒绝写回且不得修改 capture / profiles.yaml。
7. `status` 能正确报告 `matched` / `matched-with-overrides` / `drifted` / `missing` / `no-active` / `interrupted`；oauth_capture 的 matched 基于 identity 比对而非 capture bytes。
8. `use --dry-run` 输出的 plan 不包含 secret 字段值的明文，也不打印 capture blob 内容；但能展示 identity 块。
9. 每次 `use` 前自动建立防御性备份（覆盖 file / Keychain / json_subtree 三类目标）；`use` / `restore-target` 成功后按 `keep_backups` 自动修剪。
10. `restore-target` 能从备份恢复所有类型的目标位置，恢复前再生成一份新备份。
11. `use` 在检测到目标 App 进程运行时默认拒绝；env_injection / file_template 可 `--allow-running` 跳过；**oauth_capture 不可跳过**。
12. 不提供 login / reauth 命令，不执行、引导或修复任何登录流程。
13. `remove` 能删除 profile；同时清理 `captures/<id>/`。
14. 所有命令输出（人类格式和 `--json`）都不打印 secret 字段明文，也不打印 capture blob 内容；命令行参数也不能接受敏感字段明文值。
15. profiles.yaml / apps.d/ / overrides.d/ / captures/ / backups/ 权限不正确时启动有警告；写入含 secret 的目标文件前，如果现有权限宽于 `0600`，必须收紧或拒绝写入。
16. 配置加载时 `~` 和 `${MACOS_USER}` 都从 `getpwuid` 展开，不信任 `$HOME` / `$USER`；内置白名单环境变量 `${CLAUDE_CONFIG_DIR:-~/.claude}` / `${CODEX_HOME:-~/.codex}` 展开后必须仍落在 home 内。
17. core 不包含 `claude` / `codex` 专属分支；产品差异优先在系统预置 App Definition 中表达，core 只提供通用 handler。
18. 能从 `~/.switch-cli/apps.d/*.yaml` 加载一个用户声明式 App Definition，并用已有 handler 完成 `env_injection` 或 `file_template` profile 的 add/use/dry-run。
19. 用户 Definition 引用未知 handler、写入 home 外路径或包含可执行脚本字段时，加载失败且写命令拒绝执行。
20. `switch apps` 能展示每个 App Definition 的来源（system / user / override）和支持的 kind；`switch apps show <app>` 能展示 resolved Definition；`switch apps export <app> --as override` 能生成可验证的 override 起点；`switch apps validate` 能校验单个 Definition 文件。
21. 文档明确：oauth_capture 切换前必须退出目标 App；其他 kind 也建议退出。
22. 除 `add` / `edit` / `remove` / `import-current` 等 profile 管理命令外，任何命令都不得修改 profiles.yaml；`use` 的 OAuth writeback 只更新 captures 和 capture manifest。
23. Codex `config.toml` 只能修改 Definition 声明的 managed TOML paths，必须保留 `mcp_servers` / `projects` / `plugins` 等未知配置。
24. `use` / `restore-target` 写目标前必须创建 `state/pending-switch/<app>.json`；模拟 apply 成功但 bookkeeping 前崩溃时，下一次同 App 写命令能完成提交、回滚或拒绝并报 `InterruptedSwitch`，不能开始新的切换。
25. `remove` 与 `use` 并发时不能删除正在使用的 capture；`remove` 必须按 profiles lock → app lock 顺序持锁。

**前置实测（在固化系统预置 Definition 前必须完成并记录结论）**：

A. **Claude refresh_token rotation 实测**：登录 → 等触发刷新 → 比对旋转前后 → 用 capture 中的旧 refresh_token 尝试恢复 → 看是否能续期。
B. **Claude oauthAccount 不一致容忍度实测**：仅改 Keychain 不改 `~/.claude.json.oauthAccount` → 启动 Claude Code → 观察行为（自动修正 / UI 不一致 / 报错）。
C. **Claude 并发写 `~/.claude.json` 实测**：Claude Code 运行时观察 `~/.claude.json` 的写频率与字段，识别哪些字段会和 `$.oauthAccount` / `$.userID` 同一次写入。
D. **Codex auth.json schema 实证**：已在 Codex CLI 0.133.0（macOS）确认 ChatGPT OAuth file-backed 形态为根字段 `OPENAI_API_KEY: null`、`auth_mode: "chatgpt"`、`last_refresh`、`tokens.{access_token,account_id,id_token,refresh_token}`；API-key 模式为根字段 `auth_mode: "apikey"` + `OPENAI_API_KEY` 两个字符串。
E. **Codex 外部恢复后捕获流程**：实测用户在 Codex 外部恢复当前状态后，`import-current` 能正确捕获或更新 profile。

## 14. 后续演进

### Phase 2: 更多 App、kind、backend

- Gemini CLI / OpenCode / Cursor / Windsurf 等 App Definition（优先用户扩展，成熟后可上升为系统预置）。
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

### Phase 5: 更通用的状态模型

如果内置 AI CLI 场景之外出现明确需求，再考虑更通用的 state / plan / apply 模型、跨 App 组合切换、可执行插件协议、trust / allow 机制。这些能力不应反向污染第一阶段 MVP，也不改变 core 当前的基本职责：安全、可预期地把结构化 profile 应用到一组本地 targets。

## 15. 当前设计结论

`switch-cli` 第一版是一个**本地 profile / state 切换器**。AI CLI 账号和凭据切换是 MVP 的首个应用域，因为它同时覆盖了静态字段渲染、OAuth 动态 capture、secret 脱敏、备份回滚和 App Definition 扩展等核心能力。

长期看，core 不关心"账号"这个业务概念；core 只关心如何把一条结构化 profile 安全、可预期地应用到一组本地 targets。Claude Code / Codex 的账号、凭据、端点和模型配置，是第一组随二进制内置的 target/domain 实例。

最重要的边界：

1. Profile 是结构化记录（`id` / `app` / `kind` / `fields` 或 `identity + capture`），不是 opaque 文件快照；它描述目标 App 的一组期望本地状态。
2. 四种 kind（`env_injection` / `file_template` / `oauth_capture` / `opaque_capture`）覆盖 MVP 的主要状态应用形态；凭据切换只是这些形态的第一组内置用例。
3. 动态 capture 视为会变化的本地状态资产：切换时双向写回、身份指纹校验、过期感知；登录和失效修复明确在工具边界之外。
4. 用户可以手编 profiles.yaml；`oauth_capture` 的 blob 和动态 manifest 由 `import-current` / writeback 自动维护。
5. App 专属知识优先封装在系统预置或用户扩展 App Definition 内；core 提供文件原子写、JSON 合并、TOML managed paths、Keychain backend、锁、防御性备份、hash、脱敏和受信任 handler。
6. 每次切换前对所有目标位置做防御性备份，每次切换后做 hash 校验和（对 oauth_capture）identity 校验。
7. oauth_capture 切换前**强制要求**目标 App 退出，不接受 `--allow-running`。
8. 后续扩展优先通过声明式 App Definition / override 完成；新增 handler、kind、backend 或复杂 OAuth 逻辑再通过 PR 增加受信任实现。MVP 不引入可执行插件生态。

# any-switch 设计文档

## 1. 项目定位

`any-switch` 是一个本地 profile / state 切换工具。它把某个本地 App 或 CLI 的一组可管理状态建模为 profile，并通过声明式 App Definition，把 profile 捕获、渲染或回放到具体 target，例如 JSON 子树、TOML 路径、文件、Keychain entry 或环境变量块。

MVP 阶段先聚焦 AI CLI 工具，内置 Claude Code 和 Codex 的 App Definition，用来解决账号、凭据、服务端点和模型配置的快速切换问题。但这些 AI CLI 账号场景只是第一组内置用例，不是 core 的边界。

工具不接入任何第三方登录接口。它把每个 profile 建模为一条**结构化记录**，并由 kind 决定这条记录如何作用到目标状态：

- 对于可由环境变量、JSON、TOML 或文件模板表达的状态：记录里直接存语义字段。
- 对于会被目标 App 动态改写的状态：记录里存身份指纹 + 动态 capture，并在每次切换时按需写回，避免本地存档落后于 App 的最新状态。

MVP 解决一个明确的起点问题：

```text
我在 Codex / Claude Code 中需要在多个账号、服务端点或模型 profile 之间切换，
希望一条命令搞定，不用反复手动改 settings.json 或重新登录。
```

这个问题同时验证 any-switch 的核心抽象：声明式 target、受信任 handler、防御性备份、失败回滚、secret 脱敏和动态状态写回。

示例：

```bash
# 把当前 Claude Code 的 Anthropic OAuth 登录捕获为一条 profile (oauth_capture)
any-switch import-current claude anthropic-work

# 添加一个 GLM 第三方代理 profile (env_injection)
any-switch add claude glm \
  --kind env_injection \
  --base-url https://open.bigmodel.cn/api/anthropic \
  --auth-token-stdin
# 在 TTY 中粘贴 token；或从本机 secret 命令 pipe 到 stdin，避免 token 进入 shell history / argv

# 切到 GLM 代理
any-switch use claude-glm

# 切回 Anthropic 官方 OAuth（不再需要重新登录）
any-switch use claude-anthropic-work

# Codex 当前是 ChatGPT OAuth 登录，捕获为 profile (oauth_capture)
any-switch import-current codex chatgpt-work

any-switch list
any-switch status
```

## 2. 设计目标

### 2.1 MVP 目标

- 随二进制提供 Claude Code 和 Codex 两个系统预置 App Definition。
- 支持从用户配置目录加载声明式 App Definition / override 文件，用已有 core handler 扩展新的 App、CLI 或状态切换用法。
- Profile 建模为结构化记录，可由编辑器手编，也可由 `import-current` 自动捕获。
- schema 声明四种 profile 应用方式（kind），MVP 代码路径实现前三种，`opaque_capture` 仅保留名称和校验占位。这些 kind 在 MVP 中主要承载凭据和账号状态，但本质上描述的是 profile 如何作用到目标 App 的本地状态：
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
- 登录、重新登录、引导登录或修复登录状态。凭据失效时，用户在目标 App 外部自行处理，any-switch 只负责重新捕获当前本地状态。
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

### 3.2 kind 刻画状态应用形态，MVP 实现前三种

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
- **写回身份门禁**：写回当前活动 profile 前，必须先从 live target 提取 verification identity，并与 `state/active.json` 指向的 profile identity 的所有 `verify: required` 字段比对；如果 Definition 声明了 credential source / identity source 一致性检查，也必须先通过。比对失败说明目标状态已 drift 或上次操作在提交 bookkeeping 前中断，此时拒绝 writeback，避免把错误账号的 live bytes 写入当前 profile 的 capture。
- **进程互斥强制**：任何涉及 OAuth capture 读写的操作如果检测到目标 App 在运行，**拒绝**执行——`--allow-running` 不生效。这包括切到 OAuth profile、切走当前 active OAuth profile 前的 writeback、writeback-only self-use、OAuth import / refresh，以及恢复含 OAuth 动态凭据 target 的 backup。理由是 App 运行时会刷新 token，原子性无法保证。
- **提交前后校验**：切换后先校验每个写入 target 的 hash，再从恢复的状态里读出 App Definition 声明的所有 `verify: required` identity 字段，与 capture 的 `identity` 比对，不一致即视为失败回滚。
- **中断恢复**：写入目标前记录 pending switch journal。若进程在 apply 和 bookkeeping 之间崩溃，下一次写命令必须先完成提交、回滚或拒绝并提示用户恢复，不能盲目信任旧的 active 指针。
- **陈旧感知**：capture manifest 记录 `captured_at` 和 `last_writeback_at`，切换时如果检测到 capture 长期没有被写回，警告用户该 capture 可能已经失效。
- **失效边界**：refresh 失败时不假装成功，也不提供登录修复流程。用户应在目标 App 外部恢复可用状态后，再用 `any-switch import-current` 捕获或更新 profile。

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

用户可以通过 `~/.any-switch/apps.d/*.yaml` 增加新的 App Definition，也可以通过 override 文件调整系统预置 App 的默认字段、模型和 provider 模板。App id 全局唯一，必须满足与 profile id 相同的 slug 规则。

### 4.2 App Definition

App Definition 是 any-switch 对某个本地 App / CLI 状态面的产品知识声明。它不保存用户 profile 数据，也不保存 secret。它只描述：

- 支持哪些 kind。
- 每个 kind 的字段 schema、敏感字段、默认值和展示名。
- 每个 kind 使用哪些 target、capture source 和 core handler。
- 字段如何映射到 JSON / TOML / env / file 目标。
- oauth_capture 的 identity 从哪些路径提取。
- 进程探测、doctor 检查项。

Definition 来源按优先级合并：

1. 系统预置 Definition（随二进制发布，默认只读）。
2. 用户 App Definition：`~/.any-switch/apps.d/*.yaml`。
3. 用户 override：`~/.any-switch/overrides.d/*.yaml`。

同一个 app id 的 override 只能修改允许覆盖的声明式字段，不能替换 handler 为未知值，不能声明执行脚本，不能扩大到 home 目录外的写入目标。

系统预置 Definition 本身也使用同一套 YAML schema。源码中放在 `src/app_definitions/builtin/*.yaml`，构建时扫描该目录、校验并嵌入二进制。发布包可以额外安装一份只读副本到资源目录（例如 macOS app bundle resources、Homebrew cellar 或 Linux `/usr/share/any-switch/app_definitions/`），仅用于审计和调试；运行时以二进制内嵌版本为准，避免资源文件缺失导致内置 App 不可用。

用户想调整内置 Definition 时，不直接修改只读副本，而是在 `overrides.d/` 写局部 override。为了让用户从内置内容开始，CLI 提供导出命令：

```bash
# 查看完整 resolved definition
any-switch apps show claude

# 导出系统内置原始 definition 到 stdout
any-switch apps export claude --source system

# 生成一个可编辑的 override 起点
any-switch apps export claude --as override --output ~/.any-switch/overrides.d/claude.yaml
```

`--source system` 输出二进制内嵌的系统 Definition，不包含用户 override；`any-switch apps show` 输出合并后的 resolved Definition；`--as override` 只输出允许覆盖字段的骨架和注释，避免用户复制整份系统 Definition 后误以为可以替换 handler、target path 或声明脚本。

### 4.3 Profile

用户管理的一条 profile 记录。至少包含 `id` / `app` / `kind` / `name`，其余字段随 kind 变化。profile id 全局唯一。

### 4.4 Kind

profile 的应用方式：它决定结构化记录如何被捕获、渲染或回放到目标状态。v1 schema 保留四个 kind 名称：`env_injection`、`file_template`、`oauth_capture`、`opaque_capture`（保留）；MVP 只实现前三个。前三个名称来自 MVP 的实际 target 形态，其中 `oauth_capture` 明确服务于 OAuth 动态状态；在其他 App Definition 中，`env_injection`、`file_template` 和未来的 `opaque_capture` 也可以表达非账号类配置状态。

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

App Definition 为每个 identity 字段标注 `verify: required | optional`：

- `required`：切换后必须能从恢复的目标状态提取该字段，且必须等于 `profile.identity` 中的对应值；任意 required 字段缺失、解码失败或值不等 → `IdentityMismatch`，回滚。
- `optional`：仅用于展示、`import_current` 去重和 doctor 报告。verify 阶段如果能提取就比对一致性（不一致只 warn，不阻塞）；提取失败（例如 App 升级换了 JWT issuer 导致 payload schema 变化）直接跳过，不影响切换成功。

每个 oauth_capture Definition 必须至少声明一个 `required` identity 字段，否则 `DefinitionLoadFailed`——这是 verify 不退化为空操作的保证。MVP 内置 Definition 的具体必选项见 §10.3 / §10.4。

identity 还用于：

- `list` / `show` 输出时给用户辨识用（required 与 optional 都展示）。
- `import_current` 去重：仅基于 required 字段集合判断"是否同一身份"，optional 字段差异（例如 organizationName 在 web 后台被改名）不触发新建。

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
| `toml_managed_paths` | 某个 TOML 文件中由 Definition 声明的 managed paths（部分写入 / 捕获为 TOML fragment）| ✓ |

blob 内容不内联到 profiles.yaml，而在 `~/.any-switch/captures/<id>/` 下，目录 `0700`、文件 `0600`。

`toml_managed_paths` 不捕获整份 TOML 文件。捕获时只把 Definition 声明的 managed paths 序列化为一个独立 TOML fragment 写入 `stored_as`；恢复时把该 fragment 通过 `toml_managed_paths` handler 合并回目标文件，目标文件中未声明为 managed 的 path 必须按 §9.5 保留。

**Capture 声明跨平台共用，capture bytes 不跨平台 portable**：profile 的 `capture.sources` 列表是单一声明，里面允许同时列出 macOS（`type: secret_entry` + `platforms: [macos]`）和 Linux（`type: file` + `platforms: [linux]`）两条 source，由运行时按 `platforms` 字段挑出当前平台适用的子集——这是 profile 声明跨平台共用的部分。

但 capture 的实际 **bytes**（`captures/<id>/keychain.json` 与 `captures/<id>/credentials.json`）不跨平台 portable：同一账号在 macOS Keychain 里的 entry value 与 Linux `${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json` 内容结构相近但语义来源不同（系统 secret store vs. 应用文件），MVP 不做平台间字节迁移。把 profile（含 profiles.yaml 中的声明与 `captures/<id>/`）复制到新平台后，必须重新运行 `any-switch import-current` 在该平台上重新捕获对应平台的 capture bytes——identity（accountUuid 等）可用于在新机器上确认捕获到的是同一账号。

**跨平台 capture 完整性检查**：`status` 和 `doctor` 在当前平台解析 profile 的 capture sources 时，按 `platforms` 字段筛选出适用 source；对每个适用 source 检查 `captures/<id>/<stored_as>` 是否存在。任一适用 source 的 stored_as 文件缺失 → 报 `CaptureMissing` 并给出明确 hint："profile <id> appears to be copied from another platform; run `any-switch import-current <app> <name>` on this machine to re-capture credentials for the current platform."`use` 在 plan 阶段也必须执行这项检查，缺失即拒绝执行；不允许只用部分平台 source 的 bytes 切换。

### 4.8 Capture Manifest（oauth_capture 专用）

`captures/<id>/manifest.json` 由 any-switch 自动维护，记录 capture blob 的 hash 和审计时间。它是运行时状态，不是用户 profile 配置。

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

每个 backup 目录必须包含 `manifest.json`，用于把落盘文件和真实目标位置重新绑定，避免 `restore-target` 对 json_subtree / Keychain / TOML fragment 这类非整文件目标产生歧义：

```json
{
  "schema_version": 1,
  "operation_id": "01JY1H8Y8W7Q7K3Y7A5W4M3D2P",
  "app": "claude",
  "created_at": "2026-05-23T10:00:00Z",
  "targets": [
    {
      "target_id": "file:/Users/alice/.claude/settings.json",
      "type": "file",
      "requires_app_stopped": false,
      "path": "~/.claude/settings.json",
      "resolved_path": "/Users/alice/.claude/settings.json",
      "stored_as": "settings.json",
      "sha256": "..."
    },
    {
      "target_id": "json:/Users/alice/.claude.json#$.oauthAccount",
      "type": "json_subtree",
      "requires_app_stopped": true,
      "path": "~/.claude.json",
      "resolved_path": "/Users/alice/.claude.json",
      "json_path": "$.oauthAccount",
      "stored_as": "oauthAccount.json",
      "sha256": "..."
    },
    {
      "target_id": "keychain:macos:Claude Code-credentials:alice",
      "type": "secret_entry",
      "requires_app_stopped": true,
      "backend": "macos_keychain",
      "service": "Claude Code-credentials",
      "account": "${MACOS_USER}",
      "resolved_account": "alice",
      "stored_as": "keychain.json",
      "sha256": "..."
    }
  ]
}
```

manifest 同时保存原始模板（`path` / `account`，用于展示）和备份创建时解析出的真实目标（`resolved_path` / `resolved_account`，用于恢复）。`requires_app_stopped` 是每个 target 的必填布尔值，由创建备份时的 resolved target spec 写入：任何 OAuth credential source、OAuth identity source、Keychain entry、file-backed OAuth auth 文件，或 Definition 明确标记为与动态凭据同进程竞争的 target，都必须为 `true`。`restore-target` 只信任 manifest 中声明的 resolved target spec；目录里存在但 manifest 未引用的文件一律忽略并 warning。manifest 缺失、target 缺少 `requires_app_stopped`、resolved target 不在 home 内、落入 `ANY_SWITCH_HOME`、handler 不再受支持，或 target spec 无法通过当前安全校验时，不执行恢复，报 `BackupInvalid`。恢复不重新读取当前环境变量来改变目标位置；如果用户希望恢复到新的 Definition path env 对应位置，应先在目标 App 外部迁移配置，再重新 `import-current`。

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

any-switch 的所有文件统一放在 `~/.any-switch/` 下，跨 Linux / macOS / Windows 走同一路径，不走 XDG 三段。可通过 `ANY_SWITCH_HOME` 环境变量重定向根目录（必须为绝对路径且展开后落在用户 home 内），主要用途是自动化测试以及希望走 XDG 的 Linux 用户软链到 `$XDG_CONFIG_HOME/any-switch`。MVP 不做更精细的 XDG split。

系统预置 App Definition 随二进制发布。权威版本在源码的 `src/app_definitions/builtin/*.yaml`，构建时嵌入二进制；安装包可同时放一份只读资源副本供用户查看。它们不落在 `~/.any-switch/` 下。用户目录只保存扩展、覆盖、profile、capture、备份和运行时状态。

```text
~/.any-switch/
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
      config.managed.toml
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
    edit/                          # any-switch edit 的临时片段目录；0700，片段文件 0600，用后删除
    history.jsonl                  # 操作历史
  locks/                           # 文件锁
    claude.lock
    codex.lock
    profiles.lock
    state.lock
    target-<sha256>.lock
```

`active.json`：

```json
{
  "schema_version": 1,
  "active_profiles": {
    "claude": {
      "id": "claude-anthropic-work",
      "resolved_targets": [
        {
          "target_id": "file:/Users/alice/.claude/settings.json",
          "resolved_path": "/Users/alice/.claude/settings.json"
        },
        {
          "target_id": "json:/Users/alice/.claude.json#$.oauthAccount",
          "resolved_path": "/Users/alice/.claude.json"
        },
        {
          "target_id": "keychain:macos:Claude Code-credentials:alice",
          "resolved_account": "alice"
        }
      ]
    },
    "codex": {
      "id": "codex-chatgpt-work",
      "resolved_targets": [
        {
          "target_id": "file:/Users/alice/.codex/auth.json",
          "resolved_path": "/Users/alice/.codex/auth.json"
        }
      ]
    }
  }
}
```

每个 App entry 是对象，包含活动 profile `id` 和上一次 `use` / `import-current` 写入时记录的 `resolved_targets` snapshot（仅 metadata，不含 secret）。`detach` 把对应 App entry 整体置为 `null`。`remove` 删除当前活动 profile 时同样置 `null`。`restore-target` 不写入此字段，因为它不更新 active 指针。snapshot 用于 §9.1 描述的"环境变量改动后的 drift 检测"，也用于 `status` 在 Definition path env 变化时清晰地展示新旧 resolved path。

`state/pending-switch/<app>.json` 只在 `use` / `restore-target` 已经完成防御性备份、准备改写目标位置时短暂存在。它记录 `operation`（`use` 或 `restore-target`）、`operation_id`、`app`、`from_profile`、`to_profile`（仅 `use` 有值）、`backup_id`（本次操作前创建的回滚备份）、`restore_from_backup_id`（仅 `restore-target` 有值）、目标 target 的 expected hash / identity 和当前阶段。正常成功路径在更新 `active.json`（仅 `use`）与 `history.jsonl` 后删除该文件；如果进程崩溃，下次该 App 的任意写命令在获取 App 锁后必须先读取它，并在检查或修复 live target 前先按 pending target list 获取对应 target locks：

- `operation == "use"` 且目标状态已经与 `to_profile` 的 hash / identity 匹配 → 补写 `active.json` / history（按 operation_id 去重），删除 pending journal。
- `operation == "restore-target"` 且目标状态已经与 `restore_from_backup_id` 的 manifest hash / identity 匹配 → 只补写 history，**不修改 `active.json`**，删除 pending journal。
- 目标状态仍与本次操作前新建的回滚备份 `backup_id` 匹配 → 删除 pending journal，报告上次操作未生效。
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

`active.json` 和 `history.jsonl` 是跨 App 共享状态。任何写入这两个文件的命令必须在已经持有相关 App lock 之后，再短暂持有 `locks/state.lock`，并在持锁后重新读取 `active.json`、只修改当前 App 的 entry、原子写回；追加 history 也在同一把 state lock 下完成。这样不同 App 可以并行完成 target apply，但 bookkeeping 串行提交，避免并发 `use claude` / `use codex` 时最后写入者覆盖另一个 App 的 active 指针。

## 6. 配置模型

### 6.1 配置文件家族

any-switch 把配置和状态分散在若干文件 / 目录里，分别承担清晰不同的职责：

| 文件 / 目录 | 作用 | 维护者 |
|-------------|------|--------|
| `src/app_definitions/builtin/*.yaml`（源码）/ 二进制内嵌 Definition | 系统预置 App 的 schema、handler 绑定、进程探测和 doctor 检查项等 | 源码维护，构建时校验并嵌入二进制，随二进制版本一起更新 |
| `~/.any-switch/apps.d/*.yaml` | 用户新增的 App Definition（系统未预置的 App）| 用户手编 |
| `~/.any-switch/overrides.d/*.yaml` | 对已有 App Definition 的局部覆盖（白名单字段）| 用户手编 |
| `~/.any-switch/profiles.yaml` | **profile 注册表 + CLI 偏好** | 用户手编；any-switch 只在 add / edit / remove / import-current 等 profile 管理命令中写入 |
| `~/.any-switch/captures/<id>/` | oauth_capture profile 的 blob 内容和 manifest | any-switch 自动维护（import-current / writeback）|
| `~/.any-switch/backups/<app>/<ts>/` | 防御性备份 | any-switch 在写操作前自动创建 |
| `~/.any-switch/state/active.json` | 每个 App 当前活动 profile id | any-switch 在 use 成功、remove / detach 清空活动指针时更新；restore-target 永不更新 |
| `~/.any-switch/state/pending-switch/<app>.json` | apply 与 bookkeeping 之间的崩溃恢复 journal | any-switch 在写目标前创建，成功提交或恢复后删除 |
| `~/.any-switch/state/history.jsonl` | 操作历史元数据（append-only）| any-switch 追加写 |
| `~/.any-switch/locks/` | 文件锁 | any-switch 临时持有 |

#### profiles.yaml 的职责和写入边界

profiles.yaml 是用户可手编的稳定配置。any-switch 可以读取它，但只有明确修改 profile 注册表的命令可以写它：`any-switch add`、`any-switch edit`、`any-switch remove`、`any-switch import-current`（以及未来同类 profile 管理命令）。`any-switch use`、`any-switch status`、`any-switch doctor`、`any-switch restore-target` 和 OAuth writeback 都不得修改 profiles.yaml。

1. **Profile 注册表**：记录这台机器上有哪些 profile（id / app / kind / name / created_at 等）。无论什么 kind 都需要这层 metadata；`any-switch list` / `any-switch use <id>` / `any-switch status` 都从这里读起。

2. **静态凭据的内联存储**（env_injection / file_template）：secret 字段（`auth_token` / `api_key` 等）**直接明文写在** `fields` 块里。理由：这些 token 是用户从代理服务商页面复制的静态字符串，性质上就是配置，inline 存读写都顺手，也方便手编 / 对比 / 修改 model 等批量操作。

3. **动态凭据的清单**（oauth_capture）：secret blob **不**在 profiles.yaml 里。这里只放：
   - `identity`：非敏感的身份指纹（accountUuid / email / organizationName 等）
   - `capture.sources`：每个 source 的 type / 目标位置 / `stored_as` / platform / required 等稳定声明

   实际的 OAuth blob 在 `~/.any-switch/captures/<id>/` 下作为独立文件存。`sha256`、`captured_at`、`last_writeback_at` 等动态元数据写入 `captures/<id>/manifest.json`，因此 oauth_capture 切换和 writeback 不会污染用户可编辑的 profiles.yaml。

4. **CLI 偏好**：`preferences` 块（`default_app` / `confirm_before_switch` / `keep_backups` / `oauth_stale_warn_days`）。secret 脱敏与 secret argv 拒绝是硬性安全约束，不是 preference，不可通过配置关闭（详见 §11.1）。

#### 不在 profiles.yaml 里的内容

App Definition 与 override **不写入 profiles.yaml**，独立放在 `apps.d/` 和 `overrides.d/` 下。系统预置 Definition 由二进制提供；用户扩展 / 覆盖文件在加载时与系统预置合并成运行时的 resolved definition registry。这样拆分的理由：

- 编辑权责分离：Definition 是"这个 App 的哪些本地状态可被切换、如何切换"的协议，profile 是"我要切到哪组状态"的数据。混在一起会让普通用户在新增 profile 时不慎修改 Definition 字段。
- 升级路径不同：系统 Definition 随二进制升级；profile 由用户长期持有，schema 演进策略不同。

OAuth blob、capture manifest、防御性备份、活动 profile 指针、操作历史等都属于工具自动维护的运行时状态，分别放在 `captures/` / `backups/` / `state/` / `locks/` 子目录下，不与 `profiles.yaml` 混在同一文件里。

#### 关于"静态明文 secret 与动态 blob 分离"的取舍

env_injection / file_template 的 secret 明文存在 profiles.yaml 里、oauth_capture 的 secret 存在 captures/ 里——这是**有意的不对称**：

- 静态字段的心理模型就是"一段配置"；对 MVP 的 API key / 代理 token 来说，明文 inline 存读写、diff、手编都直观，多 profile 场景下用户能在一个文件里看完整张表。
- OAuth blob 是 App 自己持续 mutate 的状态（refresh 时被改写）。若把它写进 profiles.yaml，每次刷新都会污染用户可编辑的配置文件，把"用户意图"和"工具/App 自动维护的动态状态"混在一起。
- 代价是 profiles.yaml 通常含明文 secret，**不能直接 git commit 或公开分享，也不应放入会同步到云端的目录（iCloud Drive / Dropbox / OneDrive）或自动备份工具（Time Machine 网络备份、第三方 backup agent）的覆盖范围**。§6.7 / §11.2 强制 `0600` 权限。如果用户有跨机器同步需求，留给 Phase 3 的 `any-switch export --unsafe-export` 提供脱敏选项。
- `any-switch doctor` 在默认输出中必须包含一条 "profiles.yaml secret-leak surface" 提示：列出 `~/.any-switch/` 是否位于已知云同步根目录（macOS：`~/Library/Mobile Documents/`、`~/Dropbox/`、`~/OneDrive/`、`~/Google Drive/`；Linux：用户家目录下的同名子目录）之内，并在命中时升级为 warning，引导用户把 `ANY_SWITCH_HOME` 重定向到非同步目录。
- Phase 3 引入字段级 secret 加密时，迁移路径预设为：bump profile 的 `schema_version` → 二进制读到旧版本时把 inline secret 透明迁出到 captures-style 加密 blob，profiles.yaml 中保留引用占位 → 写回时只写新格式。由于 §6.4 已经把 `schema_version` 升级路径锁定在"读到更高版本拒绝写入"，MVP 阶段无需提前实现迁移代码，但 Phase 3 设计必须遵循这条单向迁移而非破坏性 rewrite。
- 同理，`~/.any-switch/captures/<id>/manifest.json` 是工具维护的运行时索引，用户不应手编；丢失时可由 capture 文件重新计算 hash，但 `captured_at` / `last_writeback_at` 审计信息会丢失。

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
- `process_probe` 是 **append-only** override：override 只能向 `names` 列表追加进程名（例如扩展第三方 fork 的 binary 名），不允许缩减、清空或替换。`oauth_capture` 的进程互斥强制保证依赖该探测能命中目标 App，缩减 override 等于绕过 §11.4 的安全约束。加载器对 override 中的 `process_probe.names` 做"原集合的超集"校验，失败 → `DefinitionLoadFailed`。
- 任何支持 `oauth_capture` 的 resolved Definition 必须至少声明一个可执行的 `process_probe` handler（MVP 为 `process_name`，且 `names` 非空）。没有进程探测的 OAuth Definition 加载失败；如果某个产品无法被进程名可靠探测，MVP 不应为它声明 `oauth_capture`，只能先用 `env_injection` / `file_template` 或新增受信任 probe handler。
- handler 名称必须来自二进制内置 registry；未知 handler 直接拒绝加载。
- Definition 不支持 `login.command`、`reauth` 或任何登录相关字段，也不支持任何可执行命令字段。
- target path 默认必须在当前用户 home 内，且经过 `~` / `${MACOS_USER}` / `${VAR:-default}` 路径环境变量展开后再做边界检查。Definition 管理的 target / capture source 路径不得位于 any-switch 自己的配置根目录（`ANY_SWITCH_HOME` 展开后的真实路径，默认 `~/.any-switch`）之下；否则加载失败，避免用户 Definition 覆盖 `profiles.yaml`、captures、backups、state 或 locks。
- Definition 加载失败时，写命令拒绝执行；只读命令显示错误并继续展示已成功加载的 profile。
- Definition 的来源（system / user / override）由 loader 根据内嵌 registry 或文件位置计算，不从 YAML 字段读取。
- 只读资源目录中的系统 Definition 副本不参与运行时加载；`any-switch apps show <app>` 可显示 resolved Definition，并标明 system 部分来自二进制内嵌 registry，必要时显示资源副本路径供用户审计。
- `any-switch apps export <app> --as override` 生成的文件必须能被 `any-switch apps validate` 直接通过；如果目标文件已存在，默认拒绝覆盖，除非显式 `--force`。

### 6.3 Profile 配置

```yaml
schema_version: 1

preferences:
  default_app: claude
  confirm_before_switch: true
  keep_backups: 20
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
      account_id: "..."
      email: "work@example.com"
    capture:
      sources:
        - type: file
          path: "${CODEX_HOME:-~/.codex}/auth.json"
          stored_as: auth.json
        - type: toml_managed_paths
          path: "${CODEX_HOME:-~/.codex}/config.toml"
          stored_as: config.managed.toml
          required: false
    extensions: {}
```

### 6.4 Schema 约束

MVP 把所有 `schema_version` / `definition_version` **锁定为 `1`**，不引入 v1 → v2 迁移代码路径。仅保留以下不变量：

- 顶层 `schema_version` 标识整体 profiles.yaml 版本；每条 profile 的 `schema_version` 标识该 `app + kind` 组合的字段 schema 版本；App Definition 的 `definition_version` 标识该 definition 自身 schema。三者命名空间预留，独立演进。
- 读到比当前二进制更高的任一 `schema_version` → 拒绝写入，仅 read-only 命令可用，提示升级 CLI。
- 未识别的 `extensions` 字段在 profile 管理命令写回期间必须保留。
- **kind 名一旦在 v1 中发布即视为永久 stable surface**：未来 `schema_version` bump 不允许重命名、删除或重新定义 v1 已有 kind 的语义；新增形态只能新增 kind 名。这条约束让 MVP 可以暂不实现迁移代码：旧 profile 的 `kind` 字段在所有未来版本都仍可解析。

完整的旧版本迁移、迁移失败降级、profiles.yaml.bak 等策略推迟到 Phase 2 文档；MVP 出现需要 bump 时再补设计。

### 6.5 字段规范

各 App / kind 的字段 schema 由 resolved App Definition 维护。所有 kind 共享的约定：

- `id` 必须满足 `^[a-z0-9][a-z0-9-]{0,63}$`。`any-switch add` / `import-current` 自动生成的 id 形如 `<app>-<slug(name)>`，可用 `--id` 显式指定覆盖。
- `slug(s)` 的定义：转小写 → 把任何不在 `[a-z0-9]` 范围的字符替换为 `-` → 合并连续 `-` → 去掉首尾 `-`。最终 id（含 `<app>-` 前缀）总长必须 ≤ 64 字符，超长则截断 slug 部分并保留前缀。slug 结果为空时（如全中文 name）报错要求用户显式 `--id`。
- `name` 是任意 UTF-8 字符串，仅用于展示。
- `notes` 可选，多行字符串。
- `created_at` 由 CLI 自动写入。
- `extensions` 是开放对象。
- `oauth_capture` 必须含 `identity` 和 `capture.sources`；动态 hash 和时间戳由 `captures/<id>/manifest.json` 维护。
- `${MACOS_USER}` 从 `getpwuid(getuid())` 取，不信任 `$USER` 环境变量。路径模板还支持 `${VAR:-default}` 形式的 App Definition 声明式环境变量展开；变量名必须是大写字母 / 数字 / `_`，展开结果按 §9.1 做 home 边界、绝对路径和 symlink 检查。

### 6.6 命令行覆盖

命令行参数覆盖 `preferences`。没有项目级配置，也没有用环境变量覆盖 CLI 偏好的机制；§9.1 中的 Definition path env 仅用于解析目标 App 自己的配置目录，`--secret-field @env:NAME` 仅用于读取用户显式指定的 secret 值。

### 6.7 文件权限

`~/.any-switch/` 根目录及所有子目录强制 `0700`；`profiles.yaml`、用户 App Definition、override 文件、captures / backups / state 下的所有文件强制 `0600`。完整权限矩阵见 §11.2。启动时若权限被改宽，doctor 警告并提示修复。

## 7. 命令设计

### 7.1 MVP 命令

| 命令 | 说明 |
|------|------|
| `any-switch apps` | 列出已加载 App Definition、来源、支持的 kind |
| `any-switch apps show <app>` | 展示 resolved App Definition（脱敏，不含 profile 数据）|
| `any-switch apps export <app> [--source system\|resolved] [--as override] [--output <path>]` | 导出内置 / resolved Definition，或生成 override 起点 |
| `any-switch apps validate [<path>]` | 校验用户 App Definition / override 文件 |
| `any-switch list [<app>]` | 列出已注册 profile（脱敏） |
| `any-switch show <id>` | 查看单个 profile 的元数据、verification identity、非敏感字段 |
| `any-switch add <app> <name> [--id <id>] [--kind <kind>] [--field k=v ...] [--secret-field k=@stdin\|@prompt\|@env:NAME\|@file:PATH ...]` | 手动添加 env_injection / file_template profile |
| `any-switch edit <id>` | 用 `$EDITOR` 打开 profile 的 yaml 片段编辑 |
| `any-switch remove <id>` | 删除 profile；同时清理 captures/<id>/；若该 profile 是某 App 的活动 profile，将该 App 在 state/active.json 中置为 null（不动目标文件）|
| `any-switch import-current <app> <name> [--id <id>] [--kind auto\|<kind>]` | 从 App 当前状态自动识别 kind 并捕获 profile |
| `any-switch use <id>` | 切换到指定 profile（oauth_capture 会先写回当前活动 profile 的 capture，不修改 profiles.yaml）。当 `<id>` 就是当前活动 oauth_capture profile 自身时，命令退化为"只跑 writeback"——**仅更新 capture bytes + manifest，不更新 profile metadata / identity 字段**。若用户希望同时刷新 identity（例如 Anthropic 在 web 后台改了 `organizationName` / `subscriptionType`），应使用 `any-switch import-current <app> <name>`，详见 §8.2 / §8.3 short-circuit |
| `any-switch use <id> --dry-run` | 只打印 plan，不写入 |
| `any-switch detach <app>` | 把 `state/active.json` 中 `<app>` 的活动 profile 置 null，不动 live target、不动 captures、不动 profiles.yaml；用于 `status` 报 `drifted` 后用户已在 App 外部自行处理状态、希望停止 any-switch 对该 App 的追踪 |
| `any-switch status [<app>]` | 显示每个 App 的活动 profile、是否 drift |
| `any-switch backup list [<app>]` | 列出防御性备份 |
| `any-switch restore-target <app> <backup-id>` | 从防御性备份恢复目标位置 |
| `any-switch doctor [<app>]` | 检查路径、权限、字段完整性、进程状态、Keychain 可访问性 |
| `any-switch config path` | 打印 profiles.yaml 路径 |

### 7.2 命令选项约定

- `--yes` / `-y`：跳过交互确认。
- `--json`：以 JSON 格式输出（同样脱敏）。`--dry-run` 与 `--json` 组合时，plan 以 JSON 输出（字段集合与人类格式一致，secret / blob 字段同样以 `"***"` 占位），便于脚本化校验和 MVP 验收。
- `--allow-running`：在目标 App 进程运行时仍允许纯 env_injection / file_template 写操作继续。**对任何涉及 `oauth_capture` 读写的操作不生效**——包括切到 OAuth profile、从当前 active OAuth profile 切走时的 writeback、`any-switch use <当前 OAuth profile>` 的 writeback-only 操作、`import-current` 创建或刷新 OAuth profile，以及 `restore-target` 恢复 manifest 中 `requires_app_stopped: true` 的 target；这些场景永远要求 App 退出，除非使用 §11.4 的 `--assume-app-stopped` 逃生流程。
- `--assume-app-stopped`：仅用于涉及 `oauth_capture` 读写的操作发生进程探测命中、但用户确认为误报时的逃生口（见 §11.4）。必须显式拼写完整，没有短形式；必须配合 `--yes` 在非交互式生效，或在交互式 TTY 下接受额外的二次确认。触发时会把探测到的 PID 列表写入 `history.jsonl` 的 `warnings`。
- `--force`：仅在 `add` 同 id 覆盖、`remove` 跳过确认时使用。
- `--accept-resolved-change`：仅 `use` 接受；当本次解析得到的 `resolved_targets` 与 `active.json` 中保存的 snapshot 不一致（见 §9.1 / §5）时，显式承认本次环境变量改动并继续切换。命中时把"旧 snapshot vs. 新 resolved targets"写入 `history.jsonl` 的 `warnings`，便于事后审计。不接受短形式。
- `--field k=v` 只允许传非敏感字段。若字段 schema 标 `sensitive: true`，或字段名按敏感模式命中，CLI 必须拒绝明文 argv 输入并报 `UnsafeSecretArgument`。
- `--secret-field k=@stdin|@prompt|@env:NAME|@file:PATH` 用于敏感字段：`@prompt` 为 TTY masked prompt，`@stdin` 从标准输入读取一次，`@env:NAME` 只把环境变量名放进 argv、值从进程环境读取，`@file:PATH` 从本机文件读取且同样做 home 边界和权限检查。内置便捷参数（如 `--auth-token-stdin`、`--api-key-stdin`）只是对应 `--secret-field` 的别名。
- 默认所有写命令在交互式 TTY 下请求确认；非交互式必须显式 `--yes`。

### 7.3 暂缓命令

不进入 MVP：

- `any-switch rename`（用 `remove` + `add` 替代）。
- `any-switch diff` / `plan` / `apply` 通用三件套。
- `any-switch backup prune`（MVP 自动按 `keep_backups` 修剪）。
- `any-switch export` / `import` 跨机器迁移。
- shell completion 脚本。
- `any-switch module install`、`any-switch plugin`、`any-switch trust` 等可执行插件 / 沙箱概念。

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

### 8.1 `any-switch add`

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
if --force replaces an existing profile:
  existing profile's app and kind must equal requested app and kind
  if existing profile is oauth_capture -> abort; use import-current refresh or remove + add
  acquire that app's app lock after profiles lock, reload existing profile, then replace
append profile to profiles.yaml (atomic write)
```

id 自动生成示例：

- `any-switch add claude glm` → `claude-glm`
- `any-switch add claude "GLM 智谱"` → `claude-glm`（中文字符在 slug 中被丢弃，详见 §6.5）
- `any-switch add codex personal --id corp-prod` → `corp-prod`（显式 `--id` 覆盖自动规则）

不修改任何目标位置。

### 8.1.1 `any-switch edit <id>`

`edit` 是 profile 管理命令，但会改变后续 `use` 渲染出的目标状态，因此不能与同 App 的写操作并发。

```
acquire profiles lock
load profiles.yaml and resolve profile by id
acquire app lock for profile.app
reload profiles.yaml and re-resolve profile by id
if profile missing -> abort ProfileNotFound
write editable YAML fragment to ~/.any-switch/state/edit/<operation_id>.yaml (0600)
open that path in $EDITOR; do not print fragment content to stdout/stderr
validate edited fragment against resolved App Definition
reject changes to immutable fields: id, app, kind, schema_version, created_at
preserve unknown extensions
write profiles.yaml atomically
delete edit fragment best-effort before releasing locks
release app lock, then profiles lock
```

`edit` 不修改 live target、captures 或 active pointer。编辑片段可能包含 env_injection / file_template 的静态 secret，因此临时片段目录必须位于 `ANY_SWITCH_HOME` 下并继承 §11.2 的权限约束；不得使用系统默认临时目录。若用户编辑的是当前 active profile 的字段，命令成功后 `status` 可能显示 `drifted`，直到用户重新 `any-switch use <id>` 把新意图应用到目标 App。

### 8.2 `any-switch import-current <app> <name>`

`import-current` 默认 `--kind auto`。它不要求用户预先知道当前 App 是 API-key、OAuth 还是 env 配置；CLI 根据 resolved App Definition 的 `import_current` 探测规则读取当前本地状态，生成一个或多个 profile draft。

可选 `--kind <kind>` 用于约束探测范围：

- 如果指定 kind，只运行该 kind 的探测和导入逻辑；当前状态不匹配则失败。
- 如果未指定或 `--kind auto`，单一候选自动采用，多候选进入 `ImportAmbiguous`。
- 交互式 TTY 下 `ImportAmbiguous` 展示候选摘要并让用户选择；非交互式必须显式 `--kind <kind>` 或失败。

```
load profiles.yaml + resolved App Definition
acquire profiles lock, then app lock
resolve current-platform import sources and acquire their target locks in target-id order
definition-driven import probes current state:
  - settings.json $.env populated (Claude)        -> draft env_injection
  - auth.json API-key shape (Codex)               -> draft file_template
  - OAuth indicators detected                     -> draft oauth_capture
  - exactly one candidate                         -> use that kind automatically
  - multiple modes co-exist                       -> ImportAmbiguous, ask user or require --kind
  - no candidate                                  -> TargetMissing / KindNotSupported with doctor hint
for oauth_capture drafts:
  detect target app process running before reading OAuth sources:
    - if running: refuse, ignore --allow-running
    - only --assume-app-stopped with the §11.4 confirmation / audit rules may continue
  read all sources (Keychain entry + json_subtree + relevant files)
  extract identity fields from oauthAccount / auth.json
  run the same Definition-declared source-consistency checks used by status / writeback
  if credential source and identity source disagree -> abort SourceInconsistent
  check verify: required identity fields against existing profiles: if match, ask user
    whether to refresh that profile's capture instead of creating new
  on refresh:
    - capture bytes 全部用最新读到的内容覆盖
    - identity 字段全部用最新值覆盖（包括 organization_name / email /
      subscription_type 等可能在 web 后台被改的字段，verify 仍只比对
      Definition 声明的 required identity 字段）
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

### 8.3 `any-switch use <id>`

```
load profiles.yaml + resolved App Definition + state/active.json
resolve target_profile by id
acquire app lock
reload profiles.yaml + state/active.json
if state/pending-switch/<app>.json exists:
    run pending-switch recovery first:
      - resolve target ids from pending target list and acquire target locks before reading or writing live targets
      - if pending.operation == "use" and live targets already match pending.to_profile
          -> acquire state.lock, finish active.json/history bookkeeping, release state.lock, delete journal
      - if pending.operation == "restore-target" and live targets already match pending.restore_from_backup_id
          -> acquire state.lock, finish history bookkeeping only, do not touch active.json, release state.lock, delete journal
      - if live targets still match the pre-operation backup
          -> delete journal and continue
      - otherwise refuse with InterruptedSwitch unless user explicitly runs restore-target
    release recovery target locks
reload profiles.yaml + state/active.json again and re-resolve target_profile
if target_profile no longer exists -> abort ProfileNotFound
previous = state.active_profiles[app] ? load profile(state.active_profiles[app].id) : null
# also load state.active_profiles[app].resolved_targets snapshot for §9.1 drift check
oauth_io_required =
  target_profile.kind == oauth_capture
  OR (previous != null AND previous.kind == oauth_capture)   # includes writeback-only self-use and switching away from OAuth
resolve live target ids for:
  - previous oauth_capture writeback sources, if any
  - target_profile apply targets
  - extra cleanup targets declared by the Definition (for example Claude managed env keys when switching to OAuth)
acquire target locks in target-id order
detect target app process running:
  - if oauth_io_required: refuse, ignore --allow-running
  - else: refuse unless --allow-running

# Short-circuit: 切到当前活动 profile 自身
# 注意：use <self> 只刷新 capture bytes + manifest；它不更新 profile.identity 或
# profile.fields。若 live target 中的 optional identity 字段（例如 organizationName /
# subscriptionType）相对 profile 已变化，writeback 完成后 status 仍可能展示 warnings。
# 用户若想同步这些字段，应改用 `any-switch import-current` 走 refresh 分支（§8.2），
# 该分支会在 identity 变化时把 profile metadata 原子写回 profiles.yaml。
if state.active_profiles[app] != null and target_profile.id == state.active_profiles[app].id:
    if target_profile.kind == oauth_capture:
        # 这是保护当前 capture 的非破坏性时机：只跑 Step B writeback；
        # writeback 仍必须通过 live identity 门禁
        build writeback-only plan
        if --dry-run: print plan, release lock, exit
        ask confirmation unless --yes
        run Step B only
        acquire state.lock; append history (operation=use, no_op_apply=true, writeback_ok=...); release state.lock
        release lock; exit
    else:
        # env_injection / file_template 切到自身真正是 no-op
        acquire state.lock; append history (operation=use, no_op=true); release state.lock
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
    if previous missing -> abort ProfileNotFound, no writeback or target apply
    if previous.kind == oauth_capture:
        # B0. 身份门禁：先从 live target 提取 verification identity，
        #     并执行 Definition 声明的 source-consistency checks。
        #     - Codex: auth.json tokens.account_id / id_token
        #     - Claude: ~/.claude.json $.oauthAccount；若 Definition 能从
        #       Keychain / .credentials.json 的 accessToken JWT payload 提取同一
        #       required identity，也必须交叉比对。若 credential source 与
        #       oauthAccount identity 不一致，或 required identity 无法可靠提取，
        #       视为 drift，拒绝 writeback。
        live_identity = extract identity from live target
        if live_identity does not match previous.identity required fields:
            abort entire switch, no captures touched
            error DriftBeforeWriteback with hint:
              - current target state is not the profile recorded in active.json
              - run status / import-current / restore-target before switching
        if source-consistency checks fail:
            abort entire switch, no captures touched
            error DriftBeforeWriteback with hint:
              - credential source and identity source describe different accounts
              - run status / import-current / detach before switching

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
for each target (files, Keychain entries, json subtrees, TOML managed paths):
    read current bytes
    write to backups/<app>/<timestamp>/<file>
write backup manifest
write state/pending-switch/<app>.json with:
  operation="use", operation_id, app, from_profile, to_profile, backup_id, target list,
  expected hashes, expected oauth identity, stage="applying"

# Step D: apply
for each target:
    stage new bytes (atomic file replace / Keychain write / json_subtree merge)
update state/pending-switch/<app>.json stage="verifying"

# Step E: verify
sha256 of each written file / subtree / Keychain entry / TOML fragment equals expected
for kind == oauth_capture:
    re-read all identity fields declared by the App Definition
    every field marked verify: required must be present and equal target_profile.identity
    optional fields mismatch -> append warning, do not block
    if any required field is missing / undecodable / mismatched:
        rollback from defensive backup, error IdentityMismatch
update state/pending-switch/<app>.json stage="bookkeeping"

# Step F: bookkeeping
acquire state.lock
reload state/active.json
update state.active_profiles[app] only (set { id: target_profile.id, resolved_targets: [...] })
append history.jsonl with operation_id (skip if already present during recovery)
release state.lock
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

### 8.4 `any-switch status [<app>]`

```
load config + state
for each app:
  if state/pending-switch/<app>.json exists -> interrupted (show recovery hint; continue next app)
  read active profile id from state/active.json
  if no active profile                      -> no-active
  resolve profile
  read actual bytes from each target
  for env_injection / file_template:
    compare only the App Definition managed surface:
      - json_env_merge: managed env keys only; unmanaged env keys do not affect matched
      - file_capture whole-file targets: full rendered bytes
      - toml_managed_paths: managed TOML paths by parsed AST semantics
      - mixed targets: all managed surfaces must match
    any unmanaged field/table/comment/order difference outside managed surfaces is ignored
  for oauth_capture:
    read identity from actual live target using the App Definition extractor
    compare all verify: required identity fields to profile.identity -> matched / drifted
    run Definition-declared source-consistency checks (for example Claude credential source vs oauthAccount)
      if credential source and identity source disagree -> drifted
    (不比对 Keychain 内容——token 可能刚被 App 刷新，bytes 必然变；
     但会比对能从各 source 提取出的 required identity；身份指纹才是稳定不变量)
  check high-priority external overrides declared by App Definition
    if managed target matched but override exists -> matched-with-overrides
  if any required target missing -> missing
```

MVP 状态集合：

| 状态 | 含义 |
|------|------|
| `matched` | env/file kind 的 managed surface 一致 / oauth required identity 和 source-consistency checks 一致 |
| `matched-with-overrides` | any-switch 管理的 target 与 active profile 一致，但检测到更高优先级的外部配置、环境变量或 managed policy 可能覆盖实际运行认证 |
| `drifted` | 活动 profile id 已知，但实际状态与预期不符 |
| `missing` | required target 不存在 |
| `no-active` | state 里没有该 App 的活动 profile |
| `interrupted` | 存在 pending-switch journal，上次写操作需要恢复或人工处理 |

### 8.5 `any-switch restore-target <app> <backup-id>`

从防御性备份恢复指定 App 的所有目标位置。来源 bytes 来自 `backups/<app>/<backup-id>/`，操作不改 state/active.json，恢复前再生成一份新备份。

对包含 Keychain entry / json_subtree 的 backup，恢复时按对应机制写回（不是简单 file copy）。

`restore-target` 与 `use` 使用同一套 App 锁、防御性备份、pending journal、apply、verify 和失败回滚协议；区别是它不更新 `active.json`，恢复完成后 `status` 可能显示 `drifted` 或 `no-active`，这是预期行为。

`restore-target` 在读取指定 backup manifest 并解析 target locks 后，必须检查 manifest 中是否存在 `requires_app_stopped: true` 的 target。若存在，恢复被视为 OAuth 动态凭据 I/O：目标 App 进程运行时默认拒绝，`--allow-running` 不生效，只能使用 §11.4 的 `--assume-app-stopped` 逃生流程。若不存在，按普通 env_injection / file_template 写操作处理：命中进程时默认拒绝，可 `--allow-running` 跳过。

`restore-target` 写目标前创建的 pending journal 必须显式写 `operation="restore-target"`、`restore_from_backup_id=<用户指定的 backup-id>` 和 `backup_id=<恢复前新建的回滚备份>`。崩溃恢复时如果 live targets 已经等于 `restore_from_backup_id` 的 manifest，只补写 history 并删除 journal，绝不把 `active.json` 改成某个 profile。

`any-switch list` / `show` 的流程平凡，略。

### 8.6 `any-switch remove <id>`

```
acquire profiles lock
load profiles.yaml + state/active.json
resolve profile by id
acquire app lock for profile.app before mutating active state or deleting captures
if state/pending-switch/<profile.app>.json exists:
    refuse with InterruptedSwitch; user must complete recovery / restore first
acquire state.lock
reload state/active.json after app lock + state lock
if state.active_profiles[profile.app] != null and state.active_profiles[profile.app].id == id:
    set state.active_profiles[profile.app] = null
    write state/active.json atomically
    # 不动目标文件；下一次 status 将报 no-active
append history (operation=remove)
release state.lock
delete profile entry from profiles.yaml (atomic write)
delete captures/<id>/ recursively (if exists)
release app lock, then profiles lock
```

注意：remove 只清理 any-switch 自身的注册表和 capture，不还原目标 App 当前的本地状态——用户如果希望"删掉 profile 同时把目标文件清空到无配置状态"，应先 `any-switch restore-target` 到一个早期备份，再 `any-switch remove`。

### 8.7 `any-switch detach <app>`

`detach` 是 `drifted` 状态后的官方恢复路径：用户已经在 App 外部把本地登录状态改成了 any-switch 不再追踪的样子（比如 `claude logout`、用 web 登录别的账号、手动改 settings.json 等），现在希望明确告诉 any-switch 停止把任何 profile 标为该 App 的活动 profile，但既不删除原 profile（capture 可能还想留作历史），也不动 live target（用户可能正在用现在的状态）。

```
acquire app lock
if state/pending-switch/<app>.json exists:
    refuse with InterruptedSwitch; 必须先完成恢复
acquire state.lock
load state/active.json
if state.active_profiles[app] is already null:
    append history.jsonl (operation=detach, no_op=true, app=<app>, ok=true)
    release state.lock; release app lock; exit 0
set state.active_profiles[app] = null
atomic write state/active.json
append history.jsonl (operation=detach, app=<app>, from_profile=<prev>, ok=true)
release state.lock, then app lock
```

`detach` 不持有 profiles lock，不动 profiles.yaml，不动 captures/，不动 backups/，不动任何 target 文件 / Keychain entry / json_subtree。

`detach` 之后的推荐下一步**默认是 `any-switch import-current`，而不是 `any-switch use`**。理由：`detach` 多半发生在 `status` 报 `drifted` 之后，意味着 live 状态已经偏离 any-switch 的认知；此时如果直接 `any-switch use <旧 profile>`，会用 capture 中的旧 bytes（含很可能已被 server 端旋转作废的 refresh_token）覆盖当前 live 状态，且因为没有"当前活动 profile"可 writeback，旧 capture 也不会得到一次刷新机会。`detach` 成功输出和 `status <app>` 在 `no-active` 状态下都必须打印这条提示：

```text
claude is now detached. To capture the current state as a new or refreshed
profile, run:
  any-switch import-current claude <name>
If you instead want to roll the live state back to an existing profile, run
`any-switch use <id>` — note this will overwrite live state with a (possibly
stale) capture and will NOT writeback first.
```

技术上 `detach` 之后两条路径都允许：

- **推荐**：`any-switch import-current <app> <new-name>` 把当前 live state 捕获成新 profile，或在 required identity 与已有 profile 重合时刷新现有 capture（参见 §8.2）。
- **次选**：`any-switch use <existing-id>` 切回任意已注册 profile；该次 `use` 因 active 为 null，**不执行 writeback**（没有"当前活动 profile"可写回），但仍走完整的 defensive backup → apply → verify → bookkeeping。

`detach` 与 `remove` 的差异：`remove` 删 profile + capture 并把 active 置 null；`detach` 只把 active 置 null，profile 与 capture 全部保留。

## 9. 文件操作协议

### 9.1 路径解析

- 所有 managed 路径中的 `~` 在配置加载时一次性展开为 `getpwuid(getuid())` 得到的真实 home 目录。
- **不信任 `$HOME` / `$USER` 环境变量**——`${MACOS_USER}` 也从 getpwuid 解出。
- App Definition 可以在路径模板中声明 `${VAR:-default}` 形式的路径根环境变量；core 不维护 App 专属白名单，也不展开无默认值的任意 `${VAR}`。变量名必须是大写字母 / 数字 / `_`，默认值和实际环境变量值都按同一套路径安全规则处理。
- 这些环境变量的值必须是绝对路径或 `~/` 前缀路径；展开、规范化和 symlink 解析后仍必须落在当前用户 home 内。plan / doctor 必须展示最终 resolved path，而不是只展示模板路径。
- 展开后路径必须落在 home 之下；否则 `doctor` 标红，`use` 拒绝写入。
- **解析时机与 active 一致性**：`use` / `import-current` / `restore-target` 在持有 App lock 后必须重新做一次模板展开，并把本次展开得到的 resolved target snapshot 写入 `state/active.json` 的对应 App entry（详见 §5 `active.json` 字段说明）。下次写命令在持有 App lock 后比较新 resolved target 与 `active.json` 中保存的 snapshot，不一致即视为 drift（可能用户在两次 shell 调用之间改了 Definition path env）：`status` 报 `drifted` 并展示新旧两套 resolved path；`use` 默认拒绝并提示用户先 `import-current` 或显式 `--accept-resolved-change` 才能继续。`restore-target` 永远以 backup manifest 的 `resolved_path` 为准，不读当前环境，也不更新 active snapshot。
- 展开后的 target / capture source 路径还必须落在 any-switch 配置根目录之外。`ANY_SWITCH_HOME` 先按同样规则展开、规范化和 symlink 解析；任何 managed target、capture source 或用户 Definition 声明的写入路径如果等于该根目录或位于其子路径下，加载或写命令必须拒绝。`~/.any-switch` 只允许由 any-switch 内部配置 / 状态代码访问，不能作为 App Definition 的 target。
- `capture.sources[].stored_as` 不是任意文件路径，只能是相对 `captures/<profile-id>/` 的普通文件名或子路径；禁止绝对路径、空路径、`.` / `..` 片段、symlink 逃逸和与 `manifest.json` 冲突。删除 `captures/<id>/` 时也必须在规范化后确认路径仍位于该 profile 的 capture 目录内。

### 9.2 锁

```text
~/.any-switch/locks/<app>.lock     # 单 App 的状态写操作
~/.any-switch/locks/profiles.lock    # profiles.yaml 写操作
~/.any-switch/locks/state.lock       # active.json / history.jsonl 共享状态写操作
~/.any-switch/locks/target-<sha256>.lock  # 规范化 target id 的跨 App 锁
```

`use` / `restore-target` 持有 App 锁，不持有 profiles lock，也不写 profiles.yaml；获取 App 锁后必须重新读取 profiles.yaml 和 state，避免与 `remove` 竞态。它们在 plan 阶段解析出本次会读取 / 写入的所有 live target 后，还必须按规范化 target id 的字典序获取 target locks，防止两个不同 App Definition 指向同一个文件、JSON/TOML 子树所在文件或 Keychain entry 时并发写坏目标。

共享 state 写入的锁顺序固定为：先持有相关 App lock（以及本次操作需要的 target locks），到 bookkeeping 阶段再获取 `state.lock`。持有 `state.lock` 后必须重新读取 `active.json`，只更新当前 App 的 entry，并在同一临界区追加 `history.jsonl`；释放 `state.lock` 后再释放 target locks / App lock。任何命令不得先持有 `state.lock` 再尝试获取 App lock，避免与写命令形成死锁。

target id 规则：

- 整文件、JSON subtree、TOML managed paths 都以真实文件路径为锁粒度：`file:<realpath>`。同一文件内不同 JSONPath / TOML path 不允许并发写，因为读-改-写会互相覆盖。
- 不存在的文件以"已存在父目录 realpath + 文件名"构造 target id；父目录不存在时先解析最近存在的祖先目录，并把后续路径片段规范化。
- Keychain entry 以 `keychain:<backend>:<service>:<account>` 为 target id。
- target lock 文件名使用 target id 的 sha256，避免路径分隔符和 secret-like service 名进入锁文件名。

`import-current` 同时写 profiles.yaml 和 captures，按固定顺序持有 profiles lock → App lock → target locks；target locks 覆盖它读取的 live sources，避免导入过程中另一条写命令改动同一目标。`add` 创建全新 profile 时只持有 profiles lock；`add --force` 覆盖已有 profile 时必须额外持有该 profile 所属 App 的 App lock，且只允许覆盖同 App、同 kind 的非 oauth_capture profile，不允许跨 App / 跨 kind 复用 id。`edit` 会改变已有 profile 的渲染意图，必须按 profiles lock → profile.app 的 App lock 顺序持锁。`remove` 会删除 capture 并可能修改 `state/active.json`，因此同样按 profiles lock → profile.app 的 App lock 顺序持有两把锁；它不读取或写入 live target，不需要 target locks。`list` / `show` / `status` / `doctor` 无锁读取。

锁实现使用 POSIX advisory lock（`flock(2)` 或 `fcntl(F_SETLK)`），持有者状态由 OS 维护——any-switch 进程崩溃、被 kill 或异常退出时，OS 自动释放锁。锁文件本身允许残留为空文件，不需要 stale lock 清理逻辑，也不在锁文件中写 PID。

注意：锁只在 any-switch 自身实例之间生效。OAuth capture I/O 已通过"拒绝目标 App 运行时执行"额外保护。不同 App 只有在 resolved target id 集合不重叠时才真正并行；target 重叠时必须串行。

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

MVP 中 `json_path` 只支持确定性的单节点对象路径子集：`$.foo.bar` 这种以 `$` 开头、只包含对象字段名的路径。字段名若包含 `.`、空格或特殊字符，必须使用 JSON Pointer 风格的显式转义形式（实现可统一归一化为内部路径段数组）。不支持 wildcard、filter、递归下降、数组切片或一次匹配多个节点的表达式。加载 Definition 时如果路径不能静态判定为单目标路径，直接 `DefinitionLoadFailed`；运行时如果 required 子树路径解析到 0 个或多个目标，按 `TargetMissing` / `RenderFailed` 处理。

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

两种场景都必须保持 JSON 文件的**字段顺序与排版风格稳定**，避免每次切换都产生大量无意义 diff（diff 本身会触发目标 App 在下次启动时全量回写，可能引入新的并发窗口）。具体要求：

1. **字段顺序保留**：使用保序的反序列化器（如 Rust `serde_json::Value` 配合 `preserve_order` feature）。managed 子树外的所有顶层和嵌套字段顺序必须与读入时一致。
2. **排版风格采样**：读入目标文件时一次性采样 `indent`（缩进字符 + 宽度）、`trailing_newline`（文件末尾是否有换行）、`is_compact`（是否为 minified 单行 JSON），把这三个参数挂在内存中的文件模型上，写回时按相同参数序列化。
3. **新建文件的默认风格**：`indent = 2 空格`、`trailing_newline = true`、`is_compact = false`。
4. **采样优先级**：若文件存在但首尾空白被破坏（例如只有部分字段是 minified），采用文件大多数缩进单元的众数；无法判定时退到默认风格并 doctor 提示。

managed 子树内部的字段顺序由 capture / Definition 决定（capture restore 时按 capture bytes 的顺序写入；env_injection 渲染时按 `managed_keys` 列出的顺序写入）；不复用外层文件的字段顺序。

这套规则确保：用户从未编辑过的目标 JSON 文件，切换前后字节级 diff 仅限于 managed 子树本身。

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

**实现层强制要求**：MVP 必须使用 lossless TOML 库（Rust 生态推荐 `toml_edit`，而不是 `toml`）。正确性依赖只到以下层级：

1. **语义等价**：未知 key / table 在 round-trip 后必须仍能被任意符合规范的 TOML 解析器解析为相同的值（值类型、值内容、key path 完全相同）。
2. **未知字段顺序保留**：在文件中相对未变化部分的位置（即 managed paths 周围）必须保持原 key 顺序。
3. **注释与空行保留 best effort**：依赖 `toml_edit` 的能力，不作为正确性依赖；MVP 不为保留注释而额外手写 AST 编辑逻辑。
4. **不要求字节相等**：datetime 的 offset 表示（`+00:00` vs `Z`）、inline table 内部空白、等价语法变体（`a.b.c = 1` 与 `[a.b]\nc = 1`）的 round-trip 允许产生差异。

也就是说：用户没编辑过 managed paths 之外的字段时，TOML 文件在 round-trip 后**语义不变**，但 byte-identical 不是承诺。验收时用 TOML 解析后的 AST 比对，不用字节 diff。

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
| `toml_managed_paths` | 捕获 / 写入 App Definition 声明的 TOML key / table，保留其他 TOML 配置 |
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
- writeback 身份门禁：
  - 必须先从 live `~/.claude.json.$.oauthAccount` 提取 `accountUuid` / `organizationUuid` 并与当前 active profile 的 required identity 比对。
  - 如果能从当前平台的 credential source（macOS Keychain value 或 Linux `.credentials.json`）中的 `claudeAiOauth.accessToken` JWT payload 提取同一账号 / 组织标识，也必须与 `oauthAccount` 和 profile identity 三方一致后才允许 writeback。
  - 如果 credential source 与 `oauthAccount` 指向不同身份，或 Definition 标记为 required 的任一 live identity 缺失 / 解码失败 / 不一致，拒绝 writeback 并报 `DriftBeforeWriteback`；不得把不一致的 Keychain / credentials bytes 回写进当前 profile capture。
- identity 字段必选 / 可选标注：
  - `account_uuid`：**required**（Anthropic 账号稳定唯一标识，verify 必比、import 去重唯一键）
  - `organization_uuid`：**required**（同账号下切换 org 视为不同 profile）
  - `organization_name` / `email` / `subscription_type`：**optional**（web 后台可改、展示用）
- verify：切换后从 `~/.claude.json` 读 `$.oauthAccount.accountUuid` 和 `$.oauthAccount.organizationUuid`，必须分别等于 `profile.identity.account_uuid` / `organization_uuid`。optional 字段如果能读到且与 profile 不一致，写入 history.jsonl 的 `warnings`，不阻塞切换。
- 不能与 env_injection 同时活动：如果 `settings.json.env` 设了 `ANTHROPIC_AUTH_TOKEN`，会**覆盖** OAuth 走 API key。切到 oauth_capture 时如果发现 Claude managed env keys，plan 必须展示"将清空这些 env 键"，并把 `settings.json` 纳入 defensive backup / apply / rollback；交互模式要求用户确认，非交互模式要求 `--yes`。

**`import_current` 行为**

1. 探测 macOS Keychain `Claude Code-credentials` 是否存在；存在 → 候选 oauth_capture。
2. 探测 Linux `${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json` 是否存在；存在 → 候选 oauth_capture。
3. 探测 `~/.claude/settings.json` `$.env.ANTHROPIC_AUTH_TOKEN` 是否非空；非空 → 候选 env_injection。
4. 两者都存在 → `ImportAmbiguous`；交互式展示"Anthropic OAuth 与第三方代理 env 同时配置，请选择捕获哪一项"，非交互式要求 `--kind`。
5. 仅 oauth_capture 候选时，自动捕获 oauthAccount + userID，提取 identity，提示用户确认 name。

**doctor / status 输出**：settings.json 存在性、`managed_env_keys` 填充状态、`~/.claude.json` 中 `oauthAccount.accountUuid` / `email`、Keychain entry 是否可读、是否检测到 Claude Code 进程。对 oauth_capture，status 必须执行与 writeback 身份门禁同一组 source-consistency checks：如果 Keychain / `.credentials.json` 能提取出的 required identity 与 `oauthAccount` 或 active profile 不一致，报告 `drifted`；doctor 可展示不含 secret 的不一致摘要。只有纯 existence 检查不足以支持 `matched`。

doctor / status 还必须检测可能覆盖目标 profile 的高优先级认证来源：当前进程环境中的 `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY`、Claude settings scopes 中的相关 env 键、`apiKeyHelper`、以及 managed policy 强制项。MVP 不尝试修改项目级、local 或 managed scope，但如果这些来源存在，`status` 不能简单报告 `matched`；应报告 `matched-with-overrides` 或 `drifted` 并提示实际运行时可能不会使用目标 OAuth / env profile。

**Claude Code 升级时的字段倒退提示**：`json_subtree` 的应用语义是"用 capture 中的子树**整体替换**目标位置的子树"。如果 Claude Code 升级后在 `$.oauthAccount` 引入新字段（例如新增 `subscriptionRegion`），用 v2.1 时期捕获的旧 capture 恢复，会让这些新字段消失——目标 App 一般会在下一次启动时回填，但部分字段可能引发短暂的 UI 不一致。doctor 在检测到当前 `~/.claude.json.$.oauthAccount` 的字段集合超出最近一次实测固化在 Definition 注释中的 schema 时，给出"建议对所有 oauth_capture profile 重新 `any-switch import-current` 以同步新字段"的提示。MVP 不在 use 时阻塞，只 doctor 提示。

### 10.4 Codex 系统预置 Definition

以下基于 Codex CLI 0.133.0（macOS）本机实测，并与 OpenAI Codex 官方文档交叉确认：Codex CLI 支持 ChatGPT 登录和 API key 登录；`${CODEX_HOME:-~/.codex}/auth.json` 是 file-backed credential store 的凭据文件，但 Codex 也可能配置为 OS credential store。MVP 只管理 file-backed `auth.json`。Credential store 选择逻辑：

1. 如果 `config.toml` 中 `cli_auth_credentials_store` 明确设置为非 `file`，无论 `auth.json` 是否存在，Codex `oauth_capture` / `file_template` 写命令都拒绝，并报 `CredentialStoreUnsupported`，提示用户先在 Codex 外部切到 file store 后重新 `import-current`。
2. 如果 `cli_auth_credentials_store` 未显式设置且 `auth.json` 存在，**还需进一步确认 file store 是当前真实在用的 store**：必须验证 `auth.json` 包含至少一个有效 `auth_mode` 字段（`"apikey"` 或 `"chatgpt"`）；如果 `auth_mode` 缺失、为 `null` 或既无 `tokens` 又无 `OPENAI_API_KEY`，则 `auth.json` 可能是切到 OS keyring 后未清理的残留——此时 `import-current` 报 `ImportAmbiguous`，明确提示"当前未能确认 Codex 使用 file-backed credential store，请在 Codex 外部确认登录状态后重试"，**不要 silent 选 file-backed**。
3. 如果 `auth.json` 不存在且无法确认其他 credential store → 报 `TargetMissing`。
4. `auth.json` 存在但 `last_refresh` 早于当前时间 90 天以上时，doctor 给出 warning：可能是 store 切换后遗留的过期文件，建议在 Codex 外部确认后再使用 `import-current`。

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
  - `${CODEX_HOME:-~/.codex}/config.toml` 中的 managed TOML paths（可选，`type: toml_managed_paths`，`required: false`；捕获为 TOML fragment，不捕获整文件）
- ChatGPT OAuth `auth.json` 实测形态：
  - 根字段：`OPENAI_API_KEY: null`、`auth_mode: "chatgpt"`、`last_refresh: <timestamp>`、`tokens: {...}`
  - `tokens` 字段：`access_token`、`account_id`、`id_token`、`refresh_token`
  - `id_token` payload 可解码出 `sub`、`email`、`name`、`auth_provider`、`https://api.openai.com/auth` 等 claims；MVP 只解码 payload 用于 identity，不校验 JWT 签名。
- identity 提取与必选 / 可选标注：
  - `account_id`：`$.tokens.account_id` —— **required**（Codex 后端账号稳定标识、import 去重唯一键）
  - `subject`：`jwt_payload($.tokens.id_token).sub` —— **optional**（IdP 可能换 issuer 导致 payload schema 变化，按 §4.6 降级处理）
  - `email`：`jwt_payload($.tokens.id_token).email` —— **optional**
  - `name`：`jwt_payload($.tokens.id_token).name` —— **optional**
- verify：切换后从恢复的 `auth.json` 读 `auth_mode == "chatgpt"`，并比对 `tokens.account_id` 与 `profile.identity.account_id`（required）。optional 字段如果 id_token 仍可解码，比对一致性写 `warnings`；解码失败或字段缺失视为正常 drift，不阻塞切换。
- refresh token 是否旋转：按动态凭据处理，`use` 切走当前 active Codex OAuth profile 前必须 writeback 整个 `auth.json` 并更新 capture manifest。

**`import_current` 行为**

- 读 `auth.json`：
  - `auth_mode == "chatgpt"` 且 `tokens.refresh_token` / `tokens.id_token` 为字符串 → draft `oauth_capture`。
  - `OPENAI_API_KEY` 为非空字符串，且 `auth_mode == "apikey"` 或缺失 OAuth `tokens` → draft `file_template`。
  - `auth_mode == "chatgpt"` 但 `OPENAI_API_KEY` 同时为字符串 → `ImportAmbiguous`，提示用户先在 Codex 外部整理登录状态。
  - credential store 不是 file-backed → 报 `CredentialStoreUnsupported`；`auth.json` 不存在且无法确认其他 credential store → 报 `TargetMissing`。

**doctor 输出**：auth.json / config.toml 存在性、当前形态识别结果、识别到的 identity 字段、是否检测到 Codex 进程。

### 10.5 开源贡献边界

新增 App 的首选路径是新增声明式 App Definition。如果现有 handler 足够表达，可以只贡献 YAML 和 fixture；如果需要新的 source、target、extractor 或特殊实证逻辑，再贡献新的 core handler。源码组织建议：

```text
src/
  app_definitions/
    builtin/
      *.yaml
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

macOS OAuth capture 的 Keychain entry 会被复制到 `~/.any-switch/captures/` 和 `backups/` 的 `0600` 文件中。MVP 明确接受这个本地明文存储取舍以换取离线切换能力，但输出、plan、history 不得打印其内容；Phase 3/4 再引入字段级加密或 Keychain-per-profile 存储。

### 11.2 文件权限强制

- `~/.any-switch/` 根目录：`0700`。
- `~/.any-switch/profiles.yaml`：`0600`。
- `~/.any-switch/apps.d/*.yaml`、`~/.any-switch/overrides.d/*.yaml`：`0600`。
- `~/.any-switch/apps.d/`、`~/.any-switch/overrides.d/`：目录 `0700`。
- `~/.any-switch/captures/`：目录 `0700`，文件 `0600`。
- `~/.any-switch/backups/`：目录 `0700`，文件 `0600`。
- `~/.any-switch/state/`：目录 `0700`，文件 `0600`。
- `~/.any-switch/locks/`：目录 `0700`。
- 目标文件继承现有权限，新建默认 `0600`。
- Keychain entry：依赖 macOS Keychain 自身权限模型；any-switch 只用当前用户 login keychain。

启动时若发现 `~/.any-switch/` 任一文件 / 目录权限被改宽，doctor 警告并提示修复。

### 11.3 防御性备份保留

默认 `keep_backups: 20`（每 App 独立计数）。

MVP 自动清理：在 `use` / `restore-target` 成功后、释放锁前执行。按 mtime 倒序保留最近 N 份。失败不影响主流程结果，但写入 history 的 `warnings`。

**Hardlink dedup**：备份目录用 hardlink 去重。落 backup 时先计算每个待备份文件的 sha256，与同 App 已有备份中相同 sha256 的文件 hardlink（仅当文件系统支持、目标 inode 权限仍为 `0600` 且 owner 为当前用户时）；不支持或失败时退化为常规 copy。`~/.claude.json` 这类大文件在多次连续切换里通常 sha256 不变，dedup 后实际磁盘占用接近一份。Keychain entry 备份（小 JSON）和 json_subtree 备份（小字符串）也走同一机制，但收益有限。修剪 backup 时按 mtime 删整个 backup 目录即可，hardlink 引用计数会自动回收。

**doctor 报告**：`any-switch doctor [<app>]` 输出每个 App 的：

- 备份数量、最旧 / 最新备份时间（已有）。
- **备份目录总大小**（du-style 累计 inode 占用，反映 dedup 后真实磁盘占用）。
- **逻辑总大小**（不计 hardlink dedup 的字节总和，便于用户判断 dedup 是否生效）。
- 若 `keep_backups` 内总大小超过 100 MB 给出 warning，提示用户调小 `keep_backups` 或检查目标文件是否异常膨胀。

### 11.4 并发与进程检测

同 App 写操作互斥（App 锁）。不同 App 只有在 resolved target id 集合不重叠时可并行切换；一旦两个 App 指向同一个文件或 Keychain entry，target lock 会把它们串行化。

`use` / `restore-target` / `import-current` 执行前通过进程名匹配（由 App Definition 声明）粗粒度检测目标 App 是否运行：

- env_injection / file_template 操作：命中默认拒绝，可 `--allow-running` 跳过；但仍建议先退出目标 App，因为 App 运行时也可能重写相同配置文件。
- **任何涉及 oauth_capture 读写的操作：命中默认强制拒绝，`--allow-running` 不生效**。这包括切到 OAuth profile、从当前 active OAuth profile 切走时的 writeback、`any-switch use <当前 OAuth profile>` 的 writeback-only 操作、`import-current` 创建或刷新 OAuth profile，以及 `restore-target` 恢复 manifest 中 `requires_app_stopped: true` 的 target。理由：App 运行时会刷新 token，原子性无法保证，且我们读取、写回或恢复的瞬间 App 也可能在写。

**进程探测误报的逃生口**：进程名匹配本质是粗粒度信号，会有误报（同名 wrapper / launcher / 孤儿进程）和漏报（用户用别名启动）。对涉及 oauth_capture 读写的操作命中时：

- 错误信息（`AppRunning`）必须列出探测到的 **PID + 进程命令行 + 启动时间**，让用户能精确判断是真的目标 App 还是误报。
- 提供 `--assume-app-stopped` 显式逃生开关，**仅**在用户能自证"列出的 PID 不是目标 App"时使用。该开关：
  - 必须显式拼写完整，没有短形式（避免被 muscle memory 误用）。
  - 必须同时配合 `--yes` 才生效（非交互式拒绝）或在交互式 TTY 下额外二次确认。
  - 触发时把"用户已确认的 PID 列表"写入 `history.jsonl` 的 `warnings`，便于事后审计。
- doctor `--app <id>` 单独跑探测时，对每个命中的 PID 输出 `/proc` 或 `ps` 拿到的命令行，帮助用户判断要不要 kill 或加白名单。

漏报方向无法用 CLI flag 防御——这是 App Definition `process_probe` 表达力的限制。第一类用户教育：任何涉及 oauth_capture 读写的操作前，用户应主动退出目标 App（例如 GUI Quit 或结束对应 CLI 进程）并通过 `any-switch doctor <app>` 二次确认；用户必须理解 §11.4 默认拒绝是"对漏报偏保守"的设计选择，而非额外强制保证。Phase 2 可考虑给 `process_probe` 增加 `lockfile` / `socket` 等次级 handler 来收紧漏报。

### 11.5 OAuth refresh token rotation

`oauth_capture` 假设 App 后端**可能**旋转 refresh_token。两条机制对抗这个风险：

1. **切换时写回**：切走当前活动 oauth_capture profile 前，先把最新 Keychain / 凭据文件内容回灌到该 profile 的 capture。这样只要用户在多个 profile 之间循环切换，每个 capture 都保持最新。
2. **失效边界**：如果 capture 长期不被切到（即 refresh_token 已老化），用户切过去时 App 会在首次 refresh 时失败。CLI 基于 capture manifest 的 `last_writeback_at ?? captured_at` 提前警告，但不提供登录修复能力；用户在目标 App 外部恢复可用状态后，再运行 `any-switch import-current` 捕获或更新 profile。

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
| `DriftBeforeWriteback` | active profile 指向 OAuth profile，但 live target identity 与该 profile 不一致，或 credential source / identity source 一致性检查失败，拒绝写回以避免污染 capture |
| `SourceInconsistent` | import-current 读取到的 OAuth credential source 与 identity source 指向不同身份，拒绝创建或刷新 capture |
| `BackupFailed` | 切换前防御性备份失败 |
| `BackupInvalid` | 指定 backup 缺少 manifest、manifest 校验失败，或 manifest 中 resolved target spec 已不符合当前路径 / handler 安全约束 |
| `ReplaceFailed` | 替换目标位置失败 |
| `VerifyFailed` | 替换后 hash 与预期不一致 |
| `IdentityMismatch` | 替换后 identity 与 capture 中的 identity 不一致（oauth_capture）|
| `InterruptedSwitch` | 发现上次写操作留下 pending-switch journal，必须先完成提交、回滚或人工恢复 |
| `ImportAmbiguous` | `import_current --kind auto` 检测到多种可能 kind；交互式可选择，非交互式需显式 `--kind` |
| `LockBusy` | 另一个写操作正在执行 |
| `AppRunning` | 目标 App 进程在运行；任何涉及 oauth_capture 读写的操作默认拒绝（仅 `--assume-app-stopped` + `--yes` 可逃生），纯 env_injection / file_template 操作默认拒绝（可 `--allow-running` 跳过）；错误信息含探测到的 PID + 命令行 + 启动时间 |
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
next: any-switch restore-target claude 20260523T100000Z
hint: target profile's capture may be from a different Anthropic account.
      fix the current App state outside any-switch, then run
      `any-switch import-current claude <name>` to capture or update it.
```

## 12.5 落地里程碑（MVP 内的渐进式交付）

§13 的 41 条验收是 MVP 的最终验收基线，但不要求一次性同时通过。实现按以下三个里程碑分批推进，避免任意一条边缘加固阻塞核心演示：

**M1 — 核心可演示路径**（必须通过 §13 第 1、2、3、4、5、6、7、8、9、10、11、12、13、14、15、16、17、18、19、20、21、22、27、38、40、41 条）

- Claude `env_injection`、Claude `oauth_capture`、Codex `file_template`、Codex `oauth_capture` 的 add / import-current / use / status / restore-target / remove / doctor 全链路。
- 单 App 顺序操作下的 writeback、身份门禁、防御性备份、verify、回滚。
- 进程互斥（含 `--assume-app-stopped` 逃生口）、secret argv 拒绝、脱敏输出、路径边界、`ANY_SWITCH_HOME` 隔离。
- 用户声明式 App Definition 的加载、校验、override 合并、`apps` 子命令族。
- M1 阶段允许 `pending-switch` journal 仅做"检测 + 拒绝"，不要求自动补提交 / 自动回滚。

**M1.5 — 并发与崩溃恢复加固**（在 M1 之上叠加 §13 第 23、24、25、26、32、33、34、35、36、37、39 条）

- pending-switch journal 的自动补提交、自动回滚和 `restore-target` 分支。
- `state.lock` 串行 bookkeeping、`target-<sha256>.lock` 跨 App 串行化、`profiles.lock → app.lock` 持锁顺序的并发测试。
- `edit` / `add --force` / `detach` / `remove` 与 `use` 的并发不变量。
- Codex `toml_managed_paths` 的语义等价 round-trip。

**M2 — 体验与体积优化**（在 M1.5 之上叠加 §13 第 28、29、30、31 条以及本节列出的次级项）

- JSON 文件 indent / trailing newline / key order 的"读入时采样"——M1 / M1.5 允许使用固定 2-space + preserve_order 默认风格。
- 备份目录 hardlink dedup、`doctor` 备份大小报告与 100 MB 软警告——M1 / M1.5 允许使用普通 copy，且仅按数量裁剪 `keep_backups`。
- 用户 Definition 路径与 `ANY_SWITCH_HOME` 边界的完整校验矩阵——M1 仅做最小必要校验（绝对路径 + home 内 + 非 `ANY_SWITCH_HOME` 子路径）。
- 跨 App target lock 在真实并发负载下的性能与公平性测试。

里程碑之间的功能 / 安全约束是**单调累加**的：M1 已经实现的安全护栏（OAuth 进程互斥、secret 脱敏、home 边界等）不允许在 M1.5 / M2 中放宽。前置实测（§13 末尾 A–E 项）必须在 M1 发布前全部完成，不可推迟。

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
6. `any-switch use` 只要当前活动 profile 是 oauth_capture 且需要 writeback（包括切到 oauth_capture B、切到 env/file profile、以及 `any-switch use <当前 OAuth profile>` 的 writeback-only 操作），都必须先校验 live target identity 仍等于当前活动 oauth_capture A，并执行 Definition 声明的 credential source / identity source 一致性检查；通过后才把 A 的最新 Keychain / 凭据文件 / json_subtree / managed TOML fragment 写回 A 的 capture，并更新 `captures/A/manifest.json`。identity 不匹配或 source 不一致时必须拒绝写回且不得修改 capture / profiles.yaml。
7. `status` 能正确报告 `matched` / `matched-with-overrides` / `drifted` / `missing` / `no-active` / `interrupted`；oauth_capture 的 matched 基于 required identity 比对和 Definition 声明的 source-consistency checks，而非 capture bytes。
8. `use --dry-run` 输出的 plan 不包含 secret 字段值的明文，也不打印 capture blob 内容；但能展示 identity 块。
9. 每次 `use` 前自动建立防御性备份（覆盖 file / Keychain / json_subtree / TOML managed paths 相关目标）；`use` / `restore-target` 成功后按 `keep_backups` 自动修剪。
10. `restore-target` 能从备份 manifest 记录的 resolved target 恢复所有类型的目标位置，恢复前再生成一份新备份；恢复不受当前 Definition path env 变化影响，除非原 resolved target 已不再满足 home / handler 安全约束。
11. `use` / `import-current` / `restore-target` 在检测到目标 App 进程运行时默认拒绝；纯 env_injection / file_template 写操作可 `--allow-running` 跳过；**任何涉及 oauth_capture 读写的操作都不接受 `--allow-running`**，包括 OAuth import / refresh 和恢复 `requires_app_stopped: true` 的 backup target。`AppRunning` 错误必须列出探测到的 PID + 命令行 + 启动时间；仅在用户对每个 PID 都能自证非目标 App 时，可叠加 `--assume-app-stopped` + `--yes`（非交互式）或额外二次确认（交互式）跳过，逃生记录写入 history `warnings`。
12. 不提供 login / reauth 命令，不执行、引导或修复任何登录流程。
13. `remove` 能删除 profile；同时清理 `captures/<id>/`。
14. 所有命令输出（人类格式和 `--json`）都不打印 secret 字段明文，也不打印 capture blob 内容；命令行参数也不能接受敏感字段明文值。
15. profiles.yaml / apps.d/ / overrides.d/ / captures/ / backups/ 权限不正确时启动有警告；写入含 secret 的目标文件前，如果现有权限宽于 `0600`，必须收紧或拒绝写入。
16. 配置加载时 `~` 和 `${MACOS_USER}` 都从 `getpwuid` 展开，不信任 `$HOME` / `$USER`；App Definition 路径模板里的 `${VAR:-default}` 按通用规则展开，展开后必须仍落在 home 内。
17. core 不包含 `claude` / `codex` 专属分支；产品差异优先在系统预置 App Definition 中表达，core 只提供通用 handler。
18. 能从 `~/.any-switch/apps.d/*.yaml` 加载一个用户声明式 App Definition，并用已有 handler 完成 `env_injection` 或 `file_template` profile 的 add/use/dry-run。
19. 用户 Definition 引用未知 handler、写入 home 外路径或包含可执行脚本字段时，加载失败且写命令拒绝执行。
20. `any-switch apps` 能展示每个 App Definition 的来源（system / user / override）和支持的 kind；`any-switch apps show <app>` 能展示 resolved Definition；`any-switch apps export <app> --as override` 能生成可验证的 override 起点；`any-switch apps validate` 能校验单个 Definition 文件。
21. 文档明确：任何涉及 oauth_capture 读写的操作前必须退出目标 App；其他 kind 也建议退出。
22. 除 `add` / `edit` / `remove` / `import-current` 等 profile 管理命令外，任何命令都不得修改 profiles.yaml；`use` 的 OAuth writeback 只更新 captures 和 capture manifest。
23. Codex `config.toml` 只能修改 Definition 声明的 managed TOML paths，必须保留 `mcp_servers` / `projects` / `plugins` 等未知配置。Codex oauth_capture 捕获 `config.toml` 时使用 `toml_managed_paths` source，只保存 managed paths 的 TOML fragment，不保存整文件。验收以 TOML AST 比对（语义等价 + managed paths 外 key 顺序一致），不要求字节相等；datetime offset 表示、inline table 内部空白与等价语法变体的 round-trip 差异允许存在。
24. `use` / `restore-target` 写目标前必须创建 `state/pending-switch/<app>.json`；模拟 apply 成功但 bookkeeping 前崩溃时，下一次同 App 写命令能完成提交、回滚或拒绝并报 `InterruptedSwitch`，不能开始新的切换。
25. `remove` 与 `use` 并发时不能删除正在使用的 capture；`remove` 必须按 profiles lock → app lock 顺序持锁。
26. `any-switch detach <app>` 仅把 `state/active.json` 中该 App 的活动 profile 置 null，不动 profiles.yaml、captures/、backups/ 和任何 live target；detach 后 `status` 报 `no-active`，`import-current` 与 `use` 仍可正常工作（首次 `use` 因无 active 不执行 writeback）。
27. oauth_capture identity 字段在 App Definition 中按 `verify: required | optional` 标注：required 字段在 verify 阶段缺失 / 解码失败 / 值不等都触发 `IdentityMismatch` 回滚；optional 字段一致性差异只写 `warnings`，不阻塞切换；每个 oauth_capture Definition 至少声明一个 required 字段，否则 `DefinitionLoadFailed`。Claude required: `account_uuid` + `organization_uuid`；Codex required: `account_id`。
28. JSON 目标文件（如 `~/.claude.json`、`settings.json`）写回时保留读入时采样的 indent / trailing newline / minified 风格与未管理子树外的 key 顺序；新建文件用 indent=2 / trailing newline / pretty 默认风格。验收：对已存在 JSON 文件做 use → 立刻再 use 同 profile，managed 子树外字节级 diff 必须为空。
29. 备份目录使用 hardlink dedup（文件系统支持时），`any-switch doctor` 同时报告备份的 inode 占用与逻辑字节总和；`keep_backups` 内总大小超过 100 MB 给出 warning。
30. 用户 App Definition 的 target / capture source 路径如果落在 `ANY_SWITCH_HOME`（默认 `~/.any-switch`）之内，加载失败；写命令不得通过 Definition 覆盖 profiles.yaml、captures、backups、state 或 locks。
31. 两个不同 App Definition 指向同一个真实文件或 Keychain entry 时，`use` / `restore-target` / `import-current` 通过 target locks 串行化；并发测试不得产生丢失更新或交错写入。
32. `restore-target` 的 pending journal 必须带 `operation="restore-target"` 和 `restore_from_backup_id`；模拟恢复 apply 成功但 bookkeeping 前崩溃时，下次写命令只补 history / 删除 journal，不修改 `active.json`。
33. `status` 对 `json_env_merge` / `toml_managed_paths` 等部分管理型 handler 只比较 Definition 声明的 managed surface；managed 范围外的 key / table / 注释 / 排版差异不得导致 `drifted`。
34. 支持 `oauth_capture` 的 resolved Definition 如果没有非空 `process_probe`，加载失败；`json_path` / `managed_json_subtrees` 只允许单目标路径子集，不允许 wildcard / filter / 多节点匹配。
35. `edit` / `add --force` 与同 App 的 `use` 并发时不能让 live target 和最终 profiles.yaml 中的 profile 意图错位；`edit` 必须按 profiles lock → app lock 顺序持锁，且拒绝修改 `id` / `app` / `kind` / `schema_version` / `created_at` 等不可变字段。`add --force` 只能覆盖同 App、同 kind 的非 oauth_capture 既有 id，并按同样顺序持锁。
36. 不同 App 并发 `use` / `restore-target` / `detach` / `remove` 时，`active.json` 和 `history.jsonl` 写入必须通过 `state.lock` 串行提交；并发测试不得出现 active entry 丢失、history 行交错或重复 operation_id。
37. `active.json` 必须为每个 App entry 保存 `resolved_targets` snapshot；下一次同 App 写命令在持锁后重新展开模板路径，与 snapshot 不一致时 `status` 报 `drifted`，`use` 默认拒绝，要求用户先 `import-current` 或显式 `--accept-resolved-change`。`restore-target` 不更新 snapshot，且永远以 backup manifest 的 `resolved_path` 为准。
38. 跨平台 capture 完整性：`use` / `status` / `doctor` 在当前平台筛出适用 `capture.sources` 后，必须验证 `captures/<id>/<stored_as>` 全部存在；任一缺失即报 `CaptureMissing`，并在错误信息中提示重跑 `import-current`。
39. `doctor` 默认输出必须包含 "profiles.yaml secret-leak surface" 检查：当 `~/.any-switch/` 位于 iCloud Drive / Dropbox / OneDrive / Google Drive 等已知云同步目录之内时升级为 warning。
40. `detach` 成功输出和 `status` 在 `no-active` 状态下必须显式打印 "推荐 `any-switch import-current` 而非 `any-switch use`" 的提示（含原因：`use` 不 writeback，会用可能已陈旧的 capture 覆盖 live state）。
41. Codex `import-current` 在 `cli_auth_credentials_store` 未显式设置且 `auth.json` 存在但缺少有效 `auth_mode` 字段时，必须报 `ImportAmbiguous` 而非 silent 选 file-backed。

**前置实测（在固化系统预置 Definition 前必须完成并记录结论）**：

A. **Claude refresh_token rotation 实测**：登录 → 等触发刷新 → 比对旋转前后 → 用 capture 中的旧 refresh_token 尝试恢复 → 看是否能续期。
B. **Claude oauthAccount 不一致容忍度实测**：仅改 Keychain 不改 `~/.claude.json.oauthAccount` → 启动 Claude Code → 观察行为（自动修正 / UI 不一致 / 报错）。
C. **Claude 并发写 `~/.claude.json` 实测**：Claude Code 运行时观察 `~/.claude.json` 的写频率与字段，识别哪些字段会和 `$.oauthAccount` / `$.userID` 同一次写入。同时采样 Claude Code 自己写出的 JSON 格式参数（indent 字符与宽度、是否 trailing newline、是否 minified、顶层 key 顺序），把结果固化为 §9.4 采样算法的默认值或 doctor 校验项；如果 Claude Code 写出的格式与 §9.4 默认值不同，说明 any-switch 必须严格沿用读入时的采样而不能落回默认。
D. **Codex auth.json schema 实证**：已在 Codex CLI 0.133.0（macOS）确认 ChatGPT OAuth file-backed 形态为根字段 `OPENAI_API_KEY: null`、`auth_mode: "chatgpt"`、`last_refresh`、`tokens.{access_token,account_id,id_token,refresh_token}`；API-key 模式为根字段 `auth_mode: "apikey"` + `OPENAI_API_KEY` 两个字符串。
E. **Codex 外部恢复后捕获流程**：实测用户在 Codex 外部恢复当前状态后，`import-current` 能正确捕获或更新 profile。

## 14. 后续演进

### Phase 2: 更多 App、kind、backend

- Gemini CLI / OpenCode / Cursor / Windsurf 等 App Definition（优先用户扩展，成熟后可上升为系统预置）。
- Linux Secret Service / Windows Credential Manager backend。
- 新 kind：`dotenv_file`（Gemini 风格）、`composite`（多 kind 组合，例如 OAuth 加额外环境变量）。
- `opaque_capture` 的首个实例（如果出现没有刷新语义的纯 blob 凭据场景）。

### Phase 3: 体验增强

- `any-switch rename` / `any-switch tag` / shell completion。
- 更细的 drift 展示：diff 出具体哪些键 / 字段被外部改了。
- `any-switch export` / `import`：跨机器迁移（明文导出需 `--unsafe-export`）。
- 字段级 secret 加密（passphrase-based）。
- `daemon` 模式：常驻监听 Keychain / credentials.json 变化，实时回写当前 active profile 的 capture，进一步降低旋转失效风险。

### Phase 4: 凭据管理器集成

- 与 1Password / pass / Bitwarden 集成。
- 项目级 scope（按工作目录绑定 profile）。
- 加密快照、跨机器同步。

### Phase 5: 更通用的状态模型

如果内置 AI CLI 场景之外出现明确需求，再考虑更通用的 state / plan / apply 模型、跨 App 组合切换、可执行插件协议、trust / allow 机制。这些能力不应反向污染第一阶段 MVP，也不改变 core 当前的基本职责：安全、可预期地把结构化 profile 应用到一组本地 targets。

## 15. 当前设计结论

`any-switch` 第一版是一个**本地 profile / state 切换器**。AI CLI 账号和凭据切换是 MVP 的首个应用域，因为它同时覆盖了静态字段渲染、OAuth 动态 capture、secret 脱敏、备份回滚和 App Definition 扩展等核心能力。

长期看，core 不关心"账号"这个业务概念；core 只关心如何把一条结构化 profile 安全、可预期地应用到一组本地 targets。Claude Code / Codex 的账号、凭据、端点和模型配置，是第一组随二进制内置的 target/domain 实例。

最重要的边界：

1. Profile 是结构化记录（`id` / `app` / `kind` / `fields` 或 `identity + capture`），不是 opaque 文件快照；它描述目标 App 的一组期望本地状态。
2. schema 中保留四种 kind（`env_injection` / `file_template` / `oauth_capture` / `opaque_capture`），MVP 实现前三种，`opaque_capture` 等出现真实场景后再接入代码路径；凭据切换只是这些形态的第一组内置用例。
3. 动态 capture 视为会变化的本地状态资产：切换时双向写回、身份指纹校验、过期感知；登录和失效修复明确在工具边界之外。
4. 用户可以手编 profiles.yaml；`oauth_capture` 的 blob 和动态 manifest 由 `import-current` / writeback 自动维护。
5. App 专属知识优先封装在系统预置或用户扩展 App Definition 内；core 提供文件原子写、JSON 合并、TOML managed paths、Keychain backend、锁、防御性备份、hash、脱敏和受信任 handler。
6. 每次切换前对所有目标位置做防御性备份，每次切换后做 hash 校验和（对 oauth_capture）identity 校验。
7. 任何涉及 oauth_capture 读写的操作前**强制要求**目标 App 退出，不接受 `--allow-running`。
8. 后续扩展优先通过声明式 App Definition / override 完成；新增 handler、kind、backend 或复杂 OAuth 逻辑再通过 PR 增加受信任实现。MVP 不引入可执行插件生态。

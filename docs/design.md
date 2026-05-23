# switch-cli 设计文档

## 1. 项目定位

`switch-cli` 是一个基于命令行的状态切换工具，用于读取、展示、开启、关闭、切换和收敛任意可管理状态。

它面向的状态不只包含布尔开关，也包括枚举、字符串、对象和配置集合。例如：

- 系统代理：`off` / `http` / `socks` / `pac`
- macOS 深色模式：`on` / `off`
- Git 身份：`work` / `personal`
- 项目环境变量：一组 key-value
- Shell profile 片段：启用或禁用
- 本地服务：`running` / `stopped`
- Hosts 条目：存在或不存在
- Feature flag：`enabled` / `disabled` / 分组配置

项目的核心目标是提供一个统一的状态模型和命令体验，让用户可以用同一套方式管理跨操作系统、跨工具、跨项目的状态。

## 2. 设计原则

### 2.1 状态优先

工具的核心不是“执行脚本”，而是管理状态。

每个功能模块都必须尽量支持读取当前状态，并能把当前状态和期望状态进行比较。只有能读取状态，`toggle`、`status`、`diff`、`plan`、`apply --dry-run` 才是可信的。

### 2.2 声明式和命令式并存

用户需要两类使用方式：

- 命令式：立即打开、关闭或切换某个状态。
- 声明式：通过配置文件声明期望状态，然后执行 `apply` 进行收敛。

示例：

```bash
switch on macos.dark_mode
switch off macos.dark_mode
switch use git.identity work
switch set proxy.system:mode http
switch toggle git.identity
switch apply --dry-run
switch apply
```

`set` 命令的字段路径以 `:` 分隔模块名与字段，避免模块命名空间（含 `.`）与字段路径冲突。`on` / `off` 只对 `state_type == boolean` 的模块有效；对 enum / profile 状态请使用 `use` 或 `set`。

配置示例：

```yaml
modules:
  proxy.system:
    desired:
      mode: http
      http: http://127.0.0.1:7890
      https: http://127.0.0.1:7890

  git.identity:
    desired:
      profile: work

  shell.env:
    desired:
      vars:
        EDITOR: nvim
        LANG: en_US.UTF-8
```

### 2.3 模块和插件分层

`module` 和 `plugin` 是两个不同概念：

- 模块定义一个状态域如何读取、计划、应用和验证。
- 插件扩展 `switch-cli` 应用本身的能力，例如新增命令、同步配置、安装模块、提供 TUI、集成远程配置源。

模块负责“管理什么状态”。插件负责“扩展这个 CLI 能做什么”。

### 2.4 跨平台能力下沉到 provider

核心层不直接写大量 OS 分支，而是通过 provider 处理平台差异。

同一个模块可以有多个 provider：

```text
proxy.system
  darwin provider  -> networksetup / scutil
  linux provider   -> gsettings / environment / systemd
  windows provider -> PowerShell / Registry
```

模块对核心暴露统一协议，provider 对模块处理平台细节。

### 2.5 可预览、可回滚、可诊断

修改系统状态的工具必须提供足够的安全边界：

- `status`：查看实际状态。
- `diff`：比较实际状态和期望状态。
- `plan`：生成执行计划。
- `apply --dry-run`：复用 plan 流程，只展示即将执行的动作，不产生副作用。
- `doctor`：检查依赖命令、权限、配置格式和平台支持。
- `revert`：在模块支持时恢复上一次变更。

## 3. 参考项目

每个项目只取一条核心借鉴：

- [chezmoi](https://www.chezmoi.io/)：把用户配置视为 desired state，所有修改先 plan/diff 后 apply。
- [asdf](https://asdf-vm.com/) / [mise](https://github.com/jdx/mise)：核心定义外部协议，第三方模块以任意语言实现，只要遵守 JSON 协议。
- [direnv](https://direnv.net/)：按目录加载配置必须有信任机制（allow），环境变更生成 shell 可执行片段。
- [Home Manager](https://nix-community.github.io/home-manager/) / [NixOS module](https://nixos.org/)：模块声明 schema、默认值、平台支持和依赖，配置合并规则显式。
- [oclif](https://oclif.io/)：CLI 插件贡献命令 + hooks + topic 的生命周期模型。
- [Starship](https://starship.rs/)：模块配置简洁稳定，内置与用户模块共用统一入口。

## 4. 核心概念

### 4.1 State

实际状态，也就是从系统、文件、命令或远程服务中读到的当前值。

示例：

```json
{
  "module": "proxy.system",
  "state": {
    "mode": "off",
    "http": null,
    "https": null
  },
  "source": "darwin.networksetup"
}
```

### 4.2 Desired State

期望状态，来自用户配置或命令参数。

示例：

```json
{
  "module": "proxy.system",
  "desired": {
    "mode": "http",
    "http": "http://127.0.0.1:7890",
    "https": "http://127.0.0.1:7890"
  }
}
```

### 4.3 Plan

执行计划，描述为了从当前状态变更到期望状态，需要执行哪些动作。

示例：

```json
{
  "module": "proxy.system",
  "actions": [
    {
      "type": "command",
      "description": "Enable HTTP proxy for Wi-Fi",
      "command": ["networksetup", "-setwebproxystate", "Wi-Fi", "on"]
    }
  ],
  "requires_admin": false,
  "reversible": true
}
```

### 4.4 Apply Result

执行结果，记录动作是否成功、最终状态是什么、是否需要用户手工处理。

示例：

```json
{
  "module": "proxy.system",
  "ok": true,
  "changed": true,
  "state": {
    "mode": "http"
  },
  "warnings": []
}
```

### 4.5 Scope

作用域决定配置和状态的生效范围。MVP 只承诺两个：

- `global`：用户全局配置（`~/.config/switch/config.yaml`）。
- `project`：当前项目目录配置（`./.switch.yaml`），需要 trust 才会生效。

后续扩展（不在 MVP 范围）：

- `machine`：当前机器（与 user config 区分硬件相关字段）。
- `os`：当前操作系统下的条件配置。
- `profile`：用户自定义 profile，例如 `work` / `home` / `travel`。
- `session`：当前 shell 会话内生效；需要 shell hook 配合，并入 Phase 3。

## 5. 模块系统

### 5.1 模块职责

模块负责定义一个状态域的完整生命周期：

- 声明模块元信息。
- 声明配置 schema。
- 读取当前状态。
- 解析期望状态。
- 生成变更计划。
- 应用变更。
- 验证变更结果。
- 可选支持回滚。

### 5.2 模块接口

模块支持的操作 = manifest 中可声明的 capability，一一对应。MVP 必选 `detect / plan / apply`，可选 `revert / doctor`：

| 操作       | 必选 | 说明 |
|------------|------|------|
| `metadata` | ✓    | 返回模块元信息（核心通过 manifest 已知，仅在动态发现场景调用）|
| `schema`   | ✓    | 返回配置 schema |
| `detect`   | ✓    | 读取当前状态 |
| `plan`     | ✓    | 基于当前状态和期望状态生成计划 |
| `apply`    | ✓    | 执行核心传入的 plan |
| `doctor`   | 可选 | 检查依赖和平台支持 |
| `revert`   | 可选 | 回滚最近一次变更 |

`metadata` 和 `schema` 视为 manifest 静态信息的一部分，不计入 `capabilities` 数组；`capabilities` 只列运行时操作（`detect`/`plan`/`apply`/`doctor`/`revert`）。

`status` 不是独立模块操作：CLI 的 `switch status` 调用 `detect` 后由核心格式化展示。

### 5.3 模块类型

#### Built-in Module

随 `switch-cli` 发布的内置模块，适合覆盖高频、稳定、跨平台或基础能力。

初期建议内置：

- `shell.env`：环境变量和 shell 片段。
- `git.identity`：Git 用户身份切换。
- `proxy.system`：系统代理。
- `app.config_file`：配置文件片段启用/禁用。
- `service.process`：本地服务运行状态。

#### External Module

第三方模块，作为独立可执行文件安装。

命名约定：

```text
switch-module-<name>
```

例如：

```text
switch-module-vscode
switch-module-docker
switch-module-raycast
```

### 5.4 模块 manifest

示例：

```json
{
  "schema_version": "1",
  "kind": "module",
  "name": "proxy.system",
  "version": "0.1.0",
  "description": "Manage system proxy settings",
  "entry": "switch-module-proxy",
  "platforms": ["darwin", "linux", "windows"],
  "capabilities": ["detect", "plan", "apply", "revert", "doctor"],
  "state_type": "object",
  "requires": {
    "commands": {
      "darwin": ["networksetup"],
      "windows": ["powershell"]
    }
  }
}
```

### 5.5 模块协议

外部模块通过标准输入、标准输出和 JSON 与核心通信。

调用示例：

```bash
switch-module-proxy detect --input -
switch-module-proxy plan --input -
switch-module-proxy apply --input -
```

输入示例：

```json
{
  "schema_version": "1",
  "module": "proxy.system",
  "operation": "plan",
  "context": {
    "os": "darwin",
    "arch": "arm64",
    "scope": "global",
    "cwd": "/Users/river/project"
  },
  "current": {
    "mode": "off"
  },
  "desired": {
    "mode": "http",
    "http": "http://127.0.0.1:7890"
  }
}
```

输出示例：

```json
{
  "ok": true,
  "changed": true,
  "plan": {
    "actions": [
      {
        "type": "command",
        "description": "Enable HTTP proxy",
        "command": ["networksetup", "-setwebproxystate", "Wi-Fi", "on"]
      }
    ]
  }
}
```

### 5.6 状态类型

模块不应该被限制为布尔开关。建议支持以下状态类型：

- `boolean`：开/关。
- `enum`：多选一状态。
- `profile`：具名配置档，本质是带 payload 的多选一状态。
- `string`：字符串值。
- `number`：数值。
- `object`：结构化配置。
- `list`：列表型状态。
- `set`：集合型状态。

`toggle` 只对可明确取反或轮转的状态有效。

模块需要在 schema 中声明 toggle 规则：

```json
{
  "toggle": {
    "type": "cycle",
    "values": ["off", "http", "socks"]
  }
}
```

### 5.7 多选一状态

多选一状态应该是一等模型，而不是把所有状态都压成 `on` / `off`。

典型例子：

```text
proxy.system.mode     off / http / socks / pac
git.identity.profile  work / personal / oss
theme.current         light / dark / system
node.version          20 / 22 / latest
```

#### 5.7.1 Enum

`enum` 表示同一时间只能选择一个值。

模块 schema 示例：

```json
{
  "state_type": "enum",
  "enum": {
    "values": ["off", "http", "socks", "pac"],
    "default": "off"
  },
  "toggle": {
    "type": "cycle",
    "values": ["off", "http"]
  }
}
```

命令示例：

```bash
switch options proxy.system          # 列出模块可选值
switch use proxy.system http         # 选模块预定义选项，展开为完整 desired
switch cycle proxy.system            # 按 toggle.cycle 顺序轮转
switch set proxy.system:mode socks   # 只改单字段，<module>:<field.path>
```

`use` 与 `set` 的核心区别：`use` 选模块预定义选项并展开为完整 desired state；`set` 只改单个字段，不触发选项展开。例如 `switch use git.identity work` 会同时设置 name / email / signing_key，而 `switch set git.identity:email river@company.com` 只改 email。

#### 5.7.2 Profile

`profile` 是更实用的多选一：用户选择一个名字，模块把它展开成完整期望状态。

示例：

```yaml
modules:
  git.identity:
    desired:
      profile: work
    profiles:
      work:
        name: River
        email: river@company.com
        signing_key:
          secret_ref: env:GIT_SIGNING_KEY_WORK
      personal:
        name: River
        email: river@example.com
```

命令：

```bash
switch options git.identity
switch use git.identity personal
```

内部展开：

```json
{
  "module": "git.identity",
  "selected": "personal",
  "desired": {
    "name": "River",
    "email": "river@example.com"
  }
}
```

#### 5.7.3 Toggle 和多选一的关系

`toggle` 只适合两种情况：

- `boolean` 状态，例如 `on -> off`。
- 模块显式声明轮转规则，例如 `off -> http -> off`。

对于超过两个候选值的 enum，不应该默认猜测用户想切到哪个值。必须通过 `use`、`set` 或显式 `cycle` 规则表达。

示例：

```json
{
  "toggle": {
    "type": "cycle",
    "values": ["work", "personal"]
  }
}
```

如果模块有三个以上 profile：

```text
work / personal / oss
```

但没有声明 `cycle`，执行 `switch toggle git.identity` 应该失败并提示：

```text
git.identity has multiple options: work, personal, oss
Use: switch use git.identity <option>
```

## 6. 插件系统

### 6.1 插件职责

插件扩展 `switch-cli` 应用本身能力，而不是直接定义某个状态如何切换。

MVP 协议承诺以下三类贡献点，其余能力（远程同步、TUI、telemetry、策略引擎等）由后续版本以新增 contributes 字段方式开放，未在 manifest schema 中出现的字段必须被核心忽略：

- `commands`：新增子命令或命令别名。
- `hooks`：在核心生命周期阶段插入回调。
- `secret_backends`：贡献 secret URI scheme 的解析后端。

### 6.2 插件 manifest

示例：

```json
{
  "schema_version": "1",
  "kind": "plugin",
  "name": "switch-plugin-tui",
  "version": "0.1.0",
  "description": "Interactive TUI for switch-cli",
  "entry": "switch-plugin-tui",
  "contributes": {
    "commands": [
      {
        "name": "ui",
        "description": "Open interactive UI"
      }
    ],
    "hooks": ["before_apply", "after_apply"]
  }
}
```

插件也可以贡献 Secret backend。Secret backend 属于插件能力的一种，但 core 仍然负责 secret ref 解析调度、权限检查、脱敏、日志策略和传递给模块的方式。

示例：

```json
{
  "schema_version": "1",
  "kind": "plugin",
  "name": "switch-plugin-1password",
  "version": "0.1.0",
  "description": "Resolve op:// secret references through 1Password CLI",
  "entry": "switch-plugin-1password",
  "contributes": {
    "secret_backends": [
      {
        "name": "op",
        "schemes": ["op"],
        "description": "Resolve op://vault/item/field references"
      }
    ]
  }
}
```

### 6.3 插件生命周期

核心暴露 4 个阶段的前后置 hook，外加错误 hook：

```text
config_load  before / after
detect       before / after
plan         before / after
apply        before / after
on_error
```

Hook 契约：

- **输入**：JSON via stdin，含阶段名、当前模块、context、对应阶段的中间结果（如 `after_detect` 含 state，`after_plan` 含 plan）。
- **输出**：JSON via stdout，包含 `ok: bool` 与可选 `reason`。
- **中止能力**：`before_*` hook 可以通过返回 `{"ok": false, "abort": true, "reason": "..."}` 中止本次主流程；`after_*` hook 不能中止。
- **修改能力**：MVP 阶段 hook **不能** 修改 plan、desired state 或 detect 结果。修改路径留给后续 mutating-hook 提案。
- **失败处理**：未声明 `abort: true` 的失败（非零退出、JSON 解析失败、stdout 缺 `ok`）记入日志和 `on_error`，但不阻断主流程，除非用户在配置中显式 `hooks.strict: true`。
- **可选性**：核心流程不能依赖任何插件 hook 才能正常运行。

### 6.4 插件和模块的边界

判断标准：

- 如果它定义某个状态如何被读写，它是模块。
- 如果它扩展 CLI 使用体验或生命周期，它是插件。

示例：

```text
proxy.system              -> module
git.identity              -> module
switch-plugin-tui         -> plugin
switch-plugin-gist-sync   -> plugin
switch-plugin-policy      -> plugin
```

## 7. 配置模型

### 7.1 配置来源与合并顺序

只用一张表表达：列表顺序 = 应用顺序，后者覆盖前者。

| 顺序 | 来源 | 位置 / 说明 |
|------|------|------|
| 1 | 内置默认值 | 编译进二进制 |
| 2 | 机器配置 | `~/.config/switch/machine.yaml`（macOS/Linux）<br>`%APPDATA%\switch\machine.yaml`（Windows） |
| 3 | 用户配置 | `$XDG_CONFIG_HOME/switch/config.yaml`，回退 `~/.config/switch/config.yaml`<br>`%APPDATA%\switch\config.yaml`（Windows） |
| 4 | 项目配置 | `./.switch.yaml`（trust 后生效）|
| 5 | 环境变量覆盖 | `SWITCH_<MODULE>__<FIELD>=...` |
| 6 | 命令行参数 | `--set <path>=<value>`、`--secret <path>=<ref>` 等 |

### 7.2 配置结构

示例：

```yaml
version: 1

profile: work

modules:
  git.identity:
    enabled: true
    desired:
      profile: work
    profiles:
      work:
        name: River
        email: river@company.com
      personal:
        name: River
        email: river@example.com

  proxy.system:
    enabled: true
    desired:
      mode: http
      http: http://127.0.0.1:7890
      https: http://127.0.0.1:7890

plugins:
  switch-plugin-tui:
    enabled: true
```

### 7.3 条件配置

跨平台配置需要条件表达式。

示例：

```yaml
modules:
  proxy.system:
    when:
      os: [darwin, windows]
    desired:
      mode: http
      http: http://127.0.0.1:7890
```

后续可以支持更复杂条件：

```yaml
when:
  all:
    - os: darwin
    - hostname: river-mbp
    - profile: work
```

### 7.4 配置合并规则

来源顺序见 7.1。合并规则：

- 标量值后者覆盖前者。
- 对象深度合并。
- 列表默认整体覆盖。
- 模块可在 schema 中声明字段级合并策略（如 `merge: append`）。

## 8. 命令设计

### 8.1 命令清单

带 `[MVP]` 标签的命令在第一版交付；其余按所属 Phase 出现。`set` 的路径用 `<module>:<field.path>` 形式，避免与模块命名空间（含 `.`）冲突。

| 命令 | Phase | 说明 |
|------|-------|------|
| `switch status [<module>]` | MVP | 展示当前状态摘要 |
| `switch list` | MVP | 列出已注册模块 |
| `switch get <module>` | MVP | 输出单个模块当前状态（结构化） |
| `switch on <module>` | MVP | 仅 boolean 模块；其余报错引导 `use` |
| `switch off <module>` | MVP | 同上 |
| `switch toggle <module>` | MVP | boolean 或显式声明 `toggle.cycle` 的模块 |
| `switch options <module>` | MVP | 列出 enum / profile 模块的候选值 |
| `switch use <module> <option>` | MVP | 选模块预定义选项，展开为完整 desired |
| `switch cycle <module>` | MVP | 按 `toggle.cycle` 顺序轮转 |
| `switch set <module>:<path> <value>` | MVP | 只改单字段 |
| `switch plan` | MVP | 只读：生成并展示执行计划 |
| `switch apply [--dry-run]` | MVP | 执行计划；`--dry-run` 复用 plan 流程，不产生副作用 |
| `switch diff` | MVP | 比较当前状态与期望状态 |
| `switch doctor [<module>]` | MVP | 检查依赖、权限、平台支持 |
| `switch config init / path / edit / validate` | MVP | 配置文件管理 |
| `switch trust [status|revoke]` | Phase 3 | 项目配置信任管理（见 11.2） |
| `switch revert <module>` | Phase 2 | 调用模块 `revert` 能力 |
| `switch config profile list / use <profile>` | Phase 3 | profile scope |
| `switch module list / info / install / uninstall / update / doctor` | Phase 2 | 外部模块管理 |
| `switch module test <path>` | Phase 2 | 开发者工具：跑 contract test（见 17.4） |
| `switch plugin list / info / install / uninstall / update` | Phase 4 | 插件管理 |

### 8.2 输出格式

默认输出适合人读；机器消费在任意命令后加 `--json`：

```bash
switch status --json
switch plan --json
switch apply --json
switch apply --dry-run --json
```

`switch plan` 是独立只读命令；`switch apply --dry-run` 与 `switch plan` 共用同一段计划生成流程（见 9.3），唯一区别是 `--dry-run` 的输出落在 apply 的人读/JSON 模板下，便于在准备执行前加上预览参数。

## 9. 执行流程

### 9.1 status

```text
load config
resolve modules
for each module:
  check condition
  select provider
  detect current state
  format result
print status
```

### 9.2 diff

```text
load config
resolve desired state
detect current state
compare current and desired
print diff
```

### 9.3 plan / apply 共享流程

`plan`、`apply --dry-run`、`apply` 共享同一段「准备阶段」，差别仅在执行阶段是否进入：

```text
[准备阶段 — 三者共用]
load config
validate config
resolve enabled modules (apply filter by --module flags)
detect current states
build plans

[输出与执行 — 按命令分支]
switch plan              -> render plans (human/json)  → exit
switch apply --dry-run   -> render plans (human/json)  → exit
switch apply             -> render plans
                            ask confirmation when needed
                            execute plans
                            detect final states
                            write state history
                            print summary
```

任何分支都不会在执行阶段重新生成 plan：传入 executor 的是准备阶段已构造的同一份 plan 对象。

### 9.4 toggle

```text
load module
detect current state
resolve toggle rule
derive desired state
build plan
apply plan
detect final state
print summary
```

### 9.5 use

```text
load module
load options from schema or config profiles
validate requested option
expand option to desired state
detect current state
build plan
apply plan
detect final state
print summary
```

## 10. 状态历史和回滚

本地 jsonl 历史，用于审计与回滚。路径：

- macOS / Linux：`$XDG_STATE_HOME/switch/history.jsonl`，回退 `~/.local/state/switch/history.jsonl`
- Windows：`%LOCALAPPDATA%\switch\history.jsonl`

并发写入：每次 apply 在内存中构造完整 `Vec<HistoryEntry>` 后，以 `O_APPEND` + 单次 write(2) 整块写入；同时对历史文件持文件锁（`flock` / `LockFileEx`）防止两条 `switch apply` 同时写入造成行撕裂。无锁的并发查看（如 status）始终读取 append-only 文件，不受影响。

记录示例：

```json
{
  "time": "2026-05-23T10:00:00Z",
  "operation": "apply",
  "module": "git.identity",
  "before": { "profile": "personal" },
  "desired": { "profile": "work" },
  "after":   { "profile": "work" },
  "ok": true
}
```

回滚策略：

- 模块支持 `revert` 时优先调用。
- 不支持但历史中保留了 previous state 时，核心尝试生成反向 plan（仅当所有字段都属于可安全反向应用的类型）。
- 否则明确提示用户手工处理，不做猜测性回滚。

## 11. 安全模型

### 11.1 明确权限边界

模块 manifest 必须声明可能需要的权限：

```json
{
  "permissions": {
    "filesystem": ["read_config", "write_config"],
    "commands": ["networksetup"],
    "secrets": ["proxy_token"],
    "network": false,
    "admin": false
  }
}
```

### 11.2 自动执行限制

来自项目目录的 `.switch.yaml` 不应该默认自动执行。

建议机制：

```bash
switch trust
switch trust status
switch trust revoke
```

信任记录与目录路径和配置文件 hash 绑定。

### 11.3 命令执行

模块返回的命令应该使用 argv 数组，而不是 shell 字符串。

推荐：

```json
["networksetup", "-setwebproxystate", "Wi-Fi", "on"]
```

避免：

```json
"networksetup -setwebproxystate Wi-Fi on"
```

这样可以减少 shell 注入风险。

### 11.4 Secret

设计目标：配置中只存引用不存明文；核心统一解析、脱敏、控制传递阶段；模块按 schema 声明 secret 字段并按需获取值。延期能力（系统 keychain、1Password、插件 backend 等）参考附录 A。

#### 11.4.1 概念

- **secret ref**：配置中的引用，例如 `env:GITHUB_TOKEN`。
- **secret backend**：解析 ref 的组件（MVP 只内置 `env:` 与 `cmd:`）。
- **secret value**：运行时解析出来的明文。

#### 11.4.2 配置格式

显式对象（规范形式）：

```yaml
modules:
  app.config_file:
    desired:
      auth:
        token:
          secret:
            ref: env:APP_TOKEN
            optional: false
            encoding: text       # text | base64 | json，默认 text
            expose: apply        # never | plan | apply，默认 apply
```

紧凑写法 `secret_ref: env:APP_TOKEN` 在配置加载后规范化为上述对象。`expose` 控制明文最早可在哪个阶段展开，`never` 表示模块只能拿到引用与存在性。

#### 11.4.3 Secret ref URI

```text
env:NAME      从环境变量读取               [MVP]
cmd:name      调用 allowlist 中的命令       [MVP]
prompt:name   apply 时交互式输入            [MVP, 仅 TTY]
file:/path    本地文件                      [Phase 5]
keychain:...  macOS Keychain                [Phase 5]
op://...      1Password CLI                 [Phase 5，插件 backend]
```

#### 11.4.4 Backend 接口

```text
metadata     返回 scheme 列表与能力声明
doctor       检查依赖（如 op CLI 是否存在）
exists       检查 secret 是否存在，不返回明文
resolve      解析 secret 值
fingerprint  可选；与 resolve 独立，返回加盐不可关联的指纹
```

`fingerprint` 是独立操作，不经过 `resolve`，因此 `diff` 比较 fingerprint 不构成 secret 解析（与 11.4.6 一致）。只有 backend 能提供加盐指纹时才默认启用，否则关闭以防止低熵 secret 被反推。

#### 11.4.5 模块 schema 声明

```json
{
  "properties": {
    "auth": {
      "type": "object",
      "properties": {
        "token": { "type": "string", "x-switch-secret": true, "x-switch-expose": "apply" }
      }
    }
  }
}
```

模块 manifest 同时声明 secret 权限（用于外部模块授权检查）：

```json
{
  "permissions": {
    "secrets": [
      { "name": "app_token", "reason": "Authenticate to app", "required": true }
    ]
  }
}
```

#### 11.4.6 解析时机

| 阶段       | 是否解析 secret |
|------------|------------------|
| config load | 否，只解析 YAML/JSON 结构 |
| validate   | 否，校验 ref 语法与 schema |
| doctor     | 否（可选检查存在性，调用 backend.exists） |
| status     | 否 |
| diff       | 否；仅比较 ref 与 backend.fingerprint |
| plan       | 默认否；仅当字段 `expose: plan` 时才解析必要 secret |
| apply      | 是，按最小化原则解析 `expose: apply` 与 `expose: plan` 字段 |
| history    | 否；只记录 ref / redacted / fingerprint |

这避免普通 `switch status` 意外触发 keychain、1Password 或外部命令。

#### 11.4.7 传递给模块的方式

按字段 schema 的 `x-switch-expose` 与当前阶段决定模块收到什么：

| 阶段允许？ | 模块收到 |
|------------|----------|
| 当前阶段 < schema 声明的最早展开阶段 | `{"ref": "env:X", "redacted": "********", "available": true}` |
| 当前阶段 ≥ 最早展开阶段，且模块 manifest 声明了对应 secret 权限 | `{"value": "...明文...", "redacted": "********"}` |
| 当前阶段允许，但模块未声明权限 | 报 `SecretPolicyError`，不调用模块 |

不引入「空壳 handle」中间态：要么给引用 + 是否存在，要么给明文，决定权完全在 schema + manifest，不需要模块回调核心。

模块约束：

- 不得把明文写入 stdout、stderr、plan、history 或错误信息。
- 模块返回值经过核心二次脱敏。
- 外部模块未在 manifest 中声明对应 secret 权限时不会收到明文。

#### 11.4.8 Diff、Plan、History 脱敏

```text
app.config_file.auth.token
  before: secret_ref(env:OLD_TOKEN)
  after:  secret_ref(env:APP_TOKEN)
```

ref 未变但指纹变化时（需 backend 支持加盐指纹）：

```text
app.config_file.auth.token
  ref: env:APP_TOKEN
  fingerprint: changed
```

History 仅保存 ref / redacted / fingerprint，三者均不构成明文。

#### 11.4.9 缓存

进程内内存缓存，TTL = 当前命令执行期间，key = `backend + ref`。不写磁盘、不入 crash report、不入 debug log。跨命令缓存留给 backend 自身（如 1Password CLI session）管理。

#### 11.4.10 `cmd:` backend

```yaml
secrets:
  commands:
    app_token:
      command: ["op", "read", "op://dev/app/token"]
      timeout_ms: 3000
      trim_trailing_newline: true

modules:
  app.config_file:
    desired:
      token:
        secret_ref: cmd:app_token
```

规则：argv 数组（不允许 shell 字符串）；只能引用 allowlist 命名；stdout = value，stderr 仅诊断且经过脱敏；超时 / 非零退出 / 空输出转为 `SecretError`。如可能访问网络需在 `secrets.commands.<name>.network: true` 显式声明，`doctor` 与 `apply --dry-run` 展示。

#### 11.4.11 `prompt:` 与 CLI 覆盖

- `prompt:name`：apply 阶段交互输入，禁用回显，仅进程内存。非 TTY 环境必须用 CLI 覆盖。
- CLI 覆盖只接受 ref：`switch apply --secret <module>:<field.path>=env:NAME`。
- 明文覆盖 `--unsafe-secret-value` 仅供调试，且不进入 history / shell completion / 日志。

#### 11.4.12 错误类型

```text
SecretRefError       ref 格式错误
SecretBackendError   backend 不存在或不可用
SecretNotFoundError  secret 不存在
SecretAccessError    无权限读取
SecretResolveError   解析失败
SecretPolicyError    当前阶段不允许展开
```

错误输出脱敏：只能出现 ref 与原因，不允许出现 secret 值片段或长度。

#### 11.4.13 MVP 范围

实现：`env:` / `cmd:` / `prompt:` backend；schema 标记 secret 字段；status/diff/plan/history 完整脱敏；apply 按需解析；CLI ref 覆盖。

延期到附录 A：系统 Keychain / Credential Manager / Secret Service、1Password / pass、插件贡献 backend、加密本地缓存、secret 写入与轮换。

## 12. 平台适配

### 12.1 平台识别

核心 context 应包含：

```json
{
  "os": "darwin",
  "arch": "arm64",
  "shell": "zsh",
  "hostname": "river-mbp",
  "user": "river",
  "cwd": "/Users/river/project"
}
```

### 12.2 Provider 选择

选择顺序：

```text
用户显式指定 provider
模块按当前 OS 推荐 provider
第一个可用 provider
失败并提示 doctor 信息
```

### 12.3 Provider 能力

Provider 需要声明：

- 支持的平台。
- 依赖命令。
- 是否需要管理员权限。
- 是否支持 detect。
- 是否支持 revert。
- 已知限制。

## 13. 错误处理

错误应该分层：

```text
ConfigError       配置错误
SchemaError       schema 校验失败
ModuleError       模块执行失败
ProviderError     平台 provider 失败
PermissionError   权限不足
DependencyError   缺少依赖命令
SecretError       secret 引用、权限、backend 或解析失败
StateDriftError   应用后状态和期望状态不一致
```

CLI 输出需要直接告诉用户：

- 哪个模块失败。
- 失败原因。
- 可以执行什么命令诊断。
- 是否发生了部分变更。

示例：

```text
proxy.system failed: missing dependency "networksetup"
Run: switch doctor proxy.system
Changed: no
```

## 14. 技术选型

核心采用 **Rust**。理由：单文件分发体验最好；类型系统适合表达 state / plan / schema；跨平台 syscall 与命令执行成熟；与「可靠 CLI」的目标契合。

依赖选型：

| 用途 | 库 |
|------|----|
| CLI 参数 | `clap` |
| 序列化 | `serde` |
| JSON schema | `schemars` + `jsonschema` |
| 配置目录 | `directories` |
| 进程调度 | `tokio` |
| 子进程 | `tokio::process` |

外部模块与插件通过 stdin/stdout + JSON 协议通信，不限制语言；Go、TypeScript、Python 等都可实现。Go / TypeScript 的选型讨论保留在附录 B 的 ADR 中，不再作为正文备选。

## 15. 初期 MVP

### 15.1 MVP 目标

第一阶段只实现最小闭环：

- 配置加载。
- 模块注册。
- 当前状态读取。
- plan / dry-run。
- apply。
- 内置 2 到 3 个模块。
- JSON 输出。

### 15.2 MVP 内置模块

选择标准：覆盖三类状态形态（boolean / enum / object），并尽量回避 OS-specific provider 的复杂度，让 MVP 聚焦核心模型而非 provider 抽象。

- **`git.identity`**（enum / profile）：跨平台、无 admin、状态可读可写、能验证 profile 展开机制。
- **`shell.env`**（object）：高频，验证对象状态、`set` 字段路径、深度合并；session scope 留到 Phase 3。
- **`app.config_file`**（boolean / enum）：通过启用/禁用配置片段（include / symlink / `# BEGIN switch-cli` 块）验证 boolean toggle 与对象 diff，无需 admin、跨平台一致，最适合 MVP 验证 provider 抽象的最小路径。

`proxy.system` 推迟到 Phase 2：它要求 3 个 OS provider、管理员权限路径以及独立的 provider 测试基础设施，对 MVP 来说成本与价值不匹配，应在核心稳定后再投入。

### 15.3 MVP 命令

MVP 命令与 `Phase` 标签见 8.1 一节，此处不再重复。

### 15.4 暂缓能力

以下能力不进入第一版（详见 20 章演进路线）：

- `proxy.system` 等需要多 OS provider 的系统级模块。
- 外部模块协议、模块安装/索引。
- 项目目录自动 hook、`.switch.yaml` trust 流程。
- 插件系统、TUI、远程同步。
- 复杂策略引擎、签名校验、分布式配置同步。
- 非 MVP secret backend（keychain、1Password、pass、Secret Service、插件 backend）。

待核心 state / plan / apply 模型稳定后逐项开放。

## 16. 推荐仓库结构

如果使用 Rust：

```text
switch-cli/
  Cargo.toml
  crates/
    switch-cli/
      src/
        main.rs
        commands/
    switch-core/
      src/
        config/
        module/
        plugin/
        provider/
        planner/
        executor/
        state/
    switch-modules/
      src/
        git_identity/
        shell_env/
        proxy_system/
  schemas/
    module.schema.json
    plugin.schema.json
    config.schema.json
  docs/
    design.md
```

如果使用 Go：

```text
switch-cli/
  go.mod
  cmd/
    switch/
  internal/
    config/
    module/
    plugin/
    provider/
    planner/
    executor/
    state/
  modules/
    builtin/
  schemas/
  docs/
```

## 17. 测试策略

### 17.1 核心测试

覆盖：

- 配置加载和合并。
- schema 校验。
- module registry。
- plan 生成。
- dry-run 不产生副作用。
- JSON 输出稳定性。

### 17.2 模块测试

每个模块至少测试：

- detect 解析。
- desired state 校验。
- current -> desired 的 plan。
- apply 后的状态验证。
- 不支持平台时的错误。

### 17.3 Provider 测试

Provider 需要隔离真实系统。

优先做：

- command builder 测试。
- fixture 输出解析测试。
- mock executor 测试。

真实系统集成测试只在特定 CI runner 或本机手动执行。

### 17.4 Contract Test

外部模块必须能通过协议测试，由开发者子命令 `switch module test <path>` 执行（命令已在 8.1 中登记为 Phase 2 开发者工具）：

```bash
switch module test ./switch-module-example
```

测试内容：

- `metadata` 输出合法。
- `schema` 输出合法。
- `detect` 输出符合 schema。
- `plan` 输出动作合法。
- 错误输出格式稳定。

## 18. 版本和兼容性

需要为以下内容单独设版本：

- CLI 版本。
- 配置文件 schema 版本。
- 模块协议版本。
- 插件协议版本。
- 每个模块自身版本。

配置文件示例：

```yaml
version: 1
```

协议输入示例：

```json
{
  "schema_version": "1"
}
```

兼容策略：

- patch 版本不能破坏协议。
- minor 版本可以新增字段。
- major 版本才允许破坏性变更。
- 未识别字段默认忽略，除非 schema 明确禁止。

## 19. 用户体验细节

### 19.1 状态展示

`STATE` / `DESIRED` 列只放状态值；漂移与多字段对象的应用进度分别由 `DRIFT` 与 `PROGRESS` 列承载，避免值与元状态混在一列。

```text
MODULE             STATE       DESIRED     DRIFT  PROGRESS
git.identity       work        work        no     100%
app.config_file    disabled    enabled     yes    0%
shell.env          object      object      yes    3/5
```

- `STATE` / `DESIRED`：boolean/enum/profile 展示具体值；object/list 展示类型占位符，详细 diff 走 `switch diff`。
- `DRIFT`：当前值与期望值是否一致，`yes` / `no` / `unknown`（detect 失败）。
- `PROGRESS`：对象状态展示「已与期望一致的字段数 / 期望字段总数」；标量状态用百分比 0% / 100%。

### 19.2 Apply 摘要

```text
Plan:
  app.config_file
    - enable include line in ~/.gitconfig
    - write ~/.gitconfig.d/work.inc

Summary:
  changed: 1
  unchanged: 2
  failed: 0
```

### 19.3 失败展示

```text
Failed:
  app.config_file
    reason: target file "~/.gitconfig" is not writable
    next: switch doctor app.config_file
```

### 19.4 命令名风险

`switch` 是一个直观命令名，但在部分 shell 或语言上下文里可能有关键字冲突。发布时可以考虑：

- 主命令仍叫 `switch`。
- 同时提供短命令 `sw` 或 `swx`。
- 文档中说明 alias 配置方式。

## 20. 后续演进路线

### Phase 1: Core

- CLI 框架。
- 配置加载。
- 内置模块注册。
- status / diff / plan / apply / dry-run。
- JSON 输出。

### Phase 2: Module Protocol

- 外部模块 manifest。
- 外部模块 JSON 协议。
- 模块安装和卸载。
- contract test。

### Phase 3: Project Scope

- `.switch.yaml`。
- trust / allow 机制。
- 按目录状态切换。
- shell hook 原型。

### Phase 4: Plugin System

- 插件 manifest。
- 插件命令注册。
- 生命周期 hooks。
- 插件配置。

### Phase 5: Ecosystem

- 模块索引。
- 插件索引。
- 签名和校验。
- TUI。
- 远程同步。
- 高级 Secret backend，例如系统 Keychain、1Password、pass、Secret Service 和插件贡献的 backend。

## 21. 当前建议结论

`switch-cli` 最重要的设计取舍是：

1. 核心围绕 state / desired state / plan / apply 建模。
2. 模块负责具体状态，插件负责扩展 CLI。
3. 跨平台差异由 provider 承担。
4. 外部扩展优先使用进程 + JSON 协议，而不是语言级动态加载。
5. 第一版先做少量高质量内置模块，把状态模型跑通。

只要这几个边界保持清晰，后续无论是做系统设置、开发环境、项目 profile、自动 hook 还是插件生态，都能比较自然地扩展。

## 附录 A：延期的 Secret 能力

以下能力的协议设计草稿留作未来参考，但 MVP 不实现，正文不再展开。

### A.1 系统 Keychain / Credential Manager / Secret Service

后续可作为内置 backend：

- `keychain:service/account`（macOS Security framework）
- `credential-manager:target`（Windows `CredRead`）
- `secret-service:collection/item#attribute`（freedesktop Secret Service）

### A.2 插件贡献的 Secret backend

core 维护 secret backend registry，插件通过 manifest 的 `contributes.secret_backends` 注册 scheme：

```json
{
  "contributes": {
    "secret_backends": [
      {
        "name": "op",
        "schemes": ["op"],
        "requires": { "commands": ["op"] }
      }
    ]
  }
}
```

调用协议（草稿）：

```bash
switch-plugin-1password secret resolve --input -
```

```json
{
  "schema_version": "1",
  "operation": "secret.resolve",
  "ref": "op://dev/proxy/password",
  "context": { "phase": "apply", "module": "app.config_file" }
}
```

安全约束：stdout 只能是 JSON；stderr 由 core 二次脱敏；backend 不得绕过 schema 中的 `expose` 阶段约束；如访问网络需在 manifest 中声明。

### A.3 加密本地缓存 / Secret 写入 / 轮换

跨命令明文缓存（哪怕加密）、模块通过 backend 写入 secret、自动轮换 secret 都属于扩大攻击面与责任边界的能力。它们引入的复杂度足以单独立一个章节，待 MVP 稳定且有明确用例后再设计。

### A.4 `file:` / `pass:` 等本地 backend

```text
file:/absolute/path             本机私有文件，需要严格权限校验
pass:path/to/item#field          pass 密码存储
```

均属于扩展项，不进入 MVP。

## 附录 B：语言选型 ADR

### Context

设计早期考虑过 Rust、Go、TypeScript 三种核心语言。最终选 Rust 进入正文，但保留比较以便未来翻案：

| 语言 | 优势 | 劣势 |
|------|------|------|
| Rust | 单文件分发；类型系统强；跨平台 syscall 成熟；适合可靠 CLI | 编译期相对慢；模块开发门槛较高 |
| Go | 编译分发简单；标准库 OS 操作齐全；生态成熟 | 类型系统对 sum type / 状态机表达较弱 |
| TypeScript | 插件生态成熟（oclif）；JSON schema 支持好 | 需要 Node runtime；单文件分发体验差 |

### Decision

核心二进制使用 Rust；外部模块和插件以独立进程通过 JSON 协议交互，因此插件作者可以自由选择 Go / TypeScript / Python / Shell 等。

### Consequences

正面：发布物为单二进制，无运行时依赖；模块协议与核心语言解耦，生态保持开放。

负面：核心贡献门槛比 Go 高；与 Node 生态既有工具的整合需要额外胶水。

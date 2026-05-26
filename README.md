# switch-cli

`switch-cli` is a local profile/state switcher for AI CLI tools. The MVP is
focused on Claude Code and OpenAI Codex profile switching while keeping the core
model generic: app definitions describe targets, and trusted core handlers apply
structured profile records to local files.

The design document is in [docs/design.md](docs/design.md). Current acceptance
evidence is tracked in [docs/acceptance.md](docs/acceptance.md), with real-app
checks in [docs/manual-verification.md](docs/manual-verification.md).

## Current Implementation Status

This repository now contains a Rust CLI foundation with:

- built-in Claude and Codex app definitions embedded in the binary;
- `apps`, `add`, `edit`, `list`, `show`, `use --dry-run`, `use`, `status`,
  `backup list`, `restore-target`, `remove`, `detach`, `doctor`, and
  `config path` command surfaces;
- user app definitions loaded from `apps.d/*.yaml`, with path boundary checks;
- safe override support for app definitions, currently limited to appending
  process probe names and overriding field defaults / sensitivity flags;
- static profile support for Claude `env_injection`, Codex `file_template`,
  and user-defined `file_template` targets rendered from Definition templates;
- `import-current` for Claude env profiles, Claude file-backed OAuth captures,
  Codex API-key profiles, Codex file-backed ChatGPT OAuth captures, and
  user-defined OAuth captures that use trusted file, JSON subtree, or managed
  TOML handlers;
- `secret_entry` capture sources backed by macOS Keychain generic passwords,
  using Security.framework with a fixture backend for tests; Claude OAuth import
  prefers the `Claude Code-credentials` Keychain entry when available;
- file-backed `oauth_capture` replay, identity verification, and writeback for
  Codex, Claude, and Definition-driven JSON/file/TOML sources;
- OAuth required identity mismatches fail the switch; optional identity
  mismatches are recorded as history warnings without blocking;
- defensive backups for managed file, JSON subtree, TOML managed-path, and
  Keychain targets;
- backup manifests preserve whether a target requires the app to be stopped;
  `restore-target` enforces the same OAuth process-safety rule and ignores
  `--allow-running` for those backups;
- `restore-target` validates backup manifests before writing: schema/app match,
  stored blob names, resolved target path boundaries, target type, and blob
  sha256 must all pass;
- OAuth profiles are checked for current-platform capture completeness before
  `use` writes back, creates a backup, or opens a pending-switch journal;
- backup creation hardlinks duplicate blobs when the filesystem and permissions
  allow it; `doctor` reports backup count, deduplicated inode bytes, and logical
  bytes per app;
- pending-switch journal creation, cleanup, interrupted-state reporting,
  bookkeeping-stage recovery, backup-backed rollback, and restore-target
  bookkeeping recovery before the next same-app write command;
- process-name probing for target apps; static writes can opt into
  `--allow-running`, while OAuth I/O still refuses when a target process is
  detected unless `--assume-app-stopped --yes` is explicitly supplied;
  `AppRunning` diagnostics include PID, process start time, and command line;
  successful `--assume-app-stopped --yes` escape hatches are recorded in
  history warnings for audit;
- POSIX advisory locks for profile writes, app writes, shared state writes, and
  target files; `import-current` also locks the live sources it reads before
  capturing profile data;
- active profile resolved-target snapshots, with `status` drift reporting when
  a Definition-declared path environment changes and `use --accept-resolved-change`
  to explicitly accept the new target locations;
- target path checks follow existing symlink parents and reject paths that
  resolve outside the current user's home;
- `status` reports `matched-with-overrides` when managed targets match but
  Definition-declared higher-priority authentication sources may override them;
- `edit` opens profile YAML fragments under `state/edit` using `$VISUAL` or
  `$EDITOR`, validates immutable fields and schema, and removes the fragment
  after use;
- `doctor` reports the required profiles.yaml secret-leak surface check and
  warns when files or directories under `SWITCH_CLI_HOME` have widened Unix
  permissions;
- `doctor <app>` also reports process probe matches, active profile metadata,
  identity, OAuth capture completeness, and resolved managed target paths
  without secret values, and supports `--json` for scriptable diagnostics;
- app definitions can add `doctor.json_fields` diagnostics such as auth mode or
  stale timestamp warnings, plus `doctor.json_object_schemas` warnings for
  upgraded app JSON state, without hardcoding built-in app names;
- release archives are produced by `scripts/package-release.sh` and include
  read-only copies of the built-in Claude and Codex App Definitions for audit;
  runtime still uses the definitions embedded in the binary;
- `scripts/manual-evidence.sh` initializes a redacted, read-only manual
  verification evidence file for release-candidate real-app checks;
- secret argv rejection and sanitized output;
- basic state tracking under `~/.switch-cli` or `SWITCH_CLI_HOME`;
- CI and release workflows for Linux x86_64, macOS x86_64, and macOS arm64
  binaries.

Remaining unproven items require real Claude/Codex application runs on macOS
and Linux; those checks are tracked in `docs/manual-verification.md`.

## Install From Source

```bash
cargo install --path .
```

## Quick Start

Use a test home first:

```bash
export SWITCH_CLI_HOME="$HOME/.switch-cli-dev"

switch-cli apps
switch-cli config path

printf '%s' "$ANTHROPIC_AUTH_TOKEN" | switch-cli add claude glm \
  --kind env_injection \
  --field base_url=https://open.bigmodel.cn/api/anthropic \
  --field models.default=glm-4.6 \
  --secret-field auth_token=@stdin

switch-cli use claude-glm --dry-run
switch-cli use claude-glm --yes
switch-cli status claude
```

Codex API-key profile:

```bash
printf '%s' "$OPENAI_API_KEY" | switch-cli add codex openai \
  --kind file_template \
  --field model=gpt-5-codex \
  --field model_provider=openai \
  --secret-field api_key=@stdin
```

## Safety Notes

- Static secrets for `env_injection` and `file_template` are stored in
  `profiles.yaml` with file mode `0600`; existing target files with wider
  permissions are tightened on write, while stricter owner-only permissions are
  preserved.
- Do not commit `~/.switch-cli/profiles.yaml`.
- Commands redact sensitive fields in human and JSON output.
- Quit target apps before any write operation when practical. OAuth capture
  reads and writes require the target app to be stopped; `--allow-running` is
  intentionally ignored for those operations, with only the audited
  `--assume-app-stopped --yes` escape hatch for process-probe false positives.
  Static `env_injection` and `file_template` writes can use `--allow-running`,
  but stopping the app first is still recommended because the app may rewrite
  the same config files.

## Development

```bash
scripts/verify-local.sh
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines,
[SECURITY.md](SECURITY.md) for vulnerability reporting, and
[docs/release.md](docs/release.md) for the binary release workflow.

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license

# any-switch

`any-switch` switches local app profiles and state.

Use it when one app has several local setups and you want to move between them
without hand-editing files, copying tokens, or remembering which config entries
belong together.

Examples:

- switch Claude Code between a personal account and a work account;
- switch Codex between ChatGPT OAuth and an API-key provider;
- switch any supported local tool between different endpoints, models, accounts,
  workspaces, or other file-backed state.

The first built-in apps are Claude Code and OpenAI Codex. The tool itself is not
AI-specific: app definitions describe what local state can be captured and
restored, and `any-switch` handles the backup, redaction, drift checks, and
write safety around that state.

## What It Does

`any-switch` keeps named profiles on your machine. A profile is the local state
you want an app to use, such as:

- account identity and OAuth credential state;
- API keys and provider settings;
- model, endpoint, and environment settings;
- JSON, TOML, file, Keychain, or environment fragments declared by an app
  definition.

When you switch profiles, `any-switch` shows the plan, creates backups, writes
only the declared targets, and avoids printing secret values.

## Install

The recommended installation path is source compilation through Cargo or npm.
This avoids distributing unsigned macOS and Windows binaries.

The current release scope is a macOS-evidenced stage release: macOS Claude
OAuth import has real local evidence, while broader restart checks plus Linux
and Windows real-app evidence are tracked as follow-up work. This release does
not claim full `docs/design.md` section 13 coverage.

Install Rust first:

```bash
rustup toolchain install 1.95.0
```

Then install with Cargo:

```bash
cargo install any-switch --locked
```

You can also install through npm. The npm package compiles this Rust project
locally with Cargo during installation:

```bash
npm install -g any-switch
any-switch --version
```

For local development, build from a checkout:

```bash
cargo install --path .
```

Check the installation:

```bash
any-switch --version
any-switch doctor
```

## Quick Start

See the apps that this build knows about:

```bash
any-switch apps
```

Capture the current login or state of an app as a profile:

```bash
any-switch import-current <app> personal
```

List saved profiles:

```bash
any-switch list
```

Switch to a profile:

```bash
any-switch use <profile-id> --dry-run
any-switch use <profile-id>
```

Check what is active:

```bash
any-switch status <app>
any-switch doctor <app>
```

Built-in examples:

```bash
any-switch import-current codex personal
any-switch import-current claude work --kind oauth_capture
any-switch use codex-personal
```

## Common Workflows

### Save the Current App State

Use `import-current` after you have already logged in or configured the target
app in the normal way:

```bash
any-switch import-current <app> personal
```

This is the right flow for OAuth-based app state, because the app owns the real
login process. `any-switch` captures the local state after login; it does not log
in for you. For built-in apps this may look like
`any-switch import-current codex personal` or
`any-switch import-current claude work --kind oauth_capture`.

### Add a Static Profile

Use `add` when the profile can be described with fields such as an API key,
model, provider, or base URL. Field names are defined by the selected app
definition and profile kind:

```bash
any-switch add <app> work --kind <kind> --field key=value
```

Built-in Codex example:

```bash
any-switch add codex openai --kind file_template \
  --secret-field api_key=@prompt \
  --field model=gpt-5-codex \
  --field model_provider=openai
```

Secret fields can be read from a masked interactive prompt, stdin, an
environment variable, or a local file:

```bash
--secret-field api_key=@prompt
--secret-field api_key=@stdin
--secret-field api_key=@env:OPENAI_API_KEY
--secret-field api_key=@file:~/secrets/openai-api-key
```

Use `@prompt` for normal interactive setup. Use `@env:NAME`, `@stdin`, or
`@file:PATH` when scripting.

### Preview Before Writing

Use `--dry-run` to inspect a switch without changing local files:

```bash
any-switch use <profile-id> --dry-run
```

### Recover a Target From Backup

Backups are created before managed targets are overwritten. Inspect them with:

```bash
any-switch backup list
```

Restore an app from a specific backup when needed:

```bash
any-switch restore-target <app> <backup-id>
```

`restore-target` restores live app state from the backup. It does not mark a
profile active, so run `any-switch status <app>` afterwards to inspect the
result. Confirm the restore by typing `yes` in an interactive terminal, or add
`--yes` in scripts and CI.

## Safety Notes

- Profiles are stored under `~/.any-switch` by default. This directory can
  contain static secrets, OAuth captures, and defensive backups. Keep it out of
  cloud-synced folders such as iCloud Drive, Dropbox, OneDrive, and Google
  Drive; `doctor` warns when it detects a known sync root. Set `ANY_SWITCH_HOME`
  to an absolute path under your home directory if you want a separate state
  directory.
- Secret values are redacted from normal command output and JSON output.
- Do not commit `~/.any-switch` or any generated profile/capture files.
- Quit the target app before OAuth or process-sensitive operations. OAuth
  credentials can rotate while the app is running, so `--allow-running` does not
  apply to those operations.
- Use `--assume-app-stopped` only when the app is actually stopped but process
  detection reports a false positive; confirm with `--yes` in scripts or by
  typing `yes` in an interactive terminal. Do not pass it preemptively: if no
  matching process was detected, any-switch rejects the flag and asks you to
  retry without it.
- For static file or environment profiles, `--allow-running` is available,
  but stopping the app first is still safer because the app may rewrite its own
  config files.
- `--yes` confirms high-risk actions non-interactively, such as `use`,
  `restore-target`, `remove`, or `--assume-app-stopped`. In an interactive
  terminal you may omit `--yes` and type `yes` at the prompt instead. Neither
  confirmation path disables identity checks, backup checks, path checks, locks,
  schema validation, or secret redaction. `add` and ordinary `import-current`
  do not accept `--yes` because they create or capture state instead of
  overwriting the target app. `import-current --yes` is valid only with
  `--assume-app-stopped`.

## Troubleshooting

Start with:

```bash
any-switch doctor
any-switch doctor <app>
any-switch status <app>
```

Useful next steps:

- `IdentityMissing`: the app does not currently expose the identity fields that
  the profile kind requires. Make sure the app is logged in, then run
  `doctor <app>` again.
- `TargetMissing`: run `doctor <app>` and look for `definition_capture_source`
  rows. They show whether the current platform credential source, such as a
  Keychain entry or credentials file, is `exists`, `missing`, or `warning`
  because existence could not be confirmed. If a warning row includes `hint:`,
  follow that source-specific next step first. For macOS Keychain checks, avoid
  `security find-generic-password -w` unless you intentionally need to reveal
  the credential.
- `DriftBeforeWriteback`: the live app state no longer matches the active
  profile. Run `status <app>` to inspect it. If the live state is valuable,
  import it as a new profile before switching away.
- `AppRunning`: quit the target app and retry. Use `--assume-app-stopped` only
  for a process-detection false positive, then confirm with `--yes` or the
  interactive prompt.
- `ImportAmbiguous`: pass `--kind <kind>` or clean up the app's current auth
  files so only one import rule matches.

## Custom Apps

`any-switch` can be extended with app definitions under `apps.d/*.yaml`.
Definitions declare the local targets an app uses and which trusted handlers can
capture or write them. This lets new apps reuse the same safety model without
adding app-specific branches to the core CLI.

For the full model, see [docs/design.md](docs/design.md).

To inspect or customize app definitions:

```bash
any-switch apps show <app>
any-switch apps export <app> --source system
any-switch apps export <app> --source resolved
any-switch apps export <app> --as override --output ~/.any-switch/overrides.d/<app>.yaml
any-switch apps validate ~/.any-switch/overrides.d/<app>.yaml
```

## More Docs

- [docs/user-guide.md](docs/user-guide.md): practical user guide with common
  workflows, safety flags, and troubleshooting.
- [docs/design.md](docs/design.md): architecture and safety model.
- [docs/manual-verification.md](docs/manual-verification.md): real-app checks
  that cannot be fully proven in CI.
- [docs/acceptance.md](docs/acceptance.md): acceptance coverage.
- [docs/evidence-followups.md](docs/evidence-followups.md): deferred manual
  evidence tracking before full section 13 coverage is claimed.
- [docs/release.md](docs/release.md): release packaging and signing.
- [CHANGELOG.md](CHANGELOG.md): user-facing release notes.
- [CONTRIBUTING.md](CONTRIBUTING.md): development and contribution rules.
- [.github/ISSUE_TEMPLATE/release_checklist.yml](.github/ISSUE_TEMPLATE/release_checklist.yml):
  release evidence checklist for maintainers.
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md): community standards.
- [SECURITY.md](SECURITY.md): vulnerability reporting.

## Development

Run the local verification script before opening a pull request:

```bash
scripts/verify-local.sh
```

## License

MIT. See [LICENSE](LICENSE).

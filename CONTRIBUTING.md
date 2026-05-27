# Contributing

`any-switch` is intentionally conservative because it edits local credential and
configuration files. Contributions should keep that bias.

## Development Setup

```bash
rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy
cargo fmt -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
```

For the full local gate, prefer `scripts/verify-local.sh`; it adds shell syntax
checks, patch whitespace checks, offline source-package verification, release
archive packaging, and checksum verification.

Before publishing or changing package contents, also run
`scripts/verify-packages.sh`. It verifies the Cargo package list, npm tarball
contents, and temporary installs from both Cargo and the packed npm tarball.

The repository includes `.editorconfig` and `.gitattributes`; keep UTF-8, LF
line endings, final newlines, and the documented indentation settings when
editing non-Rust files. Rust formatting is enforced by `cargo fmt`, and
`scripts/verify-local.sh` also runs `git diff --check` for patch whitespace.

Use a disposable any-switch home while testing manually:

```bash
export ANY_SWITCH_HOME="$HOME/.any-switch-smoke"
export CODEX_HOME="$PWD/.smoke-codex"
```

Do not commit local agent configuration, generated manual evidence, smoke-test
state, or OS metadata files. In particular, `.claude/`, `.codex/`,
`.any-switch/`, `.any-switch-*`, `manual-evidence-*.md`, `.smoke-*`,
`.test-*`, `.tmp/`, and `.DS_Store` are local-only artifacts.

## Change Guidelines

- Use the GitHub issue and pull request templates. They are written to collect
  OS, app, command, and redacted diagnostic context without exposing local
  credentials.
- Keep product-specific behavior in App Definitions when possible.
- Add core handlers only for trusted, declarative operations.
- Do not add commands that perform login, reauth, browser automation, or remote
  account repair.
- Treat secrets as non-printable. Human output, JSON output, history, and errors
  must not include secret values or capture blob contents.
- Keep repository and Cargo source packages free of local credential material,
  local agent settings, and generated real-app evidence.
- Add focused tests for state, locking, backup, path safety, and process-safety
  behavior whenever those surfaces change.

## Required Checks

Before opening a pull request, run:

```bash
scripts/verify-local.sh
scripts/verify-packages.sh
```

These are the Unix/macOS local gates and mirror the main CI verification plus
the package verification job. Windows-specific release behavior is covered by
the `windows build` GitHub Actions job, which runs the Windows target checks,
`scripts/manual-evidence.ps1 -Help`, and a Windows archive packaging smoke
test. Dependabot updates for Cargo dependencies and GitHub Actions should pass
those CI gates before merging.

## Release Changes

Release automation is defined in `.github/workflows/release.yml`. Tag releases
with `vX.Y.Z`; the workflow verifies the source-build release and publishes
GitHub Release notes. It does not upload unsigned prebuilt macOS or Windows
binaries as end-user installation artifacts.

Changes that affect release packaging should keep `docs/release.md`,
`docs/acceptance.md`, `scripts/package-release.sh`,
`scripts/verify-packages.sh`, and `tests/release.rs` in sync. Package scripts
should keep staging and temporary files self-cleaning so failed local release
checks can be rerun without manual directory cleanup. Windows release changes
should also preserve the `scripts/manual-evidence.ps1 -Help` workflow check.

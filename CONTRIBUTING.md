# Contributing

`switch-cli` is intentionally conservative because it edits local credential and
configuration files. Contributions should keep that bias.

## Development Setup

```bash
rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy
cargo test
cargo clippy --all-targets -- -D warnings
```

Use a disposable switch home while testing manually:

```bash
export SWITCH_CLI_HOME="$PWD/.smoke-switch"
export CODEX_HOME="$PWD/.smoke-codex"
```

## Change Guidelines

- Keep product-specific behavior in App Definitions when possible.
- Add core handlers only for trusted, declarative operations.
- Do not add commands that perform login, reauth, browser automation, or remote
  account repair.
- Treat secrets as non-printable. Human output, JSON output, history, and errors
  must not include secret values or capture blob contents.
- Add focused tests for state, locking, backup, path safety, and process-safety
  behavior whenever those surfaces change.

## Required Checks

Before opening a pull request, run:

```bash
scripts/verify-local.sh
```

## Release Changes

Release automation is defined in `.github/workflows/release.yml`. Tag releases
with `vX.Y.Z`; the workflow builds Linux and macOS binaries and uploads archives
to the GitHub Release.

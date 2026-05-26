# Release Process

This project ships source plus prebuilt command-line binaries through GitHub
Actions.

## Local Verification

Run the same checks used by CI:

```bash
scripts/verify-local.sh
```

The repository pins the Rust toolchain in `rust-toolchain.toml`; run
`rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy`
before release verification if the pinned toolchain is not already installed.
The script also checks shell script syntax, verifies the Cargo source package
offline, creates a temporary local archive through `scripts/package-release.sh`,
verifies its checksum, and lists the tarball.

## Tagging

Create and push a semantic version tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

## GitHub Actions Output

`.github/workflows/release.yml` first runs the release verification gate:

- `rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy`
- `scripts/verify-local.sh`

If verification passes, it builds:

- `x86_64-unknown-linux-gnu` on `ubuntu-latest`
- `x86_64-apple-darwin` on `macos-15-intel`
- `aarch64-apple-darwin` on `macos-15`

The macOS runner labels are pinned by architecture instead of using
`macos-latest`, so release packages are built on the matching GitHub-hosted
runner family.

The workflow packages each binary through `scripts/package-release.sh` as:

```text
switch-cli-<tag>-<target>.tar.gz
```

Each archive contains:

- `switch-cli`
- `README.md`
- `CONTRIBUTING.md`
- `SECURITY.md`
- `LICENSE-APACHE`
- `LICENSE-MIT`
- `docs/design.md`
- `docs/release.md`
- `docs/acceptance.md`
- `docs/manual-verification.md`
- `docs/manual-evidence-template.md`
- `scripts/manual-evidence.sh`
- `app_definitions/builtin/*.yaml`

The workflow also uploads `switch-cli-<tag>-<target>.tar.gz.sha256` next to each
archive.

## Post-Release Checks

Download each archive from the release page and verify:

```bash
shasum -a 256 -c switch-cli-<tag>-<target>.tar.gz.sha256
tar -tzf switch-cli-<tag>-<target>.tar.gz
tar -xzf switch-cli-<tag>-<target>.tar.gz
./switch-cli-<tag>-<target>/switch-cli --version
```

Before claiming full MVP coverage, run the real-app manual checks in
`docs/manual-verification.md`. The packaged `scripts/manual-evidence.sh` can
initialize a redacted evidence file against the packaged binary:

```bash
SWITCH_CLI_BIN=./switch-cli-<tag>-<target>/switch-cli \
  ./switch-cli-<tag>-<target>/scripts/manual-evidence.sh \
  manual-evidence-$(date -u +%Y%m%dT%H%M%SZ).md
```

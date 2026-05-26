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

## Optional macOS Signing

macOS binaries are signed before packaging only when signing secrets are present
in GitHub Actions. Missing secrets do not block release artifacts; the workflow
prints a skip message and packages the unsigned binary.

Required secrets for signing:

- `APPLE_DEVELOPER_ID_CERTIFICATE_BASE64`: base64-encoded Developer ID
  Application `.p12` certificate.
- `APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD`: password for the `.p12` file.
- `APPLE_CODESIGN_IDENTITY`: Developer ID Application signing identity name.

Additional secrets for notarization:

- `APPLE_ID`: Apple ID used with `notarytool`.
- `APPLE_APP_SPECIFIC_PASSWORD`: app-specific password for that Apple ID.
- `APPLE_TEAM_ID`: Apple Developer Team ID.

If signing secrets are present but notarization secrets are missing, the workflow
signs and verifies the Mach-O binary, skips notarization, and continues. If all
secrets are present, `scripts/sign-macos-binary.sh` signs the binary and submits
a temporary ZIP to Apple's notary service before packaging the signed binary in
the release tarball.

The workflow packages each binary through `scripts/package-release.sh` as:

```text
any-switch-<tag>-<target>.tar.gz
```

Each archive contains:

- `any-switch`
- `README.md`
- `CONTRIBUTING.md`
- `SECURITY.md`
- `LICENSE`
- `docs/design.md`
- `docs/release.md`
- `docs/acceptance.md`
- `docs/manual-verification.md`
- `docs/manual-evidence-template.md`
- `scripts/manual-evidence.sh`
- `app_definitions/builtin/*.yaml`

The workflow also uploads `any-switch-<tag>-<target>.tar.gz.sha256` next to each
archive.

## Post-Release Checks

Download each archive from the release page and verify:

```bash
shasum -a 256 -c any-switch-<tag>-<target>.tar.gz.sha256
tar -tzf any-switch-<tag>-<target>.tar.gz
tar -xzf any-switch-<tag>-<target>.tar.gz
./any-switch-<tag>-<target>/any-switch --version
```

Before claiming full MVP coverage, run the real-app manual checks in
`docs/manual-verification.md`. The packaged `scripts/manual-evidence.sh` can
initialize a redacted evidence file against the packaged binary:

```bash
ANY_SWITCH_BIN=./any-switch-<tag>-<target>/any-switch \
  ./any-switch-<tag>-<target>/scripts/manual-evidence.sh \
  manual-evidence-$(date -u +%Y%m%dT%H%M%SZ).md
```

#!/usr/bin/env bash
set -euo pipefail

target="${1:?usage: sign-macos-binary.sh <target> <binary>}"
binary="${2:?usage: sign-macos-binary.sh <target> <binary>}"

if [[ "${target}" != *-apple-darwin ]]; then
  echo "macOS signing: skipped for non-macOS target ${target}"
  exit 0
fi

if [[ "${RUNNER_OS:-}" != "macOS" && "$(uname -s)" != "Darwin" ]]; then
  echo "macOS signing: skipped because this runner is not macOS"
  exit 0
fi

missing_signing=()
for name in \
  APPLE_DEVELOPER_ID_CERTIFICATE_BASE64 \
  APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD \
  APPLE_CODESIGN_IDENTITY; do
  if [[ -z "${!name:-}" ]]; then
    missing_signing+=("${name}")
  fi
done

if (( ${#missing_signing[@]} > 0 )); then
  echo "macOS signing: skipped; missing ${missing_signing[*]}"
  exit 0
fi

if [[ ! -f "${binary}" ]]; then
  echo "macOS signing: binary not found: ${binary}" >&2
  exit 2
fi

temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/any-switch-sign.XXXXXX")"
keychain="${temp_dir}/codesign.keychain-db"
keychain_password="$(LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c 32)"
cert_path="${temp_dir}/certificate.p12"
notary_zip="${temp_dir}/any-switch-notary.zip"

cleanup() {
  security delete-keychain "${keychain}" >/dev/null 2>&1 || true
  rm -rf "${temp_dir}"
}
trap cleanup EXIT

printf '%s' "${APPLE_DEVELOPER_ID_CERTIFICATE_BASE64}" | base64 --decode >"${cert_path}"

security create-keychain -p "${keychain_password}" "${keychain}"
security set-keychain-settings -lut 21600 "${keychain}"
security unlock-keychain -p "${keychain_password}" "${keychain}"
security import "${cert_path}" \
  -k "${keychain}" \
  -P "${APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD}" \
  -T /usr/bin/codesign \
  -T /usr/bin/productsign
security set-key-partition-list \
  -S apple-tool:,apple:,codesign: \
  -s \
  -k "${keychain_password}" \
  "${keychain}"

codesign \
  --force \
  --timestamp \
  --options runtime \
  --keychain "${keychain}" \
  --sign "${APPLE_CODESIGN_IDENTITY}" \
  "${binary}"
codesign --verify --strict --verbose=2 "${binary}"

missing_notary=()
for name in APPLE_ID APPLE_APP_SPECIFIC_PASSWORD APPLE_TEAM_ID; do
  if [[ -z "${!name:-}" ]]; then
    missing_notary+=("${name}")
  fi
done

if (( ${#missing_notary[@]} > 0 )); then
  echo "macOS notarization: skipped; missing ${missing_notary[*]}"
  exit 0
fi

ditto -c -k --keepParent "${binary}" "${notary_zip}"
xcrun notarytool submit "${notary_zip}" \
  --apple-id "${APPLE_ID}" \
  --password "${APPLE_APP_SPECIFIC_PASSWORD}" \
  --team-id "${APPLE_TEAM_ID}" \
  --wait

echo "macOS signing/notarization: completed for ${binary}"

#!/usr/bin/env bash
set -euo pipefail

tag="${1:?usage: package-release.sh <tag> <target> [binary] [output-dir]}"
target="${2:?usage: package-release.sh <tag> <target> [binary] [output-dir]}"
binary_name="any-switch"
if [[ "${target}" == *-windows-* ]]; then
  binary_name="any-switch.exe"
fi
binary="${3:-target/${target}/release/${binary_name}}"
output_dir="${4:-.}"
staging_root=""
archive_tmp=""
checksum_tmp=""
checksum_named_tmp=""

cleanup() {
  if [[ -n "${staging_root}" && -d "${staging_root}" ]]; then
    rm -rf "${staging_root}"
  fi
  if [[ -n "${archive_tmp}" && -e "${archive_tmp}" ]]; then
    rm -f "${archive_tmp}"
  fi
  if [[ -n "${checksum_tmp}" && -e "${checksum_tmp}" ]]; then
    rm -f "${checksum_tmp}"
  fi
  if [[ -n "${checksum_named_tmp}" && -e "${checksum_named_tmp}" ]]; then
    rm -f "${checksum_named_tmp}"
  fi
}
trap cleanup EXIT

if [[ ! "${tag}" =~ ^[A-Za-z0-9._-]+$ || ! "${target}" =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "tag and target may only contain letters, numbers, '.', '_', and '-'" >&2
  exit 2
fi

if [[ ! -f "${binary}" ]]; then
  echo "binary not found: ${binary}" >&2
  exit 2
fi

package="any-switch-${tag}-${target}"
mkdir -p "${output_dir}"
output_dir="$(cd "${output_dir}" && pwd)"
staging_root="$(mktemp -d "${output_dir}/.package-${package}.XXXXXX")"
package_dir="${staging_root}/${package}"
archive="${output_dir}/${package}.tar.gz"
checksum="${archive}.sha256"
archive_tmp="$(mktemp "${output_dir}/.${package}.tar.gz.XXXXXX")"
checksum_tmp="${archive_tmp}.sha256"

mkdir -p "${package_dir}/docs" "${package_dir}/scripts" "${package_dir}/app_definitions/builtin"
cp "${binary}" "${package_dir}/${binary_name}"
chmod 0755 "${package_dir}/${binary_name}"
cp README.md CHANGELOG.md CODE_OF_CONDUCT.md CONTRIBUTING.md SECURITY.md LICENSE "${package_dir}/"
cp docs/user-guide.md docs/design.md docs/release.md docs/acceptance.md docs/evidence-followups.md docs/manual-verification.md docs/manual-evidence-template.md "${package_dir}/docs/"
cp scripts/manual-evidence.sh scripts/manual-evidence.ps1 "${package_dir}/scripts/"
chmod 0755 "${package_dir}/scripts/manual-evidence.sh"
cp src/app_definitions/builtin/*.yaml "${package_dir}/app_definitions/builtin/"

tar -C "${staging_root}" -czf "${archive_tmp}" "${package}"
(
  cd "${output_dir}"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$(basename "${archive_tmp}")" > "${checksum_tmp}"
  else
    sha256sum "$(basename "${archive_tmp}")" > "${checksum_tmp}"
  fi
)
checksum_named_tmp="${checksum_tmp}.named"
awk -v name="${package}.tar.gz" '{print $1 "  " name}' "${checksum_tmp}" > "${checksum_named_tmp}"
mv "${checksum_named_tmp}" "${checksum_tmp}"
checksum_named_tmp=""
mv "${archive_tmp}" "${archive}"
archive_tmp=""
mv "${checksum_tmp}" "${checksum}"
checksum_tmp=""

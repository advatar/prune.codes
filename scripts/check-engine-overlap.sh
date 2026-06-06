#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/check-engine-overlap.sh [--strict] [--standalone PATH]

Audits overlap between this repo's embedded engine at ./prune and a sibling
standalone engine checkout, defaulting to ../prune.

Without --strict, this reports overlap/drift and exits 0. With --strict, any
drift, standalone-only files, or embedded-only files exit non-zero.
USAGE
}

strict=0
standalone_path=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --strict)
      strict=1
      shift
      ;;
    --standalone)
      if [[ $# -lt 2 ]]; then
        echo "missing value for --standalone" >&2
        exit 2
      fi
      standalone_path="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
embedded_path="${repo_root}/prune"

if [[ -z "${standalone_path}" ]]; then
  standalone_path="$(cd "${repo_root}/.." && pwd)/prune"
fi

if [[ ! -d "${embedded_path}" ]]; then
  echo "embedded engine directory not found: ${embedded_path}" >&2
  exit 1
fi

if [[ ! -d "${repo_root}/.git" ]]; then
  echo "product repo git directory not found: ${repo_root}" >&2
  exit 1
fi

if [[ ! -d "${standalone_path}/.git" ]]; then
  echo "standalone engine checkout not found; skipped: ${standalone_path}"
  exit 0
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

git -C "${standalone_path}" ls-files | sort > "${tmp_dir}/standalone.files"
git -C "${repo_root}" ls-files 'prune/**' | sed 's#^prune/##' | sort > "${tmp_dir}/embedded.files"

comm -12 "${tmp_dir}/standalone.files" "${tmp_dir}/embedded.files" > "${tmp_dir}/shared.files"
comm -23 "${tmp_dir}/standalone.files" "${tmp_dir}/embedded.files" > "${tmp_dir}/standalone_only.files"
comm -13 "${tmp_dir}/standalone.files" "${tmp_dir}/embedded.files" > "${tmp_dir}/embedded_only.files"
: > "${tmp_dir}/drifted.files"

while IFS= read -r path; do
  standalone_hash="$(git -C "${standalone_path}" hash-object "${path}")"
  embedded_hash="$(git -C "${repo_root}" hash-object "prune/${path}")"
  if [[ "${standalone_hash}" != "${embedded_hash}" ]]; then
    printf '%s\n' "${path}" >> "${tmp_dir}/drifted.files"
  fi
done < "${tmp_dir}/shared.files"

shared_count="$(wc -l < "${tmp_dir}/shared.files" | tr -d ' ')"
drifted_count="$(wc -l < "${tmp_dir}/drifted.files" | tr -d ' ')"
standalone_only_count="$(wc -l < "${tmp_dir}/standalone_only.files" | tr -d ' ')"
embedded_only_count="$(wc -l < "${tmp_dir}/embedded_only.files" | tr -d ' ')"

echo "embedded: ${embedded_path}"
echo "standalone: ${standalone_path}"
echo "shared paths: ${shared_count}"
echo "drifted shared files: ${drifted_count}"
echo "standalone-only files: ${standalone_only_count}"
echo "embedded-only files: ${embedded_only_count}"

if [[ "${drifted_count}" != "0" ]]; then
  echo
  echo "Drifted shared files:"
  sed 's/^/  /' "${tmp_dir}/drifted.files"
fi

if [[ "${standalone_only_count}" != "0" ]]; then
  echo
  echo "Standalone-only files:"
  sed 's/^/  /' "${tmp_dir}/standalone_only.files"
fi

if [[ "${embedded_only_count}" != "0" ]]; then
  echo
  echo "Embedded-only files:"
  sed 's/^/  /' "${tmp_dir}/embedded_only.files"
fi

if [[ "${strict}" == "1" ]] && {
  [[ "${drifted_count}" != "0" ]] ||
  [[ "${standalone_only_count}" != "0" ]] ||
  [[ "${embedded_only_count}" != "0" ]]
}; then
  echo
  echo "strict mode failed: engine checkouts are not synchronized" >&2
  exit 1
fi

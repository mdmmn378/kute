#!/bin/sh
# kute installer — downloads a prebuilt binary from GitHub releases.
#
#   curl -fsSL https://raw.githubusercontent.com/mdmmn378/kute/main/install.sh | sh
#
# Environment overrides:
#   KUTE_REPO         GitHub owner/name          (default: mdmmn378/kute)
#   KUTE_VERSION      tag or version to install  (default: latest release)
#   KUTE_INSTALL_DIR  where to put the binary    (default: /usr/local/bin, else ~/.local/bin)
#   KUTE_NO_VERIFY    set to 1 to skip checksum verification

set -eu

REPO="${KUTE_REPO:-mdmmn378/kute}"
VERSION="${KUTE_VERSION:-latest}"
NO_VERIFY="${KUTE_NO_VERIFY:-0}"

say() { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
err() { printf 'error: %s\n' "$*" >&2; exit 1; }

have() { command -v "$1" >/dev/null 2>&1; }

if ! have curl && ! have wget; then
  err "curl or wget is required"
fi
have tar || err "tar is required"

download() {
  if have curl; then
    curl -fsSL "$1" -o "$2"
  else
    wget -qO "$2" "$1"
  fi
}

fetch() {
  if have curl; then
    curl -fsSL "$1"
  else
    wget -qO- "$1"
  fi
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux) os_part="unknown-linux-gnu" ;;
    Darwin) os_part="apple-darwin" ;;
    *) err "unsupported operating system: $os — build from source with 'cargo install --path .'" ;;
  esac

  case "$arch" in
    x86_64 | amd64) arch_part="x86_64" ;;
    aarch64 | arm64) arch_part="aarch64" ;;
    *) err "unsupported architecture: $arch — build from source with 'cargo install --path .'" ;;
  esac

  printf '%s-%s\n' "$arch_part" "$os_part"
}

resolve_tag() {
  if [ "$VERSION" != "latest" ]; then
    case "$VERSION" in
      v*) printf '%s\n' "$VERSION" ;;
      *) printf 'v%s\n' "$VERSION" ;;
    esac
    return 0
  fi

  # Follow the /releases/latest redirect rather than the API, which is rate
  # limited for anonymous callers. `curl -I` + url_effective is the cheap path.
  if have curl; then
    effective="$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
      "https://github.com/${REPO}/releases/latest" 2>/dev/null || true)"
    tag="${effective##*/}"
    case "$tag" in
      v*) printf '%s\n' "$tag"; return 0 ;;
    esac
  fi

  tag="$(fetch "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"

  [ -n "$tag" ] || err "could not work out the latest release of ${REPO}"
  printf '%s\n' "$tag"
}

verify_checksum() {
  directory="$1"
  archive="$2"
  tag="$3"
  sums_url="https://github.com/${REPO}/releases/download/${tag}/SHA256SUMS.txt"

  if [ "$NO_VERIFY" = "1" ]; then
    say "  checksum verification skipped (KUTE_NO_VERIFY=1)"
    return 0
  fi

  if have sha256sum; then
    if download "$sums_url" "${directory}/SHA256SUMS.txt" 2>/dev/null; then
      if (cd "$directory" && sha256sum -c --ignore-missing SHA256SUMS.txt >/dev/null 2>&1); then
        say "  checksum verified"
        return 0
      fi
      err "checksum verification failed for ${archive}"
    fi
  elif have shasum; then
    expected="$(fetch "$sums_url" 2>/dev/null | awk -v file="$archive" '$2 == file { print $1 }')"
    if [ -n "$expected" ]; then
      actual="$(shasum -a 256 "${directory}/${archive}" | awk '{ print $1 }')"
      [ "$expected" = "$actual" ] || err "checksum verification failed for ${archive}"
      say "  checksum verified"
      return 0
    fi
  fi

  warn "could not verify the checksum (no SHA256SUMS.txt or no hashing tool)"
  return 0
}

resolve_install_dir() {
  if [ -n "${KUTE_INSTALL_DIR:-}" ]; then
    printf '%s\n' "$KUTE_INSTALL_DIR"
  elif [ -w /usr/local/bin ] 2>/dev/null; then
    printf '%s\n' "/usr/local/bin"
  else
    printf '%s\n' "${HOME}/.local/bin"
  fi
}

main() {
  target="$(detect_target)"
  tag="$(resolve_tag)"
  version="${tag#v}"
  archive="kute-${version}-${target}.tar.gz"
  base_url="https://github.com/${REPO}/releases/download/${tag}"

  say "Installing kute ${version} (${target})"

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT INT TERM

  say "  fetching ${archive}"
  download "${base_url}/${archive}" "${tmp}/${archive}" \
    || err "download failed — does ${REPO} publish a ${target} build for ${tag}?"

  verify_checksum "$tmp" "$archive" "$tag"

  tar -xzf "${tmp}/${archive}" -C "$tmp" || err "could not extract ${archive}"

  binary="${tmp}/kute-${version}-${target}/kute"
  [ -f "$binary" ] || err "the archive did not contain the expected binary"

  install_dir="$(resolve_install_dir)"
  mkdir -p "$install_dir"
  cp "$binary" "${install_dir}/kute"
  chmod +x "${install_dir}/kute"

  installed="$("${install_dir}/kute" --version 2>/dev/null || true)"
  say "Installed ${installed:-kute} to ${install_dir}/kute"

  case ":${PATH}:" in
    *":${install_dir}:"*) ;;
    *)
      say ""
      say "${install_dir} is not on your PATH. Add it with:"
      say "  export PATH=\"${install_dir}:\$PATH\""
      ;;
  esac

  say ""
  say "Optional — tab completion:"
  say "  kute completions bash > ~/.local/share/bash-completion/completions/kute"
  say "  kute completions zsh  > \"\${fpath[1]}/_kute\""
  say "  kute completions fish > ~/.config/fish/completions/kute.fish"
}

main "$@"

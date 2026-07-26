#!/usr/bin/env bash
# Inari online installer / updater.
#
# Install or update to the latest release (.deb) in one line:
#   curl -fsSL https://raw.githubusercontent.com/fbnlrz/Inari/main/get-inari.sh | bash
#   wget -qO-  https://raw.githubusercontent.com/fbnlrz/Inari/main/get-inari.sh | bash
#
# Re-run any time to update to the latest release. To uninstall:
#   curl -fsSL https://raw.githubusercontent.com/fbnlrz/Inari/main/get-inari.sh | bash -s -- --uninstall
#
# Downloads the .deb from the latest GitHub release and installs it with apt
# (which pulls in any missing runtime dependencies). Debian/Ubuntu and
# derivatives; for other distros grab the .rpm or AppImage from the releases
# page. Your config in ~/.config/inari is never touched.
set -euo pipefail

REPO="fbnlrz/Inari"
PKG="inari"
API="https://api.github.com/repos/${REPO}/releases/latest"

msg()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# ---- downloader (curl or wget) --------------------------------------------
if command -v curl >/dev/null 2>&1; then
  fetch()    { curl -fsSL "$1"; }
  download() { curl -fL --progress-bar -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch()    { wget -qO- "$1"; }
  download() { wget -q --show-progress -O "$2" "$1"; }
else
  die "need curl or wget"
fi

# ---- privilege helper ------------------------------------------------------
if [ "$(id -u)" -eq 0 ]; then
  SUDO=""
else
  command -v sudo >/dev/null 2>&1 || die "need to run as root, or install sudo"
  SUDO="sudo"
fi

# ---- uninstall -------------------------------------------------------------
if [ "${1:-}" = "--uninstall" ]; then
  msg "Removing $PKG"
  $SUDO apt-get remove -y "$PKG"
  echo "Done. Your config in ~/.config/inari is kept (delete it manually for a clean slate)."
  exit 0
fi

# ---- must be a .deb system -------------------------------------------------
command -v dpkg >/dev/null 2>&1 || die \
  "This installs the .deb build. On non-Debian systems grab the .rpm or AppImage from https://github.com/${REPO}/releases"

# ---- resolve the latest release -------------------------------------------
msg "Checking the latest release"
json=$(fetch "$API") || die "cannot reach the GitHub API"
tag=$(printf '%s' "$json" | grep -oP '"tag_name":\s*"\K[^"]+' | head -1)
url=$(printf '%s' "$json" | grep -oP '"browser_download_url":\s*"\K[^"]*_amd64\.deb' | head -1)
[ -n "$tag" ] && [ -n "$url" ] || die "no .deb asset found in the latest release"
latest="${tag#v}"

installed=$(dpkg-query -W -f='${Version}' "$PKG" 2>/dev/null || true)
if [ -n "$installed" ]; then
  if [ "$installed" = "$latest" ]; then
    echo "Inari $installed is already the latest release. Nothing to do."
    exit 0
  fi
  msg "Updating Inari $installed -> $latest"
else
  msg "Installing Inari $latest"
fi

# ---- download + install ----------------------------------------------------
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
deb="$tmp/${PKG}_${latest}_amd64.deb"
download "$url" "$deb"

msg "Installing (needs ${SUDO:-root})"
# apt resolves runtime deps and upgrades over any existing install; the dpkg
# fallback plus `apt-get -f install` covers older apt that can't take a path.
$SUDO apt-get install -y "$deb" \
  || { $SUDO dpkg -i "$deb" || true; $SUDO apt-get -f install -y; }

echo
echo "Inari $tag installed. Launch it from your application menu or run: inari"

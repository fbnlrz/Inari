#!/usr/bin/env bash
# Migrate an existing Sink setup to Inari.
#
# Inari is a rebrand of this fork: the app, its command, its config directory
# and its autostart unit were renamed from "sink" to "inari". Only the *names*
# changed — your channels, mixes, profiles, EQ, prefs and routing are all kept.
#
# What this moves:
#   ~/.config/sink            -> ~/.config/inari         (all your settings)
#   systemd user unit sink.service -> disabled/removed   (re-enable in Settings)
#
# What deliberately stays the same (do NOT need migrating):
#   - PipeWire node names (sink_game, sink_mic, …) are internal ids; keeping
#     them means your app-to-channel routing and other apps' device picks
#     (Discord/OBS) keep working untouched.
#   - The WirePlumber drop-ins under ~/.config/wireplumber.
#
#   ./migrate-to-inari.sh                 migrate config + autostart
#   ./migrate-to-inari.sh --remove-old    also remove the old /usr install
set -euo pipefail

CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
SRC="$CONFIG_HOME/sink"
DST="$CONFIG_HOME/inari"
OLD_UNIT="$CONFIG_HOME/systemd/user/sink.service"
REMOVE_OLD=0
[[ "${1:-}" == "--remove-old" ]] && REMOVE_OLD=1

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m warn:\033[0m %s\n' "$*"; }

# ---------------------------------------------------------------- config dir
if [[ -d "$DST" && -d "$SRC" ]]; then
  warn "Both ~/.config/sink and ~/.config/inari exist."
  warn "Refusing to overwrite $DST. Merge or remove one, then re-run."
elif [[ -d "$SRC" ]]; then
  info "Moving $SRC -> $DST"
  mv "$SRC" "$DST"
  bold "Config migrated."
else
  info "No ~/.config/sink found — nothing to migrate (fresh install)."
fi

# ---------------------------------------------------------------- autostart
if [[ -f "$OLD_UNIT" ]]; then
  info "Disabling and removing the old sink.service autostart unit"
  systemctl --user disable --now sink.service 2>/dev/null || true
  rm -f "$OLD_UNIT"
  systemctl --user daemon-reload 2>/dev/null || true
  warn "Autostart was renamed to inari.service. Re-enable it in Inari's"
  warn "Settings once, and Inari will write the new unit for you."
fi

# ---------------------------------------------------------------- old install
if [[ "$REMOVE_OLD" -eq 1 ]]; then
  info "Removing the old system install (needs sudo)"
  sudo rm -f /usr/bin/sink
  sudo rm -f /usr/share/applications/sink.desktop
  sudo rm -f /usr/share/icons/hicolor/512x512/apps/sink.png
  bold "Old /usr install removed."
else
  if [[ -e /usr/bin/sink ]]; then
    warn "An old /usr/bin/sink is still installed. Remove it with:"
    warn "  ./migrate-to-inari.sh --remove-old"
    warn "or just run ./install.sh to install Inari alongside it."
  fi
fi

bold ""
bold "Done. Build and install Inari with ./install.sh, then launch 'inari'."

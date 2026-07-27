---
title: Configuration & files
description: Every file Inari writes under ~/.config/inari and ~/.config/wireplumber, plus autostart, command line and migration from upstream Sink.
---

# Configuration & files

Inari keeps its state in `~/.config/inari`, created with `0700` (owner-only —
routing rules and app history are nobody else's business, see
`src-tauri/src/persistence/mod.rs`). Most files are JSON; two are plain-text
markers.

| Path | What |
| --- | --- |
| `~/.config/inari/channels.json` | Channels (Game/Chat/Music/…), max 10 |
| `~/.config/inari/buses.json` | Mixes for OBS |
| `~/.config/inari/assignments.json` | App → channel routing |
| `~/.config/inari/aliases.json` | Renamed apps, channels and mixes |
| `~/.config/inari/outputs.json` | Output device per channel, plus which channels have auto-failover turned off |
| `~/.config/inari/seen_apps.json` | History of every app ever seen playing audio, including "ignored" marks |
| `~/.config/inari/eq.json`, `eq_presets/` | Per-channel EQ + saved presets |
| `~/.config/inari/mic.json` | Mic chain config |
| `~/.config/inari/profiles/` | Saved layouts |
| `~/.config/inari/prefs.json` | App preferences |
| `~/.config/inari/active_profile` | Plain text: the name of the live-bound profile |
| `~/.config/inari/notify_mirror` | Marker file; its presence means OLED notification mirroring is on |

Writes are atomic (temp file + `rename`), so a crash mid-save leaves either the
old file or the complete new one.

## WirePlumber fragments

Two more files live outside the config directory, under
`~/.config/wireplumber/wireplumber.conf.d/`:

| Path | What |
| --- | --- |
| `90-sink-routing.conf` | The `stream.rules` fragment that pins apps to their channel |
| `51-arctis-nova-pro-headroom.conf` | Optional anti-crackle headroom for the Nova Pro (presence of the file *is* the setting) |

The routing fragment still carries the old `sink` prefix from before the rename
— worth knowing when you go looking for it. WirePlumber reads conf fragments at
startup only, so changes take effect at the next login or WirePlumber restart.

## Autostart

Enable autostart in **Settings**; Inari writes a systemd user unit
(`~/.config/systemd/user/inari.service`) anchored to your graphical session.

## Command line

```bash
inari                 # launch
inari --minimized     # start to tray (used by autostart)
```

`--minimized` is the only argument Inari looks at. The installed version is
shown in the **Settings** screen.

## Migrating from upstream Sink

If you used the upstream Sink, run `./migrate-to-inari.sh` once to move
`~/.config/sink` → `~/.config/inari` and clean up the old autostart unit. Your
channels, mixes, profiles and routing are preserved; PipeWire node names are
kept stable on purpose.

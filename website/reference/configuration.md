---
title: Configuration & files
description: Every file Inari writes under ~/.config/inari and ~/.config/wireplumber, the remote token, the log file, autostart and migration from upstream Sink.
---

# Configuration & files

Inari keeps its state in `~/.config/inari`, created with `0700` (owner-only —
routing rules and app history are nobody else's business, see
`src-tauri/src/persistence/mod.rs`). Most files are JSON; two are plain-text
markers and one is a secret.

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
| `~/.config/inari/prefs.json` | App preferences — theme, device naming, balance slider, notification display, **your [hotkey](/features/hotkeys) bindings** and the **[remote](/guide/remote)'s enabled flag, address and port** |
| `~/.config/inari/remote-token` | The remote's bearer token. Its own file, `0600` |
| `~/.config/inari/active_profile` | Plain text: the name of the live-bound profile |
| `~/.config/inari/notify_mirror` | Marker file; its presence means OLED notification mirroring is on |

Writes are atomic (temp file + `fsync` + `rename`), so a crash mid-save leaves
either the old file or the complete new one. Each file carries a schema version,
and keys a build doesn't recognise are preserved across a load/save round trip —
so running an older Inari once does not silently drop settings a newer one
wrote.

## The remote token

`remote-token` is 32 bytes from the OS random source, hex-encoded, written with
mode `0600` and kept **out of `prefs.json`** — that file is ordinary
configuration people paste into bug reports. Deleting the file mints a new token
on the next start; **Settings → Remote → Regenerate token** does the same on
demand, and invalidates every device that had paired with the old one.

## Log file

```
~/.local/share/com.fbnlrz.inari/logs/inari.log
```

One file, capped at 512 KiB. When it fills, the previous contents are
*discarded* rather than archived, so grab the log reasonably soon after a
problem. **Settings → About** has a button that opens the folder. Collecting a
log for a bug report is covered in
[Troubleshooting](/troubleshooting#collecting-logs-for-a-bug-report).

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
With **Start minimized** on, the unit's `ExecStart` gains `--minimized`, so the
unit is rewritten whenever you toggle it.

**Reset Inari** wipes `~/.config/inari`, removes the routing fragment and
deletes this unit. The one thing it leaves behind is
`51-arctis-nova-pro-headroom.conf` — delete that by hand if you want it gone.

## Command line

`inari` takes `--minimized` to boot straight to the tray, and a handful of
verbs (`status`, `mute`, `volume`, `profile`) that talk to the already-running
instance. A second launch never opens a second window — it raises the one that
is running. See [Command line](/reference/cli).

## Migrating from upstream Sink

If you used the upstream Sink, run `./migrate-to-inari.sh` once to move
`~/.config/sink` → `~/.config/inari` and clean up the old autostart unit. Your
channels, mixes, profiles and routing are preserved; PipeWire node names are
kept stable on purpose.

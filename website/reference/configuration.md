# Configuration & files

Inari stores everything as plain JSON under `~/.config/inari`.

| Path | What |
| --- | --- |
| `~/.config/inari/channels.json` | Channels (Game/Chat/Music/…) |
| `~/.config/inari/buses.json` | Mixes for OBS |
| `~/.config/inari/assignments.json` | App → channel routing |
| `~/.config/inari/eq.json`, `eq_presets/` | Per-channel EQ + saved presets |
| `~/.config/inari/mic.json` | Mic chain config |
| `~/.config/inari/profiles/` | Saved layouts |
| `~/.config/inari/prefs.json` | App preferences |

Two more files live under your WirePlumber config
(`~/.config/wireplumber/wireplumber.conf.d/`): the routing rules and the
optional anti-crackle headroom fragment.

## Autostart

Enable autostart in **Settings**; Inari writes a systemd user unit
(`~/.config/systemd/user/inari.service`) anchored to your graphical session.

## Command line

```bash
inari                 # launch
inari --minimized     # start to tray (used by autostart)
inari --version
```

## Migrating from upstream Sink

If you used the upstream Sink, run `./migrate-to-inari.sh` once to move
`~/.config/sink` → `~/.config/inari` and clean up the old autostart unit. Your
channels, mixes, profiles and routing are preserved; PipeWire node names are
kept stable on purpose.

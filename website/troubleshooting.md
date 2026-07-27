---
title: Troubleshooting
description: Symptom, cause and fix for the problems people actually hit — a silent channel with its fader up, dead meters, a black window, empty OLED modes, devices that stay disconnected, and how to collect a log.
---

# Troubleshooting

Symptom → cause → fix. Short "why does it work that way" questions are in the
[FAQ](/faq). If none of these help, search
[issues](https://github.com/fbnlrz/Inari/issues) or ask in
[Discussions](https://github.com/fbnlrz/Inari/discussions).

## A channel is silent, but its fader is at 100 %

**Symptom.** One channel plays nothing. Routing looks right, the app is on the
channel, other channels are fine — and the strip reads 100 %, unmuted. Nudging
the fader fixes it instantly.

**Cause.** Versions **before v1.0.10** wrote 100 %, unmuted to every sink at
startup and then showed that value. WirePlumber remembers a volume per PipeWire
node name and restores it *after* Inari's write, so a channel you had left at
0 % (or muted) came back that way while the interface still claimed 100 %.
Moving the fader was the first moment anyone wrote the value again.

**Fix.** Update. Since v1.0.10 Inari reads each channel's volume and mute off
the sink instead of setting it, so the strips show what you will actually hear —
and your channel volumes survive a restart. On an older build, confirm it with:

```bash
pactl list sinks short | grep sink_
pactl get-sink-volume sink_music
```

## The VU meters never move

**Cause.** Level metering only exists on the native PipeWire backend. If the
native loop can't start, Inari falls back to `pactl` subprocesses and the meters
stay flat — as do the EQ, the mixes and the processed microphone. A third case:
the native engine started and later *died*, in which case audio control stops
altogether and the fallback does not step in.

**Fix.** Check the pill in the title bar, or **Settings → About → Audio
engine**. A `fallback` tag means no native engine; a `stopped` tag means it
died and Inari has to be restarted. Either way, quit from the tray and relaunch
once your session's PipeWire is fully up. Full explanation:
[Why don't my VU meters move?](/faq#why-don-t-my-vu-meters-move).

## The window is black, or Inari won't draw at all

**Symptom.** An empty black window, a white flash and nothing else, or a crash
on launch with a Wayland protocol error — often reported as **Gdk-Error 71**.

**Cause.** WebKitGTK's DMABUF renderer breaks on some GPU/driver combinations.
Inari already works around this: it sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` for
itself at startup — but only if you haven't set that variable yourself. An
explicit value in your environment (a shell profile, a systemd drop-in, a
wrapper script) wins over the built-in default.

**Fix.** Make sure nothing in your session sets it to something else:

```bash
env | grep WEBKIT_DISABLE_DMABUF_RENDERER
```

If that prints `=0`, remove it and restart Inari. To confirm the workaround is
what you need, start it explicitly from a terminal:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 inari
```

If the window is fine that way and broken otherwise, the variable is being
overridden somewhere in your session.

## A SteelSeries device shows "not connected"

**Cause.** The udev rule hasn't applied to the already-plugged device yet.

**Fix.** Re-plug the device once, then reopen the tab. Confirm your user can
read/write the device's `/dev/hidraw*` node — the installer's `uaccess` rule
handles this (see [Supported hardware](/reference/hardware)).

## The OLED tab says "display not supported"

**Cause.** That model has no drivable display (e.g. the older Arctis Pro
Wireless). The 128×64 OLED is specific to the Arctis Nova Pro base station.

## One OLED mode stays empty

**Symptom.** Every mode works except one or two — *Now playing* and *Album art*
stay blank, *Spectrum* shows no bars, notifications never appear, or a video
refuses to play. The mode is still listed in the picker.

**Cause.** Those modes shell out to a helper that isn't installed. Inari treats
each one as optional: the mode stays selectable but has nothing to show.

| Empty mode | Needs |
| --- | --- |
| Now playing · Album art | `playerctl` |
| Notification mirroring | `dbus-monitor` |
| Video playback | `ffmpeg` |
| Spectrum | `parec` (PulseAudio utils) |

**Fix.** Install the helper for the mode you want; the package names per distro
are in [Optional dependencies](/guide/getting-started#optional-dependencies).
Inari picks each helper up on the next use, so there is no need to restart it —
reselect the mode, or toggle notification mirroring off and on again.

The same goes for the [Media tab](/features/media): without `playerctl` it says
so rather than sitting empty.

## Album art is blank for anything played in a browser

**Cause.** Fixed in **v1.0.9**. Browsers hand MPRIS an extensionless temp file
(Chromium writes `/tmp/.org.chromium.Chromium.XXXXXX`), and Inari used to pick
its image decoder from the file name — so a perfectly good PNG decoded as
nothing and the panel fell back to `NO ART`. Local files named `cover.jpg`
always worked, which is why it went unnoticed.

**Fix.** Update. Covers are now identified by their content.

## A global hotkey never fires

**Cause.** Almost always Wayland. Inari's grabs go through X11, and on Wayland
the compositor owns the keyboard: the binding registers fine and then simply
never triggers. A binding the desktop refused outright is shown as `inactive`
with the reason in **Settings → Hotkeys**.

**Fix.** Bind the shortcut in your desktop environment instead and point it at
the [command line](/reference/cli) — `inari mute mic`, `inari profile Gaming`.
See [Hotkeys](/features/hotkeys).

## No audio after enabling anti-crackle headroom

**Cause.** The headroom fix is a WirePlumber fragment, and WirePlumber only
reads those at startup.

**Fix.** Log out and back in (or reboot). Inari deliberately does *not* restart
WirePlumber live, which would tear down its virtual sinks.

## "Download is performed unsandboxed as root" during install

Harmless — apt just couldn't sandbox a local `.deb` in a private temp dir. The
install still succeeds. The installer and in-app updater make the temp file
world-readable to avoid the notice on current versions.

## Collecting logs for a bug report

Inari logs to a small file, so you don't have to reproduce the problem to report
it — the run it happened in is already on disk:

```
~/.local/share/com.fbnlrz.inari/logs/inari.log
```

**Settings → About → Logs → Open** opens that folder for you.

It is a single file capped at 512 KiB, and when it fills the older contents are
discarded rather than archived — so collect it while the problem is fresh.

Started by the autostart unit, the same lines also land in the journal:

```bash
journalctl --user -u inari -n 200
```

The default level records startup, device connect/disconnect, profile switches
and every warning or error. For more detail, quit Inari and start it from a
terminal with the level raised — this affects the log file too:

```bash
RUST_LOG=debug inari
```

Attach the log to your [issue](https://github.com/fbnlrz/Inari/issues/new/choose).
It contains device and audio-node names, no credentials.

::: details Migrating from Sink (and pre-1.0.3 installs)

### Devices show as "Sink" / "(Sink)" in Discord or OBS

You migrated from the upstream Sink. PipeWire **node names are kept stable on
purpose** so existing routing keeps working; only the display label follows the
app name for new setups. Re-select the "Inari" devices in the other app if you
want the new label.

### I still have both `sink` and `inari` installed

The upstream Sink was a separate package. Remove it and keep Inari:

```bash
sudo apt remove sink
```

Leftover files from an old source install can be cleared with
`./migrate-to-inari.sh --remove-old`, and an old `~/.config/sink` is moved over
by running `./migrate-to-inari.sh` once.

### The in-app update installed but didn't restart

Fixed in **v1.0.3**. Earlier builds couldn't relaunch after an in-place upgrade
(the running binary's file had been replaced). Update once more from the command
line, then in-app updates restart cleanly:

```bash
curl -fsSL https://raw.githubusercontent.com/fbnlrz/Inari/main/get-inari.sh | bash
```

:::

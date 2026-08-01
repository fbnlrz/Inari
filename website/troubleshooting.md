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
and your channel volumes survive a restart.

**v1.0.13** closes the rest of it. That read happened once, during startup, and
never again — so anything that moved a level afterwards (`pavucontrol`,
`wpctl`, WirePlumber restoring a level when a device reappears) put the strip
back out of step, and if the reading had not arrived yet when Inari looked, the
100 % placeholder simply stayed for the session. The strips now follow the
sinks continuously. Confirm what the sink is really at with:

```bash
pactl list sinks short | grep sink_
pactl get-sink-volume sink_music
```

## A channel plays a continuous tone or hum that will not stop

**Symptom.** One channel hums or drones on, at a pitch a bit below wherever you
set a shelf band, and it carries on after the music stops. Turning the channel
down does not remove it.

**Cause.** Before **v1.0.13**, a shelf band with a steep slope could be given a
filter design with both poles exactly on the unit circle — an undamped
resonator. It rings for ever at an amplitude that can exceed full scale, so it
also clips. Slope 10 reached it at only ±8 dB of shelf gain, so an ordinary
steep bass boost was enough.

**Fix.** Update. The slope is now capped to the steepest one the chosen gain
can support. On an older build, open that channel's EQ and turn the shelf's Q
back down (or flatten the band) — recomputing the coefficients stops the
oscillation immediately.

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

## The meters twitch or read low while the keyboard or OLED is lit

**Symptom.** The mixer's level meters look sluggish or under-read, and it is
worse with an Apex keyboard on an audio-reactive effect or the headset OLED in
VU mode.

**Cause.** Before **v1.0.13** all three read the same peak store, and reading a
peak clears it. Whichever ticked first — the keyboard every 33 ms, the OLED
every 40, the window every 100 — took the value, and the others saw whatever
had accumulated since.

**Fix.** Update. Each of the three has its own peaks now.

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

**Start here, before guessing:**

```sh
inari doctor
```

It runs in your terminal — no running Inari needed, which is the point — and
prints every SteelSeries HID node, which one Inari's table claims, which passed
the interface/descriptor probe, whether it can be opened at all, and, for each
candidate, whether the device actually answers. Reading it top to bottom
usually names the cause outright.

**Cause 1 — permissions.** A node reading `cannot open (permission denied)`
means the udev rule has not been applied to an already-plugged device.

**Fix.** Re-plug the device once, or:

```sh
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=hidraw
```

**Cause 2 — nothing passed the probe.** The report says a product id matched
but no node passed. The device exposes its configuration collection on a
different interface than the table expects; that is a bug worth reporting, with
the report attached.

**Cause 3 — the device is silent.** A node opens but never answers. Inari
handles this by itself since 1.0.12: it only reports "connected" once the
device has actually replied, and moves on to the next candidate node otherwise.
Older versions announced the first node that merely *opened*, which is why a
headset could sit there showing nothing until it was unplugged and the app
restarted.

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

Fixed in **v1.0.3**, again in **v1.0.11**, and again in **v1.0.13** — three
different reasons with the same symptom. v1.0.3: the running binary's file had
been replaced. v1.0.11: systemd killed the relaunch helper when it swept our
own unit's control group. v1.0.13: launched from the application menu, KDE
wraps the app in a transient `app-*.service` with `KillMode=control-group`, and
`setsid` leaves the session but stays in that cgroup — so the helper was swept a
second after being spawned. The relaunch now goes through
`systemd-run --user`, which puts it in a unit of its own.

Update once more from the command line, then in-app updates restart cleanly:

```bash
curl -fsSL https://raw.githubusercontent.com/fbnlrz/Inari/main/get-inari.sh | bash
```

:::

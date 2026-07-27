---
title: Settings
description: Theme, device naming, default input and output, the software ChatMix slider, autostart, the engine indicator — and the factory reset that wipes everything.
---

# Settings

## Appearance

**Theme** — **Original** (the default dark look) or **Tokyo Night**, a
blue/purple palette that matches the popular desktop theme. The choice applies
immediately and is remembered per machine.

## Device naming

**Device naming** controls how Inari's channels, mixes and the virtual mic are
labelled in *other* programs' device lists. Three styles:

| Style | A channel called Game appears as |
| --- | --- |
| Plain (default) | `Game` |
| Suffix | `Game (Inari)` |
| Prefix | `Inari · Game` |

This is the setting to reach for when devices "have weird names" in Discord or
OBS, or when you cannot tell Inari's virtual devices apart from real hardware in
a long list. Existing nodes keep their current label until they are recreated —
restart Inari, or rename the channel, to see the change.

## Default output and input

**Default output** is the system default sink: where every channel set to
*System default* plays, and where [monitoring](/features/mixer#monitor) is
heard.

**Default input** is the system default source — among other things, the
microphone the [mic chain](/features/mic) captures when its input is left on
*System default*.

Both write the real PipeWire/PulseAudio default, so the change applies
system-wide, not just inside Inari.

## Balance slider (software ChatMix)

**Balance slider** shows a two-sided slider in the title bar. It blends two
channels: centre means both at 100 %, and sliding toward one side ducks the
other, down to silence at the extreme. Click either end icon to choose which
channels the two sides control; Inari defaults to Game and Chat when they exist.

This is Inari's **software ChatMix** — the same game/chat balance a SteelSeries
headset gives you on its hardware wheel, available on any hardware. It is not
tied to a headset in any way. (If you *do* have an Arctis Nova Pro, its physical
ChatMix wheel is read separately on the [Headset](/features/headset) page.)

The slider is stateless: its position is derived from the two channel faders, so
moving a fader by hand moves the balance too, and profiles capture it for free.

## Start at login

**Start at login** installs a systemd user service
(`~/.config/systemd/user/inari.service`) anchored to your graphical session.

With it enabled, **Start minimized** appears: Inari boots straight to the tray
instead of opening the window. Toggling it rewrites the unit, so it takes effect
on the next login.

## About

- **Audio engine** — which backend is live. `native` is the native PipeWire
  engine (live metering, passive routing) and is what you want; `fallback` means
  the native engine was unavailable and Inari is driving `pactl` as a
  subprocess, where mixes, the mic chain, the software EQ and monitoring are not
  available.
- **Inari `<version>`** — the installed version, the licence, and a reminder
  that the config lives in `~/.config/inari`.
- **Updates** — check for a new release and, on `.deb` installs, apply it in
  place. See [Updating](/guide/updating).
- **Tutorial** — replays the first-run tour at any time.

## Reset Inari

::: danger Factory reset — this deletes everything
**Reset Inari** is not a "restore defaults" button. It permanently deletes:

- the entire `~/.config/inari` directory — channels, mixes, profiles, app
  assignments and history, renamed apps, EQ settings and your saved EQ presets,
  and all preferences;
- the WirePlumber routing fragment Inari wrote outside that directory.

It also tears down Inari's audio nodes, turns off autostart, and relaunches the
app as if it had just been installed. There is no undo and nothing is backed up.
If you only want a clean layout, create a fresh [profile](/features/profiles)
instead.
:::

A confirmation dialog stands in front of it, but that is the only safeguard.

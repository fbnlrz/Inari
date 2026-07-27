---
title: Settings
description: Theme, device naming, default input and output, the software ChatMix slider, autostart, global hotkeys, the remote, the engine indicator — and the factory reset that wipes everything.
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

Starting Inari a second time does not open a second copy — the running instance
raises and focuses its window instead. That is also how the
[command line](/reference/cli) reaches a running Inari.

## Hotkeys

Four actions can be bound to a global shortcut: **Mute microphone**, **Mute a
channel** (with a picker underneath for which channel), **Next profile** and
**Show/hide Inari**. Nothing is bound out of the box — click a row, press the
combination, or **Clear** to unbind.

A binding the desktop refuses to hand over is shown as `inactive` with the
reason, rather than looking bound and doing nothing. On Wayland that is the
normal case: the compositor owns the keyboard and Inari's grabs go through X11.
Bind your compositor's own shortcuts to the [CLI](/reference/cli) instead.

Full detail: [Hotkeys](/features/hotkeys).

## Remote

**Inari Remote** serves this same interface over your network, so a tablet on
the couch can move faders. It is **off** by default and, when first switched on,
listens on loopback only — nothing on the network reaches it until you pick an
address. Pair a device by scanning the QR code; **Regenerate token** cuts every
paired device loose.

The remote is deliberately less capable than the desktop window: it can mix,
route, drive the headset, the OLED and media, and load profiles, but it cannot
create or delete channels, profiles or mixes, change settings, update, or reset
Inari. Those controls are simply not drawn in the browser.

Setup, addresses, ports and the full permission list:
[Remote control](/guide/remote).

## Tray menu

Beyond **Show Window** and **Quit**, the tray carries two submenus:

- **Mute** — one checkable row for the microphone and one per channel, so you
  can mute Chat without opening the window. A mute the engine refuses snaps
  back.
- **Profiles** — switch [layouts](/features/profiles) with the window closed.

## About

- **Audio engine** — which backend is live, as one of three states:

  | Tag | Means |
  | --- | --- |
  | `native` | The native PipeWire engine — live metering, passive routing. What you want. |
  | `fallback` | The native engine was unavailable, so Inari drives `pactl` as a subprocess. Mixes, the mic chain, the software EQ and monitoring are not available. |
  | `stopped` | The native engine came up and later died. Audio control is gone until you restart Inari; the fallback does **not** take over. |

  The same indicator sits in the title bar, so a stopped engine is visible
  without opening Settings.
- **Inari `<version>`** — the installed version, the licence, and a reminder
  that the config lives in `~/.config/inari`.
- **Updates** — check for a new release and, on `.deb` installs, apply it in
  place. See [Updating](/guide/updating).
- **Logs** — opens the log folder, so you can attach the file to a bug report
  without hunting for the path. See
  [Collecting logs](/troubleshooting#collecting-logs-for-a-bug-report).
- **Tutorial** — replays the first-run tour at any time.

## Reset Inari

::: danger Factory reset — this deletes everything
**Reset Inari** is not a "restore defaults" button. It permanently deletes:

- the entire `~/.config/inari` directory — channels, mixes, profiles, app
  assignments and history, renamed apps, EQ settings and your saved EQ presets,
  your hotkey bindings, the remote's pairing token, and all preferences;
- the WirePlumber routing fragment Inari wrote outside that directory.

It also tears down Inari's audio nodes, removes the autostart unit, and
relaunches the app as if it had just been installed. The one thing it leaves is
the optional anti-crackle headroom fragment. There is no undo and nothing is
backed up.
If you only want a clean layout, create a fresh [profile](/features/profiles)
instead.
:::

A confirmation dialog stands in front of it, but that is the only safeguard.

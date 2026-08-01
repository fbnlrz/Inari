---
title: Command line
description: Mute, set volumes and switch profiles on a running Inari from a terminal or a desktop shortcut — without the window popping up.
---

# Command line

Inari has a small non-interactive command line for acting on an **already
running** instance. It is what a desktop keyboard shortcut calls when
[global hotkeys](/features/hotkeys) cannot be grabbed.

## `inari doctor`

Prints what Inari sees of your hardware: every SteelSeries HID node, its product
id, USB interface and report-descriptor prefix, whether Inari's device table
claims it, whether it can be opened, and — per candidate, in the order Inari
tries them — whether the device answers.

Unlike the other verbs this one runs in **your** process and prints to **your**
terminal, so it works when no Inari is running. It is read-only: it opens nodes
and listens, and never writes to a device.

## Usage

```
Inari - Linux audio routing and mixing

Usage:
  inari [--minimized]                 start Inari (--minimized: stay in the tray)
  inari status                        print channels, mic and active profile
  inari mute <channel|mic> [on|off]   mute, unmute, or toggle (default)
  inari volume <channel> <0-150>      set a channel's volume in percent
  inari profile <name>                load a saved profile
  inari --help                        this text

<channel> is a channel's label ("Chat") or its sink name ("sink_chat").
Everything but a plain start acts on the already running Inari; results are
printed by that process (see its log: Settings -> Logs).
```

## How it reaches the running app

Inari is a single-instance application. `inari mute chat` does start a process,
but the single-instance plugin hands that process's arguments to the instance
already running and the new process exits. There is **no second service, no
socket and no daemon** — the channel that already prevents a duplicate window is
the same one the CLI rides on.

Two consequences follow directly from that:

- **The window is not raised.** A control command returns before the
  window-revealing branch, so a shortcut pressed in the middle of a game mutes
  the channel and leaves your screen alone. (A plain `inari` with no arguments
  *does* bring the window to the front, which is what you want from the
  application menu.)
- **Output does not come back to your terminal.** The hand-off is one-way: your
  process is gone by the time the running instance has an answer, so results are
  printed on *that* process's stdout and mirrored into the log file. Commands
  that act — `mute`, `volume`, `profile` — are therefore the useful ones.
  `status` is for a session where Inari itself was started from a shell, or for
  reading back out of **Settings → Logs**.

If nothing is running, a control command does not silently boot a whole mixer:
it prints `inari: not running - start Inari first` and exits `1`. `--help` exits
`0`, a malformed invocation prints the usage text and exits `2`.

## Arguments in detail

**Targets.** `mic` and `microphone` (any capitalisation) mean the mic chain.
Everything else is matched against channels — first by sink name (`sink_chat`),
then by label (`Chat`), both case-insensitively. Nobody should have to type
`sink_chat`.

**`mute`** takes `on`, `off` or `toggle`; bare `mute <target>` toggles, which is
almost always what a shortcut wants.

**`volume`** takes 0–150 percent — the same ceiling the UI enforces — and
applies to channels only. Mic gain is set on the Mic tab.

**`profile`** takes exactly one profile name and loads it, exactly as picking it
in the UI would.

Unknown `-`-prefixed arguments are ignored rather than rejected: desktop files,
session managers and the webview itself all append flags nobody asked for, and
refusing to launch over one would be a bad trade. An unknown *word*, on the
other hand, is a typo'd subcommand and is reported as one.

`--minimized` is used by the autostart unit to boot straight to the tray; see
[Configuration & files](/reference/configuration).

## Example: a desktop shortcut that mutes chat

This is the practical answer when a global hotkey registers but never fires on
Wayland — see [Hotkeys](/features/hotkeys).

**GNOME.** *Settings → Keyboard → View and Customize Shortcuts → Custom
Shortcuts → +*

| Field | Value |
| --- | --- |
| Name | `Mute chat` |
| Command | `inari mute chat` |
| Shortcut | e.g. `Super+F2` |

**KDE.** *System Settings → Shortcuts → Add → Command or Script*, with
`inari mute chat` as the command.

Pressing the key toggles the Chat channel's mute. The Inari window stays exactly
where it was — hidden in the tray, or behind your game — and the tray menu and
the window both re-read the change, so nothing is left showing a stale state.

The same pattern works for `inari mute mic`, `inari volume game 40` or
`inari profile Streaming`.

## Related

- [Hotkeys](/features/hotkeys) — Inari's own global shortcuts, and why they
  sometimes cannot work
- [Profiles](/features/profiles) — what `inari profile` loads
- [Configuration & files](/reference/configuration) — autostart and the files
  these commands change

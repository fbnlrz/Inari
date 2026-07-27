---
title: Profiles
description: Save a whole mixer layout, switch it from the tray, and have a profile load itself automatically when a specific output device connects.
---

# Profiles — saved layouts

A profile is a snapshot of your entire mixer: the channel set (names, icons,
volumes, mutes), every app→channel assignment, the output device and failover
setting per channel, all per-channel EQs, and your mixes with their membership,
level and mute.

On first run Inari saves the current layout as **Default**, so there is always a
known-good state to come back to.

## Switching

Profiles live in the profile menu in the title bar, and in the **Profiles**
submenu of the tray icon — the tray path lets you switch layouts without opening
the window at all. There are three more ways in that never touch the window:

- a global hotkey bound to **Next profile** ([Hotkeys](/features/hotkeys));
- `inari profile <name>` from a script or a desktop shortcut
  ([Command line](/reference/cli));
- the [remote](/guide/remote), which may *load* a profile but not create,
  delete or re-bind one.

However you switch, the rest of the UI follows.

Loading a profile reconciles the layout: missing channels are created, extra
ones are removed (their apps fall back to the default output first), then
volumes, mutes, outputs, failover, EQs and mixes are applied.

**Create** in the profile menu makes a fresh profile with the default four
channels, no routing and everything following the system default. Names may be
1–64 characters, letters, digits, spaces, `-` and `_`.

## Live binding — there is no Save button

::: warning Your active profile is written continuously
The profile you last loaded is **live-bound**: every change you make — moving a
fader, muting, routing an app, editing an EQ, adding a mix — is saved into it
immediately. There is no separate "save" step, and no way to try something out
and discard it by switching away.
:::

This is deliberate: switching to another profile and back never loses work. But
it does surprise people who expect profiles to be read-only presets. If you want
a layout frozen, do not use it as your working profile — create a second one to
experiment in.

The name of the active profile is shown on the profile button in the title bar
and checked in the tray submenu, and it survives restarts.

One consequence of the volume read added in v1.0.10: a profile is not replayed
at startup. Inari takes each channel's volume and mute from the sink itself
(see [Mixer](/features/mixer#volume-and-mute-survive-a-restart)), and the
autosave then writes those values into the active profile. The profile follows
the audio, not the other way round — loading a profile explicitly is still what
applies its stored levels.

## Auto-switch when a device connects

A profile can bind itself to an **output device** and load automatically the
moment that device appears. Plug in your headset and your headset layout is
live; switch on the speakers and the living-room profile takes over.

To set it up, open the profile menu, click the **bolt** icon on a profile row
and pick a device from the list — or **No auto-switch** to clear it. Rows with a
trigger show `auto-loads with <device>` underneath.

Notes:

- The trigger fires on a device *appearing*, i.e. a device that was not present
  a moment ago. Devices already connected when Inari starts do not re-trigger.
- Bind a device to one profile only. If several profiles name the same device,
  the first one alphabetically wins.
- The trigger is stored with the profile, so it comes along when the profile is
  autosaved.

## Deleting

The delete button on a profile row removes its file. Deleting the profile you
are currently in unbinds the autosave — your live setup keeps running, it is
just no longer being written to any profile until you load one again.

Profiles are plain JSON under `~/.config/inari/profiles/` — see
[Configuration & files](/reference/configuration).

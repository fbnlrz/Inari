---
title: FAQ
description: Answers to the questions new Inari users ask — dead VU meters, the tray, root access, packaging, what happens to your audio when Inari stops, and keyboard control.
---

# FAQ

Short answers. Anything that turns into a symptom → cause → fix walkthrough
lives in [Troubleshooting](/troubleshooting).

## Why don't my VU meters move?

Almost always because Inari is running on its **fallback audio backend**.

Inari prefers a native PipeWire connection. If that loop can't come up within
five seconds of launch, it falls back to driving audio through `pactl`
subprocesses so the basics keep working. Level metering only exists on the
native backend, so on the fallback the meters simply never move — including the
one on the Mic tab.

Check which one you're on: **Settings → About → Audio engine**, or the pill in
the title bar. It reads "Native PipeWire (pipewire-rs)" with a `native` tag, or
"pactl fallback" with a `fallback` tag. A third state, `stopped`, means the
native engine started and later died — the fallback does *not* take over, and
audio control stays gone until you restart Inari. The reason is also the second
line in the log:

```bash
grep 'audio backend\|native PipeWire backend unavailable' \
  ~/.local/share/com.fbnlrz.inari/logs/inari.log
```

On the fallback you also lose the parametric EQ, the mixes (the Mixes group is
hidden entirely) and the processed microphone — all three need the native
engine. Channels, volume, mute and app routing still work.

The usual cause is that there is no reachable PipeWire session for Inari to
connect to: PipeWire isn't running, you're on plain PulseAudio, or Inari started
before the session's PipeWire did. Quit from the tray and start it again once
your session is fully up.

If the meters are dead *and* the engine says `native`, the audio isn't going
through Inari at all — the app is still playing on your default output. See
[Put your apps on channels](/guide/first-steps#put-your-apps-on-channels).

## Does Inari keep running when I close the window?

Yes. The close button hides the window to the tray instead of quitting — the
audio nodes, the routing and the headset connection all stay up. Use **Quit** in
the tray menu to actually stop it. The tray menu also carries per-channel mute
rows and the profile list, and starting `inari` again just raises the window
that is already running rather than opening a second copy.

One side effect: Inari stops watching the PipeWire graph while the window is
hidden, so an app started while Inari sits in the tray may not be moved onto its
channel until you show the window again.

## Do I need root, or a background daemon?

No. Inari is a normal user application — no daemon, no system service, nothing
running as root.

The only privileged piece is a udev rule (`60-inari.rules`) that the package
installs for SteelSeries hardware. It tags the vendor's `hidraw` nodes with
`uaccess`, which hands read/write to whoever is logged in at the seat, so Inari
can talk to a headset or mouse as your own user. Autostart, if you enable it, is
a **systemd user** unit at `~/.config/systemd/user/inari.service`.

Nothing listens on the network either, unless you switch on
[Inari Remote](/guide/remote) — and even then it binds loopback until you pick
another address, and refuses any client that doesn't present the paired token.

## Does it run on ARM / aarch64?

No. Builds are x86_64/amd64 only — the `.deb`, `.rpm` and AppImage on the
releases page are all amd64. There is no aarch64 build, so a Raspberry Pi or an
ARM laptop is out.

## Is there a Flatpak or an AUR package?

No Flatpak. Releases ship three things: a `.deb`, an `.rpm` and an AppImage.
On a distro without native packaging — Arch included — use the AppImage.

The `packaging/arch` and `packaging/fedora` directories in the repository are
inherited from the upstream Sink project and describe *its* AUR package
(`sink-bin`) and *its* COPR repository. Neither publishes Inari.

## What happens to my audio if I quit or uninstall Inari?

Nothing breaks. Quitting from the tray tears down the virtual sinks it created;
apps that were playing into them fall back to your system default output, the
same as if the channels had never existed. Saved assignments stay on disk, so
starting Inari again puts everything back where it was.

Uninstalling removes the binary and the udev rule. Your configuration in
`~/.config/inari` and the WirePlumber routing fragment are left behind — see
[Configuration & files](/reference/configuration) if you want them gone too.

## Do I need SteelSeries hardware?

No. The audio side — channels, per-app routing, mixes, EQ, the processed
microphone — works on any PipeWire system with no special hardware at all.

Device control is the optional half. The Headset, OLED and Mouse tabs show a
"not connected" state when the matching device is absent and stay out of your
way. See [Supported hardware](/reference/hardware) for what's covered.

## Are there keyboard shortcuts?

Yes, three kinds.

**Global hotkeys** (since v1.0.8). Four actions can be bound in
**Settings → Hotkeys**: mute the microphone, mute a chosen channel, load the
next profile, and show/hide the window. Nothing is bound by default. On Wayland
the compositor owns the keyboard and a grab can register but never fire — bind
your desktop's own shortcuts to the [CLI](/reference/cli) instead. Details:
[Hotkeys](/features/hotkeys).

**The tray.** The **Mute** submenu has a row for the microphone and one per
channel, so you can mute Chat without opening the window.

**In-window keys.** Every slider is keyboard-operable. Tab to a fader, an app
volume slider, a mic DSP slider or the balance bar, then:

| Key | Effect |
| --- | --- |
| `↑` / `→` | One step up |
| `↓` / `←` | One step down |
| `PageUp` / `PageDown` | Ten steps |
| `Home` | Minimum |
| `End` | Maximum |

For channel faders a step is 1%, so `PageUp` moves 10%. `Esc` closes any open
menu or dialog.

## What is the relationship to the upstream "sink" project?

Inari is a fork of [NC1107/sink](https://github.com/NC1107/sink). All of the
original PipeWire audio-mixing work — channels, routing, mixes, the mic chain —
is by [@NC1107](https://github.com/NC1107); if you find it useful, please
support the upstream project. This fork adds the Linux-only SteelSeries device
control (headset, OLED, mouse), which upstream deliberately keeps out of the
main app.

PipeWire node names are kept identical to upstream's on purpose, so existing
OBS and Discord setups keep working after a migration. See
[Contributing](/contributing) for the full credits.

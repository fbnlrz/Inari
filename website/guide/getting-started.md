---
title: Getting started
description: Install Inari on Debian/Ubuntu, Fedora, openSUSE or Arch, and set up the optional helpers the OLED modes shell out to.
---

# Getting started

Inari is a Linux audio mixer for PipeWire with SteelSeries device control. It
runs on Debian/Ubuntu (and derivatives), Fedora and Arch.

Only **x86_64 / amd64** builds are published — there is no ARM/aarch64 package
or AppImage.

## Quick install (Debian / Ubuntu)

Install or update to the latest release in one line:

```bash
curl -fsSL https://raw.githubusercontent.com/fbnlrz/Inari/main/get-inari.sh | bash
```

Or with wget:

```bash
wget -qO- https://raw.githubusercontent.com/fbnlrz/Inari/main/get-inari.sh | bash
```

This grabs the latest `.deb` from the
[releases](https://github.com/fbnlrz/Inari/releases) and installs it with apt.
Re-run it any time to update — or let the app update itself (see
[Updating](/guide/updating)).

To uninstall:

```bash
curl -fsSL https://raw.githubusercontent.com/fbnlrz/Inari/main/get-inari.sh | bash -s -- --uninstall
```

## Prebuilt packages

Every release ships prebuilt bundles on the
[Releases page](https://github.com/fbnlrz/Inari/releases):

::: code-group

```bash [Debian / Ubuntu]
sudo apt install ./Inari_*_amd64.deb
```

```bash [Fedora]
sudo dnf install ./Inari-*.x86_64.rpm
```

```bash [openSUSE]
sudo zypper install ./Inari-*.x86_64.rpm
```

```bash [AppImage]
chmod +x Inari_*_amd64.AppImage
./Inari_*_amd64.AppImage
```

:::

## Requirements

- PipeWire with `pipewire-pulse` and WirePlumber 0.5+ (the default on most
  current distros).
- For SteelSeries devices: the installer sets up a udev rule so Inari can talk
  to the hardware over USB without root. It reloads and re-triggers udev for
  you; re-plug the device only if it still isn't detected.

## Optional dependencies

Several OLED features shell out to programs Inari does not bundle. None of them
are required to run the app, and none of them are installed for you. When one is
missing the feature fails silently — the mode still shows up in the picker, it
just stays empty.

| Program | Usual package | Needed for |
| --- | --- | --- |
| `playerctl` | `playerctl` | The **Now playing** and **Album art** OLED modes (title, artist, progress and cover come from `playerctl metadata`) |
| `dbus-monitor` | `dbus-bin` on Debian/Ubuntu, `dbus` elsewhere | Mirroring desktop notifications to the OLED |
| `ffmpeg` | `ffmpeg` | Playing video files on the OLED (images and GIFs decode in-process and need nothing) |
| `nvidia-smi` | your NVIDIA driver package | GPU load in the **CPU/GPU graphs** mode. AMD GPUs are read from amdgpu sysfs and need nothing |
| `curl` | `curl` | The **Weather** OLED mode, and the in-app updater |

::: warning Weather sends a request to a third party
The **Weather** mode calls `https://wttr.in/?format=j1` with no location of its
own, so wttr.in derives your approximate location from your IP address. Inari
otherwise runs entirely locally with no daemon and no root; this one mode is the
exception. Leave it out of the rotation if you'd rather not make that request.
:::

## Limits

- Up to **10 channels**.
- The per-channel software EQ starts with **5 bands** and can be extended to
  **10**. (The headset's own hardware EQ is a separate, fixed 10-band EQ.)

## Next steps

- [Building from source](/guide/building)
- [Updating](/guide/updating)
- [SteelSeries devices](/features/headset)
- [Supported hardware](/reference/hardware)

Config lives in `~/.config/inari` — see
[Configuration & files](/reference/configuration). Coming from the upstream
Sink? Run `./migrate-to-inari.sh` once to move your existing `~/.config/sink`
over.

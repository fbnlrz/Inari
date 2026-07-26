# Getting started

Inari is a Linux audio mixer for PipeWire with SteelSeries device control. It
runs on Debian/Ubuntu (and derivatives), Fedora and Arch.

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

```bash [Fedora / openSUSE]
sudo dnf install ./Inari-*.x86_64.rpm
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
  to the hardware over USB without root. Re-plug the device once after the first
  install.
- `ffmpeg` is optional and only needed to play videos on the headset OLED.

## Next steps

- [Building from source](/guide/building)
- [Updating](/guide/updating)
- [SteelSeries devices](/guide/devices)

Config lives in `~/.config/inari` as plain JSON. Coming from the upstream Sink?
Run `./migrate-to-inari.sh` once to move your existing `~/.config/sink` over.

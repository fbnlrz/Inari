# Building from source

Works on any distro. The `install.sh` script handles dependencies, builds a
release binary, and installs the app, desktop entry and udev rule.

```bash
git clone https://github.com/fbnlrz/Inari && cd Inari
./install.sh
```

`install.sh` installs the build/runtime dependencies for Debian/Ubuntu (and
derivatives), Fedora and Arch. Useful flags: `--deps-only`, `--no-deps`,
`--uninstall` (uninstall keeps your `~/.config/inari`).

## Manual dependencies (Debian / Ubuntu)

```bash
sudo apt update
sudo apt install -y build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
  libssl-dev libxdo-dev libpipewire-0.3-dev clang ffmpeg
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # if you have no Rust
npm install && ./node_modules/.bin/tauri build --no-bundle
sudo install -Dm755 target/release/inari /usr/bin/inari
```

`ffmpeg` is optional and only needed to play videos on the headset OLED.

## Development

```bash
npm install
npm run tauri dev      # run
npm run tauri build    # package
```

Config lives in `~/.config/inari` as plain JSON. Contributions welcome — the
repo is a fork of [NC1107/sink](https://github.com/NC1107/sink); all the
original PipeWire audio-mixing work is by [@NC1107](https://github.com/NC1107).

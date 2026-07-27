# Contributing to Inari

Thanks for your interest! Inari is a fork of
[NC1107/sink](https://github.com/NC1107/sink) that adds SteelSeries device
control. Contributions — bug fixes, new device support, docs — are welcome.

- **Questions & help:** [Discussions](https://github.com/fbnlrz/Inari/discussions)
- **Bugs:** open a [bug report](https://github.com/fbnlrz/Inari/issues/new?template=bug_report.yml)
- **Want another device supported?** open a
  [device support request](https://github.com/fbnlrz/Inari/issues/new?template=device_support.yml)
- **Full docs:** https://fbnlrz.github.io/Inari/

## Development setup

```bash
git clone https://github.com/fbnlrz/Inari && cd Inari
./install.sh --deps-only     # install build/runtime deps for your distro
npm install
npm run tauri dev            # run in dev mode
```

See [Building from source](https://fbnlrz.github.io/Inari/guide/building) for
manual dependency lists.

## Before you open a PR

The CI (and the release gate) run these — please make sure they pass locally:

```bash
npx tsc --noEmit
npm test
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

PRs target `main`. Releases are cut automatically from the version in the
manifest, so you don't need to bump versions in a normal PR.

## Adding a new device

Device support is the most valuable kind of contribution. The hardware layer
lives in `src-tauri/src/headset/` (Arctis + OLED) and `src-tauri/src/mouse/`
(Aerox), talking to `/dev/hidraw*` directly (no libhidapi). Protocol notes and
the "how devices are wired up" overview are in the
[Protocols reference](https://fbnlrz.github.io/Inari/reference/protocols).

Because the maintainer usually won't own your device, **support depends on you
being able to test on real hardware** (and ideally capture USB traffic). Start
by opening a device support request with the `lsusb` id and any protocol
references you can find.

## Code style

Match the surrounding code. Rust is checked with clippy (`-D warnings`); the
frontend is TypeScript + React with `tsc --noEmit`. Keep the udev/PipeWire and
"call the system tool" patterns consistent with the existing modules.

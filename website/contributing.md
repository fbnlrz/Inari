---
title: Contributing
description: How to build Inari, which checks have to pass, and what to include when contributing support for a new SteelSeries device.
---

# Contributing

Contributions are welcome — bug fixes, new device support and docs. Inari is a
fork of [NC1107/sink](https://github.com/NC1107/sink); all the original PipeWire
audio-mixing work is by [@NC1107](https://github.com/NC1107).

- **Questions & help:** [Discussions](https://github.com/fbnlrz/Inari/discussions)
- **Report a bug:** [bug report](https://github.com/fbnlrz/Inari/issues/new?template=bug_report.yml)
- **Request another device:** [device support request](https://github.com/fbnlrz/Inari/issues/new?template=device_support.yml)

## Development setup

```bash
git clone https://github.com/fbnlrz/Inari && cd Inari
./install.sh --deps-only     # build/runtime deps for your distro
npm install
npm run tauri dev
```

## Checks (run before a PR)

The CI and the release gate run all of these:

```bash
npx tsc --noEmit
npm test
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

PRs target `main`. Releases are cut automatically from the manifest version, so
no manual version bump is needed in a normal PR.

## Adding a device

The most valuable contribution. See the
[Protocols reference](/reference/protocols) for where the code lives and how a
device is wired up. Because the maintainer usually won't own your hardware,
**support depends on you being able to test on real hardware.** Start with a
[device support request](https://github.com/fbnlrz/Inari/issues/new?template=device_support.yml).

The full `CONTRIBUTING.md` lives
[in the repo](https://github.com/fbnlrz/Inari/blob/main/CONTRIBUTING.md).

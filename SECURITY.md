# Security policy

## Reporting a vulnerability

Please report security issues privately via GitHub's
[**Report a vulnerability**](https://github.com/fbnlrz/Inari/security/advisories/new)
form rather than opening a public issue. I'll respond as soon as I can.

## Scope & context

Inari is a Linux desktop app that:

- Talks to SteelSeries USB HID devices via a udev `uaccess` rule (no root
  daemon; access is scoped to the logged-in user).
- Runs audio entirely in user space on PipeWire.
- For in-app `.deb` updates, downloads a release asset and installs it with
  `pkexec apt-get` (a polkit prompt) — only over HTTPS and only assets from this
  repo's releases.

The supported version is always the latest release. Fixes ship in a new release.

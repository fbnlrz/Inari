---
title: Updating
description: How Inari's in-app updater works, when the one-click self-update is offered, and how to update from the command line.
---

# Updating

## In-app updates

Inari checks for new releases on launch and shows a banner when one is
available. You can also check manually in **Settings → Updates → Check now**.

For `.deb` installs, click **Update now**: Inari downloads the latest release,
installs it via `pkexec` (a graphical polkit password prompt), and then
**restarts itself** on the new version. No terminal needed.

::: tip
The one-click self-update is offered only when Inari was installed from the
`.deb` (via the installer or the release page) and `pkexec` is available. For
AppImage or source installs, the banner links to the release notes instead.
:::

## Command line

Re-running the installer updates to the latest release (and no-ops if you are
already current):

```bash
curl -fsSL https://raw.githubusercontent.com/fbnlrz/Inari/main/get-inari.sh | bash
```

## How releases work

Releases are cut automatically from the version in the project manifest. Every
tagged release ships a `.deb`, an `.rpm` and an AppImage, with `SHA256SUMS` to
verify your download — all on the
[Releases page](https://github.com/fbnlrz/Inari/releases).

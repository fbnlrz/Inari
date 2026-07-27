# Troubleshooting

Symptom → cause → fix. If none of these help, search
[issues](https://github.com/fbnlrz/Inari/issues) or ask in
[Discussions](https://github.com/fbnlrz/Inari/discussions).

## A SteelSeries device shows "not connected"

The udev rule may not have applied yet. **Re-plug the device once** so it takes
effect, then reopen the tab. Confirm your user can read/write the device's
`/dev/hidraw*` node (the installer's `uaccess` rule handles this — see
[Supported hardware](/reference/hardware)).

## The OLED tab says "display not supported"

That model has no drivable display (e.g. the older Arctis Pro Wireless). The
128×64 OLED is specific to the Arctis Nova Pro base station.

## No audio after enabling anti-crackle headroom

The headroom fix is a WirePlumber fragment that applies **at next login** — log
out and back in (or reboot). It deliberately does *not* restart WirePlumber
live, which would tear down Inari's virtual sinks.

## The in-app update installed but didn't restart

Fixed in **v1.0.3**. Earlier builds couldn't relaunch after an in-place upgrade
(the running binary's file had been replaced). Update once more from the command
line, then in-app updates restart cleanly:

```bash
curl -fsSL https://raw.githubusercontent.com/fbnlrz/Inari/main/get-inari.sh | bash
```

## Devices show as "Sink" / "(Sink)" in Discord or OBS

You migrated from the upstream Sink. PipeWire **node names are kept stable on
purpose** so existing routing keeps working; only the display label follows the
app name for new setups. Re-select the "Inari" devices in the other app if you
want the new label.

## "Download is performed unsandboxed as root" during install

Harmless — apt just couldn't sandbox a local `.deb` in a private temp dir. The
install still succeeds. The installer and in-app updater make the temp file
world-readable to avoid the notice on current versions.

## I still have both `sink` and `inari` installed

The upstream Sink was a separate package. Remove it and keep Inari:

```bash
sudo apt remove sink
```

Leftover files from an old source install can be cleared with
`./migrate-to-inari.sh --remove-old`.

## Videos won't play on the OLED

Install `ffmpeg` — it's optional and only needed for OLED video playback.

## Collecting logs for a bug report

Inari logs to a small rotating file, so you don't have to reproduce the problem
to report it — the run it happened in is already on disk:

```
~/.local/share/com.fbnlrz.inari/logs/inari.log
```

Started by the autostart unit, the same lines also land in the journal:

```bash
journalctl --user -u inari -n 200
```

The default level records startup, device connect/disconnect, profile switches
and every warning or error. For more detail, quit Inari and start it from a
terminal with the level raised — this affects the log file too:

```bash
RUST_LOG=debug inari
```

Attach the log to your [issue](https://github.com/fbnlrz/Inari/issues/new/choose).
It contains device and audio-node names, no credentials.

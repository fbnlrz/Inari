# Headset — Arctis Nova Pro Wireless

Full control of the base station over USB — no root, no SteelSeries GG. The tab
shows a friendly "not connected" state when the headset is absent.

![Headset](/headset.png)

## What you can control

- **Battery** — live headset and charge-slot levels.
- **Noise control** — Off / Transparency (with level) / ANC.
- **Audio gain** — Low / High.
- **Anti-crackle headroom** — a WirePlumber tweak for setups that crackle
  (applies at next login; see [Troubleshooting](/troubleshooting)).
- **Microphone** — mic volume, sidetone (Off/Low/Med/High), mute-LED brightness.
- **Hardware equalizer** — a 10-band EQ baked into the headset, with a large
  library of presets. This is separate from Inari's per-channel software EQ.
- **Line out** — Speaker / Stream mode and per-output volumes.
- **Power & wireless** — auto-off timer, 2.4 GHz Speed/Range mode.
- **ChatMix** — the hardware wheel's game/chat balance, read live.

## Notes

This is the EQ and hardware baked into the headset itself. The firmware
repaints the base station's own UI constantly, which is why the
[OLED](/features/oled) feature runs a continuous redraw loop.

See the [Protocols reference](/reference/protocols) for the HID details.

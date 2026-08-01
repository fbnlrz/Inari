---
title: Headset
description: Control the Arctis Nova Pro Wireless base station over USB — battery, ANC, mic, the headset's own 10-band EQ, line out and ChatMix, without root or SteelSeries GG.
---

# Headset — Arctis Nova Pro Wireless

Full control of the base station over USB — no root, no SteelSeries GG. Inari
speaks the vendor HID protocol on `/dev/hidraw` directly. The tab shows a
friendly "not connected" state when the headset is absent.

Supported base stations (USB product ids): `1038:12e0`, `12e5`, `225d` — the
Nova Pro Wireless and the Xbox "X" variants.

![Headset](/headset.png)

## What you can control

- **Live status** — headset and charge-slot battery, output volume, wireless
  pairing, Bluetooth, mic mute, power/charging state. Pushed live from the
  device's own event stream, plus a 2-second heartbeat.
- **Noise control** — Off / Transparency (with passthrough level) / ANC.
- **Audio gain** — Low / High, showing what the station is actually set to.
  (Before v1.0.13 the control was hard-wired to "Low", so clicking "Low" to
  toggle actually undid the change.)
- **Anti-crackle headroom** — an optional WirePlumber fragment for setups that
  crackle. WirePlumber only reads it at startup, so it applies at next login
  (see [Troubleshooting](/troubleshooting)).
- **Microphone** — mic volume, sidetone (Off/Low/Med/High), mute-LED brightness.
- **Hardware equalizer** — the 10-band EQ baked into the headset, ±10 dB per
  band (the device stores it in 0.5 dB steps), plus its built-in presets and a
  large preset library, grouped by category. This is separate from Inari's
  per-channel software EQ.

  The faders start from the curve the station already holds and show their
  value in dB against a 0 dB line. Before v1.0.13 they started at ten zeros,
  which meant the first fader you touched wrote a flat curve over whatever the
  station was playing — and followed it with a save to the headset's flash.
- **Line out** — Speaker / Stream mode, with the stream mix: main, aux and
  microphone shares. The station stores these in steps of 5 and floors anything
  between, so the slider snaps to match what the device will actually keep.
- **Power & wireless** — auto shut-off timer, 2.4 GHz Speed/Range mode.
- **ChatMix** — the hardware wheel's game/chat balance, read live.

Changes apply immediately and are saved to the headset's own memory (debounced,
so dragging a slider doesn't hammer the flash).

## Notes

- Don't run another Arctis controller at the same time (SteelSeries GG under
  Wine, `Linux-Arctis-Manager`/`lam-daemon`, …) — two programs writing to the
  same base station will fight over it.
- The firmware constantly repaints the base station's own UI, which is why the
  [OLED](/features/oled) feature runs a continuous redraw loop.
- The protocol was cross-checked against HeadsetControl,
  elegos/Linux-Arctis-Manager and loteran/Arctis-Sound-Manager, and verified
  against real hardware. OLED encoding follows JerwuQu/ggoled.

See the [Protocols reference](/reference/protocols) for the HID details and
[Supported hardware](/reference/hardware) for the udev setup.

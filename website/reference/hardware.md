---
title: Supported hardware
description: USB ids and support status for the Arctis Nova Pro Wireless base station, its OLED, the Aerox 9 Wireless and the older Arctis Pro Wireless — plus how Inari picks a device and what adding a model takes.
---

# Supported hardware

Inari's audio mixing works on any PipeWire system. Device control is
Linux-only and currently targets the hardware below. Builds are x86_64/amd64
only.

| Device | USB IDs | Support |
| --- | --- | --- |
| **Arctis Nova Pro Wireless** (base station) | `1038:12e0` · `12e5` · `225d` | ✅ Full — battery, ANC/transparency, sidetone, mic, hardware EQ, line-out, power, ChatMix |
| **Arctis Nova Pro** — OLED (128×64) | (same base station) | ✅ Full — live modes, media, images/GIFs/video, notifications |
| **Aerox 9 Wireless** (mouse, cable) | `1038:185a` · `1876` (WOW Edition) | ✅ DPI, polling, RGB, power saving, battery (write paths verified for lighting/DPI) |
| **Aerox 9 Wireless** (mouse, 2.4 GHz dongle) | `1038:1858` · `1874` (WOW Edition) | ✅ Same, over the dongle |
| **Arctis Pro Wireless** (older, 2019) | `1038:1290` · `1294` | ⚠️ Partial — different protocol; display not reverse-engineered |
| **Apex Pro TKL Wireless (2023)** (keyboard) | `1038:1632` (cable) · `1630` (dongle) | ✅ Per-key RGB, OLED, brightness, battery, firmware — all verified on hardware |
| **Apex Pro Gen 3** / **Pro TKL Gen 3** (keyboard) | `1038:1640` · `1642` | ✅ Per-key RGB, OLED — from OpenRGB/apex-tux/OmniLED, not yet measured |
| **Apex Pro TKL Wireless Gen 3** (keyboard) | `1038:1646` (cable) · `1644` (dongle) | ✅ Same, over either transport |
| **Apex Pro / Pro TKL / 7 / 7 TKL / 5** (2019 keyboards) | `1038:1610` · `1614` · `1612` · `1618` · `161c` | ✅ Per-key RGB, OLED — legacy dialect, not measured |
| **Apex Pro TKL (2023)** (keyboard, wired) | `1038:1628` | ✅ Per-key RGB, OLED transport is a guess — the picker finds it |
| **Apex 9 TKL** / **9 Mini** (keyboard) | `1038:1634` · `1620` | ✅ Per-key RGB (shorter report); no display |
| Adjustable **actuation** on any Apex Pro | — | ❌ The known command is accepted and does nothing; Rapid Trigger and Protection Mode have never been captured |

## How a device is picked

All of the above live in one table in `src-tauri/src/device/mod.rs`. Inari walks
`/sys/class/hidraw`, keeps the nodes whose vendor is `1038` and whose product id
the table claims, and then confirms each candidate, because a product id alone
is not enough on devices that expose several HID interfaces:

- the **Nova Pro** base station is identified by its report descriptor starting
  with the vendor usage page `06 c0 ff` — the other interface is consumer/media
  keys and cannot drive status or the OLED;
- the **Aerox 9** is identified by USB interface number **3**, the only one that
  accepts configuration commands;
- the older **Arctis Pro Wireless** has a single control interface, so the
  product id settles it;
- the **wireless Apex keyboards** are identified by the same vendor usage page
  `06 c0 ff`, which is what separates their configuration collection from the
  plain keyboard, the media keys and the mouse-emulation collection they also
  expose — six HID interfaces on the board that was measured. The wired Apex
  boards are identified by USB interface number **1**.

When more than one node still matches, the highest-priority entry wins: the Nova
Pro beats the older Arctis Pro, and the Aerox's **cable beats its dongle** — with
both plugged in the mouse is physically on the cable, and the idle dongle just
answers `0xff`.

::: tip Want another device?
Adding a model is one entry in that table — product ids, display name, class,
how to confirm a candidate node, report length, capabilities and priority —
plus, for a headset, one implementation of the per-generation protocol trait
(handshake, heartbeat, sidetone, auto-off, status decoding). Nothing else in the
app branches on the model.

Open a
[**device support request**](https://github.com/fbnlrz/Inari/issues/new?template=device_support.yml)
with the `lsusb` id and any protocol references you can find. Support depends on
someone with the hardware being able to test — see
[Contributing](/contributing).
:::

## Access & permissions

The installer ships a udev rule (`60-inari.rules`) that grants the
logged-in desktop user access to SteelSeries HID devices via `uaccess` — no root
and no background daemon. Both the `.deb` postinst and `install.sh` reload and
re-trigger udev, so the rule normally applies immediately. If the device still
isn't detected, re-plug it once.

# Supported hardware

Inari's audio mixing works on any PipeWire system. Device control is
Linux-only and currently targets the hardware below.

| Device | USB IDs | Support |
| --- | --- | --- |
| **Arctis Nova Pro Wireless** (base station) | `1038:12e0` · `12e5` · `225d` | ✅ Full — battery, ANC/transparency, sidetone, mic, hardware EQ, line-out, power, ChatMix |
| **Arctis Nova Pro** — OLED (128×64) | (same base station) | ✅ Full — live modes, media, images/GIFs/video, notifications |
| **Aerox 9 Wireless** (mouse) | `1038:1858` (dongle) · `185a` (wired) · `1874`/`1876` (WOW) | ✅ DPI, polling, RGB, power saving, battery (write paths verified for lighting/DPI) |
| **Arctis Pro Wireless** (older, 2019) | `1038:1290` | ⚠️ Partial — different protocol; display not reverse-engineered |

::: tip Want another device?
Open a
[**device support request**](https://github.com/fbnlrz/Inari/issues/new?template=device_support.yml)
with the `lsusb` id and any protocol references you can find. Support depends on
someone with the hardware being able to test — see
[Contributing](/contributing).
:::

## Access & permissions

The installer ships a udev rule (`50-sink-steelseries.rules`) that grants the
logged-in desktop user access to SteelSeries HID devices via `uaccess` — no root
and no background daemon. Re-plug the device once after the first install so the
rule applies.

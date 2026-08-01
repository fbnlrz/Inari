---
title: Protocols
description: How Inari finds and talks to SteelSeries devices over /dev/hidraw — report formats, the OLED feature report and what to check when adding a device.
---

# Protocols (for contributors)

Inari talks to SteelSeries devices by writing HID reports to `/dev/hidraw*`
directly — no libhidapi/libusb dependency. This page is an orientation for
anyone adding or fixing a device; the code is always the source of truth.

The protocols were learned and cross-checked from
[HeadsetControl](https://github.com/Sapd/HeadsetControl),
[elegos/Linux-Arctis-Manager](https://github.com/elegos/Linux-Arctis-Manager),
[loteran/Arctis-Sound-Manager](https://github.com/loteran/Arctis-Sound-Manager),
[rivalcfg](https://github.com/flozz/rivalcfg) and
[JerwuQu/ggoled](https://github.com/JerwuQu/ggoled) (OLED encoding).

## Where the code lives

- `src-tauri/src/headset/protocol.rs` — Arctis Nova Pro commands & status frame
- `src-tauri/src/headset/oled.rs`, `oled_controller.rs` — OLED framebuffer + redraw loop
- `src-tauri/src/headset/hidraw.rs` — raw `/dev/hidraw` access & feature reports
- `src-tauri/src/mouse/protocol.rs` — Aerox 9 commands
- `packaging/udev/60-inari.rules` — `uaccess` rule for vendor `0x1038`

## Arctis Nova Pro Wireless

- The base station exposes several HID interfaces. Inari does **not** pick one
  by interface number — it walks `/sys/class/hidraw` and takes the node whose
  `report_descriptor` starts with the vendor usage page `06 c0 ff`
  (`Usage Page (0xFFC0)`). That is what separates the control/OLED collection
  from the consumer/media-key one. See `VENDOR_USAGE_PAGE` and `find_device()`
  in `headset/hidraw.rs`.
- Output reports are report id `0x06`, zero-padded to 64 bytes. Replies are
  tagged `0x06`, unsolicited events `0x07`.
- Status is queried with `06 b0`; the reply frame carries battery, ANC, mic,
  power and wireless fields at fixed byte offsets.
- Commands set sidetone, mic volume/LED, ANC/transparency, auto-off, gain,
  wireless mode, line-out and a 10-band EQ. `06 09` saves to the device.
- **OLED** is a *Feature* report (needs the `HIDIOCSFEATURE` ioctl, not
  `write()`): a 1024-byte frame `[0x06, 0x93, dst_x, 0, strip_w, padded_h, …]`,
  128 px sent as two 64 px strips, body column-major LSB-first. The firmware
  repaints its own UI, so frames must be pushed **continuously** to hold custom
  content.

## Aerox 9 Wireless

- Control is on **USB interface 3**; reports are **unnumbered**, variable
  length, not padded.
- **Wireless quirk:** on the dongle PIDs, OR `0x40` onto the *first command
  byte only*. Leave ~50 ms between commands.
- Commands set DPI presets, polling rate, per-zone RGB, reactive lighting,
  rainbow, startup lighting and sleep/dim timeouts. `11 00` saves to onboard
  memory.
- DPI uses a **lookup table**, not `dpi/100`.
- Battery: write `92`, read 2 bytes; `percent = (level - 1) * 5`, bit 7 =
  charging. Reject out-of-range values (a disconnected wireless mouse reports
  `0xff`).

## Apex keyboards

Measured on an **Apex Pro TKL Wireless (2023)** (`1038:1632`, firmware 3.24.1)
over `/dev/hidraw` on 2026-08-01. The vendor interface (USB interface 3, usage
page `0xFFC0`) declares exactly three reports, and probing confirmed each:

- **Output, 64 bytes** — configuration. Written as 65 bytes,
  `[0x00 report id][cmd][payload…]`: zone colour `0x21`, brightness `0x22`
  (0–100), apply `0x09`, reactive `0x25`, colour shift `0x26`, profile `0x89`,
  firmware query `0x90`, battery `0x92`, region `0xF5`.
- **Input, 64 bytes** — query replies, which arrive **one command behind**.
  Drain the queue before trusting an answer. `0x90` replies with bare ASCII
  (`3.24.1`); `0x92` replies `92 95`, where `0x95` is 95 % as two BCD digits —
  matching what the keyboard's own indicator showed.
- **Feature, 641 bytes** — bulk payloads, with the **command byte first**:
  `[cmd][payload…]`, padded to exactly 641. This is the part that bites:
  prefixing a `0x00` report id instead (what OpenRGB and hidapi's convention
  produce) makes this device stall the transfer, and 640, 644 or 64 bytes are
  refused outright. Enough stalls in a row make the keyboard re-enumerate.

### Per-key lighting

`[cmd][count][hid R G B]…` in a feature report. The command is `0x3A` on the
2019 boards, `0x40` on wired 2023/Gen 3 ones and `0x61` on the wireless ones.
One packet carries all **112 HID usage ids** in a fixed order (OpenRGB's
`SteelSeriesApexController`); the firmware picks the ones its board has and
ignores the rest, which is why the same packet drives full-size, TKL and mini
boards. Verified by lighting four scattered keys and leaving the other 108 dark.

Direct mode is **sticky**: the board holds the last frame indefinitely, so
`0x41` (`0x3B` on the 2019 boards) has to be sent to hand the LEDs back.

Gen 3 firmware wants `0x4B` first. The measured board accepted direct frames
with and without it. A Gen 1/2 board on firmware 1.19.7 or newer speaks the
Gen 3 dialect — the product id does not change with the update, so the firmware
string decides.

### The 128×40 OLED

Two independent choices: addressing (one report, or eight offset-addressed
80-byte chunks) and pixel packing (row-major, or SSD1306 page-major). Verified
on the 2023 wireless board: **eight chunks, command `0x0C`, row-major** —
`[cmd][0x01][offset LE][0x50][pad][80 bytes]`. `apex-tux` and OmniLED use other
combinations for other models (single reports with `0x61`, `0x0A`, `0x4A`, or a
four-byte `38 83 00 00` header), so Inari keeps all of them and the Display tab
can send a test picture with each.

The firmware composites its own status strip — profile name and battery — over
the top of whatever is sent, so Inari's screens start 10 px down. It also
repaints on its own events, so frames are re-sent about twice a second.

### What nobody has

**Rapid Trigger, Rapid Tap/SOCD and Protection Mode have never been captured.**
The actuation command `0x2D` from third-party reverse engineering is accepted by
the hardware and changes nothing on firmware 3.24.1. The keyboard tab has a raw
command probe for anyone who wants to help find them.

## Adding a device

1. Confirm the USB `Vendor:Product` id, then work out how to recognise the
   control node — a distinctive report-descriptor prefix if the device exposes
   several HID collections (as the Nova Pro does), or the product id alone if it
   exposes only one (as the older Arctis Pro does).
2. Find or capture the command set (existing tools above, or USBPcap/Wireshark).
3. Add a protocol module mirroring the existing ones, plumb it through a store
   and a presence-aware screen.
4. Verify on real hardware — this is essential, since the maintainer usually
   won't have the device.

See [Contributing](/contributing) to get started.

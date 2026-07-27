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

- Vendor control is on **USB interface 4**; output reports are report id `0x06`,
  zero-padded to 64 bytes. Replies are tagged `0x06`, unsolicited events `0x07`.
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

## Adding a device

1. Confirm the USB `Vendor:Product` id and which interface carries control.
2. Find or capture the command set (existing tools above, or USBPcap/Wireshark).
3. Add a protocol module mirroring the existing ones, plumb it through a store
   and a presence-aware screen.
4. Verify on real hardware — this is essential, since the maintainer usually
   won't have the device.

See [Contributing](/contributing) to get started.

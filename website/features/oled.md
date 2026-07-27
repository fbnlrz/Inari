# OLED — the base station's 128×64 display

Drive the Arctis Nova Pro base station's OLED directly. The tab shows a
"display not supported on this device" state for models without a drivable
panel.

![OLED](/oled.png)

## Live modes

A timed rotation of self-contained screens:

- **Audio** — VU meters (dB scale, peak hold), ChatMix, FFT spectrum.
- **System** — headset status, CPU/GPU graphs, network, uptime/load,
  temperatures.
- **Media** — now playing with progress, dithered album art.
- **Clocks** — several styles, plus a timer and weather.
- **Misc** — mouse battery, active apps.

## Your own content

- **Images, GIFs and video** — scaled to 128×64 and dithered to 1-bit on the
  fly (video needs `ffmpeg`).
- **Text** and **built-in clips** — Japan-themed animations, effects and demos.
- **Notifications** — mirror desktop notifications to the panel, with scrolling
  for long text.

::: tip Why it needs a redraw loop
The base-station firmware continuously repaints its own UI, so a single frame
gets overwritten instantly. Inari holds custom content by pushing frames
continuously (~24 fps). See the [Protocols reference](/reference/protocols).
:::

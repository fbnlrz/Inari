---
layout: home

hero:
  name: Inari
  text: SteelSeries Sonar for Linux
  tagline: Per-app audio routing, capturable OBS mixes and a processed mic for PipeWire — plus Arctis headset, base-station OLED and Aerox mouse control.
  image:
    src: /logo.svg
    alt: Inari
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: View on GitHub
      link: https://github.com/fbnlrz/Inari

features:
  - icon: 🎚️
    title: Per-app audio routing
    details: Send each app to a Game / Chat / Music channel with its own volume, mute, meters and output device.
  - icon: 📼
    title: Capturable mixes for OBS
    details: Master Mix carries everything; custom mixes can carry "everything except music" and stay current as channels change.
  - icon: 🎙️
    title: Processed virtual mic
    details: Noise gate, compressor and limiter into a virtual microphone you select in Discord or OBS.
  - icon: 🎧
    title: Arctis Nova Pro control
    details: Battery, ANC / transparency, sidetone, mic, 10-band hardware EQ, auto-off and more over USB — no root, no SteelSeries GG.
  - icon: 🖥️
    title: Base-station OLED
    details: Drive the 128×64 display — live dashboards, system monitor, now playing, notifications, images, GIFs and video.
  - icon: 🖱️
    title: Aerox 9 Wireless
    details: DPI presets, polling rate, per-zone RGB, reactive lighting, sleep / dim timeouts and battery.
  - icon: 🌃
    title: Tokyo Night theme
    details: Ships the original look plus an opt-in Tokyo Night palette, switchable in Settings.
  - icon: ⬇️
    title: One-line install & in-app updates
    details: Install with a single curl command; the app checks for new releases and updates itself in place.
---

<div style="max-width:960px;margin:3rem auto 0;padding:0 24px;">

## Install in one line

```bash
curl -fsSL https://raw.githubusercontent.com/fbnlrz/Inari/main/get-inari.sh | bash
```

Debian / Ubuntu and derivatives. See [Getting started](/guide/getting-started)
for prebuilt packages, the AppImage, and building from source.

![Mixer](/mixer.png)

</div>

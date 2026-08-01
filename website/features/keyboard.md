---
title: Keyboard
description: Control a SteelSeries Apex keyboard from Linux — per-key RGB with host-rendered effects, the 128×40 OLED, brightness, and an honest account of what the actuation commands actually do.
---

# Keyboard — SteelSeries Apex

Per-key RGB, the OLED and the settings the firmware exposes, for the Apex Pro,
Apex 7, Apex 5 and Apex 9 families — Gen 1 through Gen 3, wired or over the
2.4 GHz dongle. The tab shows a "not connected" state when no board is present.

The screen is split into four sections — **Lighting**, **Keys**, **Display**
and **System** — switched at the top. *Display* and *Keys* only appear for
boards that have a panel and adjustable switches. The key map sits above
*Lighting* and *Keys*, because it is the only place you can see what the board
is actually doing: pick an effect or a scene and the result is right there,
rather than four cards further down.

## Lighting

Ten effects. Six of them Inari renders itself and streams to the board about 30
times a second; the last three are rendered by the keyboard's own firmware, so
they keep running after Inari is closed.

| Effect | Rendered by | Notes |
| --- | --- | --- |
| **Static** | Inari | One colour across the board |
| **Per key** | Inari | Whatever you painted on the key map |
| **Breathing** | Inari | Fades, never fully dark |
| **Wave** | Inari | Rainbow sweeping across, reversible |
| **Rainbow** | Inari | Whole board cycles together |
| **Gradient** | Inari | Blend between two colours |
| **Audio** | Inari | Lights up left to right with what is playing — the same peak levels the mixer's meters read |
| **Scenes** | Inari | Twenty-four themed scenes, below |
| **Reactive** | Keyboard | Keys flash on press |
| **Colour shift** | Keyboard | Two-colour fade |
| **Off** | Keyboard | Hands the LEDs back to the onboard profile |

Brightness is applied host-side for the Inari-rendered effects, so it never
fights the firmware's own brightness setting.

### Scenes

Twenty-four of them. Each is a small piece of shader-style maths over the key's
position and a phase, so they animate across whatever board is attached instead
of being baked for one layout. Speed and brightness come from the Lighting card;
Ripple and Slash use the colour set there.

| Scene | What it looks like |
| --- | --- |
| **Tokyo Night** | The palette Inari themes with, drifting diagonally |
| **Osaka Neon** | Dotonbori signage: saturated blocks with tube flicker and the odd dropout |
| **Sakura** | Petals drifting down-right across a pale dusk, with sway |
| **Kanagawa** | Hokusai's wave — indigo sea, a white foam crest that curls as it rolls through |
| **Foxfire** | Kitsune-bi: three pale flames wandering their own paths over a dark board |
| **Rain** | A drop per column, each with its own speed, trailing as it falls |
| **Fuji Sunrise** | Night at the top, dawn climbing the horizon, the sun sitting on it |
| **Ripple** | Rings spreading from the middle in your colour |
| **Aurora** | Slow curtains of green into violet, warped and brightest up top |
| **Lantern** | Warm amber, every key flickering on its own — no two in sync |
| **Amanogawa** | The Milky Way as a diagonal band, with keys twinkling in it |
| **Slash** | A blade sweeping across every few seconds, trailing light, then dark |
| **Hanabi** | Four shells in flight, each bursting into a shock front with sparks trailing |
| **Torii** | A vermilion gate — two pillars, two beams — breathing, throwing light behind it |
| **Shibuya** | The scramble: long streaks of traffic crossing in two directions |
| **Neon Rain** | Blade Runner downpour in magenta and cyan, with the puddle glow at the bottom |
| **Glitch** | Datamosh — bands tear and hold, channels split, the odd block blows out white |
| **Vaporwave** | A sliced sun above the horizon, perspective grid scrolling below it |
| **Koi** | Two koi circling in dark water, each with a wake |
| **Komorebi** | Bamboo stalks with shafts of light drifting across them |
| **Taiko** | Rings on the beat, every fourth one gold — the silence between hits is the point |
| **Inferno** | Fire climbing from the bottom, eaten away by turbulence on the way up |
| **Plasma** | The demoscene classic: a sum of sines into a very wide palette |
| **Kaminari** | Dark, then a jagged bolt and a flash that lights the whole board |

::: tip Direct mode is sticky
Whatever Inari paints last stays on the board indefinitely — that is how the
hardware works, not a bug. Inari hands the LEDs back when you quit from the
tray or switch the master toggle off.
:::

## The key map

The keyboard is drawn to scale from the same table the effect engine uses, so a
wave that sweeps left to right in the picture sweeps left to right on the desk.

- **Click** a key to paint it with the brush colour, **right-click** to clear it.
- Painting switches the effect to **Per key**; your painting is kept when you
  switch to another effect and back.
- **Fill all** and **Clear** do what they say.
- **ISO / ANSI** picks the layout drawn. The keyboard reports a region byte, not
  a layout, so this one is your call — it only affects the picture and where
  effects think the keys are, never which key lights up.

Keys are addressed by USB HID usage id, and one packet carries all 112 of them;
the firmware picks the ones its board actually has. That is why the same code
drives a full-size, a TKL and a mini board.

## Display

For boards with the 128×40 OLED. Modes: clock, clock with date, CPU/RAM,
temperatures, now playing, a keyboard-status screen, and your own text.

Two things are worth knowing:

- **The firmware draws over the top of the panel.** Its profile name and battery
  indicator are composited over whatever Inari sends, so every Inari screen
  starts 10 px down and treats the panel as 128×30.
- **The screensaver is Inari's.** While Inari is pushing frames the keyboard's
  own screensaver never fires, so Inari blanks the panel itself after the idle
  time you set (default 10 minutes, 0 disables it).

### If the display stays blank or shows noise

Which packet shape the panel understands differs per model, and only one of them
is confirmed on hardware. The **Transport** section sends a test picture — a
bordered X with a filled corner block — with any shape you pick:

- **the X appears** → pin that one;
- **noise** → right transport, wrong pixel packing: try the same command byte
  with the other packing;
- **blank** → the frame reaches the panel and clears it, so the payload is
  landing in the wrong place; try a different addressing mode;
- **the keyboard's own screen** → nothing is getting through.

## Switches

Shown for the Apex Pro TKL Wireless (2023) family, where every command below
was verified on hardware. Other HyperMagnetic boards have adjustable switches
too, but their firmware uses a different set of opcodes, so Inari hides this
section rather than offering controls that would move nothing.

- **Actuation point** — 0.1 mm to 4.0 mm, per key. The slider sets every key;
  the key map's *Per-key actuation* mode overrides individual ones on top. With
  no global setting, only the keys you gave an override are written — painting
  one key does not move the rest of the board.
- **Rapid Trigger** — re-arms a key as soon as it starts travelling back up
  instead of waiting for a fixed reset point. 0 turns it off.
- **Protection Mode** — dampens the keys around the one you meant to press.
- **Rapid Tap (SOCD)** — opposite keys cancel each other, using the pairs
  stored in the keyboard's own profile. Editing those pairs is not exposed yet.

These writes are deliberately not saved to the keyboard's flash, so Inari never
edits the profile the board falls back to on another machine. The trade-off is
that they are lost when the board loses power — Inari re-sends them whenever it
reconnects.

::: info An earlier version of this page was wrong
It said these commands "have never been captured by anyone" and that actuation
did not work. That was true of the third-party write-ups Inari was built from —
`0x2D`, the command they describe, really is accepted and really does nothing.
The actual commands exist and are ordinary: `0x2F` carries a per-key actuation
table in tenths of a millimetre, `0x37` Rapid Trigger, `0x14` Protection Mode,
`0x17` Rapid Tap. Writing 4.0 mm to a single key and feeling it trigger late is
what settled it.
:::

## Idle & power

These are the keyboard's own timers, stored in the board, so they keep working
while Inari is closed — and Inari reads them back rather than assuming, so the
sliders show what the keyboard is actually set to even if something else
changed it.

- **Dim the lighting after** — the keyboard's idle timeout (0 never dims). This
  is the setting people mean by "the keyboard's screensaver".
- **Brightness once dimmed** — 0 to 10.
- **Sleep after** — minutes before the board sleeps (0 never).
- **High-efficiency mode** — wireless boards only; trades lighting for runtime.

::: tip Not the same as the display's screensaver
The *Display* section has its own blanking timer. That one only governs what
Inari paints on the OLED, because the firmware's own screensaver never fires
while Inari is pushing frames. The timers here govern the keyboard.
:::

## Command probe

Sends one raw 65-byte control report. This is how the working commands were
found. The keyboard ignores what it does not understand — but a wrong report
length makes it stall the transfer, and enough stalls in a row make it
re-enumerate. If it stops responding, unplug it once.

## What is verified, and on what

Everything below was measured on an **Apex Pro TKL Wireless (2023)**,
`1038:1632`, firmware 3.24.1, over `/dev/hidraw` on 2026-08-01:

- ✅ **Per-key RGB** — proven by lighting exactly four scattered keys (A, N, R, I)
  and leaving the other 108 dark, which no zone-based control could do.
- ✅ **The OLED** — a full-panel test picture came back on the display.
- ✅ Brightness, zone colour, apply, firmware query (`3.24.1`), battery (95 %,
  matching the keyboard's own indicator).
- ❌ Actuation — accepted, no effect.

The wired boards, including the Apex Pro Gen 3, follow OpenRGB, `apex-tux` and
OmniLED rather than measurement. See the
[Protocols reference](/reference/protocols) for the wire format.

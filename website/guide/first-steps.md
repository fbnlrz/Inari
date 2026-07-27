---
title: First steps
description: What to do after installing Inari — the first-run choice, putting apps on channels, wiring OBS and Discord, and checking that audio really flows.
---

# First steps

Inari is installed. This page takes you from "it launches" to "game, chat and
music sit on their own faders, and OBS records them as separate tracks".

## First launch

Start Inari from your application menu, or run `inari` in a terminal. The
window comes up on its own — unless you enabled autostart with **Start
minimized**, in which case it boots straight to the tray icon.

On the very first run you get a short tour: three cards (*Your sound, on a
board* · *Sort your apps* · *Microphone*), then one decision:

| Choice | What you get |
| --- | --- |
| **Set up a board for me** | Four channels — Game, Chat, Music and System |
| **I'll build my own** | A single channel called **Main**; add the rest as you go |

Skipping the tour does the same thing as "Set up a board for me". Either way
nothing is locked in — you can add, rename, reorder and delete channels at any
time from the Mixer tab (up to 10). You can replay the tour later from
**Settings → Tutorial**.

Each channel becomes a real PipeWire sink (`sink_game`, `sink_chat`, …), and
Inari also creates one always-on **Master Mix** that carries every channel.
That mix is what recorders capture.

![Mixer](/mixer.png)

## Put your apps on channels

Open the **Apps** tab. Anything currently playing audio shows up by itself,
grouped by the channel it's on, with everything else under **Unrouted**. The
list refreshes every two seconds while the window is open.

1. Find the app's row.
2. Open the channel dropdown on the right.
3. Pick a channel.

The stream moves immediately, and the choice is remembered by app identity —
next time that app makes a sound it lands on the same channel without you doing
anything. Apps you have routed before but that aren't running appear under
**Not running**, so you can pre-route something before you launch it.

![Apps](/apps.png)

Two things worth knowing:

- **Assignments are enforced while the Inari window is on screen.** The stream
  poll pauses while Inari sits in the tray, so an app you start during a game
  may play on your default output until you bring the window up once. It moves
  on the next poll — no restart needed. The saved rules are also written to a
  WirePlumber fragment, but WirePlumber only reads those at login.
- **Apps that pick an output device themselves don't follow assignments.**
  Discord, OBS and anything else with its own device menu need the Inari channel
  selected *inside that app* — that's what the two recipes below do.

## Record with OBS

Inari's mixes show up as capture devices, so OBS takes them as **audio inputs**,
not as desktop audio.

::: warning Don't use "Desktop Audio"
Desktop Audio captures your default output — that is everything, already mixed
together. The whole point of the channels is that game, chat and music stay
apart; capturing the desktop throws that split away again.
:::

**One mix, one source:**

1. In OBS, **Sources → + → Audio Input Capture**.
2. Give it a name (e.g. `Game`).
3. Under **Device**, pick the Inari mix you want — `Master Mix` to start with.

**Separate tracks per mix:**

1. On the Mixer tab, add a mix per group you want to record (the **+** on the
   Mixes header — up to four next to the Master Mix). Tick the channels it
   carries, or flip it to exclude mode so new channels join automatically
   ("everything except Music").
2. In OBS, add one **Audio Input Capture** source per mix.
3. **Settings → Output**, switch Output Mode to **Advanced**, and enable the
   audio tracks you want under Recording.
4. In the Audio Mixer panel, **⚙ → Advanced Audio Properties**, and tick the
   track number for each source.

Rename a mix by double-clicking its label — recorders see that name. Each mix
also has its own volume and mute, which only affect what recorders hear.

## Talk on Discord

The processed microphone is **off by default**. Turn it on first:

1. **Mic** tab → enable the chain, and pick your hardware microphone as the
   input.
2. The result is published as a virtual microphone called **Inari Mic**
   (node `sink_mic`), with a noise gate, compressor and limiter in front of it.

![Mic](/mic.png)

Then in Discord, **User Settings → Voice & Video**:

- **Input Device** → `Inari Mic`
- **Output Device** → your chat channel (`Chat`, if you took the default board)

Turn **Noise Suppression** and **Automatic Gain Control** off while you're
there. Inari's gate and compressor already do that job, and stacking two of each
makes the signal pump and swallow word beginnings. Echo Cancellation is a
different thing that Inari does *not* do — keep it on if you listen on speakers.

Discord's output now goes to a channel with its own fader, so you can duck chat
under the game without touching the game's volume.

## Shape a channel

Every channel has its own parametric EQ (up to 10 bands) with bundled presets
and AutoEq import. Open it from the **Equalizer** button on the channel strip.

![Equalizer](/eq.png)

## Check that it's actually working

Three quick tests, in order of how much they tell you:

**The meters move.** Play something and watch the VU meter on the channel strip.
Movement means audio really is flowing through Inari's node. If nothing moves,
see [Why don't my VU meters move?](/faq#why-don-t-my-vu-meters-move).

**The sinks exist.** In a terminal:

```bash
pactl list sinks short | grep sink_
```

You should see one line per channel — `sink_game`, `sink_chat`, and so on. The
mixes and the microphone are sources, not sinks:

```bash
pactl list sources short | grep sink_
```

**The engine is native.** **Settings → About → Audio engine** should read
"Native PipeWire" with a `native` tag. A `fallback` tag means the native engine
couldn't start, and metering, EQ, mixes and the mic chain are unavailable — see
the [FAQ](/faq#why-don-t-my-vu-meters-move).

## Next

- [Audio mixer](/features/mixer) — channels, mixes, EQ, profiles
- [SteelSeries headset](/features/headset) and the [OLED](/features/oled)
- [Configuration & files](/reference/configuration) — every file Inari writes
- [FAQ](/faq) and [Troubleshooting](/troubleshooting)

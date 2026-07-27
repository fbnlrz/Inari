---
title: Mixes
description: Build recordable sources for OBS — the Master Mix carries everything, and a custom mix can carry "everything except music" and stay current as you add channels.
---

# Mixes — recordable sources for OBS

A mix is a virtual **capture source** that carries a chosen set of channels. It
shows up in OBS, Discord or any recorder as a normal input device you can select
by name. Because a mix is a copy of the channels feeding it, muting or fading a
mix changes what the recorder hears — not what you hear.

Mixes need the native PipeWire engine. On the `pactl` fallback the Mixes group
is hidden (see the engine row in [Settings](/features/settings)).

## Master Mix vs. your own mixes

- **Master Mix** always exists, always carries every channel, and cannot be
  deleted. Its membership is managed for you — add a channel and it joins
  automatically.
- **Your own mixes** — up to four, added with the `+` in the *Mixes* group
  header. Give it a name (1–24 characters); that name is exactly what recorders
  display.

## "Carry these" vs. "everything except these"

This is the part that makes "everything except music" possible. Open a mix's
membership button (it reads `3 channels`, `all but 1`, …) and you get a checkbox
per channel plus one switch at the bottom:

**Auto-include new channels**

- **On** (the default for a new mix) — Inari stores the channels you
  **unchecked**. The mix carries everything else. Uncheck *Music* and you have
  an "everything except music" mix. When you add a *Podcast* channel next month
  it joins this mix automatically, because it was never on the excluded list.
- **Off** — Inari stores exactly the channels you **checked**. The mix carries
  those and nothing more. A new channel does *not* join; you have to check it
  yourself.

Flipping the switch never changes what the mix carries right now — only what
happens to channels created later. That is why the auto-include form is the one
to use for a stream layout you intend to keep: it stays correct as your channel
set grows, while a manually picked set silently goes stale.

## Using a mix in OBS

Add the mix as an **audio input capture** source and pick it by its name — not
Desktop Audio, which would grab everything at once and defeat the point. The
full walkthrough, including the classic "stream hears everything but the music"
setup, is in [First steps](/guide/first-steps).

Renaming a mix updates what recorders display; the underlying node name stays
stable, so an existing OBS source re-attaches on its own instead of breaking.

## Level, mute and monitoring

- **Fader** (0–150 %) and **mute** shape what recorders capture. Muting a mix
  makes the recording silent while your own output is untouched.
- Both are saved with the mix, so they survive restarts and profile switches.
- The **headphones** button monitors the mix on your default output — the
  fastest way to confirm a mix really carries what you think before you go live.
  Monitoring is session-only and is off again after a restart.

Deleting a mix stops any recorder capturing it; channels are unaffected.

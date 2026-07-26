# Audio mixer

Inari routes each app to its own channel — Game, Chat, Music — and gives you
volume, mute and output device per channel, plus recordable mixes for OBS and a
processed virtual microphone.

```
 apps ─► channels ─► your ears
              └────► a Mix ─► OBS / recorder
```

## Channels

Per-app routing with volume, mute, meters, and a choice of output device per
channel.

![Mixer](/mixer.png)

## Apps

Running apps appear automatically; assign once and they are remembered forever.

![Apps](/apps.png)

## Mixes

Recordable sources for OBS. The Master Mix carries everything; custom mixes can
carry "everything except music" and stay current as channels change. In OBS,
add a mix as an audio input — not Desktop Audio.

## Equalizer

Per-channel parametric EQ (up to 10 bands) with a draggable response curve,
bundled community presets, and import/export including AutoEq text blocks.

![Equalizer](/eq.png)

## Microphone

A noise gate, compressor and limiter feed a virtual mic you select in Discord or
OBS. Pairs well with [NoiseTorch](https://github.com/noisetorch/NoiseTorch) on
the input for noise suppression before the chain.

![Mic](/mic.png)

## Profiles

Save and switch full layouts from the tray.

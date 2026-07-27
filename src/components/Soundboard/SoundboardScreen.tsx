import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { cx } from "../../lib/cx";
import { isTauri } from "../../lib/platform";
import { Ms } from "../Icons";
import { Toggle } from "../Toggle";
import { ClipRow } from "./ClipRow";
import { MAX_DUCK_DB, MIN_DUCK_DB, useSoundboard } from "../../store/soundboard";
import type { ClipInfo, PlayTargets } from "../../store/soundboard";

/** The three destinations, in the order a soundboard needs them. */
const TARGETS: { id: PlayTargets; label: string; title: string }[] = [
  { id: "both", label: "Chat + me", title: "The chat hears it and so do you" },
  { id: "chat", label: "Chat only", title: "Fire it without hearing it again" },
  { id: "me", label: "Me only", title: "Audition it - the call hears nothing" },
];

/** Why a pad cannot be pressed, or null when it can. */
function blockedBecause(clip: ClipInfo): string | null {
  if (clip.missing) return "File missing";
  if (!clip.playable) return "Needs ffmpeg";
  return null;
}

/**
 * The board: one big pad per clip.
 *
 * Pressing a pad toggles it - the same pad again stops it, another one swaps.
 * That is the backend's doing in a single command (see store/soundboard.ts);
 * the pad only has to make it obvious which one is currently on, or the user
 * cannot tell whether the next press starts or stops something.
 */
function Board({ clips, playingIds }: Readonly<{ clips: ClipInfo[]; playingIds: string[] }>) {
  const toggle = useSoundboard((s) => s.toggle);

  return (
    <div className="sb-grid">
      {clips.map((clip) => {
        const blocked = blockedBecause(clip);
        const on = playingIds.includes(clip.id);
        return (
          <button
            key={clip.id}
            type="button"
            className={cx("sb-pad", on && "on", blocked && "blocked")}
            // Not hidden: the user added this clip and has to be able to find
            // it again, plugged the drive back in or installed ffmpeg.
            disabled={blocked !== null}
            aria-pressed={on}
            aria-label={
              blocked ? `${clip.name} - ${blocked.toLowerCase()}` : on ? `Stop ${clip.name}` : `Play ${clip.name}`
            }
            onClick={() => void toggle(clip.id)}
          >
            <Ms name={on ? "stop_circle" : "play_arrow"} style={{ fontSize: 26 }} />
            <span className="sb-pad-name">{clip.name}</span>
            {blocked && <span className="sb-pad-tag">{blocked}</span>}
          </button>
        );
      })}
    </div>
  );
}

/** How far the microphone drops while a clip plays. Off by default. */
function DuckCard() {
  const duck = useSoundboard((s) => s.status.duck);
  const setDuck = useSoundboard((s) => s.setDuck);

  return (
    <section className="hs-card">
      <div className="hs-card-head">
        <Ms name="mic" style={{ fontSize: 18 }} />
        <h2>Ducking</h2>
        <div className="hs-card-right">
          <Toggle on={duck.enabled} onClick={() => setDuck(!duck.enabled, duck.attenuation_db)} />
        </div>
      </div>
      <p className="hs-hint">
        Your microphone is turned down while a clip plays, so the chat hears the
        clip over your voice. Your own gain setting is left alone.
      </p>
      <div className={cx("hs-slider", !duck.enabled && "disabled")}>
        <div className="hs-slider-top">
          <span>Attenuation</span>
          <span className="hs-slider-val">{Math.round(duck.attenuation_db)} dB</span>
        </div>
        <input
          type="range"
          min={MIN_DUCK_DB}
          max={MAX_DUCK_DB}
          step={1}
          value={duck.attenuation_db}
          aria-label="Ducking attenuation"
          // Off means the value cannot do anything; taking it out of the tab
          // order beats a focusable control that is inert.
          disabled={!duck.enabled}
          onChange={(e) => setDuck(duck.enabled, Number(e.target.value))}
        />
      </div>
    </section>
  );
}

export function SoundboardScreen() {
  const { clips, status, playingIds, targets, loaded, error } = useSoundboard();
  const setTargets = useSoundboard((s) => s.setTargets);
  const stopAll = useSoundboard((s) => s.stopAll);
  const [adding, setAdding] = useState(false);

  // The poll loop belongs to this screen: it starts when the tab opens and
  // stops when it closes, so nothing keeps asking in the background. Read off
  // getState so the effect never re-runs.
  useEffect(() => {
    useSoundboard.getState().start();
    return () => useSoundboard.getState().stop();
  }, []);

  /** Desktop only: the file dialog, filtered by what the decoder accepts. */
  async function pickClip() {
    setAdding(true);
    try {
      const extensions = await useSoundboard.getState().formats();
      const path = await open({
        multiple: false,
        filters: [{ name: "Audio", extensions }],
      });
      if (typeof path === "string") await useSoundboard.getState().addClip(path);
    } finally {
      setAdding(false);
    }
  }

  const nowPlaying = clips.find((c) => playingIds.includes(c.id));
  // The optimistic highlight runs a poll ahead of the count, so either one
  // saying "something is playing" is enough for the stop button and the
  // announcement to agree with the board.
  const anything = status.playing > 0 || playingIds.length > 0;

  let body;
  if (!loaded) {
    // First read still out. Nothing here beats an empty state about to be
    // replaced.
    body = <div className="hs-body" />;
  } else if (!status.available) {
    body = (
      <div className="hs-body">
        <div className="hs-empty">
          <Ms name="voice_over_off" style={{ fontSize: 46, opacity: 0.5 }} />
          <p>The soundboard needs the native audio engine.</p>
          <span className="hs-empty-sub">
            Inari is running on its <code>pactl</code> fallback, which cannot
            publish a clip into the chat at all. The rest of the app works;
            this screen fills itself in once the PipeWire engine is running
            (Settings › Audio engine).
          </span>
        </div>
      </div>
    );
  } else {
    body = (
      <div className="hs-body">
        <section className="hs-card">
          <div className="hs-card-head">
            <Ms name="campaign" style={{ fontSize: 18 }} />
            <h2>Board</h2>
            <div className="hs-card-right sb-bar">
              {/* The setting that gets flipped most often, so it is a row of
                  buttons rather than something behind a menu. */}
              <div className="hs-seg" role="group" aria-label="Play to">
                {TARGETS.map((t) => (
                  <button
                    key={t.id}
                    type="button"
                    className={cx("hs-seg-btn", targets === t.id && "active")}
                    aria-pressed={targets === t.id}
                    title={t.title}
                    onClick={() => setTargets(t.id)}
                  >
                    {t.label}
                  </button>
                ))}
              </div>
              <button
                type="button"
                className="hs-chip sb-stop"
                aria-label="Stop all clips"
                disabled={!anything}
                onClick={() => void stopAll()}
              >
                <Ms name="stop_circle" style={{ fontSize: 16 }} /> Stop all
              </button>
            </div>
          </div>

          {/* Which pad is lit changes without anyone touching the screen (a
              clip ends on its own), so it is announced rather than only drawn. */}
          <p className="sb-live" role="status" aria-live="polite">
            {anything ? (nowPlaying ? `Playing ${nowPlaying.name}` : "Playing") : "Nothing playing"}
          </p>

          {/* Said once, here, rather than on every compressed pad. */}
          {!status.ffmpeg && (
            <p className="hs-hint">
              <code>ffmpeg</code> isn&apos;t installed. WAV and FLAC clips play
              as usual; compressed ones (MP3, OGG, M4A…) stay unavailable until
              it is on your PATH.
            </p>
          )}

          {clips.length === 0 ? (
            <div className="hs-empty sb-empty">
              <Ms name="library_music" style={{ fontSize: 46, opacity: 0.5 }} />
              <p>No clips yet.</p>
              <span className="hs-empty-sub">
                {isTauri
                  ? "Add a WAV, FLAC or - with ffmpeg installed - an MP3, OGG or M4A, and it turns up here as a pad you can fire into the chat."
                  : "Clips are added on the PC running Inari; from here you fire whatever is on the board."}
              </span>
            </div>
          ) : (
            <Board clips={clips} playingIds={playingIds} />
          )}
        </section>

        {/* Renaming, levelling and removing clips are off the remote
            allowlist by design - the tablet fires the board, it does not
            curate it. So the whole section is absent, not disabled. */}
        {isTauri && clips.length > 0 && (
          <section className="hs-card">
            <div className="hs-card-head">
              <Ms name="tune" style={{ fontSize: 18 }} />
              <h2>Library</h2>
            </div>
            <p className="hs-hint">
              Levels are per clip because recorded snippets arrive at wildly
              different volumes. Removing one leaves the file on disk.
            </p>
            <div className="sb-lib">
              {clips.map((clip) => (
                <ClipRow key={clip.id} clip={clip} />
              ))}
            </div>
          </section>
        )}

        <DuckCard />

        {error && (
          <p className="hs-hint" role="alert">
            {error}
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="content">
      <div className="screen-head">
        <h1>Soundboard</h1>
        {/* Adding takes a path off this machine's disk: desktop only. */}
        {isTauri && loaded && status.available && (
          <div className="screen-head-actions">
            <button
              type="button"
              className="hs-chip primary"
              disabled={adding}
              onClick={() => void pickClip()}
            >
              <Ms name="add" style={{ fontSize: 14 }} /> Add clip…
            </button>
          </div>
        )}
      </div>
      {body}
    </div>
  );
}

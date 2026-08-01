import { useEffect, useState } from "react";
import { Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import { displayPosition, useMedia } from "../../store/media";
import type { MediaPlayer } from "../../store/media";
import { MediaSeek } from "./MediaSeek";
import { formatUs } from "./time";

/** How often the interpolated position is committed to React state. Ten
 *  updates a second is smooth on a progress bar and a fraction of the frames
 *  a raw rAF loop would re-render on. */
const FRAME_MS = 100;

/**
 * A clock that ticks only while something is playing.
 *
 * The store polls once a second; between polls the position is worked out
 * from the wall clock, which needs a re-render to be seen. The loop stops the
 * moment playback pauses and is cancelled on unmount, so leaving the tab
 * leaves nothing running.
 */
function useNow(animate: boolean): number {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!animate) return;
    let frame = 0;
    let last = 0;
    const step = () => {
      frame = requestAnimationFrame(step);
      const t = Date.now();
      if (t - last >= FRAME_MS) {
        last = t;
        setNow(t);
      }
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [animate]);

  return now;
}

/**
 * Two Chromium windows are both called "chromium", so the bare label cannot
 * tell them apart. Only the ones that clash get their instance suffix.
 */
function playerName(player: MediaPlayer, players: MediaPlayer[]): string {
  const clashes = players.filter((p) => p.label === player.label).length > 1;
  const suffix = player.id.slice(player.label.length + 1);
  return clashes && suffix ? `${player.label} · ${suffix}` : player.label;
}

/** Which player the transport drives. Only shown when there is a choice. */
function PlayerPicker({
  players,
  selected,
  onPick,
}: Readonly<{
  players: MediaPlayer[];
  selected: string | null;
  onPick: (id: string | null) => void;
}>) {
  const [open, setOpen] = useState(false);
  const current = players.find((p) => p.id === selected);

  return (
    <div style={{ position: "relative" }}>
      <button
        type="button"
        className="select device-select"
        aria-label="Player"
        onClick={() => setOpen((o) => !o)}
      >
        <span className="device-select-name">
          {current ? playerName(current, players) : "Automatic"}
        </span>
        <Ms name="expand_more" />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="end">
        <MenuItem
          icon="auto_mode"
          selected={selected === null}
          showCheck
          onClick={() => {
            onPick(null);
            setOpen(false);
          }}
        >
          Automatic
        </MenuItem>
        {players.map((p) => (
          <MenuItem
            key={p.id}
            icon="play_circle"
            selected={p.id === selected}
            showCheck
            onClick={() => {
              onPick(p.id);
              setOpen(false);
            }}
          >
            {playerName(p, players)}
          </MenuItem>
        ))}
      </Popover>
    </div>
  );
}

export function MediaScreen() {
  const m = useMedia();
  const { status, statusAt, art, players, selected, loaded } = m;

  // The poll loop belongs to this screen: it starts when the tab opens and
  // stops when it closes, so nothing keeps forking playerctl in the
  // background. Read off getState so the effect never re-runs.
  useEffect(() => {
    useMedia.getState().start();
    return () => useMedia.getState().stop();
  }, []);

  const now = useNow(status.playing);
  const position = displayPosition(status, statusAt, now);

  let body;
  if (!loaded) {
    // First poll still out. Nothing here yet is better than an empty state
    // that is about to be replaced.
    body = <div className="hs-body" />;
  } else if (!status.available) {
    body = (
      <div className="hs-empty">
        <Ms name="music_off" style={{ fontSize: 46, opacity: 0.5 }} />
        <p>
          <code>playerctl</code> isn&apos;t installed.
        </p>
        <span className="hs-empty-sub">
          Media control reads what is playing over MPRIS, through{" "}
          <code>playerctl</code> — an optional dependency. Install it from your
          distribution&apos;s repositories (see Getting started › Optional
          dependencies in the guide) and this screen fills itself in.
        </span>
      </div>
    );
  } else if (!status.player) {
    body = (
      <div className="hs-empty">
        <Ms name="play_circle" style={{ fontSize: 46, opacity: 0.5 }} />
        <p>Nothing is playing.</p>
        <span className="hs-empty-sub">
          Start something in a player that speaks MPRIS — a browser, Spotify,
          VLC — and it turns up here with its cover and transport.
        </span>
      </div>
    );
  } else {
    body = (
      <div className="hs-body">
        <section className="hs-card media-card">
          <div className="media-head">
            {art?.url ? (
              <img className="media-art" src={art.url} alt="" />
            ) : (
              <div className="media-art media-art-empty">
                <Ms name="music_note" style={{ fontSize: 40, opacity: 0.4 }} />
              </div>
            )}
            {/* The title changes on its own as tracks roll over, so it is
                announced rather than silently swapped. */}
            <div className="media-meta" aria-live="polite">
              <div className="media-title" title={status.title}>
                {status.title || "Unknown track"}
              </div>
              {status.artist && <div className="media-sub">{status.artist}</div>}
              {status.album && <div className="media-album">{status.album}</div>}
            </div>
          </div>

          {status.length_us > 0 ? (
            <MediaSeek
              positionUs={position}
              lengthUs={status.length_us}
              onSeek={(us) => void m.seek(us)}
            />
          ) : (
            // A live stream reports no length: there is nothing to scrub
            // along, so the bar would be a control that cannot mean anything.
            <div className="media-times media-times-live">
              <span>{formatUs(position)}</span>
              <span className="tag">
                <Ms name="sensors" style={{ fontSize: 11 }} /> Live
              </span>
            </div>
          )}

          <div className="media-transport">
            <button
              type="button"
              className="media-btn"
              aria-label="Previous track"
              onClick={() => void m.previous()}
            >
              <Ms name="skip_previous" style={{ fontSize: 24 }} />
            </button>
            <button
              type="button"
              className="media-btn primary"
              aria-label={status.playing ? "Pause" : "Play"}
              onClick={() => void m.playPause()}
            >
              <Ms name={status.playing ? "pause" : "play_arrow"} style={{ fontSize: 28 }} />
            </button>
            <button
              type="button"
              className="media-btn"
              aria-label="Next track"
              onClick={() => void m.next()}
            >
              <Ms name="skip_next" style={{ fontSize: 24 }} />
            </button>
          </div>
        </section>

        {m.error && (
          <p className="hs-hint" role="alert">
            {m.error}
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="content narrow device">
      <div className="screen-head">
        <h1>Media</h1>
        {players.length > 1 && (
          <div className="screen-head-actions">
            <PlayerPicker players={players} selected={selected} onPick={(id) => m.select(id)} />
          </div>
        )}
      </div>
      {body}
    </div>
  );
}

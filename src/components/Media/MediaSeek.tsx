import { useCallback, useEffect, useRef, useState } from "react";
import { useSliderKeys } from "../../hooks/useSliderKeys";
import { formatUs } from "./time";

interface MediaSeekProps {
  /** Where playback is now, in microseconds (already interpolated). */
  positionUs: number;
  /** Track length in microseconds. Callers only render this when it is > 0. */
  lengthUs: number;
  /** Absolute seek target, in microseconds. */
  onSeek: (positionUs: number) => void;
}

/** Arrow keys move five seconds, Page* half a minute. */
const STEP_S = 5;
const PAGE_S = 30;

/**
 * The scrub bar: a pointer- and keyboard-driven slider over the track.
 *
 * Dragging shows the position under the finger and sends nothing; the seek
 * goes out once on release. Each pointermove would otherwise be a `playerctl
 * position` call, and the player would spend the drag chasing the finger.
 */
export function MediaSeek({ positionUs, lengthUs, onSeek }: Readonly<MediaSeekProps>) {
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const [drag, setDrag] = useState<number | null>(null);
  // The value to commit on release. pointerup may carry no movement of its
  // own (a finger lifting off), so the last dragged value is kept here.
  const dragValue = useRef(0);

  const fromEvent = useCallback(
    (clientX: number) => {
      const el = trackRef.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      if (r.width <= 0) return;
      const pct = Math.max(0, Math.min(1, (clientX - r.left) / r.width));
      dragValue.current = Math.round(pct * lengthUs);
      setDrag(dragValue.current);
    },
    [lengthUs],
  );

  // Read through refs so the window listeners are attached once: they outlive
  // every re-render the once-a-second position poll causes.
  const fromEventRef = useRef(fromEvent);
  fromEventRef.current = fromEvent;
  const onSeekRef = useRef(onSeek);
  onSeekRef.current = onSeek;

  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (dragging.current) fromEventRef.current(e.clientX);
    };
    const up = () => {
      if (!dragging.current) return;
      dragging.current = false;
      onSeekRef.current(dragValue.current);
      // The store answers the seek optimistically, so dropping the local
      // value here does not bounce the handle back to the last poll.
      setDrag(null);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
  }, []);

  const shown = Math.max(0, Math.min(lengthUs, drag ?? positionUs));
  const seconds = Math.round(shown / 1_000_000);
  const total = Math.round(lengthUs / 1_000_000);

  const onKeyDown = useSliderKeys({
    value: seconds,
    min: 0,
    max: total,
    step: STEP_S,
    pageStep: PAGE_S,
    onChange: (s) => onSeek(s * 1_000_000),
  });

  const pct = lengthUs > 0 ? (shown / lengthUs) * 100 : 0;

  return (
    <div className="media-seek">
      <div
        className="media-seek-track"
        ref={trackRef}
        role="slider"
        tabIndex={0}
        aria-label="Seek"
        aria-valuemin={0}
        aria-valuemax={total}
        aria-valuenow={seconds}
        aria-valuetext={`${formatUs(shown)} of ${formatUs(lengthUs)}`}
        onKeyDown={onKeyDown}
        onPointerDown={(e) => {
          dragging.current = true;
          fromEvent(e.clientX);
        }}
      >
        <div className="media-seek-fill" style={{ width: pct + "%" }} />
        <div className="media-seek-cap" style={{ left: pct + "%" }} />
      </div>
      <div className="media-times">
        <span>{formatUs(shown)}</span>
        <span>{formatUs(lengthUs)}</span>
      </div>
    </div>
  );
}

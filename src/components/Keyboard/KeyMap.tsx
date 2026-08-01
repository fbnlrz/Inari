import { useEffect, useRef, useState } from "react";
import { KeyDef, Rgb } from "../../store/keyboard";

/** What clicking a key does. */
export type PaintMode = "color" | "actuation";

/** Pixels per key unit. The board scales to the panel between these. */
const MIN_UNIT = 30;
const MAX_UNIT = 62;

function css([r, g, b]: Rgb) {
  return `rgb(${r} ${g} ${b})`;
}

/** Dark keys need light captions and vice versa, or the legend disappears. */
function readable([r, g, b]: Rgb) {
  return r * 0.299 + g * 0.587 + b * 0.114 > 130 ? "#0d0d11" : "rgba(255,255,255,.92)";
}

/** How lit a key is, for the glow — an unlit key must not haze the deck. */
function intensity([r, g, b]: Rgb) {
  return Math.min(1, (r * 0.299 + g * 0.587 + b * 0.114) / 190);
}

/**
 * The keyboard, drawn to scale and clickable.
 *
 * Positions come from the backend (`keyboard::keys`), so the picture and the
 * effect engine agree on where every key is — a wave that sweeps left to right
 * here sweeps left to right on the board. The keycaps are drawn with the light
 * coming through them rather than as flat swatches, because that is what the
 * board actually looks like and it makes a dim scene readable at a glance.
 */
export function KeyMap({
  layout,
  extent,
  preview,
  actuation,
  mode,
  onPick,
  onErase,
}: Readonly<{
  layout: KeyDef[];
  extent: [number, number];
  preview: Record<number, Rgb>;
  actuation: Record<number, number>;
  mode: PaintMode;
  onPick: (hid: number) => void;
  onErase: (hid: number) => void;
}>) {
  const [w, h] = extent;
  const wrapRef = useRef<HTMLDivElement>(null);
  const [unit, setUnit] = useState(MIN_UNIT);

  // Fill the card's width instead of sitting in a corner of it. Measured
  // rather than guessed, because the window is resizable and the board's own
  // width depends on the form factor.
  useEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap) return;
    const fit = () => {
      const available = wrap.clientWidth - 24;
      setUnit(Math.max(MIN_UNIT, Math.min(MAX_UNIT, available / Math.max(w, 1))));
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(wrap);
    return () => observer.disconnect();
  }, [w]);

  const gap = Math.max(2, unit * 0.08);

  return (
    <div className="kb-map-wrap" ref={wrapRef}>
      <div
        className="kb-deck"
        style={{ width: w * unit, height: h * unit }}
        role="group"
        aria-label="Keyboard layout"
      >
        {layout.map((key) => {
          const color = preview[key.hid] ?? [0, 0, 0];
          const tenths = actuation[key.hid];
          const lit = intensity(color);
          return (
            <button
              type="button"
              key={key.hid}
              className={"kb-key" + (mode === "actuation" && tenths ? " tuned" : "")}
              style={{
                left: key.x * unit,
                top: key.y * unit,
                width: key.w * unit - gap,
                height: key.h * unit - gap,
                // The cap is a dark plastic top lit from underneath, so the
                // colour lives in the glow and the rim, not in a flat fill.
                background: `
                  linear-gradient(180deg,
                    rgba(255,255,255,.10) 0%,
                    rgba(255,255,255,.02) 42%,
                    rgba(0,0,0,.30) 100%),
                  ${css(color)}`,
                color: readable(color),
                borderRadius: Math.max(4, unit * 0.14),
                boxShadow: lit
                  ? `0 0 ${unit * 0.5}px ${(unit * 0.1).toFixed(1)}px rgba(${color[0]},${color[1]},${color[2]},${(lit * 0.55).toFixed(2)}),
                     inset 0 1px 0 rgba(255,255,255,.22),
                     inset 0 -2px 3px rgba(0,0,0,.45)`
                  : "inset 0 1px 0 rgba(255,255,255,.10), inset 0 -2px 3px rgba(0,0,0,.5)",
                fontSize: Math.max(8, Math.min(13, unit * 0.29)),
              }}
              title={`${key.label || "Space"} — HID 0x${key.hid
                .toString(16)
                .padStart(2, "0")}${tenths ? ` — ${(tenths / 10).toFixed(1)} mm` : ""}`}
              onClick={() => onPick(key.hid)}
              // Right-click erases, which is what every paint tool does.
              onContextMenu={(e) => {
                e.preventDefault();
                onErase(key.hid);
              }}
            >
              <span className="kb-key-label">
                {mode === "actuation" && tenths ? (tenths / 10).toFixed(1) : key.label}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

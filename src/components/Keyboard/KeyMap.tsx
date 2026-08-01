import { KeyDef, Rgb } from "../../store/keyboard";

/** What clicking a key does. */
export type PaintMode = "color" | "actuation";

/** How wide one key unit is drawn, in pixels. */
const UNIT = 30;

function css([r, g, b]: Rgb) {
  return `rgb(${r} ${g} ${b})`;
}

/** Dark keys need light captions and vice versa, or the legend disappears. */
function readable([r, g, b]: Rgb) {
  return r * 0.299 + g * 0.587 + b * 0.114 > 130 ? "#101014" : "rgba(255,255,255,.82)";
}

/**
 * The keyboard, drawn to scale and clickable.
 *
 * Positions come from the backend (`keyboard::keys`), so the picture and the
 * effect engine agree on where every key is — a wave that sweeps left to right
 * here sweeps left to right on the board.
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
  return (
    <div className="kb-map-wrap">
      <div
        className="kb-map"
        style={{ width: w * UNIT, height: h * UNIT }}
        role="group"
        aria-label="Keyboard layout"
      >
        {layout.map((key) => {
          const color = preview[key.hid] ?? [0, 0, 0];
          const tenths = actuation[key.hid];
          return (
            <button
              type="button"
              key={key.hid}
              className={"kb-key" + (mode === "actuation" && tenths ? " tuned" : "")}
              style={{
                left: key.x * UNIT,
                top: key.y * UNIT,
                width: key.w * UNIT - 3,
                height: key.h * UNIT - 3,
                background: css(color),
                color: readable(color),
              }}
              title={`${key.label || "Space"} — HID ${key.hid
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

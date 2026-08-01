import { useEffect, useState } from "react";
import { Ms } from "../Icons";
import { Toggle } from "../Toggle";
import { KeyMap, PaintMode } from "./KeyMap";
import {
  EFFECTS,
  KbMode,
  OledWire,
  Rgb,
  SCENE_SWATCHES,
  useKeyboard,
} from "../../store/keyboard";

function toHex([r, g, b]: Rgb) {
  return "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
}
function fromHex(hex: string): Rgb {
  return [
    Number.parseInt(hex.slice(1, 3), 16),
    Number.parseInt(hex.slice(3, 5), 16),
    Number.parseInt(hex.slice(5, 7), 16),
  ];
}

/** A transport is an object or a bare string; compare by shape. */
function sameWire(a: OledWire | null, b: OledWire | null) {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function KeyboardScreen() {
  const kb = useKeyboard();
  const cfg = kb.config;
  const light = cfg.lighting;
  const [brush, setBrush] = useState<Rgb>([255, 82, 0]);
  const [mode, setMode] = useState<PaintMode>("color");
  const [brushActuation, setBrushActuation] = useState(8);
  const [rawCmd, setRawCmd] = useState("0x2d");
  const [rawPayload, setRawPayload] = useState("8");

  useEffect(() => {
    void kb.init();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!kb.connected) {
    return (
      <div className="content narrow">
        <div className="screen-head">
          <h1>Keyboard</h1>
        </div>
        <div className="hs-empty">
          <Ms name="keyboard" style={{ fontSize: 46, opacity: 0.5 }} />
          <p>No SteelSeries keyboard connected.</p>
          <span className="hs-empty-sub">
            Plug in an Apex Pro, Apex&nbsp;7 or Apex&nbsp;9 (Gen&nbsp;1 to
            Gen&nbsp;3, wired or over its dongle). If it is connected, make sure
            your user can reach <code>/dev/hidraw*</code> — the bundled udev rule
            does that.
          </span>
        </div>
      </div>
    );
  }

  const effect = EFFECTS.find((e) => e.kind === light.kind) ?? EFFECTS[0];
  const battery = kb.status.battery_percent;

  return (
    <div className="content narrow">
      <div className="screen-head">
        <h1>{kb.status.model ?? "Keyboard"}</h1>
        <div className="screen-head-actions">
          {battery != null && (
            <span className="tag">
              <Ms name="battery_full" style={{ fontSize: 11 }} />
              {battery}%
            </span>
          )}
          <span className="tag live">
            <Ms name={kb.status.wireless ? "wifi" : "usb"} style={{ fontSize: 11 }} />
            {kb.status.firmware ? `FW ${kb.status.firmware}` : "Connected"}
          </span>
        </div>
      </div>

      <div className="hs-body">
        {kb.error && (
          <div className="error-banner" role="alert" style={{ borderRadius: 8 }}>
            <span className="error-banner-msg">{kb.error}</span>
            <button
              type="button"
              className="error-banner-x"
              aria-label="Dismiss error"
              title="Dismiss"
              onClick={kb.clearError}
            >
              <Ms name="close" style={{ fontSize: 16 }} />
            </button>
          </div>
        )}

        {/* ---- master switch ---- */}
        <section className="hs-card">
          <div className="row">
            <div className="ricon">
              <Ms name="lightbulb" />
            </div>
            <div className="rtext">
              <div className="rtitle">Inari drives the lighting</div>
              <div className="rsub">
                Off hands every key back to the keyboard&apos;s own profile.
              </div>
            </div>
            <Toggle on={cfg.enabled} onClick={() => void kb.setEnabled(!cfg.enabled)} />
          </div>
        </section>

        {/* ---- lighting ---- */}
        <section className="hs-card">
          <div className="hs-card-head">
            <Ms name="palette" style={{ fontSize: 18 }} />
            <h2>Lighting</h2>
          </div>

          <div className="hs-eq-presets">
            {EFFECTS.map((e) => (
              <button
                type="button"
                key={e.kind}
                className={"hs-chip" + (light.kind === e.kind ? " active" : "")}
                title={e.hint}
                onClick={() => void kb.setLighting({ kind: e.kind })}
              >
                {e.label}
                {e.onboard && <span className="kb-chip-tag">HW</span>}
              </button>
            ))}
          </div>
          <p className="hs-hint">{effect.hint}</p>

          {light.kind === "scene" && (
            <div className="kb-scenes">
              {kb.scenes.map((s) => (
                <button
                  type="button"
                  key={s.scene}
                  className={"kb-scene" + (light.scene === s.scene ? " active" : "")}
                  onClick={() => void kb.setLighting({ kind: "scene", scene: s.scene })}
                >
                  <span className="kb-scene-strip">
                    {(SCENE_SWATCHES[s.scene] ?? []).map((c) => (
                      <span key={c} style={{ background: c }} />
                    ))}
                  </span>
                  <span className="kb-scene-name">{s.label}</span>
                  <span className="kb-scene-hint">{s.hint}</span>
                </button>
              ))}
            </div>
          )}

          <div className="kb-controls">
            {(effect.colors ?? 0) >= 1 && (
              <label className="kb-color">
                <span>Colour</span>
                <input
                  type="color"
                  value={toHex(light.color)}
                  onChange={(ev) => void kb.setLighting({ color: fromHex(ev.target.value) })}
                />
              </label>
            )}
            {(effect.colors ?? 0) >= 2 && (
              <label className="kb-color">
                <span>Second</span>
                <input
                  type="color"
                  value={toHex(light.color2)}
                  onChange={(ev) => void kb.setLighting({ color2: fromHex(ev.target.value) })}
                />
              </label>
            )}
          </div>

          <div className="hs-slider-top">
            <span>Brightness</span>
            <span className="hs-slider-val">{light.brightness}%</span>
          </div>
          <input
            type="range"
            min={0}
            max={100}
            value={light.brightness}
            onChange={(ev) => void kb.setLighting({ brightness: Number(ev.target.value) })}
          />

          {effect.speed && (
            <>
              <div className="hs-slider-top">
                <span>Speed</span>
                <span className="hs-slider-val">{light.speed}</span>
              </div>
              <input
                type="range"
                min={1}
                max={100}
                value={light.speed}
                onChange={(ev) => void kb.setLighting({ speed: Number(ev.target.value) })}
              />
              <div className="row">
                <div className="rtext">
                  <div className="rtitle">Reverse direction</div>
                </div>
                <Toggle
                  on={light.reverse}
                  onClick={() => void kb.setLighting({ reverse: !light.reverse })}
                />
              </div>
            </>
          )}
        </section>

        {/* ---- the board itself ---- */}
        <section className="hs-card">
          <div className="hs-card-head">
            <Ms name="keyboard" style={{ fontSize: 18 }} />
            <h2>Keys</h2>
            <div className="hs-card-right hs-seg">
              <button
                type="button"
                className={"hs-seg-btn" + (cfg.physical === "iso" ? " active" : "")}
                onClick={() => void kb.setPhysical("iso")}
              >
                ISO
              </button>
              <button
                type="button"
                className={"hs-seg-btn" + (cfg.physical === "ansi" ? " active" : "")}
                onClick={() => void kb.setPhysical("ansi")}
              >
                ANSI
              </button>
            </div>
          </div>

          <div className="hs-seg">
            <button
              type="button"
              className={"hs-seg-btn" + (mode === "color" ? " active" : "")}
              onClick={() => setMode("color")}
            >
              Paint colours
            </button>
            <button
              type="button"
              className={"hs-seg-btn" + (mode === "actuation" ? " active" : "")}
              onClick={() => setMode("actuation")}
            >
              Per-key actuation
            </button>
          </div>

          {mode === "color" ? (
            <>
              <div className="kb-controls">
                <label className="kb-color">
                  <span>Brush</span>
                  <input
                    type="color"
                    value={toHex(brush)}
                    onChange={(ev) => setBrush(fromHex(ev.target.value))}
                  />
                </label>
                <button type="button" className="btn" onClick={() => void kb.fillKeys(brush)}>
                  Fill all
                </button>
                <button type="button" className="btn" onClick={() => void kb.clearKeys()}>
                  Clear
                </button>
              </div>
              <p className="hs-hint">
                Click a key to paint it, right-click to clear it. Painting switches
                the effect to <b>Per key</b>.
              </p>
            </>
          ) : (
            <>
              <div className="hs-slider-top">
                <span>Brush actuation</span>
                <span className="hs-slider-val">{(brushActuation / 10).toFixed(1)} mm</span>
              </div>
              <input
                type="range"
                min={1}
                max={40}
                value={brushActuation}
                onChange={(ev) => setBrushActuation(Number(ev.target.value))}
              />
              <p className="hs-hint kb-warn">
                <Ms name="science" style={{ fontSize: 14 }} />
                Experimental. The actuation command is accepted by the hardware
                but changed nothing on the one board this could be tested against
                (Apex Pro TKL Wireless 2023, firmware 3.24.1). Values are stored
                and sent; whether the switches follow is unknown.
              </p>
            </>
          )}

          <KeyMap
            layout={kb.layout}
            extent={kb.extent}
            preview={kb.preview}
            actuation={cfg.per_key_actuation}
            mode={mode}
            onPick={(hid) =>
              mode === "color"
                ? void kb.setKeyColor(hid, brush)
                : void kb.setKeyActuation(hid, brushActuation)
            }
            onErase={(hid) =>
              mode === "color" ? void kb.setKeyColor(hid, null) : void kb.setKeyActuation(hid, null)
            }
          />
        </section>

        {/* ---- OLED ---- */}
        {kb.status.has_oled && (
          <section className="hs-card">
            <div className="hs-card-head">
              <Ms name="monitor" style={{ fontSize: 18 }} />
              <h2>Display</h2>
            </div>

            <div className="hs-eq-presets">
              <button
                type="button"
                className={"hs-chip" + (cfg.oled_mode == null ? " active" : "")}
                onClick={() => void kb.setOled(null)}
              >
                Keyboard&apos;s own
              </button>
              {kb.oledModes.map((m) => (
                <button
                  type="button"
                  key={m.mode}
                  className={"hs-chip" + (cfg.oled_mode === m.mode ? " active" : "")}
                  onClick={() => void kb.setOled(m.mode)}
                >
                  {m.label}
                </button>
              ))}
            </div>

            {cfg.oled_mode === "text" && (
              <textarea
                className="kb-text"
                rows={3}
                placeholder="Up to three lines"
                value={cfg.oled_text.join("\n")}
                onChange={(ev) =>
                  void kb.setOled("text" as KbMode, ev.target.value.split("\n").slice(0, 3))
                }
              />
            )}

            <div className="hs-slider-top">
              <span>Screensaver</span>
              <span className="hs-slider-val">
                {cfg.oled_screensaver_minutes === 0
                  ? "off"
                  : `${cfg.oled_screensaver_minutes} min`}
              </span>
            </div>
            <input
              type="range"
              min={0}
              max={60}
              value={cfg.oled_screensaver_minutes}
              onChange={(ev) => void kb.setScreensaver(Number(ev.target.value))}
            />
            <p className="hs-hint">
              While Inari drives the panel the keyboard&apos;s own screensaver never
              fires, so this one blanks it instead. The firmware keeps drawing its
              profile and battery over the top row whatever Inari sends.
            </p>

            <div className="hs-card-head" style={{ marginTop: 4 }}>
              <Ms name="cable" style={{ fontSize: 16 }} />
              <h2 style={{ fontSize: "var(--fs-body)" }}>Transport</h2>
            </div>
            <p className="hs-hint">
              Which packet shape the panel understands differs per model. If the
              display stays blank or shows noise, send the test picture with each
              one until a bordered X with a filled corner appears, then pin it.
            </p>
            <div className="kb-wires">
              <button
                type="button"
                className={"hs-chip" + (cfg.oled_wire == null ? " active" : "")}
                onClick={() => void kb.setOledWire(null)}
              >
                Automatic
              </button>
              {kb.oledWires.map((w) => (
                <div className="kb-wire-row" key={w.label}>
                  <button
                    type="button"
                    className={"hs-chip" + (sameWire(cfg.oled_wire, w.wire) ? " active" : "")}
                    onClick={() => void kb.setOledWire(w.wire)}
                  >
                    {w.label}
                  </button>
                  <button
                    type="button"
                    className="btn small"
                    onClick={() => void kb.testOled(w.wire)}
                  >
                    Test
                  </button>
                </div>
              ))}
            </div>
          </section>
        )}

        {/* ---- switches ---- */}
        {kb.status.has_actuation && (
          <section className="hs-card">
            <div className="hs-card-head">
              <Ms name="tune" style={{ fontSize: 18 }} />
              <h2>Switches</h2>
            </div>
            <div className="hs-slider-top">
              <span>Actuation point</span>
              <span className="hs-slider-val">
                {cfg.actuation ? `${(cfg.actuation / 10).toFixed(1)} mm` : "keyboard default"}
              </span>
            </div>
            <input
              type="range"
              min={1}
              max={40}
              value={cfg.actuation ?? 8}
              onChange={(ev) => void kb.setActuation(Number(ev.target.value))}
            />
            <p className="hs-hint kb-warn">
              <Ms name="science" style={{ fontSize: 14 }} />
              Experimental — see the note above. Rapid Trigger, Rapid Tap and
              Protection Mode have never been captured by anyone; use the probe
              below if you want to help find them.
            </p>
          </section>
        )}

        {/* ---- raw probe ---- */}
        <section className="hs-card">
          <div className="hs-card-head">
            <Ms name="terminal" style={{ fontSize: 18 }} />
            <h2>Command probe</h2>
          </div>
          <p className="hs-hint">
            Sends one raw 65-byte control report. This is how the commands above
            were found; the keyboard ignores what it does not understand, but a
            wrong length can make it stall — if it stops responding, unplug it
            once.
          </p>
          <div className="kb-controls">
            <label className="kb-field">
              <span>Command</span>
              <input value={rawCmd} onChange={(ev) => setRawCmd(ev.target.value)} />
            </label>
            <label className="kb-field">
              <span>Payload</span>
              <input
                value={rawPayload}
                onChange={(ev) => setRawPayload(ev.target.value)}
                placeholder="e.g. 1 0x20 255"
              />
            </label>
            <button
              type="button"
              className="btn"
              onClick={() => {
                const cmd = Number(rawCmd);
                const payload = rawPayload
                  .split(/[\s,]+/)
                  .filter(Boolean)
                  .map(Number);
                if (Number.isNaN(cmd) || payload.some(Number.isNaN)) return;
                void kb.sendRaw(cmd & 0xff, payload.map((v) => v & 0xff));
              }}
            >
              Send
            </button>
          </div>
        </section>
      </div>
    </div>
  );
}

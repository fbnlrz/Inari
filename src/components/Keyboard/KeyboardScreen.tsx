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

/** The screen's sections. Eight cards in one column meant two were visible at
 *  the default window size and the key map — the only picture of what the board
 *  is actually doing — started around 800 px down. */
type KbTab = "light" | "keys" | "display" | "system";

/** Cards whose controls the backend ignores while the master switch is off.
 *
 *  With `enabled: false` the render loop hands the LEDs back and `continue`s
 *  (keyboard/manager.rs), so every chip, scene and OLED mode here still shows
 *  an active state while nothing reaches the board. Marked inert rather than
 *  hidden, so it is visible *why* they do nothing. */
/** Where a setting actually lives.
 *
 *  Three different things wear the same card here, and the difference decides
 *  what happens when Inari closes or the board loses power. It used to be
 *  explained in prose in three places; a label on the card says it once. */
function Origin({ kind }: Readonly<{ kind: "live" | "stored" | "volatile" }>) {
  const map = {
    live: {
      label: "Live from Inari",
      title:
        "Drawn by Inari; the keyboard takes its own lighting back when Inari closes.",
    },
    stored: {
      label: "Stored in the board",
      title:
        "Written into the keyboard's own memory; keeps working with Inari closed.",
    },
    volatile: {
      label: "Sent to the board",
      title:
        "Applied to the keyboard but deliberately not saved to its flash, so Inari never edits the profile you take to another machine. Re-sent whenever Inari reconnects.",
    },
  } as const;
  const o = map[kind];
  return (
    <span className={`hs-card-right kb-origin ${kind}`} title={o.title}>
      {o.label}
    </span>
  );
}

function inert(off: boolean) {
  return "hs-card" + (off ? " is-inert" : "");
}

/** The scene's own palette as a tile background, so twenty-four tiles are told
 *  apart at a glance rather than by reading their names. */
function sceneTint(colors: string[] | undefined) {
  if (!colors || colors.length < 2) return undefined;
  return {
    "--scene-tint": `linear-gradient(135deg, ${colors.join(", ")})`,
  } as React.CSSProperties;
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
  const [tab, setTab] = useState<KbTab>("light");

  // The tabs a board actually has: no panel, no Display; no adjustable
  // switches, no Keys.
  const tabs: { key: KbTab; label: string; icon: string }[] = [
    { key: "light", label: "Lighting", icon: "palette" },
    ...(kb.status.has_actuation
      ? [{ key: "keys" as const, label: "Keys", icon: "keyboard" }]
      : []),
    ...(kb.status.has_oled
      ? [{ key: "display" as const, label: "Display", icon: "monitor" }]
      : []),
    { key: "system", label: "System", icon: "tune" },
  ];
  // Unplugging a board can remove the tab that is open.
  const active = tabs.some((t) => t.key === tab) ? tab : "light";

  useEffect(() => {
    void kb.init();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!kb.connected) {
    return (
      <div className="content narrow device">
        <div className="screen-head">
          <h1>Keyboard</h1>
        </div>
        <div className="hs-empty">
          <Ms name="keyboard" style={{ fontSize: 46, opacity: 0.5 }} />
          <p>No SteelSeries keyboard connected.</p>
          <span className="hs-empty-sub">
            Plug in an Apex Pro, Apex&nbsp;7 or Apex&nbsp;9 (Gen&nbsp;1 to
            Gen&nbsp;3, wired or over its dongle). If it is connected, make sure
            your user can reach <code>/dev/hidraw*</code> — the bundled udev
            rule does that.
          </span>
        </div>
      </div>
    );
  }

  const effect = EFFECTS.find((e) => e.kind === light.kind) ?? EFFECTS[0];
  const battery = kb.status.battery_percent;

  return (
    <div className="content narrow device">
      <div className="screen-head">
        <h1>{kb.status.model ?? "Keyboard"}</h1>
        <nav className="hs-seg kb-tabs" aria-label="Keyboard sections">
          {tabs.map((t) => (
            <button
              type="button"
              key={t.key}
              aria-current={active === t.key ? "page" : undefined}
              className={"hs-seg-btn" + (active === t.key ? " active" : "")}
              onClick={() => {
                setTab(t.key);
                // The key map serves both tabs; give each the mode it is for.
                if (t.key === "keys") setMode("actuation");
                if (t.key === "light") setMode("color");
              }}
            >
              <Ms name={t.icon} style={{ fontSize: 14 }} />
              {t.label}
            </button>
          ))}
        </nav>
        <div className="screen-head-actions">
          {battery != null && (
            <span className="tag">
              <Ms name="battery_full" style={{ fontSize: 11 }} />
              {battery}%
            </span>
          )}
          <span className="tag live">
            <Ms
              name={kb.status.wireless ? "wifi" : "usb"}
              style={{ fontSize: 11 }}
            />
            {kb.status.firmware ? `FW ${kb.status.firmware}` : "Connected"}
          </span>
        </div>
      </div>

      <div className="hs-body">
        {kb.error && (
          <div
            className="error-banner"
            role="alert"
            style={{ borderRadius: 8 }}
          >
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

        {/* The key map is the only place the current effect is visible, so it
            sits above the controls that change it rather than four cards
            below them. */}
        {(active === "light" || active === "keys") && (
          <>
            {/* ---- the board itself: the only picture of what is actually
               lit, so it stays at the top of every tab that can change it ---- */}
            <section className="hs-card full-bleed">
              <div className="hs-card-head">
                <Ms name="keyboard" style={{ fontSize: 18 }} />
                <h2>Keys</h2>
                <div className="hs-card-right hs-seg">
                  <button
                    type="button"
                    className={
                      "hs-seg-btn" + (cfg.physical === "iso" ? " active" : "")
                    }
                    onClick={() => void kb.setPhysical("iso")}
                  >
                    ISO
                  </button>
                  <button
                    type="button"
                    className={
                      "hs-seg-btn" + (cfg.physical === "ansi" ? " active" : "")
                    }
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
                  hidden={!kb.status.has_actuation}
                  className={
                    "hs-seg-btn" + (mode === "actuation" ? " active" : "")
                  }
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
                    <button
                      type="button"
                      className="btn"
                      onClick={() => void kb.fillKeys(brush)}
                    >
                      Fill all
                    </button>
                    <button
                      type="button"
                      className="btn"
                      onClick={() => void kb.clearKeys()}
                    >
                      Clear
                    </button>
                  </div>
                  <p className="hs-hint">
                    Click a key to paint it, right-click to clear it. Painting
                    switches the effect to <b>Per key</b>.
                  </p>
                </>
              ) : (
                <>
                  <div className="hs-slider-top">
                    <span>Brush actuation</span>
                    <span className="hs-slider-val">
                      {(brushActuation / 10).toFixed(1)} mm
                    </span>
                  </div>
                  <input
                    type="range"
                    min={1}
                    max={40}
                    value={brushActuation}
                    onChange={(ev) =>
                      setBrushActuation(Number(ev.target.value))
                    }
                  />
                  <p className="hs-hint">
                    Click a key to give it this actuation point, right-click to
                    drop the override. Keys without one keep the global setting
                    under
                    <b> Keys</b>; with no global setting, only the keys you set
                    here are written.
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
                  mode === "color"
                    ? void kb.setKeyColor(hid, null)
                    : void kb.setKeyActuation(hid, null)
                }
              />
            </section>
          </>
        )}

        {active === "light" && (
          <>
            {/* ---- master switch ---- */}
            <section className="hs-card">
              <div className="row">
                <div className="ricon">
                  <Ms name="lightbulb" />
                </div>
                <div className="rmain">
                  <div className="rtitle">Inari drives the lighting</div>
                  <div className="rsub">
                    Off hands every key back to the keyboard&apos;s own profile.
                  </div>
                </div>
                <Toggle
                  on={cfg.enabled}
                  onClick={() => void kb.setEnabled(!cfg.enabled)}
                />
              </div>
            </section>

            {/* ---- lighting ---- */}
            <section
              className={inert(!cfg.enabled)}
              aria-disabled={!cfg.enabled}
            >
              <div className="hs-card-head">
                <Ms name="palette" style={{ fontSize: 18 }} />
                <h2>Lighting</h2>
                {!cfg.enabled && (
                  <button
                    type="button"
                    className="hs-card-right kb-inert-badge"
                    onClick={() => void kb.setEnabled(true)}
                  >
                    <Ms name="power_settings_new" style={{ fontSize: 12 }} />
                    Inari is off
                  </button>
                )}
              </div>

              <div className="hs-eq-presets">
                {/* "Scenes" is deliberately not a chip. Clicking it set
                    kind: "scene" without choosing one, which silently restarted
                    whichever scene was picked last — the card below is where a
                    scene gets chosen, and it says which one is playing. The
                    EFFECTS entry itself has to stay: the colour picker and the
                    speed slider read `colors` and `speed` from it. */}
                {EFFECTS.filter((e) => e.kind !== "scene").map((e) => (
                  <button
                    type="button"
                    key={e.kind}
                    className={
                      "hs-chip" + (light.kind === e.kind ? " active" : "")
                    }
                    title={e.hint}
                    onClick={() => void kb.setLighting({ kind: e.kind })}
                  >
                    {e.label}
                    {e.onboard && <span className="kb-chip-tag">HW</span>}
                  </button>
                ))}
                {light.kind === "scene" && (
                  <span className="hs-chip active" aria-current="true">
                    Scene:{" "}
                    {kb.scenes.find((x) => x.scene === light.scene)?.label ??
                      "—"}
                  </span>
                )}
              </div>
              <p className="hs-hint">{effect.hint}</p>

              <div className="kb-controls">
                {(effect.colors ?? 0) >= 1 && (
                  <label className="kb-color">
                    <span>Colour</span>
                    <input
                      type="color"
                      value={toHex(light.color)}
                      onChange={(ev) =>
                        void kb.setLighting({ color: fromHex(ev.target.value) })
                      }
                    />
                  </label>
                )}
                {(effect.colors ?? 0) >= 2 && (
                  <label className="kb-color">
                    <span>Second</span>
                    <input
                      type="color"
                      value={toHex(light.color2)}
                      onChange={(ev) =>
                        void kb.setLighting({
                          color2: fromHex(ev.target.value),
                        })
                      }
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
                onChange={(ev) =>
                  void kb.setLighting({ brightness: Number(ev.target.value) })
                }
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
                    onChange={(ev) =>
                      void kb.setLighting({ speed: Number(ev.target.value) })
                    }
                  />
                  <div className="row">
                    <div className="rmain">
                      <div className="rtitle">Reverse direction</div>
                    </div>
                    <Toggle
                      on={light.reverse}
                      onClick={() =>
                        void kb.setLighting({ reverse: !light.reverse })
                      }
                    />
                  </div>
                </>
              )}
            </section>

            {/* ---- scenes ----
               Their own card, always on screen. Hiding them behind the "Scenes"
               effect chip meant nobody found them. */}
            <section
              className={inert(!cfg.enabled)}
              aria-disabled={!cfg.enabled}
            >
              <div className="hs-card-head">
                <Ms name="auto_awesome" style={{ fontSize: 18 }} />
                <h2>Scenes</h2>
                {light.kind === "scene" && (
                  <span className="hs-card-right tag live">Playing</span>
                )}
              </div>
              <div className="kb-scenes">
                {kb.scenes.map((s) => (
                  <button
                    type="button"
                    key={s.scene}
                    className={
                      "kb-scene" +
                      (light.kind === "scene" && light.scene === s.scene
                        ? " active"
                        : "")
                    }
                    onClick={() =>
                      void kb.setLighting({ kind: "scene", scene: s.scene })
                    }
                    style={sceneTint(SCENE_SWATCHES[s.scene])}
                  >
                    <span className="kb-scene-strip">
                      {(SCENE_SWATCHES[s.scene] ?? []).map((c) => (
                        <span key={c} style={{ background: c }} />
                      ))}
                    </span>
                    <span className="kb-scene-name">{s.label}</span>
                    <span className="kb-scene-hint">{s.hint}</span>
                    {light.kind === "scene" && light.scene === s.scene && (
                      <span className="kb-scene-live">Live</span>
                    )}
                  </button>
                ))}
              </div>
              <p className="hs-hint">
                Speed and brightness come from the Lighting card above; Ripple
                and Slash use the colour you set there.
              </p>
            </section>
          </>
        )}

        {active === "keys" && (
          <>
            {/* ---- switches ---- */}
            {kb.status.has_actuation && (
              <section className="hs-card">
                <div className="hs-card-head">
                  <Ms name="tune" style={{ fontSize: 18 }} />
                  <h2>Switches</h2>
                  <Origin kind="volatile" />
                </div>

                <div className="hs-slider-top">
                  <span>Actuation point</span>
                  <span className="hs-slider-val">
                    {cfg.actuation
                      ? `${(cfg.actuation / 10).toFixed(1)} mm`
                      : "keyboard default"}
                  </span>
                </div>
                <input
                  type="range"
                  min={1}
                  max={40}
                  value={cfg.actuation ?? 15}
                  onChange={(ev) =>
                    void kb.setActuation(Number(ev.target.value))
                  }
                />
                <p className="hs-hint">
                  How far a key travels before it registers. Per-key overrides
                  live on the key map above; this is the value every other key
                  uses.
                </p>

                <div className="hs-slider-top">
                  <span>Rapid Trigger</span>
                  <span className="hs-slider-val">
                    {cfg.rapid_trigger === 0 ? "off" : cfg.rapid_trigger}
                  </span>
                </div>
                <input
                  type="range"
                  min={0}
                  max={10}
                  value={cfg.rapid_trigger}
                  onChange={(ev) =>
                    void kb.setRapidTrigger(Number(ev.target.value))
                  }
                />
                <p className="hs-hint">
                  Re-arms a key as soon as it moves back up, instead of waiting
                  for it to pass a fixed reset point. Higher reacts to smaller
                  movements.
                </p>

                <div className="row">
                  <div className="rmain">
                    <div className="rtitle">Protection Mode</div>
                    <div className="rsub">
                      Dampens the keys around the one you meant to press.
                    </div>
                  </div>
                  <Toggle
                    on={cfg.protection_mode}
                    onClick={() =>
                      void kb.setProtectionMode(!cfg.protection_mode)
                    }
                  />
                </div>

                <div className="row">
                  <div className="rmain">
                    <div className="rtitle">Rapid Tap (SOCD)</div>
                    <div className="rsub">
                      Opposite keys cancel each other, using the pairs stored in
                      the keyboard&apos;s own profile.
                    </div>
                  </div>
                  <Toggle
                    on={cfg.rapid_tap}
                    onClick={() => void kb.setRapidTap(!cfg.rapid_tap)}
                  />
                </div>
              </section>
            )}
          </>
        )}

        {active === "display" && (
          <>
            {/* ---- OLED ---- */}
            {kb.status.has_oled && (
              <section
                className={inert(!cfg.enabled)}
                aria-disabled={!cfg.enabled}
              >
                <div className="hs-card-head">
                  <Ms name="monitor" style={{ fontSize: 18 }} />
                  <h2>Display</h2>
                  {cfg.enabled && <Origin kind="live" />}
                </div>

                <div className="hs-eq-presets">
                  <button
                    type="button"
                    className={
                      "hs-chip" + (cfg.oled_mode == null ? " active" : "")
                    }
                    onClick={() => void kb.setOled(null)}
                  >
                    Keyboard&apos;s own
                  </button>
                  {kb.oledModes.map((m) => (
                    <button
                      type="button"
                      key={m.mode}
                      className={
                        "hs-chip" + (cfg.oled_mode === m.mode ? " active" : "")
                      }
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
                      void kb.setOled(
                        "text" as KbMode,
                        ev.target.value.split("\n").slice(0, 3),
                      )
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
                  onChange={(ev) =>
                    void kb.setScreensaver(Number(ev.target.value))
                  }
                />
                <p className="hs-hint">
                  While Inari drives the panel the keyboard&apos;s own
                  screensaver never fires, so this one blanks it instead. The
                  firmware keeps drawing its profile and battery over the top
                  row whatever Inari sends.
                </p>

                {/* A diagnostic, not a setting: you touch it once, if the
                    panel shows noise. Folded away so it stops costing the
                    card its whole lower half. */}
                <details className="kb-details">
                  <summary>
                    <Ms name="cable" style={{ fontSize: 16 }} />
                    Transport
                  </summary>
                  <p className="hs-hint">
                    Which packet shape the panel understands differs per model.
                    If the display stays blank or shows noise, send the test
                    picture with each one until a bordered X with a filled
                    corner appears, then pin it.
                  </p>
                  <div className="kb-wires">
                    <button
                      type="button"
                      className={
                        "hs-chip" + (cfg.oled_wire == null ? " active" : "")
                      }
                      onClick={() => void kb.setOledWire(null)}
                    >
                      Automatic
                    </button>
                    {kb.oledWires.map((w) => (
                      <div className="kb-wire-row" key={w.label}>
                        <button
                          type="button"
                          className={
                            "hs-chip" +
                            (sameWire(cfg.oled_wire, w.wire) ? " active" : "")
                          }
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
                </details>
              </section>
            )}
          </>
        )}

        {active === "system" && (
          <>
            {/* ---- the keyboard's own power and dimming ---- */}
            <section className="hs-card">
              <div className="hs-card-head">
                <Ms name="bedtime" style={{ fontSize: 18 }} />
                <h2>Idle &amp; power</h2>
                <Origin kind="stored" />
              </div>

              <div className="hs-slider-top">
                <span>Dim the lighting after</span>
                <span className="hs-slider-val">
                  {cfg.idle_timeout_secs === 0
                    ? "never"
                    : `${cfg.idle_timeout_secs} s`}
                </span>
              </div>
              <input
                type="range"
                min={0}
                max={600}
                step={10}
                value={cfg.idle_timeout_secs}
                onChange={(ev) =>
                  void kb.setIdle(Number(ev.target.value), cfg.idle_brightness)
                }
              />

              <div className="hs-slider-top">
                <span>Brightness once dimmed</span>
                <span className="hs-slider-val">{cfg.idle_brightness}/10</span>
              </div>
              <input
                type="range"
                min={0}
                max={10}
                value={cfg.idle_brightness}
                onChange={(ev) =>
                  void kb.setIdle(
                    cfg.idle_timeout_secs,
                    Number(ev.target.value),
                  )
                }
              />

              <div className="hs-slider-top">
                <span>Sleep after</span>
                <span className="hs-slider-val">
                  {cfg.sleep_minutes === 0
                    ? "never"
                    : `${cfg.sleep_minutes} min`}
                </span>
              </div>
              <input
                type="range"
                min={0}
                max={60}
                value={cfg.sleep_minutes}
                onChange={(ev) =>
                  void kb.setPowerSaving(
                    Number(ev.target.value),
                    cfg.high_efficiency,
                  )
                }
              />

              {kb.status.wireless && (
                <div className="row">
                  <div className="rmain">
                    <div className="rtitle">High-efficiency mode</div>
                    <div className="rsub">
                      Trades lighting for battery life.
                    </div>
                  </div>
                  <Toggle
                    on={cfg.high_efficiency}
                    onClick={() =>
                      void kb.setPowerSaving(
                        cfg.sleep_minutes,
                        !cfg.high_efficiency,
                      )
                    }
                  />
                </div>
              )}
            </section>

            {/* ---- raw probe ---- */}
            <section className="hs-card">
              <details className="kb-details">
                <summary>
                  <Ms name="terminal" style={{ fontSize: 18 }} />
                  Command probe
                </summary>
                <p className="hs-hint">
                  Sends one raw 65-byte control report. This is how the commands
                  above were found; the keyboard ignores what it does not
                  understand, but a wrong length can make it stall — if it stops
                  responding, unplug it once.
                </p>
                <div className="kb-controls">
                  <label className="kb-field">
                    <span>Command</span>
                    <input
                      value={rawCmd}
                      onChange={(ev) => setRawCmd(ev.target.value)}
                    />
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
                      if (Number.isNaN(cmd) || payload.some(Number.isNaN))
                        return;
                      void kb.sendRaw(
                        cmd & 0xff,
                        payload.map((v) => v & 0xff),
                      );
                    }}
                  >
                    Send
                  </button>
                </div>
              </details>
            </section>
          </>
        )}
      </div>
    </div>
  );
}

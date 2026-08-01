import { useEffect, useMemo, useState, type ReactNode } from "react";
import { isTauri } from "../../lib/platform";
import { Ms } from "../Icons";
import {
  useHeadset,
  AUTO_OFF_STEPS,
  type AncMode,
  type LineOutMode,
} from "../../store/headset";

/** A labelled segmented control (mutually exclusive options). */
function Segmented<T extends string | number>({
  value,
  options,
  onChange,
}: Readonly<{
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}>) {
  return (
    <div className="hs-seg">
      {options.map((o) => (
        <button
          key={String(o.value)}
          type="button"
          className={"hs-seg-btn" + (o.value === value ? " active" : "")}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** A labelled horizontal slider with a live value read-out. */
function Slider({
  label,
  value,
  min,
  max,
  step = 1,
  suffix = "",
  onChange,
  disabled,
}: Readonly<{
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  suffix?: string;
  onChange: (v: number) => void;
  disabled?: boolean;
}>) {
  return (
    <label
      className={"hs-slider" + (disabled ? " disabled" : "")}
      aria-label={label}
    >
      <div className="hs-slider-top">
        <span>{label}</span>
        <span className="hs-slider-val">
          {value}
          {suffix}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </label>
  );
}

function Card({
  title,
  icon,
  right,
  children,
}: Readonly<{
  title: string;
  icon: string;
  right?: ReactNode;
  children: ReactNode;
}>) {
  return (
    <section className="hs-card">
      <div className="hs-card-head">
        <Ms name={icon} style={{ fontSize: 18 }} />
        <h2>{title}</h2>
        <div className="hs-card-right">{right}</div>
      </div>
      {children}
    </section>
  );
}

/** Battery colour tone from a percentage. */
function batteryTone(pct: number): "low" | "mid" | "ok" {
  if (pct <= 15) return "low";
  if (pct <= 40) return "mid";
  return "ok";
}

/** A battery pill with a fill bar. */
function Battery({
  label,
  pct,
  charging = false,
}: Readonly<{ label: string; pct: number | null; charging?: boolean }>) {
  // Rendering nothing while the first status frame is still in flight made the
  // card open empty and then grow, pushing everything below it down. The row
  // keeps its height and says it has no reading yet.
  const tone = pct === null ? "" : batteryTone(pct);
  return (
    <div className="hs-batt">
      <span className="hs-batt-label">{label}</span>
      <div className="hs-batt-bar">
        {pct !== null && (
          <div
            className={"hs-batt-fill " + tone + (charging ? " charging" : "")}
            style={{ width: `${pct}%` }}
          />
        )}
      </div>
      <span className={"hs-batt-pct" + (pct === null ? " pending" : "")}>
        {pct === null ? "–" : `${pct}%`}
      </span>
    </div>
  );
}

// Ten EQ bands track these centre frequencies (device custom preset slot).
const EQ_FREQS = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];

function powerTone(status: string | null): string {
  if (status === "online") return "live";
  if (status === "cable_charging") return "tag-charging";
  return "tag-off";
}

function powerLabel(status: string | null): string {
  if (status === "online") return "Online";
  if (status === "cable_charging") return "Charging";
  return "Offline";
}

function bluetoothLabel(connected: boolean | null, powered: boolean | null): string {
  if (connected) return "Connected";
  return powered ? "On" : "Off";
}

export function HeadsetScreen() {
  const h = useHeadset();
  const s = h.status;

  // Controls the device doesn't report back get local state.
  const [micVolume, setMicVolume] = useState(10);
  const [sidetone, setSidetone] = useState(0);
  const [eq, setEq] = useState<number[]>(() => new Array(10).fill(0));
  // Stream mix, seeded from the device once it reports (the base station sends
  // these back in its audio-settings frame) and then owned by the sliders.
  const [mixMain, setMixMain] = useState(100);
  const [mixAux, setMixAux] = useState(100);
  const [mixMic, setMixMic] = useState(0);
  const [eqCategory, setEqCategory] = useState("All");
  const [activePreset, setActivePreset] = useState<string | null>(null);

  // Category tabs come from the preset library itself.
  const eqCategories = useMemo(
    () => ["All", ...Array.from(new Set(h.eqPresets.map((p) => p.category)))],
    [h.eqPresets],
  );

  useEffect(() => {
    void h.init();
    // init is stable; run once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Seed the stream-mix sliders from the device the first time it reports.
  // Without this, moving one slider would send the other two at their default
  // values and silently overwrite whatever the station had.
  const [mixSeeded, setMixSeeded] = useState(false);
  useEffect(() => {
    if (mixSeeded || s.stream_main == null) return;
    setMixMain(s.stream_main);
    setMixAux(s.stream_aux ?? 100);
    setMixMic(s.stream_mic ?? 0);
    setMixSeeded(true);
  }, [mixSeeded, s.stream_main, s.stream_aux, s.stream_mic]);

  // Same story for the EQ, and it matters more: the faders write all ten bands
  // at once and follow up with a flash save, so touching one of them while the
  // others sit at a made-up zero flattens the station's stored curve for good.
  const [eqSeeded, setEqSeeded] = useState(false);
  useEffect(() => {
    if (eqSeeded || !s.eq_bands || s.eq_bands.length !== 10) return;
    setEq(s.eq_bands);
    setEqSeeded(true);
  }, [eqSeeded, s.eq_bands]);

  // Unplugging the base station has to arm both seeds again. They are one-shot
  // by design, so without this the next station — or the same one after a
  // reconnect — keeps the previous station's mix and EQ on screen while its
  // own values sit unread in the status.
  useEffect(() => {
    if (h.connected) return;
    setMixSeeded(false);
    setEqSeeded(false);
  }, [h.connected]);

  const autoOffIdx = useMemo(() => {
    const i = AUTO_OFF_STEPS.indexOf((s.auto_off_minutes ?? 30) as never);
    return i < 0 ? 5 : i;
  }, [s.auto_off_minutes]);

  if (!h.connected) {
    return (
      <div className="content narrow device">
        <div className="hs-empty">
          <Ms name="headphones" style={{ fontSize: 46, opacity: 0.5 }} />
          <p>No Arctis Nova Pro Wireless base station detected.</p>
          <span className="hs-empty-sub">
            Connect the base station over USB. If it's plugged in, make sure your
            user can read <code>/dev/hidraw*</code> (the bundled udev rule grants
            this to the <code>plugdev</code> group).
          </span>
        </div>
      </div>
    );
  }

  const anc: AncMode = s.anc ?? "off";
  const lineOut: LineOutMode = s.line_out ?? "speaker";

  return (
    <div className="content narrow device">
      <div className="screen-head">
        <h1>{h.model ?? "Headset"}</h1>
        <div className="screen-head-actions">
          {/* Three states, not two: "Charging" used to get the same grey as
              "Offline" because the check only asked whether it was online. */}
          <span className={"tag " + powerTone(s.power_status)}>
            <Ms
              name={s.power_status === "cable_charging" ? "battery_charging_full" : "bolt"}
              style={{ fontSize: 11 }}
            />
            {powerLabel(s.power_status)}
          </span>
        </div>
      </div>

      <div className="hs-body">
      {/* ---- status ---- */}
      <Card title="Status" icon="monitoring">
        <div className="hs-batts">
          <Battery
            label="Headset"
            pct={s.headset_battery_percent}
            charging={s.power_status === "cable_charging"}
          />
          <Battery label="Charge slot" pct={s.charge_slot_battery_percent} />
        </div>
        {/* Tinted by state, the same way the battery bar already is.
            Rendering "Unpaired" and "Muted" in the same weight as "Paired" and
            "Live" made the row something you had to read rather than see. */}
        <div className="hs-status-grid">
          <div>
            <span>Volume</span>
            <b>{s.volume_percent ?? "–"}%</b>
          </div>
          <div>
            <span>Wireless</span>
            <b className={s.wireless_paired ? "good" : "bad"}>
              {s.wireless_paired ? "Paired" : "Unpaired"}
            </b>
          </div>
          <div>
            <span>Bluetooth</span>
            <b className={s.bluetooth_connected ? "good" : ""}>
              {bluetoothLabel(s.bluetooth_connected, s.bluetooth_powered)}
            </b>
          </div>
          <div>
            <span>Mic</span>
            <b className={s.mic_muted ? "warn" : "good"}>
              {s.mic_muted ? "Muted" : "Live"}
            </b>
          </div>
        </div>
      </Card>

      {/* ---- audio ---- */}
      <Card title="Noise control" icon="noise_control_on">
        <Segmented<AncMode>
          value={anc}
          onChange={h.setAnc}
          options={[
            { value: "off", label: "Off" },
            { value: "transparent", label: "Transparency" },
            { value: "on", label: "ANC" },
          ]}
        />
        <Slider
          label="Transparency level"
          value={s.transparency_level ?? 5}
          min={1}
          max={10}
          disabled={anc !== "transparent"}
          onChange={h.setTransparency}
        />
        <div className="hs-row">
          <span>Audio gain</span>
          <Segmented
            value={s.gain_high ? 1 : 0}
            onChange={(v) => h.setGainHigh(v === 1)}
            options={[
              { value: 0, label: "Low" },
              { value: 1, label: "High" },
            ]}
          />
        </div>
        {/* Writes a WirePlumber fragment that only takes effect at the next
            login, so it is a desktop setting and the remote cannot send it. */}
        <div className="hs-row" hidden={!isTauri}>
          <span>
            Anti-crackle headroom{" "}
            <span className="hs-hint" style={{ display: "block" }}>
              WirePlumber quirk · +85 ms latency · applies after next login
            </span>
          </span>
          <Segmented
            value={h.alsaHeadroom ? 1 : 0}
            onChange={(v) => void h.setAlsaHeadroom(v === 1)}
            options={[
              { value: 0, label: "Off" },
              { value: 1, label: "On" },
            ]}
          />
        </div>
      </Card>

      {/* ---- microphone ---- */}
      <Card title="Microphone" icon="mic">
        <Slider
          label="Mic volume"
          value={micVolume}
          min={1}
          max={10}
          onChange={(v) => {
            setMicVolume(v);
            h.setMicVolume(v);
          }}
        />
        <div className="hs-row">
          <span>Sidetone</span>
          <Segmented
            value={sidetone}
            onChange={(v) => {
              setSidetone(v);
              h.setSidetone(v);
            }}
            options={[
              { value: 0, label: "Off" },
              { value: 1, label: "Low" },
              { value: 2, label: "Med" },
              { value: 3, label: "High" },
            ]}
          />
        </div>
        <Slider
          label="Mute-LED brightness"
          value={s.mic_led_percent ? Math.round(s.mic_led_percent / 10) : 10}
          min={1}
          max={10}
          onChange={h.setMicLed}
        />
      </Card>

      {/* ---- hardware EQ ---- */}
      <Card
        title="Hardware equalizer"
        icon="equalizer"
      >
        {/* The category was already a field on every preset but only reachable
            through a dropdown in the card header, so "All" dropped thirty
            presets into one unsorted cloud. As a row of switches the split is
            visible without being opened, and "All" keeps them grouped. */}
        <div className="hs-seg hs-eq-cats">
          {eqCategories.map((c) => (
            <button
              type="button"
              key={c}
              className={"hs-seg-btn" + (eqCategory === c ? " active" : "")}
              onClick={() => setEqCategory(c)}
            >
              {c}
            </button>
          ))}
        </div>
        {(eqCategory === "All"
          ? eqCategories.filter((c) => c !== "All")
          : [eqCategory]
        ).map((cat) => (
          <div key={cat}>
            {eqCategory === "All" && <div className="hs-eq-cat-label">{cat}</div>}
            <div className="hs-eq-presets">
              {h.eqPresets
                .filter((p) => p.category === cat)
                .map((p) => (
                  <button
                    key={p.name}
                    type="button"
                    className={"hs-chip" + (activePreset === p.name ? " primary" : "")}
                    title={p.description}
                    onClick={() => {
                      setActivePreset(p.name);
                      // null = the device rejected it (the store shows why).
                      void h.applyEqPreset(p.name).then((bands) => bands && setEq(bands));
                    }}
                  >
                    {p.name}
                  </button>
                ))}
            </div>
          </div>
        ))}
        {/* The value runs -10..+10 dB in half-steps and used to appear
            nowhere: a setting you could neither read off nor reproduce, on the
            EQ that actually lives in the headset. */}
        <div className="hs-eq-axis" aria-hidden="true">
          <span>+10</span>
          <span>0</span>
          <span>−10</span>
        </div>
        <div className="hs-eq">
          <div className="hs-eq-lines" aria-hidden="true">
            <i style={{ top: "25%" }} />
            <i className="zero" style={{ top: "50%" }} />
            <i style={{ top: "75%" }} />
          </div>
          {eq.map((v, i) => (
            <div className="hs-eq-band" key={EQ_FREQS[i]}>
              <span className={"hs-eq-val" + (v === 0 ? " zero" : "")}>
                {v > 0 ? `+${v.toFixed(1)}` : v.toFixed(1)}
              </span>
              <input
                type="range"
                className="hs-eq-fader"
                aria-label={`${EQ_FREQS[i]} band`}
                aria-valuetext={`${v > 0 ? "+" : ""}${v.toFixed(1)} dB`}
                min={-10}
                max={10}
                step={0.5}
                value={v}
                onChange={(e) => {
                  const next = eq.slice();
                  next[i] = Number(e.target.value);
                  setEq(next);
                  setActivePreset(null); // hand-edited: no longer a preset
                  h.setEqBands(next);
                }}
              />
              <span>{EQ_FREQS[i]}</span>
            </div>
          ))}
        </div>
      </Card>

      {/* ---- line out ---- */}
      <Card title="Line out" icon="cable">
        <Segmented<LineOutMode>
          value={lineOut}
          onChange={h.setLineOut}
          options={[
            { value: "speaker", label: "Speaker" },
            { value: "stream", label: "Stream" },
          ]}
        />
        {lineOut === "stream" && (
          <>
            <Slider label="Main" step={5} value={mixMain} min={0} max={100} suffix="%"
              onChange={(v) => { setMixMain(v); h.setStreamMix(v, mixAux, mixMic); }} />
            <Slider label="Aux" step={5} value={mixAux} min={0} max={100} suffix="%"
              onChange={(v) => { setMixAux(v); h.setStreamMix(mixMain, v, mixMic); }} />
            <Slider label="Microphone" step={5} value={mixMic} min={0} max={100} suffix="%"
              onChange={(v) => { setMixMic(v); h.setStreamMix(mixMain, mixAux, v); }} />
          </>
        )}
      </Card>

      {/* ---- power & wireless ---- */}
      <Card title="Power & wireless" icon="settings_power">
        <Slider
          label="Auto shut-off"
          value={autoOffIdx}
          min={0}
          max={6}
          onChange={h.setAutoOff}
          suffix={autoOffIdx === 0 ? " (never)" : ` (${AUTO_OFF_STEPS[autoOffIdx]} min)`}
        />
        <div className="hs-row">
          <span>2.4 GHz mode</span>
          <Segmented
            value={s.wireless_range_mode ? 1 : 0}
            onChange={(v) => h.setWirelessRange(v === 1)}
            options={[
              { value: 0, label: "Speed" },
              { value: 1, label: "Range" },
            ]}
          />
        </div>
      </Card>
      </div>
    </div>
  );
}

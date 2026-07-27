import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { useMixerStore } from "../../store/mixer";
import { useTheme, THEMES } from "../../store/theme";
import { useUpdate } from "../../store/update";
import type { OutputDevice } from "../../types";
import { Ms } from "../Icons";
import { ConfirmModal } from "../ConfirmModal";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Toggle } from "../Toggle";

interface DefaultDevices {
  output: string | null;
  input: string | null;
}

type LabelStyle = "plain" | "suffix" | "prefix";

const LABEL_STYLES: { value: LabelStyle; label: string; example: string }[] = [
  { value: "plain", label: "Plain", example: "Game" },
  { value: "suffix", label: "Suffix", example: "Game (Inari)" },
  { value: "prefix", label: "Prefix", example: "Inari · Game" },
];

/** Card row with a device dropdown for picking a system default. */
function DeviceRow({
  icon,
  title,
  sub,
  devices,
  current,
  onPick,
}: Readonly<{
  icon: string;
  title: string;
  /** What this default is used for. */
  sub: string;
  devices: OutputDevice[];
  current: string | null;
  onPick: (name: string) => void;
}>) {
  const [open, setOpen] = useState(false);
  const currentDesc = devices.find((d) => d.name === current)?.description ?? current ?? "-";

  return (
    <div className="row">
      <div className="ricon">
        <Ms name={icon} />
      </div>
      <div className="rmain">
        <div className="rtitle">{title}</div>
        <div className="rsub">{sub}</div>
      </div>
      <div style={{ position: "relative" }}>
        <button type="button" className="select device-select" onClick={() => setOpen((o) => !o)}>
          <span className="device-select-name">{currentDesc}</span>
          <Ms name="expand_more" />
        </button>
        <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="end">
          {devices.map((d) => (
            <MenuItem
              key={d.name}
              icon={icon}
              selected={d.name === current}
              showCheck
              onClick={() => {
                onPick(d.name);
                setOpen(false);
              }}
            >
              {d.description}
            </MenuItem>
          ))}
        </Popover>
      </div>
    </div>
  );
}

// A stopped engine is its own state, not a kind of fallback: the fallback
// still works, a stopped loop means audio control is gone until a restart.
function engineDesc(native: boolean | null, alive: boolean): string {
  if (!alive) return "Stopped - restart Inari to restore audio control";
  if (native === null) return "…";
  return native
    ? "Native PipeWire (pipewire-rs) - live metering, passive routing"
    : "pactl fallback - native engine unavailable on this system";
}

export function SettingsScreen() {
  const { theme, setTheme } = useTheme();
  const update = useUpdate();
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [startMinimized, setStartMinimized] = useState(false);
  const [backendNative, setBackendNative] = useState<boolean | null>(null);
  // One source of truth with the title bar, which re-checks on every failure.
  const engineAlive = useMixerStore((s) => s.engineAlive);
  const [version, setVersion] = useState("");
  const [defaults, setDefaults] = useState<DefaultDevices>({ output: null, input: null });
  const [labelStyle, setLabelStyle] = useState<LabelStyle>("plain");
  const [labelStyleOpen, setLabelStyleOpen] = useState(false);
  const [confirmingReset, setConfirmingReset] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const outputDevices = useMixerStore((s) => s.outputDevices);
  const inputDevices = useMixerStore((s) => s.inputDevices);
  const replayOnboarding = useMixerStore((s) => s.replayOnboarding);
  const showBalance = useMixerStore((s) => s.showBalance);
  const setBalanceVisible = useMixerStore((s) => s.setBalanceVisible);

  useEffect(() => {
    void invoke<boolean>("get_autostart").then(setAutostart);
    void invoke<{ native: boolean }>("get_backend_info").then((i) => setBackendNative(i.native));
    void invoke<DefaultDevices>("get_default_devices").then(setDefaults).catch(() => {});
    void invoke<{ device_label_style: LabelStyle; start_minimized: boolean }>("get_prefs")
      .then((p) => {
        setLabelStyle(p.device_label_style);
        setStartMinimized(p.start_minimized);
      })
      .catch(() => {});
    void getVersion().then(setVersion);
  }, []);

  const pickDefault = async (kind: "output" | "input", name: string) => {
    try {
      await invoke(kind === "output" ? "set_default_output" : "set_default_input", { name });
      setDefaults((d) => ({ ...d, [kind]: name }));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const pickLabelStyle = async (style: LabelStyle) => {
    try {
      await invoke("set_device_label_style", { style });
      setLabelStyle(style);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleAutostart = async () => {
    if (autostart === null) return;
    try {
      const actual = await invoke<boolean>("set_autostart", { enabled: !autostart });
      setAutostart(actual);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleStartMinimized = async () => {
    const next = !startMinimized;
    setStartMinimized(next);
    try {
      await invoke("set_start_minimized", { minimized: next });
      setError(null);
    } catch (e) {
      setStartMinimized(!next);
      setError(String(e));
    }
  };

  let updateSub = "Check for a new release";
  if (update.checking) updateSub = "Checking…";
  else if (update.info?.available) updateSub = `Inari ${update.info.latest} is available`;
  else if (update.error) updateSub = update.error;
  else if (update.info) updateSub = `You're on the latest (v${update.info.current})`;

  return (
    <div className="content narrow">
      <div className="screen-head">
        <h1>Settings</h1>
      </div>
      <div className="screen-scroll">
        {error && <div className="error-banner" style={{ borderRadius: 8 }}>{error}</div>}

        <div className="section-label">Appearance</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="palette" />
            </div>
            <div className="rmain">
              <div className="rtitle">Theme</div>
              <div className="rsub">Original, or Tokyo Night to match your desktop</div>
            </div>
            <div className="theme-picker">
              {THEMES.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  className={"theme-swatch" + (t.id === theme ? " active" : "")}
                  onClick={() => setTheme(t.id)}
                  title={t.label}
                >
                  <span className="theme-swatch-colors">
                    {t.swatch.map((c) => (
                      <i key={c} style={{ background: c }} />
                    ))}
                  </span>
                  <span className="theme-swatch-label">{t.label}</span>
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="section-label">Preferences</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="label" />
            </div>
            <div className="rmain">
              <div className="rtitle">Device naming</div>
              <div className="rsub">Naming scheme for Inari-managed devices</div>
            </div>
            <div style={{ position: "relative" }}>
              <button type="button" className="select" onClick={() => setLabelStyleOpen((o) => !o)}>
                <span>{LABEL_STYLES.find((s) => s.value === labelStyle)?.label}</span>
                <Ms name="expand_more" />
              </button>
              <Popover open={labelStyleOpen} onClose={() => setLabelStyleOpen(false)} side="bottom" align="end">
                {LABEL_STYLES.map((s) => (
                  <MenuItem
                    key={s.value}
                    selected={s.value === labelStyle}
                    showCheck
                    onClick={() => {
                      void pickLabelStyle(s.value);
                      setLabelStyleOpen(false);
                    }}
                  >
                    {s.example}
                  </MenuItem>
                ))}
              </Popover>
            </div>
          </div>
          <DeviceRow
            icon="speaker"
            title="Default output"
            sub="Where channels set to “System default” play"
            devices={outputDevices}
            current={defaults.output}
            onPick={(name) => void pickDefault("output", name)}
          />
          <DeviceRow
            icon="mic"
            title="Default input"
            sub="The microphone the Inari mic chain captures"
            devices={inputDevices}
            current={defaults.input}
            onPick={(name) => void pickDefault("input", name)}
          />
          <div className="row">
            <div className="ricon">
              <Ms name="balance" />
            </div>
            <div className="rmain">
              <div className="rtitle">Balance slider</div>
              <div className="rsub">ChatMix-style blend of two channels in the title bar</div>
            </div>
            <Toggle on={showBalance} onClick={() => void setBalanceVisible(!showBalance)} />
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="rocket_launch" />
            </div>
            <div className="rmain">
              <div className="rtitle">Start at login</div>
              <div className="rsub">systemd user service, starts with your desktop session</div>
            </div>
            {autostart !== null && <Toggle on={autostart} onClick={() => void toggleAutostart()} />}
          </div>
          {autostart && (
            <div className="row row-sub">
              <div className="ricon">
                <Ms name="dock_to_bottom" />
              </div>
              <div className="rmain">
                <div className="rtitle">Start minimized</div>
                <div className="rsub">Boot to the tray instead of opening the window</div>
              </div>
              <Toggle
                on={startMinimized}
                onClick={() => void toggleStartMinimized()}
              />
            </div>
          )}
        </div>

        <div className="section-label">About</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="cable" />
            </div>
            <div className="rmain">
              <div className="rtitle">Audio engine</div>
              <div className="rsub">
                {engineDesc(backendNative, engineAlive)}
              </div>
            </div>
            {!engineAlive ? (
              <span className="tag" role="status">stopped</span>
            ) : (
              backendNative !== null && (
                <span className={"tag" + (backendNative ? " live" : "")}>
                  {backendNative ? "native" : "fallback"}
                </span>
              )
            )}
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="info" />
            </div>
            <div className="rmain">
              <div className="rtitle">Inari {version}</div>
              <div className="rsub">GPL-3.0 · config in ~/.config/inari</div>
            </div>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="system_update" />
            </div>
            <div className="rmain">
              <div className="rtitle">Updates</div>
              <div className="rsub">{updateSub}</div>
            </div>
            {update.info?.available && update.info.can_self_install ? (
              <button
                type="button"
                className="select"
                disabled={update.applying}
                onClick={() => void update.apply()}
              >
                <span>{update.applying ? "Installing…" : "Update"}</span>
              </button>
            ) : (
              <button
                type="button"
                className="select"
                disabled={update.checking}
                onClick={() => void update.check()}
              >
                <span>{update.checking ? "Checking…" : "Check now"}</span>
              </button>
            )}
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="description" />
            </div>
            <div className="rmain">
              <div className="rtitle">Logs</div>
              <div className="rsub">
                Open the log folder to attach a file to a bug report
              </div>
            </div>
            <button
              type="button"
              className="select"
              onClick={() =>
                void invoke("open_log_dir").catch((e) => setError(String(e)))
              }
            >
              <span>Open</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="school" />
            </div>
            <div className="rmain">
              <div className="rtitle">Tutorial</div>
              <div className="rsub">Replay the first-run tour</div>
            </div>
            <button type="button" className="select" onClick={replayOnboarding}>
              <span>Replay</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="restart_alt" />
            </div>
            <div className="rmain">
              <div className="rtitle">Reset Inari</div>
              <div className="rsub">
                Erase all channels, mixes, profiles, app history and preferences
              </div>
            </div>
            <button type="button" className="select" onClick={() => setConfirmingReset(true)}>
              <span>Reset…</span>
            </button>
          </div>
        </div>
      </div>

      <ConfirmModal
        open={confirmingReset}
        onClose={() => setConfirmingReset(false)}
        title="Reset Inari?"
        confirmLabel="Reset everything"
        onConfirm={() => void invoke("reset_app").catch((e) => setError(String(e)))}
      >
        Everything you've set up - channels, mixes, profiles, app assignments,
        history and preferences - is permanently deleted, and Inari relaunches
        as if freshly installed.
      </ConfirmModal>
    </div>
  );
}

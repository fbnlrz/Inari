import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "./lib/platform";
import { TitleBar } from "./components/TitleBar/TitleBar";
import { MixerBoard } from "./components/MixerBoard/MixerBoard";
import { AppList } from "./components/AppList/AppList";
import { MediaScreen } from "./components/Media/MediaScreen";
import { MicScreen } from "./components/Mic/MicScreen";
import { HeadsetScreen } from "./components/Headset/HeadsetScreen";
import { OledScreen } from "./components/Oled/OledScreen";
import { MouseScreen } from "./components/Mouse/MouseScreen";
import { OnboardingModal } from "./components/Onboarding/OnboardingModal";
import { SettingsScreen } from "./components/Settings/SettingsScreen";
import { Ms } from "./components/Icons";
import { Tooltip } from "./components/Tooltip";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { useAudio } from "./hooks/useAudio";
import { useHeadset } from "./store/headset";
import { useMixerStore } from "./store/mixer";
import { useUpdate } from "./store/update";

const NAV = [
  { id: "mixer", icon: "graphic_eq", label: "Mixer" },
  { id: "apps", icon: "grid_view", label: "Apps" },
  // Playback, not hardware: it sits with the mixer and the apps whose sound it
  // is, above the row of devices. Reachable from the remote too - a tablet
  // next to the keyboard is exactly where you skip a track from.
  { id: "media", icon: "play_circle", label: "Media" },
  { id: "mic", icon: "mic", label: "Mic" },
  { id: "headset", icon: "headphones", label: "Headset" },
  { id: "oled", icon: "tv_gen", label: "OLED" },
  { id: "mouse", icon: "mouse", label: "Mouse" },
] as const;

type NavId = (typeof NAV)[number]["id"] | "settings";

// Every mouse command is off the remote by choice - a tablet mixes audio, it
// does not reconfigure the DPI of a mouse plugged into the PC. So the rail
// drops the entry rather than offering one that answers with a rejection.
const RAIL = NAV.filter((n) => n.id !== "mouse" || isTauri);

/** The app-wide failure banner; every store surfaces its errors through it. */
function ErrorBanner({
  label,
  message,
  onDismiss,
}: Readonly<{ label: string; message: string; onDismiss: () => void }>) {
  return (
    <div className="error-banner" role="alert">
      <span className="error-banner-msg">
        <strong>{label}</strong> {message}
      </span>
      <button
        type="button"
        className="error-banner-x"
        aria-label="Dismiss error"
        title="Dismiss"
        onClick={onDismiss}
      >
        <Ms name="close" style={{ fontSize: 16 }} />
      </button>
    </div>
  );
}

export default function App() {
  useAudio();
  const [nav, setNav] = useState<NavId>("mixer");
  const [version, setVersion] = useState("");
  const error = useMixerStore((s) => s.error);
  const clearError = useMixerStore((s) => s.clearError);
  const headsetError = useHeadset((s) => s.error);
  const clearHeadsetError = useHeadset((s) => s.clearError);

  const update = useUpdate();

  useEffect(() => {
    // Not available outside the desktop shell; the remote just omits it.
    if (isTauri) void getVersion().then(setVersion);
    // Non-blocking check on launch; failures stay silent (surfaced in Settings).
    // Updating is the PC's business, so the remote never asks.
    if (isTauri) void useUpdate.getState().check();
  }, []);

  // "mouse" has no screen behind it in the browser, so a nav id that arrives
  // there anyway lands on the mixer instead of on nothing.
  const active: NavId = nav === "mouse" && !isTauri ? "mixer" : nav;

  const current =
    active === "settings"
      ? { label: "Settings" }
      : (NAV.find((n) => n.id === active) ?? NAV[0]);

  let screen;
  if (active === "apps") screen = <AppList />;
  else if (active === "media") screen = <MediaScreen />;
  else if (active === "mic") screen = <MicScreen />;
  else if (active === "headset") screen = <HeadsetScreen />;
  else if (active === "oled") screen = <OledScreen />;
  else if (active === "mouse") screen = <MouseScreen />;
  else if (active === "settings") screen = <SettingsScreen />;
  else screen = <MixerBoard />;

  return (
    <div className="window">
      <TitleBar screen={current.label} />

      {error && (
        <ErrorBanner label="Audio error:" message={error} onDismiss={clearError} />
      )}

      {headsetError && (
        <ErrorBanner
          label="Headset error:"
          message={headsetError}
          onDismiss={clearHeadsetError}
        />
      )}

      {/* Checking, installing and restarting all run on the PC; none of the
       * three is reachable from the remote, so the banner stays behind. */}
      {isTauri &&
        (update.installed ? (
          <div className="update-banner" role="status">
            <Ms name="check_circle" className="update-banner-icon" />
            <span className="update-banner-msg">
              <strong>Update installed.</strong> Restarting Inari…
            </span>
            <button type="button" className="update-banner-btn" onClick={() => void update.restart()}>
              Restart now
            </button>
          </div>
        ) : (
          update.info?.available &&
          !update.dismissed && (
            <div className="update-banner" role="status">
              <Ms name="download" className="update-banner-icon" />
              <span className="update-banner-msg">
                <strong>Update available:</strong> Inari {update.info.latest}
                <span className="update-banner-sub"> (you have {update.info.current})</span>
              </span>
              {update.error && <span className="update-banner-err">{update.error}</span>}
              {update.info.can_self_install && (
                <button
                  type="button"
                  className="update-banner-btn"
                  disabled={update.applying}
                  onClick={() => void update.apply()}
                >
                  {update.applying ? "Installing…" : "Update now"}
                </button>
              )}
              <button
                type="button"
                className="update-banner-link"
                onClick={() => update.info && void update.openNotes()}
              >
                Release notes
              </button>
              <button
                type="button"
                className="update-banner-x"
                aria-label="Dismiss update notice"
                title="Dismiss"
                onClick={() => update.dismiss()}
              >
                <Ms name="close" style={{ fontSize: 16 }} />
              </button>
            </div>
          )
        ))}

      <div className="body">
        <nav className="rail">
          {RAIL.map((n) => (
            <button
              type="button"
              key={n.id}
              className={"nav-item" + (n.id === active ? " active" : "")}
              onClick={() => setNav(n.id)}
            >
              <Ms name={n.icon} />
              <span className="nav-label">{n.label}</span>
            </button>
          ))}
          <div className="rail-spacer" />
          <button
            type="button"
            className={"nav-item" + (active === "settings" ? " active" : "")}
            onClick={() => setNav("settings")}
          >
            <Ms name="settings" />
            <span className="nav-label">Settings</span>
          </button>
          {version && <div className="rail-version">v{version}</div>}
        </nav>

        {/* Keyed on the screen so a crashed one clears when the user
         * navigates away, instead of sticking until a reload. */}
        <ErrorBoundary key={active}>{screen}</ErrorBoundary>
      </div>

      <OnboardingModal />
      <Tooltip />
    </div>
  );
}

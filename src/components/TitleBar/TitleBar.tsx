import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useMixerStore } from "../../store/mixer";
import { isTauri } from "../../lib/platform";
import { Ms, InariMark } from "../Icons";
import { BalanceBar } from "../MixerBoard/BalanceBar";
import { ProfileMenu } from "./ProfileMenu";

/**
 * Frameless headerbar: brand, current screen, engine status, window
 * controls. The close button triggers the normal close-requested flow,
 * which the Rust side intercepts to hide to tray.
 *
 * The window controls only exist in the desktop shell. `getCurrentWindow()`
 * reads Tauri internals and throws in a browser — during render, so it takes
 * the whole app down rather than just the buttons. The remote gets a header
 * without them, which is what a tablet wants anyway.
 */
export function TitleBar({ screen }: Readonly<{ screen: string }>) {
  const win = isTauri ? getCurrentWindow() : null;
  const error = useMixerStore((s) => s.error);
  const initialized = useMixerStore((s) => s.initialized);
  const engineAlive = useMixerStore((s) => s.engineAlive);
  const checkEngine = useMixerStore((s) => s.checkEngine);

  // A dead PipeWire loop surfaces as an ordinary command failure, so a fresh
  // error is the cue to ask whether the engine is still there at all.
  useEffect(() => {
    if (error) void checkEngine();
  }, [error, checkEngine]);

  // A stopped engine outranks the rest: the others are recoverable, this one
  // needs a restart.
  let status = "Starting…";
  if (!engineAlive) status = "Engine stopped";
  else if (error) status = "Engine error";
  else if (initialized) status = "Engine running";
  const bad = !engineAlive || !!error;

  return (
    <header data-tauri-drag-region className="headerbar">
      <div data-tauri-drag-region className="hb-brand">
        <div className="hb-logo">
          <InariMark />
        </div>
        <div data-tauri-drag-region className="hb-title">
          Inari
        </div>
      </div>
      <div data-tauri-drag-region className="hb-sub">
        {screen}
      </div>
      <div data-tauri-drag-region className="hb-spacer" />
      <BalanceBar />
      <ProfileMenu />
      <div
        className={"hb-status" + (bad ? " err" : "")}
        role="status"
        title={!engineAlive ? "The audio engine stopped. Restart Inari to restore audio control." : undefined}
      >
        <span className="dot" />
        {status}
      </div>
      {win && (
        <div className="wctl">
          <button type="button" className="wbtn" aria-label="Minimize" onClick={() => void win.minimize()}>
            <Ms name="remove" />
          </button>
          <button
            type="button"
            className="wbtn"
            aria-label="Maximize"
            onClick={() => void win.toggleMaximize()}
          >
            <Ms name="crop_square" style={{ fontSize: 13 }} />
          </button>
          <button
            type="button"
            className="wbtn close"
            aria-label="Close (hide to tray)"
            title="Hides to tray - quit from the tray menu"
            onClick={() => void win.close()}
          >
            <Ms name="close" />
          </button>
        </div>
      )}
    </header>
  );
}

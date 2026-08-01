import { useEffect, useState } from "react";
import { useMixerStore } from "../../store/mixer";
import {
  formatAccelerator,
  toAccelerator,
  useHotkeys,
  HOTKEY_LABELS,
  type HotkeyAction,
  type HotkeyBinding,
} from "../../store/hotkeys";
import { Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";

/**
 * The capture control. While armed it eats every key press, so Ctrl+W and
 * friends can be bound without the window acting on them first; Esc leaves
 * without changing anything.
 */
function CaptureButton({
  binding,
  armed,
  onArm,
  onCapture,
  onCancel,
}: Readonly<{
  binding: HotkeyBinding;
  armed: boolean;
  onArm: () => void;
  onCapture: (accelerator: string) => void;
  onCancel: () => void;
}>) {
  useEffect(() => {
    if (!armed) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.code === "Escape") {
        onCancel();
        return;
      }
      const accelerator = toAccelerator(e);
      // Null means "still only modifiers", or a key that may not be bound on
      // its own - keep listening rather than closing on a half-press.
      if (accelerator) onCapture(accelerator);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [armed, onCapture, onCancel]);

  let text = "Not set";
  if (armed) text = "Press a key…";
  else if (binding.accelerator) text = formatAccelerator(binding.accelerator);

  return (
    <button type="button" className="select" onClick={onArm} disabled={armed}>
      <span>{text}</span>
    </button>
  );
}

/**
 * Global hotkeys. Nothing is bound until the user asks for it, and a binding
 * the session refused (or silently swallows, which is the Wayland norm) is
 * called out rather than left looking healthy.
 */
export function HotkeysSection() {
  const bindings = useHotkeys((s) => s.bindings);
  const error = useHotkeys((s) => s.error);
  const fetchHotkeys = useHotkeys((s) => s.fetch);
  const setBinding = useHotkeys((s) => s.setBinding);
  const setChannel = useHotkeys((s) => s.setChannel);
  const channels = useMixerStore((s) => s.channels);
  const [armed, setArmed] = useState<HotkeyAction | null>(null);
  const [channelOpen, setChannelOpen] = useState(false);

  useEffect(() => {
    void fetchHotkeys();
  }, [fetchHotkeys]);

  const channelBinding = bindings.find((b) => b.action === "channel_mute");
  const target = channelBinding?.channel ?? null;
  const targetLabel =
    channels.find((c) => c.name === target)?.label ??
    (channels[0] ? `${channels[0].label} (first)` : "First channel");

  return (
    <>
      <div className="section-label">Hotkeys</div>
      <div className="card">
        {error && (
          <div className="error-banner" style={{ borderRadius: 8, margin: "var(--sp-1)" }}>
            {error}
          </div>
        )}
        {bindings.map((binding) => {
          const meta = HOTKEY_LABELS[binding.action];
          const dead = binding.accelerator !== null && !binding.registered;
          return (
            <div className="row" key={binding.action}>
              <div className="ricon">
                <Ms name={meta.icon} />
              </div>
              <div className="rmain">
                <div className="rtitle">{meta.title}</div>
                <div className="rsub">
                  {dead ? `Not active: ${binding.error ?? "the session refused the grab"}` : meta.sub}
                </div>
              </div>
              {dead && (
                <span className="tag" role="status">
                  inactive
                </span>
              )}
              <CaptureButton
                binding={binding}
                armed={armed === binding.action}
                onArm={() => setArmed(binding.action)}
                onCapture={(accelerator) => {
                  setArmed(null);
                  void setBinding(binding.action, accelerator);
                }}
                onCancel={() => setArmed(null)}
              />
              <button
                type="button"
                className="select"
                disabled={binding.accelerator === null}
                onClick={() => void setBinding(binding.action, null)}
              >
                <span>Clear</span>
              </button>
            </div>
          );
        })}
        {channelBinding && (
          <div className="row row-sub">
            <div className="ricon">
              <Ms name="tune" />
            </div>
            <div className="rmain">
              <div className="rtitle">Channel to mute</div>
              <div className="rsub">Which channel the “Mute a channel” hotkey acts on</div>
            </div>
            <div style={{ position: "relative" }}>
              <button type="button" className="select" onClick={() => setChannelOpen((o) => !o)}>
                <span>{targetLabel}</span>
                <Ms name="expand_more" />
              </button>
              <Popover open={channelOpen} onClose={() => setChannelOpen(false)} side="bottom" align="end">
                {channels.map((c) => (
                  <MenuItem
                    key={c.name}
                    selected={c.name === target}
                    showCheck
                    onClick={() => {
                      void setChannel(c.name);
                      setChannelOpen(false);
                    }}
                  >
                    {c.label}
                  </MenuItem>
                ))}
              </Popover>
            </div>
          </div>
        )}
        <div className="row">
          <div className="ricon">
            <Ms name="warning" />
          </div>
          <div className="rmain">
            <div className="rtitle">If a hotkey never fires</div>
            <div className="rsub">
              Global hotkeys are grabbed through X11. On a Wayland session the
              compositor owns the keyboard, so a binding can register here and
              still never reach Inari. Bind it in your desktop's own keyboard
              settings instead and point it at a command, e.g.{" "}
              <code>inari mute mic</code> or <code>inari profile Gaming</code>.
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

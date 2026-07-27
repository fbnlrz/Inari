import { useEffect, useState } from "react";
import { isTauri } from "../../lib/platform";
import { useRemote, type RemoteInterface } from "../../store/remote";
import { Ms } from "../Icons";
import { ConfirmModal } from "../ConfirmModal";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Toggle } from "../Toggle";

/**
 * How often the connected-client count is re-read while the server is up. The
 * backend has no event for a client coming and going, and inventing one for a
 * number that changes twice an evening is not worth a subscription.
 */
const CLIENT_POLL_MS = 3000;

/** Ports below 1024 need root, and the listener deliberately does not have it. */
const PORT_MIN = 1024;
const PORT_MAX = 65535;

/** What binding to this address actually exposes, in plain words. */
function bindSub(iface: RemoteInterface | undefined): string {
  if (!iface) return "Pick the address the remote listens on";
  if (iface.loopback) return "This machine only - nothing on the network can reach it";
  if (iface.wildcard) return "Every network this machine is on, including ones you forgot about";
  return "Devices on this network can reach it at this address";
}

function clientsSub(clients: number): string {
  if (clients === 0) return "No devices connected";
  return clients === 1 ? "1 device connected" : `${clients} devices connected`;
}

/**
 * The QR arrives from our own backend as SVG markup; drawing it as-is is what
 * keeps a QR library out of the bundle. The prefix check is belt and braces -
 * anything that is not an SVG document is simply not drawn.
 */
function QrCode({ svg }: Readonly<{ svg: string }>) {
  if (!svg.trimStart().startsWith("<svg")) return null;
  return (
    <div
      className="remote-qr"
      role="img"
      aria-label="Pairing QR code. Scan it with the tablet's camera."
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}

/**
 * The token, hidden until asked for. Mounted with the token as its key, so a
 * regenerated token comes back concealed instead of inheriting the last
 * reveal. It is never put in a `title` or an attribute: a screenshot of this
 * page is a routine thing to attach to a bug report, and the token is the
 * whole of the authentication.
 */
function TokenReveal({ token }: Readonly<{ token: string }>) {
  const [shown, setShown] = useState(false);
  return (
    <div className="remote-token">
      <code className={"remote-token-value" + (shown ? "" : " hidden")}>
        {shown ? token : "••••••••••••••••"}
      </code>
      <button
        type="button"
        className="select"
        aria-pressed={shown}
        onClick={() => setShown((s) => !s)}
      >
        <span>{shown ? "Hide" : "Show token"}</span>
      </button>
    </div>
  );
}

/**
 * Inari Remote: pairing and exposure controls. Off by default - turning it on
 * is the moment the app stops being a single-machine program, so the switch
 * says so rather than reading like another preference.
 */
function RemoteControls() {
  const status = useRemote((s) => s.status);
  const pairing = useRemote((s) => s.pairing);
  const busy = useRemote((s) => s.busy);
  const error = useRemote((s) => s.error);
  const fetchStatus = useRemote((s) => s.fetch);
  const setEnabled = useRemote((s) => s.setEnabled);
  const setBind = useRemote((s) => s.setBind);
  const regenerateToken = useRemote((s) => s.regenerateToken);

  const [bindOpen, setBindOpen] = useState(false);
  const [confirmingRegen, setConfirmingRegen] = useState(false);
  // null = "whatever the backend says". Holding the draft only while the user
  // is mid-edit keeps the field in step with the status poll without an effect
  // that would fight their typing.
  const [portDraft, setPortDraft] = useState<string | null>(null);

  const enabled = status?.enabled ?? false;

  useEffect(() => {
    void fetchStatus();
  }, [fetchStatus]);

  useEffect(() => {
    if (!enabled) return;
    const id = setInterval(() => void fetchStatus(), CLIENT_POLL_MS);
    return () => clearInterval(id);
  }, [enabled, fetchStatus]);

  const interfaces = status?.interfaces ?? [];
  const current = interfaces.find((i) => i.address === status?.address);
  const portValue = portDraft ?? String(status?.port ?? "");

  const commitPort = () => {
    setPortDraft(null);
    if (!status || portDraft === null) return;
    const parsed = Number.parseInt(portDraft, 10);
    if (Number.isNaN(parsed) || parsed < PORT_MIN || parsed > PORT_MAX) return;
    if (parsed === status.port) return;
    void setBind(status.address, parsed);
  };

  return (
    <>
      <div className="section-label">Remote</div>
      <div className="card" style={{ padding: "var(--sp-2)" }}>
        {error && (
          <div className="error-banner" style={{ borderRadius: 8, margin: "var(--sp-1)" }}>
            {error}
          </div>
        )}

        <div className="row">
          <div className="ricon">
            <Ms name="cast" />
          </div>
          <div className="rmain">
            <div className="rtitle" id="remote-toggle-label">
              Inari Remote
            </div>
            <div className="rsub">
              Serves this interface on your network. While it is on, another
              device that has the pairing link can change your volumes, mic, EQ,
              headset and profiles.
            </div>
          </div>
          {/* Toggle renders a bare `<button>` and takes no label prop, so the
              switch is placed in a named group - a screen reader announces
              "Inari Remote, group" on entry instead of a nameless button. */}
          <div role="group" aria-labelledby="remote-toggle-label">
            <Toggle on={enabled} onClick={() => void setEnabled(!enabled)} />
          </div>
        </div>

        <div className="row row-sub">
          <div className="ricon">
            <Ms name="lan" />
          </div>
          <div className="rmain">
            <div className="rtitle">Listen on</div>
            <div className="rsub">{bindSub(current)}</div>
          </div>
          <div style={{ position: "relative" }}>
            <button
              type="button"
              className="select"
              disabled={busy || interfaces.length === 0}
              aria-label="Address the remote listens on"
              onClick={() => setBindOpen((o) => !o)}
            >
              <span>{current?.label ?? status?.address ?? "…"}</span>
              <Ms name="expand_more" />
            </button>
            <Popover open={bindOpen} onClose={() => setBindOpen(false)} side="bottom" align="end">
              {interfaces.map((i) => (
                <MenuItem
                  key={i.address}
                  selected={i.address === status?.address}
                  showCheck
                  onClick={() => {
                    setBindOpen(false);
                    if (status && i.address !== status.address) {
                      void setBind(i.address, status.port);
                    }
                  }}
                >
                  {i.label}
                </MenuItem>
              ))}
            </Popover>
          </div>
        </div>

        <div className="row row-sub">
          <div className="ricon">
            <Ms name="tag" />
          </div>
          <div className="rmain">
            <div className="rtitle">Port</div>
            <div className="rsub">{PORT_MIN}-{PORT_MAX}; the tablet opens this port</div>
          </div>
          <label className="eqm-field">
            <input
              type="number"
              min={PORT_MIN}
              max={PORT_MAX}
              step={1}
              value={portValue}
              disabled={busy || !status}
              aria-label="Port the remote listens on"
              onChange={(e) => setPortDraft(e.target.value)}
              onBlur={commitPort}
              onKeyDown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
              }}
            />
          </label>
        </div>

        {enabled && (
          <>
            <div className="row">
              <div className="ricon">
                <Ms name="devices" />
              </div>
              <div className="rmain">
                <div className="rtitle">Connected devices</div>
                <div className="rsub" role="status">
                  {clientsSub(status?.clients ?? 0)}
                </div>
              </div>
              <span className={"tag" + (status?.clients ? " live" : " tag-off")} aria-hidden="true">
                {status?.clients ?? 0}
              </span>
            </div>

            {pairing && (
              <div className="row remote-pair">
                <QrCode svg={pairing.qr_svg} />
                <div className="rmain">
                  <div className="rtitle">Pair a device</div>
                  <div className="rsub">
                    Scan the code, or open the address below and enter the token.
                    The code carries the token, so treat a photo of it the same
                    way you would the token itself.
                  </div>
                  <div className="remote-url">{status?.url}</div>
                  <TokenReveal key={pairing.token} token={pairing.token} />
                </div>
              </div>
            )}

            <div className="row">
              <div className="ricon">
                <Ms name="key" />
              </div>
              <div className="rmain">
                <div className="rtitle">Regenerate token</div>
                <div className="rsub">
                  Every device paired so far stops working until it scans the new
                  code
                </div>
              </div>
              <button
                type="button"
                className="select"
                disabled={busy}
                onClick={() => setConfirmingRegen(true)}
              >
                <span>Regenerate…</span>
              </button>
            </div>
          </>
        )}

        <div className="row">
          <div className="ricon">
            <Ms name="shield" />
          </div>
          <div className="rmain">
            <div className="rtitle">What a remote device can't do</div>
            <div className="rsub">
              The backend answers a fixed list of commands and rejects everything
              else, so a connected tablet cannot add, rename or delete channels,
              mixes or profiles, install updates, reset Inari, or touch files on
              this machine.
            </div>
          </div>
        </div>
      </div>

      <ConfirmModal
        open={confirmingRegen}
        onClose={() => setConfirmingRegen(false)}
        title="Regenerate the pairing token?"
        confirmLabel="Regenerate"
        onConfirm={() => void regenerateToken()}
      >
        The current token stops working immediately. Every device you have
        paired is disconnected and has to scan the new code before it can
        control Inari again.
      </ConfirmModal>
    </>
  );
}

/**
 * The section exists only inside the Tauri window. A paired tablet is talking
 * *through* this server, so switching it off or re-keying it from there is
 * only ever a way to lock itself out - and the decision to expose a machine to
 * the network belongs at that machine. `isTauri` is the same signal main.tsx
 * picks the transport with; the URL says nothing, since both sides can be on
 * localhost.
 *
 * The caller in Settings already leaves the section out over the remote. This
 * guard stays because the answer must not depend on who renders it - and it is
 * convenience either way, not the defence: what a remote client may invoke is
 * decided by the allowlist in Rust, which nothing here can widen.
 */
export function RemoteSection() {
  if (!isTauri) return null;
  return <RemoteControls />;
}

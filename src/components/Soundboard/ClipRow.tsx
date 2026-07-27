import { useState } from "react";
import { HSlider } from "../AppList/HSlider";
import { ConfirmModal } from "../ConfirmModal";
import { Ms } from "../Icons";
import { MAX_CLIP_VOLUME, useSoundboard } from "../../store/soundboard";
import type { ClipInfo } from "../../store/soundboard";

/**
 * One clip in the desktop-only library list: its name, its level and the two
 * destructive-ish actions.
 *
 * It is a list under the board rather than controls on the pads themselves -
 * a pad is pressed in the middle of a game, and every extra target on it is
 * something to hit by accident. The whole section is absent over the remote,
 * where these commands are denied (src-tauri/src/remote/allowlist.rs).
 */
export function ClipRow({ clip }: Readonly<{ clip: ClipInfo }>) {
  const renameClip = useSoundboard((s) => s.renameClip);
  const removeClip = useSoundboard((s) => s.removeClip);
  const setClipVolume = useSoundboard((s) => s.setClipVolume);

  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [confirming, setConfirming] = useState(false);

  const commit = () => {
    setEditing(false);
    const name = draft.trim();
    if (name && name !== clip.name) void renameClip(clip.id, name);
  };

  return (
    <div className="sb-lib-row">
      {editing ? (
        <input
          className="menu-input sb-lib-input"
          value={draft}
          autoFocus
          maxLength={32}
          // Not "Rename …": that is the button's name, and two controls
          // answering to it makes the row ambiguous to a screen reader.
          aria-label={`New name for ${clip.name}`}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
            if (e.key === "Escape") setEditing(false);
          }}
        />
      ) : (
        <div className="sb-lib-name" title={clip.name}>
          {clip.name}
          {clip.missing && (
            <span className="tag sb-lib-warn">
              <Ms name="error" style={{ fontSize: 12 }} /> File missing
            </span>
          )}
        </div>
      )}

      <HSlider
        value={clip.volume_percent}
        max={MAX_CLIP_VOLUME}
        label={`Volume for ${clip.name}`}
        onChange={(v) => setClipVolume(clip.id, v)}
      />

      <button
        type="button"
        className="sb-lib-btn"
        aria-label={`Rename ${clip.name}`}
        title="Rename"
        onClick={() => {
          setDraft(clip.name);
          setEditing(true);
        }}
      >
        <Ms name="edit" style={{ fontSize: 16 }} />
      </button>
      <button
        type="button"
        className="sb-lib-btn danger"
        aria-label={`Remove ${clip.name}`}
        title="Remove from the board"
        onClick={() => setConfirming(true)}
      >
        <Ms name="delete" style={{ fontSize: 16 }} />
      </button>

      <ConfirmModal
        open={confirming}
        onClose={() => setConfirming(false)}
        title="Remove clip"
        confirmLabel="Remove"
        onConfirm={() => void removeClip(clip.id)}
      >
        “{clip.name}” leaves the board. The file itself stays where it is on
        disk.
      </ConfirmModal>
    </div>
  );
}

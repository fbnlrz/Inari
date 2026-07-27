import { useEffect, useState } from "react";
import { assetUrl } from "../../lib/ipc";
import { Ms } from "../Icons";

interface AppIconProps {
  /** Resolved absolute icon path from the backend's desktop-entry
   * resolver; null when nothing matched. */
  iconPath: string | null;
}

/**
 * App icon through the transport's asset URL, generic glyph fallback.
 *
 * The path goes through `assetUrl` rather than Tauri's `convertFileSrc`
 * because the latter yields an `asset.localhost` URL that only the desktop
 * webview can resolve — on the tablet every icon came out blank.
 */
export function AppIcon({ iconPath }: Readonly<AppIconProps>) {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [iconPath]);

  if (iconPath && !failed) {
    return <img src={assetUrl(iconPath)} alt="" onError={() => setFailed(true)} />;
  }
  return <Ms name="graphic_eq" />;
}

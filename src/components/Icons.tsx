import type { CSSProperties } from "react";

/** Material Symbol glyph (self-hosted via the material-symbols package). */
export function Ms({
  name,
  className,
  style,
}: Readonly<{
  name: string;
  className?: string;
  style?: CSSProperties;
}>) {
  return (
    <span
      className={"ms material-symbols-outlined" + (className ? " " + className : "")}
      style={style}
      aria-hidden="true"
    >
      {name}
    </span>
  );
}

/** App logo mark - a torii gate (Inari). */
export function InariMark() {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor">
      <path d="M2 6 Q12 3 22 6 L22 8.6 Q12 11.6 2 8.6 Z" />
      <path d="M5 12 L19 12 L19 14 L5 14 Z" />
      <rect x="11.1" y="8.6" width="1.8" height="3.6" />
      <path d="M7 9 L9.5 9 L10.2 20.5 L6.2 20.5 Z" />
      <path d="M17 9 L14.5 9 L13.8 20.5 L17.8 20.5 Z" />
    </svg>
  );
}

/** Legacy fallback icons for channels created before icons existed. */
export const CHANNEL_ICONS: Record<string, string> = {
  sink_game: "sports_esports",
  sink_chat: "forum",
  sink_music: "music_note",
  sink_system: "desktop_windows",
};

export function channelIcon(channel: { name: string; icon?: string | null }): string {
  return channel.icon ?? CHANNEL_ICONS[channel.name] ?? "graphic_eq";
}

/** Curated icon choices for the channel icon picker. */
export const ICON_CHOICES: string[] = [
  "sports_esports",
  "forum",
  "music_note",
  "desktop_windows",
  "headphones",
  "mic",
  "movie",
  "tv",
  "videogame_asset",
  "campaign",
  "record_voice_over",
  "radio",
  "podcasts",
  "terminal",
  "public",
  "star",
];

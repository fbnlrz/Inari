/** `m:ss`, or `h:mm:ss` once a track is over an hour long. */
export function formatUs(us: number): string {
  const total = Math.max(0, Math.round(us / 1_000_000));
  const seconds = total % 60;
  const minutes = Math.floor(total / 60) % 60;
  const hours = Math.floor(total / 3600);
  const mm = hours > 0 ? String(minutes).padStart(2, "0") : String(minutes);
  return `${hours > 0 ? `${hours}:` : ""}${mm}:${String(seconds).padStart(2, "0")}`;
}

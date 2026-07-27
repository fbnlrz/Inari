/**
 * Which shell the UI is running in.
 *
 * The same bundle serves the desktop window and the tablet remote. Commands
 * and events survive that because they go through `ipc.ts`, but the *other*
 * Tauri APIs — window controls, the app version, native file dialogs — have no
 * browser equivalent and throw on `window.__TAURI_INTERNALS__` being absent.
 * A render-time call to one of them takes the whole app down before React
 * mounts, which is exactly how this was found: a blank page on the tablet.
 *
 * So anything reaching for `@tauri-apps/*` outside `ipc.ts` has to ask here
 * first, and offer something sensible when the answer is no.
 */
export const isTauri: boolean =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

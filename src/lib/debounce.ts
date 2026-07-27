/** How a rejected debounced call is reported; `key` identifies the target. */
export type DebounceErrorHandler = (key: string, error: unknown) => void;

export interface DebounceOptions {
  /** Override the debouncer's default delay for this one call. */
  ms?: number;
  /** Override the debouncer's default error handling for this one call. */
  onError?: (error: unknown) => void;
}

/**
 * Trailing-edge debounce for backend/device writes: a fader drag or a colour
 * pick must not fire one IPC call per pointer move. Calls are keyed so
 * independent targets never cancel each other, and the last call of a burst
 * wins. Shared by every store so all of them behave identically - the delay
 * and the error strategy are the only things that differ.
 */
export function createDebouncer(delayMs: number, onError: DebounceErrorHandler) {
  const pending = new Map<string, ReturnType<typeof setTimeout>>();
  return (key: string, fn: () => Promise<unknown>, opts?: DebounceOptions): void => {
    const existing = pending.get(key);
    if (existing !== undefined) clearTimeout(existing);
    pending.set(
      key,
      setTimeout(() => {
        pending.delete(key);
        void fn().catch((e: unknown) =>
          opts?.onError ? opts.onError(e) : onError(key, e),
        );
      }, opts?.ms ?? delayMs),
    );
  };
}

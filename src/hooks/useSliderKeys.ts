import { useCallback } from "react";
import type { KeyboardEvent } from "react";

interface SliderKeysOptions {
  value: number;
  min: number;
  max: number;
  /** Arrow-key increment; the grid the value is expected to sit on. */
  step: number;
  /** PageUp/PageDown increment. Defaults to ten arrow steps. */
  pageStep?: number;
  /** Same setter the pointer path uses - stores keep their own debounce. */
  onChange: (value: number) => void;
}

/**
 * Keyboard behaviour for the hand-built (non-<input>) sliders. Fader,
 * HSlider, DspSlider and BalanceBar are pointer-driven divs, so without
 * this they cannot be operated at all without a mouse; sharing one hook
 * keeps all four answering the same keys as the native ranges elsewhere.
 * Up/Right raise, Down/Left lower, Page* jumps, Home/End pin the ends.
 */
export function useSliderKeys({ value, min, max, step, pageStep, onChange }: SliderKeysOptions) {
  return useCallback(
    (e: KeyboardEvent) => {
      const page = pageStep ?? step * 10;
      let next: number;
      switch (e.key) {
        case "ArrowUp":
        case "ArrowRight":
          next = value + step;
          break;
        case "ArrowDown":
        case "ArrowLeft":
          next = value - step;
          break;
        case "PageUp":
          next = value + page;
          break;
        case "PageDown":
          next = value - page;
          break;
        case "Home":
          next = min;
          break;
        case "End":
          next = max;
          break;
        default:
          return;
      }
      // Swallow the key so arrows/Page*/Home/End don't also scroll the panel.
      e.preventDefault();
      // toFixed keeps fractional steps (0.5 dB) off binary-float drift.
      const clamped = Math.max(min, Math.min(max, Number(next.toFixed(4))));
      if (clamped !== value) onChange(clamped);
    },
    [value, min, max, step, pageStep, onChange],
  );
}

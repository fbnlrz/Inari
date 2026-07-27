import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    // Build output, the Rust tree and the docs site (which carries its own
    // package.json and toolchain) are not ours to lint.
    ignores: ["dist/", "target/", "src-tauri/", "website/", "node_modules/"],
  },
  js.configs.recommended,
  // Syntactic rules only. Type-aware linting needs a parser project, and
  // tsconfig.json only includes src/ - the root config files (vite, tailwind,
  // this one) would each need an escape hatch, for checks `tsc --noEmit`
  // already largely covers.
  tseslint.configs.recommended,
  reactHooks.configs.flat["recommended-latest"],
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      globals: { ...globals.browser },
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    rules: {
      // react-hooks 7 ships the React Compiler rule set. Three of its rules
      // fire on patterns this app uses deliberately; they are relaxed here
      // rather than per-site so the reasoning lives in one place.

      // The "latest ref" pattern (`ref.current = handler` during render) is
      // used by every component that attaches a window-level pointer or key
      // listener once and must not re-register it on each parent re-render
      // (faders re-render on every volume tick mid-drag). Each site says so;
      // the rule cannot tell it apart from a genuine render-time ref read.
      "react-hooks/refs": "off",

      // Kept visible instead of gating: a few effects reset local state when a
      // prop flips (AppIcon, OnboardingModal, Popover), and one render-time
      // loop accumulates into an outer `let` (EqCurve). Pre-existing and
      // benign, but worth a nudge - as warnings the count can only go down.
      "react-hooks/set-state-in-effect": "warn",
      "react-hooks/immutability": "warn",
    },
  },
);

import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";

// Vitest globals are off, so RTL's own auto-cleanup hook never registers -
// unmount here or every rendered tree leaks into the next test's document.
afterEach(() => {
  cleanup();
});

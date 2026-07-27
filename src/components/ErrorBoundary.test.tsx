import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { ErrorBoundary } from "./ErrorBoundary";

/** Throws on render, the way a bad backend payload would. */
function Boom({ error }: Readonly<{ error: unknown }>): never {
  throw error;
}

let consoleError: ReturnType<typeof vi.spyOn>;

// React's dev build re-throws a caught render error through a synthetic
// event; jsdom then reports it as uncaught. Cancel it - the throw is the
// point of these tests, not a failure.
const swallow = (e: ErrorEvent) => e.preventDefault();

beforeEach(() => {
  // React re-logs every caught error, and componentDidCatch logs its own;
  // keep the run readable.
  consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
  window.addEventListener("error", swallow);
});

afterEach(() => {
  window.removeEventListener("error", swallow);
  consoleError.mockRestore();
});

describe("ErrorBoundary", () => {
  it("renders children while nothing throws", () => {
    render(
      <ErrorBoundary>
        <p>mixer</p>
      </ErrorBoundary>,
    );

    expect(screen.getByText("mixer")).toBeInTheDocument();
  });

  it("shows the crash panel instead of a blank window", () => {
    render(
      <ErrorBoundary>
        <Boom error={new Error("levels payload was null")} />
      </ErrorBoundary>,
    );

    // A Tauri window has no address bar, so the boundary must both announce
    // the crash and offer the reload.
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("This screen crashed.")).toBeInTheDocument();
    expect(screen.getByText("levels payload was null")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reload" })).toBeInTheDocument();
  });

  it("falls back to stringifying throws that aren't Errors", () => {
    render(
      <ErrorBoundary>
        <Boom error={new Error("")} />
      </ErrorBoundary>,
    );

    expect(screen.getByText("Error")).toBeInTheDocument();
  });

  it("logs the failure for the developer console", () => {
    render(
      <ErrorBoundary>
        <Boom error={new Error("boom")} />
      </ErrorBoundary>,
    );

    expect(consoleError).toHaveBeenCalledWith(
      "render error:",
      expect.any(Error),
      expect.anything(),
    );
  });
});

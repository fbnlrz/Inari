import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";

// The store talks to the Rust backend through src/lib/ipc; mock that boundary.
const call = vi.fn();
vi.mock("../../lib/ipc", () => ({
  call: (...args: unknown[]) => call(...args),
  subscribe: () => Promise.resolve(() => {}),
}));

import { RemoteSection } from "./RemoteSection";
import { useRemote, type RemotePairing, type RemoteStatus } from "../../store/remote";

const TOKEN = "s3cret-pairing-token";
const QR_SVG = '<svg viewBox="0 0 21 21"><rect width="21" height="21" /></svg>';

const status = (over: Partial<RemoteStatus> = {}): RemoteStatus => ({
  enabled: false,
  address: "127.0.0.1",
  port: 7433,
  url: "http://127.0.0.1:7433",
  clients: 0,
  interfaces: [
    {
      address: "127.0.0.1",
      label: "This machine only",
      loopback: true,
      wildcard: false,
    },
    {
      address: "192.168.1.42",
      label: "Wi-Fi (wlan0)",
      loopback: false,
      wildcard: false,
    },
  ],
  ...over,
});

const pairing = (token = TOKEN): RemotePairing => ({
  url: `http://192.168.1.42:7433/#${token}`,
  qr_svg: QR_SVG,
  token,
});

const initialState = useRemote.getState();

/**
 * Answers each remote command with the given status/pairing. Commands are
 * matched by name so a drift on the Rust side shows up as an unanswered call
 * rather than a passing test.
 */
const backend = (s: RemoteStatus, p: RemotePairing = pairing()) => {
  call.mockImplementation((cmd: string) => {
    if (cmd === "get_remote_status") return Promise.resolve(s);
    if (cmd === "set_remote_enabled" || cmd === "set_remote_bind") return Promise.resolve(s);
    if (cmd === "get_remote_pairing" || cmd === "regenerate_remote_token")
      return Promise.resolve(p);
    return Promise.reject(new Error(`unexpected command ${cmd}`));
  });
};

/** Renders and lets the mount fetch settle, so assertions see real state. */
const mount = async () => {
  const view = render(<RemoteSection />);
  await act(async () => {});
  return view;
};

/**
 * The exposure switch. `Toggle` renders a bare button, so it is reached
 * through the named group around it - which is also the assertion that the
 * switch is not sitting on the page nameless.
 */
const exposureSwitch = () =>
  within(screen.getByRole("group", { name: "Inari Remote" })).getByRole("button");

/** The section renders only inside the Tauri window - fake the global it keys off. */
const asDesktop = () => {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
};

beforeEach(() => {
  call.mockReset();
  useRemote.setState(initialState, true);
  asDesktop();
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("exposure", () => {
  it("is off before the backend has answered - the switch never starts on", async () => {
    call.mockImplementation(() => new Promise(() => {}));
    await mount();

    expect(exposureSwitch()).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByText("Pair a device")).not.toBeInTheDocument();
  });

  it("is absent on a remote client, which must not re-key its own connection", async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    backend(status({ enabled: true }));
    await mount();

    expect(screen.queryByText("Inari Remote")).not.toBeInTheDocument();
    expect(call).not.toHaveBeenCalled();
  });

  it("says what turning it on lets other devices do", async () => {
    backend(status());
    await mount();

    expect(
      screen.getByText(/another device that has the pairing link can change your volumes/i),
    ).toBeInTheDocument();
  });

  it("enabling asks the backend to start the listener", async () => {
    backend(status());
    await mount();

    fireEvent.click(exposureSwitch());
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith("set_remote_enabled", {
        enabled: true,
      }),
    );
  });

  it("stays off when the backend refuses to bind", async () => {
    // The backend answers with what it actually did, not with what was asked.
    backend(status({ enabled: false }));
    await mount();

    fireEvent.click(exposureSwitch());
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith("set_remote_enabled", {
        enabled: true,
      }),
    );

    expect(exposureSwitch()).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByText("Pair a device")).not.toBeInTheDocument();
  });
});

describe("bind target", () => {
  it("names the address it listens on and what that reaches", async () => {
    backend(status());
    await mount();

    expect(screen.getByRole("button", { name: "Address the remote listens on" })).toHaveTextContent(
      "This machine only",
    );
    expect(screen.getByText(/nothing on the network can reach it/i)).toBeInTheDocument();
  });

  it("rebinds to the address picked from the menu, keeping the port", async () => {
    backend(status());
    await mount();

    fireEvent.click(screen.getByRole("button", { name: "Address the remote listens on" }));
    fireEvent.click(screen.getByText("Wi-Fi (wlan0)"));

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith("set_remote_bind", {
        address: "192.168.1.42",
        port: 7433,
      }),
    );
  });

  it("ignores a port outside the unprivileged range instead of sending it", async () => {
    backend(status());
    await mount();

    const port = screen.getByRole("spinbutton", {
      name: "Port the remote listens on",
    });
    fireEvent.change(port, { target: { value: "80" } });
    fireEvent.blur(port);
    await act(async () => {});

    expect(call).not.toHaveBeenCalledWith("set_remote_bind", expect.anything());
  });

  it("commits a valid port on blur", async () => {
    backend(status());
    await mount();

    const port = screen.getByRole("spinbutton", {
      name: "Port the remote listens on",
    });
    fireEvent.change(port, { target: { value: "8443" } });
    fireEvent.blur(port);

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith("set_remote_bind", {
        address: "127.0.0.1",
        port: 8443,
      }),
    );
  });
});

describe("pairing", () => {
  it("renders the SVG the backend returned rather than drawing its own", async () => {
    backend(status({ enabled: true }));
    await mount();

    const qr = await screen.findByRole("img", { name: /pairing qr code/i });
    expect(qr.querySelector("svg")).toBeInTheDocument();
  });

  it("draws nothing when the payload is not an SVG document", async () => {
    backend(status({ enabled: true }), {
      ...pairing(),
      qr_svg: "<img src=x onerror=1>",
    });
    await mount();

    expect(screen.queryByRole("img", { name: /pairing qr code/i })).not.toBeInTheDocument();
  });

  it("keeps the token off the page until it is asked for", async () => {
    backend(status({ enabled: true }));
    await mount();

    expect(await screen.findByText("Pair a device")).toBeInTheDocument();
    expect(document.body.innerHTML).not.toContain(TOKEN);
  });

  it("reveals the token on request", async () => {
    backend(status({ enabled: true }));
    await mount();

    fireEvent.click(await screen.findByRole("button", { name: "Show token" }));

    expect(screen.getByText(TOKEN)).toBeInTheDocument();
  });

  it("shows the base URL, which carries no token", async () => {
    backend(
      status({
        enabled: true,
        address: "192.168.1.42",
        url: "http://192.168.1.42:7433",
      }),
    );
    await mount();

    expect(await screen.findByText("http://192.168.1.42:7433")).toBeInTheDocument();
  });
});

describe("connected clients", () => {
  it("reports the count in a live region", async () => {
    backend(status({ enabled: true, clients: 2 }));
    await mount();

    expect(await screen.findByRole("status")).toHaveTextContent("2 devices connected");
  });

  it("reads as empty rather than blank when nothing is connected", async () => {
    backend(status({ enabled: true, clients: 0 }));
    await mount();

    expect(await screen.findByRole("status")).toHaveTextContent("No devices connected");
  });
});

describe("regenerating the token", () => {
  it("confirms first - a mis-tap must not cut every paired device off", async () => {
    backend(status({ enabled: true }));
    await mount();

    fireEvent.click(await screen.findByRole("button", { name: "Regenerate…" }));

    expect(
      screen.getByRole("dialog", { name: /regenerate the pairing token/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/every device you have paired is disconnected/i)).toBeInTheDocument();
    expect(call).not.toHaveBeenCalledWith("regenerate_remote_token");
  });

  it("regenerates once confirmed", async () => {
    backend(status({ enabled: true }));
    await mount();

    fireEvent.click(await screen.findByRole("button", { name: "Regenerate…" }));
    fireEvent.click(screen.getByRole("button", { name: "Regenerate" }));

    await waitFor(() => expect(call).toHaveBeenCalledWith("regenerate_remote_token"));
  });

  it("conceals the new token even if the old one was revealed", async () => {
    backend(status({ enabled: true }));
    await mount();

    fireEvent.click(await screen.findByRole("button", { name: "Show token" }));
    expect(screen.getByText(TOKEN)).toBeInTheDocument();

    call.mockImplementation((cmd: string) => {
      if (cmd === "regenerate_remote_token") return Promise.resolve(pairing("brand-new-token"));
      return Promise.resolve(status({ enabled: true }));
    });
    fireEvent.click(screen.getByRole("button", { name: "Regenerate…" }));
    fireEvent.click(screen.getByRole("button", { name: "Regenerate" }));

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Show token" })).toBeInTheDocument(),
    );
    expect(document.body.innerHTML).not.toContain("brand-new-token");
  });
});

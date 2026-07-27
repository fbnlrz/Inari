import ReactDOM from "react-dom/client";
import App from "./App";
import { installRemoteTransport } from "./lib/wsTransport";
import { bootTheme } from "./store/theme";
import "material-symbols/outlined.css";
import "./styles/globals.css";

/**
 * Are we inside the Tauri webview?
 *
 * Decided by the global Tauri injects before any of our script runs — not by
 * the URL. The dev server and the remote server can both serve this bundle on
 * localhost, so host and port say nothing about which side we are on.
 */
const inTauri = "__TAURI_INTERNALS__" in window;

// Pick the transport before the first render: stores fire commands from their
// mount effects, and those must not race the decision.
if (!inTauri) installRemoteTransport();

// Apply the saved theme before first paint to avoid a flash of the default.
bootTheme();

const root = document.getElementById("root");
if (root) {
  ReactDOM.createRoot(root).render(<App />);
}

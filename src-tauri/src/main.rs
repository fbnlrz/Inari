// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `--help` and a mistyped subcommand are answered without booting
    // anything; everything else (including the control commands, which the
    // single-instance plugin forwards to the running app) goes on to run.
    if let Some(code) = inari_lib::cli_early_exit() {
        std::process::exit(code);
    }

    // WebKitGTK's DMABUF renderer crashes with a Wayland protocol error
    // (Gdk Error 71) on some GPU/driver combinations. Disable it unless the
    // user has set the variable themselves.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    inari_lib::run()
}

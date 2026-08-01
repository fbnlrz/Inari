//! The non-interactive command line.
//!
//! Inari is a single-instance app, so `inari mute chat` typed in a terminal is
//! not a second mixer: the single-instance plugin hands the new process's argv
//! to the one already running and exits. That is the only channel we use - no
//! socket, no second D-Bus service.
//!
//! The consequence, and it is a real one: `org.SingleInstance.DBus.ExecuteCallback`
//! is one-way. The calling process is gone by the time the running instance
//! has an answer, so results are printed on the *running* instance's stdout
//! and mirrored into the log file, not into the caller's terminal. Commands
//! that act (mute/volume/profile) are therefore the useful ones; `status` is
//! for reading the log, or for a session where Inari was started from a shell.

use log::info;

use crate::mixer::state::MixerState;
use crate::state::AppState;

/// Printed by `--help`, and after a bad invocation.
pub const USAGE: &str = "\
Inari - Linux audio routing and mixing

Usage:
  inari [--minimized]                 start Inari (--minimized: stay in the tray)
  inari status                        print channels, mic and active profile
  inari mute <channel|mic> [on|off]   mute, unmute, or toggle (default)
  inari volume <channel> <0-150>      set a channel's volume in percent
  inari profile <name>                load a saved profile
  inari doctor                        report what Inari sees of your hardware
  inari --help                        this text

<channel> is a channel's label (\"Chat\") or its sink name (\"sink_chat\").
Everything but a plain start acts on the already running Inari; results are
printed by that process (see its log: Settings -> Logs).";

/// What an invocation of the binary is asking for.
#[derive(Debug, Clone, PartialEq)]
pub enum Cli {
    /// Start the app. `minimized` = boot straight to the tray.
    Launch { minimized: bool },
    /// Act on the already running instance.
    Control(Action),
    /// Print [`USAGE`] and exit 0.
    Help,
    /// Print the hardware report and exit 0. Unlike the control commands this
    /// runs in *this* process, so it works when no Inari is running — which is
    /// the situation it exists for.
    Doctor,
    /// Bad invocation: the message says what was wrong; exit 2.
    Usage(String),
}

/// Mute verbs. Bare `mute <x>` toggles, which is what a hotkey-ish CLI call
/// almost always wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuteMode {
    On,
    Off,
    Toggle,
}

/// One thing to do to the running instance.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Status,
    Mute { target: String, mode: MuteMode },
    Volume { target: String, percent: u8 },
    Profile { name: String },
}

/// What a `<channel|mic>` argument resolved to.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Mic,
    Channel(String),
}

/// Parse the arguments *after* argv[0].
///
/// Unknown `-`-prefixed arguments are ignored rather than rejected: desktop
/// files, session managers and WebKit itself all append flags we never asked
/// for, and refusing to launch over one would be a very bad trade. An unknown
/// *word*, on the other hand, is a typo'd subcommand and is worth saying so.
pub fn parse(args: &[String]) -> Cli {
    let mut minimized = false;
    let mut words = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--minimized" => minimized = true,
            "-h" | "--help" | "help" => return Cli::Help,
            _ if arg.starts_with('-') => {}
            _ => words.push(arg.as_str()),
        }
    }
    let Some((verb, rest)) = words.split_first() else {
        return Cli::Launch { minimized };
    };
    match *verb {
        "doctor" => arity(rest, 0, "doctor").unwrap_or(Cli::Doctor),
        "status" => arity(rest, 0, "status").unwrap_or(Cli::Control(Action::Status)),
        "mute" => parse_mute(rest),
        "volume" => parse_volume(rest),
        "profile" => match rest {
            [name] => Cli::Control(Action::Profile {
                name: (*name).to_string(),
            }),
            _ => Cli::Usage("profile takes exactly one profile name".into()),
        },
        other => Cli::Usage(format!("unknown command \"{other}\"")),
    }
}

/// `Some(Usage)` when `rest` carries arguments the verb has no use for.
fn arity(rest: &[&str], expected: usize, verb: &str) -> Option<Cli> {
    (rest.len() > expected).then(|| Cli::Usage(format!("{verb} takes no arguments")))
}

fn parse_mute(rest: &[&str]) -> Cli {
    let (target, mode) = match rest {
        [target] => (*target, MuteMode::Toggle),
        [target, "on"] => (*target, MuteMode::On),
        [target, "off"] => (*target, MuteMode::Off),
        [target, "toggle"] => (*target, MuteMode::Toggle),
        [_, other] => return Cli::Usage(format!("mute takes on/off/toggle, not \"{other}\"")),
        _ => return Cli::Usage("mute takes a channel and an optional on/off".into()),
    };
    Cli::Control(Action::Mute {
        target: target.to_string(),
        mode,
    })
}

fn parse_volume(rest: &[&str]) -> Cli {
    let [target, percent] = rest else {
        return Cli::Usage("volume takes a channel and a percentage".into());
    };
    match percent.parse::<u16>() {
        // The same ceiling the UI and the IPC command enforce.
        Ok(v) if v <= crate::commands::routing::MAX_VOLUME as u16 => Cli::Control(Action::Volume {
            target: (*target).to_string(),
            percent: v as u8,
        }),
        _ => Cli::Usage(format!("volume must be 0-150, not \"{percent}\"")),
    }
}

/// Resolve a user-typed target to something we own: `mic` (any case) is the
/// mic chain, otherwise a channel matched by sink name first and then by
/// label, both case-insensitively - nobody wants to type "sink_chat".
pub fn resolve_target(mixer: &MixerState, target: &str) -> Option<Target> {
    if target.eq_ignore_ascii_case("mic") || target.eq_ignore_ascii_case("microphone") {
        return Some(Target::Mic);
    }
    let by_name = mixer
        .channel_defs
        .channels
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(target))
        .or_else(|| {
            mixer
                .channel_defs
                .channels
                .iter()
                .find(|c| c.label.eq_ignore_ascii_case(target))
        })?;
    Some(Target::Channel(by_name.name.clone()))
}

/// Run an action against the live instance and return the line(s) to print.
pub fn execute(app: &tauri::AppHandle, action: &Action) -> Result<String, String> {
    use tauri::Manager;

    let state = app.state::<AppState>();
    match action {
        Action::Status => status(&state),
        Action::Mute { target, mode } => {
            let resolved = {
                let mixer = state.lock_mixer()?;
                resolve_target(&mixer, target).ok_or_else(|| format!("unknown channel: {target}"))?
            };
            let muted = match resolved {
                Target::Mic => {
                    let now = state.lock_mixer()?.mic.muted;
                    let muted = mode.applied_to(now);
                    super::mic::set_mic_muted(&state, muted)?;
                    muted
                }
                Target::Channel(ref sink) => {
                    let now = state
                        .lock_mixer()?
                        .channels
                        .iter()
                        .find(|c| &c.name == sink)
                        .is_some_and(|c| c.muted);
                    let muted = mode.applied_to(now);
                    super::routing::toggle_channel_mute(app.state(), sink.clone(), muted)?;
                    muted
                }
            };
            crate::after_external_change(app);
            Ok(format!(
                "{} {}",
                label_of(&state, &resolved),
                if muted { "muted" } else { "unmuted" }
            ))
        }
        Action::Volume { target, percent } => {
            let resolved = {
                let mixer = state.lock_mixer()?;
                resolve_target(&mixer, target).ok_or_else(|| format!("unknown channel: {target}"))?
            };
            let Target::Channel(sink) = resolved else {
                return Err("volume applies to channels; use the mic panel for mic gain".into());
            };
            super::routing::set_channel_volume(app.state(), sink.clone(), *percent)?;
            crate::after_external_change(app);
            Ok(format!(
                "{} at {percent}%",
                label_of(&state, &Target::Channel(sink))
            ))
        }
        Action::Profile { name } => {
            super::profiles::load_profile(app.clone(), app.state(), name.clone())?;
            // Same event the tray's profile switch emits, so an open window
            // re-reads everything the profile just replaced.
            let _ = tauri::Emitter::emit(app, "profile-changed", name);
            Ok(format!("profile {name} loaded"))
        }
    }
}

impl MuteMode {
    /// The mute state this verb produces, given the current one.
    fn applied_to(self, current: bool) -> bool {
        match self {
            MuteMode::On => true,
            MuteMode::Off => false,
            MuteMode::Toggle => !current,
        }
    }
}

/// Human-facing name of a resolved target, for the printed confirmation.
fn label_of(state: &AppState, target: &Target) -> String {
    match target {
        Target::Mic => "microphone".to_string(),
        Target::Channel(sink) => state
            .lock_mixer()
            .ok()
            .and_then(|m| {
                m.channel_defs
                    .channels
                    .iter()
                    .find(|c| &c.name == sink)
                    .map(|c| c.label.clone())
            })
            .unwrap_or_else(|| sink.clone()),
    }
}

fn status(state: &AppState) -> Result<String, String> {
    let mixer = state.lock_mixer()?;
    let engine = match (state.backend_native, state.backend.is_engine_alive()) {
        (_, false) => "stopped",
        (true, _) => "native PipeWire",
        (false, _) => "pactl fallback",
    };
    let mut out = format!(
        "Inari {} - engine: {engine}\nprofile: {}\n",
        env!("CARGO_PKG_VERSION"),
        mixer.active_profile.as_deref().unwrap_or("(none)")
    );
    if mixer.channels.is_empty() {
        out.push_str("channels: (not initialised yet)\n");
    }
    for channel in &mixer.channels {
        out.push_str(&format!(
            "  {:<12} {:<14} {:>3}%{}\n",
            channel.label,
            channel.name,
            channel.volume_percent,
            if channel.muted { "  muted" } else { "" }
        ));
    }
    out.push_str(&format!(
        "  {:<12} {:<14} {:>3}%{}",
        "Microphone",
        "sink_mic",
        mixer.mic.gain_percent,
        if mixer.mic.muted { "  muted" } else { "" }
    ));
    Ok(out)
}

/// Answer the invocations that need no running app at all. Returns the exit
/// code when the command line was fully handled here, `None` to go on and
/// start (or reach) the app.
pub fn early_exit(args: &[String]) -> Option<i32> {
    match parse(args) {
        Cli::Help => {
            println!("{USAGE}");
            Some(0)
        }
        Cli::Usage(msg) => {
            eprintln!("inari: {msg}\n\n{USAGE}");
            Some(2)
        }
        // Printed here, in the caller's terminal, rather than forwarded to a
        // running instance: the report is most useful precisely when nothing
        // is running properly.
        Cli::Doctor => {
            print!("{}", crate::device::doctor::report());
            Some(0)
        }
        _ => None,
    }
}

/// Handle a command line that reached the running instance (via the
/// single-instance plugin) or, when nothing was running, this process itself.
/// Returns true when the invocation was a control command, i.e. when the
/// window must stay exactly as it is.
pub fn handle_control(app: &tauri::AppHandle, args: &[String]) -> bool {
    let Cli::Control(action) = parse(args) else {
        return false;
    };
    let line = match execute(app, &action) {
        Ok(line) => line,
        Err(e) => format!("error: {e}"),
    };
    // This process's stdout, which is the caller's terminal only when Inari
    // itself was started from one; the log copy is what a tray-resident
    // instance leaves behind.
    println!("{line}");
    info!("cli: {line}");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Cli {
        super::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_bare_launch_is_still_a_launch() {
        assert_eq!(parse_args(&[]), Cli::Launch { minimized: false });
        assert_eq!(
            parse_args(&["--minimized"]),
            Cli::Launch { minimized: true }
        );
    }

    #[test]
    fn unknown_flags_never_block_the_launch() {
        // Desktop files, session restore and WebKit all add flags of their
        // own; refusing to start over one would be far worse than ignoring it.
        assert_eq!(
            parse_args(&["--enable-features=Foo", "--minimized"]),
            Cli::Launch { minimized: true }
        );
        // A typo'd subcommand is a different story - that one is worth saying.
        assert!(matches!(parse_args(&["muet", "chat"]), Cli::Usage(_)));
    }

    #[test]
    fn help_wins_over_everything() {
        assert_eq!(parse_args(&["--help"]), Cli::Help);
        assert_eq!(parse_args(&["status", "--help"]), Cli::Help);
    }

    #[test]
    fn mute_defaults_to_toggle() {
        assert_eq!(
            parse_args(&["mute", "chat"]),
            Cli::Control(Action::Mute {
                target: "chat".into(),
                mode: MuteMode::Toggle,
            })
        );
        assert_eq!(
            parse_args(&["mute", "mic", "on"]),
            Cli::Control(Action::Mute {
                target: "mic".into(),
                mode: MuteMode::On,
            })
        );
        assert!(matches!(parse_args(&["mute", "chat", "yes"]), Cli::Usage(_)));
        assert!(matches!(parse_args(&["mute"]), Cli::Usage(_)));
    }

    #[test]
    fn volume_is_bounded_by_the_same_ceiling_as_the_ui() {
        assert_eq!(
            parse_args(&["volume", "sink_game", "150"]),
            Cli::Control(Action::Volume {
                target: "sink_game".into(),
                percent: 150,
            })
        );
        assert!(matches!(parse_args(&["volume", "game", "151"]), Cli::Usage(_)));
        assert!(matches!(parse_args(&["volume", "game", "-5"]), Cli::Usage(_)));
        assert!(matches!(parse_args(&["volume", "game", "loud"]), Cli::Usage(_)));
        assert!(matches!(parse_args(&["volume", "game"]), Cli::Usage(_)));
    }

    #[test]
    fn status_and_profile_arity() {
        assert_eq!(parse_args(&["status"]), Cli::Control(Action::Status));
        assert!(matches!(parse_args(&["status", "now"]), Cli::Usage(_)));
        assert_eq!(
            parse_args(&["profile", "Gaming"]),
            Cli::Control(Action::Profile {
                name: "Gaming".into()
            })
        );
        assert!(matches!(parse_args(&["profile"]), Cli::Usage(_)));
        assert!(matches!(parse_args(&["profile", "a", "b"]), Cli::Usage(_)));
    }

    #[test]
    fn targets_resolve_by_label_or_sink_name() {
        let mixer = MixerState::default();
        assert_eq!(
            resolve_target(&mixer, "Chat"),
            Some(Target::Channel("sink_chat".into()))
        );
        assert_eq!(
            resolve_target(&mixer, "sink_chat"),
            Some(Target::Channel("sink_chat".into()))
        );
        assert_eq!(
            resolve_target(&mixer, "GAME"),
            Some(Target::Channel("sink_game".into()))
        );
        assert_eq!(resolve_target(&mixer, "mic"), Some(Target::Mic));
        assert_eq!(resolve_target(&mixer, "nope"), None);
    }

    #[test]
    fn mute_modes_are_absolute_except_toggle() {
        assert!(MuteMode::On.applied_to(false));
        assert!(MuteMode::On.applied_to(true));
        assert!(!MuteMode::Off.applied_to(true));
        assert!(MuteMode::Toggle.applied_to(false));
        assert!(!MuteMode::Toggle.applied_to(true));
    }

    #[test]
    fn early_exit_only_claims_the_invocations_it_answers() {
        assert_eq!(early_exit(&["--help".to_string()]), Some(0));
        assert_eq!(early_exit(&["nonsense".to_string()]), Some(2));
        // doctor answers in this process; a control verb does not.
        assert_eq!(parse(&["doctor".to_string()]), Cli::Doctor);
        assert_eq!(
            parse(&["doctor".to_string(), "x".to_string()]),
            Cli::Usage("doctor takes no arguments".into())
        );
        assert_eq!(early_exit(&[]), None);
        assert_eq!(early_exit(&["status".to_string()]), None);
    }
}

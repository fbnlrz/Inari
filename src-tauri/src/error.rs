use thiserror::Error;

/// All backend errors. Tauri commands convert these to `String` for
/// serialisation across IPC (`Result<T, String>`).
#[derive(Debug, Error)]
pub enum SinkError {
    #[error("pactl was not found on this system. Install pulseaudio-utils (the pactl client works against PipeWire via pipewire-pulse).")]
    PactlNotFound,

    #[error("the PulseAudio/PipeWire server is not reachable. Is PipeWire running? Try `systemctl --user status pipewire pipewire-pulse`.")]
    ServerUnreachable,

    #[error("pactl command failed: {0}")]
    CommandFailed(String),

    #[error("failed to parse pactl output: {0}")]
    Parse(String),

    #[error("unknown virtual sink: {0}")]
    UnknownSink(String),

    /// The native PipeWire loop thread left (server restart, fatal error).
    /// Everything it owned - sinks, links, EQ chains - went with it, so the
    /// only honest answer to any further request is "restart".
    #[error("the audio engine stopped. Restart Inari to restore audio control.")]
    EngineStopped,

    #[error("config error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

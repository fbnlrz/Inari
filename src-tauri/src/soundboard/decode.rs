//! Turning a clip file into the interleaved f32 the playback path wants.
//!
//! Two decode routes, and which one a file takes is a property of the format,
//! not a preference:
//!
//! - **WAV and FLAC are decoded in-process** (`hound`, `claxon`). These are the
//!   formats a user's own recordings and exports arrive in, and they have to
//!   play on a machine that has no ffmpeg installed - so they must not depend
//!   on one.
//! - **Everything compressed** (mp3, ogg, opus, m4a, …) goes through the system
//!   `ffmpeg`, the same optional dependency the OLED video path already uses
//!   (`headset/media.rs`), invoked the same guarded way: canonicalised path, a
//!   protocol whitelist, and no shell.
//!
//! When ffmpeg is missing, the native half keeps working and the status command
//! says so - a soundboard that silently does nothing on half its buttons is
//! worse than one that names the missing package.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// How a file gets decoded - and, for the UI, whether it can be played at all
/// on this machine right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipFormat {
    /// Decoded in-process; works with no system dependency whatsoever.
    Native,
    /// Needs the system ffmpeg.
    Ffmpeg,
}

/// Formats we decode ourselves.
const NATIVE_EXTS: &[&str] = &["wav", "wave", "flac"];
/// Compressed formats worth offering, all of them via ffmpeg. Deliberately a
/// list rather than "anything with an extension": a clip is picked in a file
/// dialog, and handing ffmpeg whatever came back is how a `.mkv` full of video
/// becomes a 400 MB decode.
const FFMPEG_EXTS: &[&str] = &[
    "mp3", "ogg", "oga", "opus", "m4a", "aac", "wma", "aiff", "aif", "aifc", "caf", "mka",
];

/// Longest clip we decode. A soundboard fires snippets; anything past this is a
/// backing track, and holding 48 kHz stereo f32 in memory for it is how a
/// mistyped file eats a gigabyte. Longer files play truncated rather than
/// failing - the first minute is what the user was after.
pub const MAX_SECONDS: usize = 60;
/// Rate and channel count everything ffmpeg touches is converted to. The
/// in-process decoders keep the file's own rate; PipeWire resamples either way.
const FFMPEG_RATE: u32 = 48_000;

/// Decoded audio, interleaved, ready to hand to the playback stream.
#[derive(Debug)]
pub struct Pcm {
    pub samples: Vec<f32>,
    pub rate: u32,
    /// 1 or 2 - more channels are folded down (see `fold_to_stereo`).
    pub channels: u16,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels).max(1)
    }

    /// How long the clip plays, used to schedule its own cleanup.
    pub fn duration(&self) -> std::time::Duration {
        if self.rate == 0 {
            return std::time::Duration::ZERO;
        }
        std::time::Duration::from_secs_f64(self.frames() as f64 / f64::from(self.rate))
    }
}

/// The lower-cased extension, which is all we classify by.
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Which route `path` would take, or `None` for a file we do not offer to
/// decode at all. Pure, so the rule is testable without touching a disk.
pub fn classify(path: &Path) -> Option<ClipFormat> {
    let ext = extension(path);
    if NATIVE_EXTS.contains(&ext.as_str()) {
        return Some(ClipFormat::Native);
    }
    if FFMPEG_EXTS.contains(&ext.as_str()) {
        return Some(ClipFormat::Ffmpeg);
    }
    None
}

/// Every extension the file dialog should offer, so the picker and the decoder
/// cannot disagree about what a clip is.
pub fn supported_extensions() -> Vec<&'static str> {
    NATIVE_EXTS.iter().chain(FFMPEG_EXTS.iter()).copied().collect()
}

/// Whether `ffmpeg` is on PATH. Mirrors `headset::media::ffmpeg_available`;
/// the soundboard asks on its own so the two features can be reported
/// independently.
pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Resolve a user-chosen path to something safe to decode: it must exist and
/// be a regular file, and the canonical form is what gets stored - so a clip
/// keeps pointing at the same file even if the user later renames a parent
/// directory's symlink.
pub fn resolve(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| format!("cannot open {path}: {e}"))?;
    if !path.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    Ok(path)
}

/// Decode a clip. The path comes from the library, never from a request.
pub fn decode(path: &Path) -> Result<Pcm, String> {
    // Re-checked at play time, not just at add time: a clip's file can be
    // deleted, unmounted or renamed long after it was added, and "nothing
    // happens when I press the button" must not be the way the user finds out.
    if !path.exists() {
        return Err(format!("the file is gone: {}", path.display()));
    }
    match classify(path) {
        Some(ClipFormat::Native) => {
            let native = if extension(path) == "flac" {
                decode_flac(path)
            } else {
                decode_wav(path)
            };
            match native {
                Ok(pcm) => Ok(pcm),
                // An exotic variant our parsers do not cover (ADPCM in a .wav,
                // say) is still ordinary audio to ffmpeg. Falling back keeps
                // the promise "wav and flac work" from turning into "wav and
                // flac work if they are the common flavour", without making
                // the common case depend on ffmpeg.
                Err(e) if ffmpeg_available() => decode_ffmpeg(path).map_err(|_| e),
                Err(e) => Err(e),
            }
        }
        Some(ClipFormat::Ffmpeg) => {
            if !ffmpeg_available() {
                return Err(format!(
                    "{} needs ffmpeg, which is not installed - WAV and FLAC clips still work",
                    extension(path)
                ));
            }
            decode_ffmpeg(path)
        }
        None => Err(format!(
            "unsupported clip format: {}",
            path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        )),
    }
}

/// Cap a decoded buffer at [`MAX_SECONDS`] (see there for why).
fn truncate(samples: &mut Vec<f32>, rate: u32, channels: u16) {
    let max = MAX_SECONDS * rate as usize * usize::from(channels).max(1);
    if samples.len() > max {
        log::warn!("clip is longer than {MAX_SECONDS}s - playing the first {MAX_SECONDS}s");
        samples.truncate(max);
    }
}

/// Fold anything above stereo down to stereo: odd channels left, even
/// channels right, averaged. Surround clips are rare and a proper downmix
/// matrix is not what a soundboard needs; what it does need is never to
/// publish a 6-channel stream into a mono virtual microphone.
fn fold_to_stereo(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 2 {
        return samples.to_vec();
    }
    let per_side = (channels as f32 / 2.0).ceil();
    samples
        .chunks(channels)
        .flat_map(|frame| {
            let mut left = 0.0;
            let mut right = 0.0;
            for (i, s) in frame.iter().enumerate() {
                if i % 2 == 0 {
                    left += *s;
                } else {
                    right += *s;
                }
            }
            [left / per_side, right / per_side]
        })
        .collect()
}

fn decode_wav(path: &Path) -> Result<Pcm, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| format!("read wav: {e}"))?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err("wav header declares no audio".to_string());
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| format!("read wav samples: {e}"))?,
        hound::SampleFormat::Int => {
            // One scale for every integer width: hound sign-extends 8/16/24-bit
            // samples into the i32 we ask for, so the only difference between
            // them is where full scale sits.
            let scale = 1.0 / (1i64 << (spec.bits_per_sample.min(32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(|e| format!("read wav samples: {e}"))?
        }
    };
    finish(samples, spec.sample_rate, spec.channels as usize)
}

fn decode_flac(path: &Path) -> Result<Pcm, String> {
    let mut reader = claxon::FlacReader::open(path).map_err(|e| format!("read flac: {e}"))?;
    let info = reader.streaminfo();
    if info.channels == 0 || info.sample_rate == 0 {
        return Err("flac header declares no audio".to_string());
    }
    let scale = 1.0 / (1i64 << (info.bits_per_sample.min(32) - 1)) as f32;
    let samples: Vec<f32> = reader
        .samples()
        .map(|s| s.map(|v| v as f32 * scale))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("read flac samples: {e}"))?;
    finish(samples, info.sample_rate, info.channels as usize)
}

/// Shared tail of the in-process decoders: fold, cap, and report.
fn finish(samples: Vec<f32>, rate: u32, channels: usize) -> Result<Pcm, String> {
    let mut samples = fold_to_stereo(&samples, channels);
    let channels = if channels > 2 { 2 } else { channels as u16 };
    truncate(&mut samples, rate, channels);
    if samples.is_empty() {
        return Err("the file contains no audio".to_string());
    }
    Ok(Pcm { samples, rate, channels })
}

/// Decode via the system ffmpeg, straight to interleaved f32 on stdout.
fn decode_ffmpeg(path: &Path) -> Result<Pcm, String> {
    // Same guards as the OLED video path: `ffmpeg -i` resolves its input as a
    // URL, so a canonical existing file plus a protocol whitelist is what keeps
    // "decode this clip" from becoming "fetch this URL" or "read whatever
    // `concat:` points at". The path never goes through a shell and is passed
    // as an OsStr, so a non-UTF-8 name cannot be mangled into another file.
    let path = path
        .canonicalize()
        .map_err(|e| format!("cannot open clip: {e}"))?;
    if !path.is_file() {
        return Err("not a regular file".to_string());
    }
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-protocol_whitelist",
            "file,crypto,data",
            "-i",
        ])
        .arg(&path)
        .args([
            // Album art in an mp3 is a video stream; without -vn ffmpeg
            // happily tries to put it in the output.
            "-vn",
            "-t",
            &MAX_SECONDS.to_string(),
            "-ac",
            "2",
            "-ar",
            &FFMPEG_RATE.to_string(),
            "-f",
            "f32le",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("ffmpeg not available: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffmpeg could not decode this clip: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let samples: Vec<f32> = output
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    if samples.is_empty() {
        return Err("ffmpeg produced no audio".to_string());
    }
    Ok(Pcm {
        samples,
        rate: FFMPEG_RATE,
        channels: 2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_and_flac_are_playable_without_ffmpeg() {
        // The user's explicit requirement: these two must never be gated on a
        // system package being present.
        for name in ["clip.wav", "CLIP.WAV", "voice.wave", "horn.flac"] {
            assert_eq!(
                classify(Path::new(name)),
                Some(ClipFormat::Native),
                "{name} must decode in-process"
            );
        }
    }

    #[test]
    fn compressed_formats_are_marked_as_needing_ffmpeg() {
        for name in ["clip.mp3", "clip.ogg", "clip.opus", "clip.m4a", "clip.aac"] {
            assert_eq!(classify(Path::new(name)), Some(ClipFormat::Ffmpeg), "{name}");
        }
    }

    #[test]
    fn anything_that_is_not_audio_is_refused_outright() {
        // Not "let ffmpeg have a go": a video container or a script would be
        // handed to a decoder that has no business seeing it.
        for name in ["clip.mkv", "clip.mp4", "notes.txt", "clip", "clip.sh"] {
            assert_eq!(classify(Path::new(name)), None, "{name}");
        }
    }

    #[test]
    fn the_picker_offers_exactly_what_the_decoder_accepts() {
        for ext in supported_extensions() {
            assert!(
                classify(Path::new(&format!("clip.{ext}"))).is_some(),
                "the dialog offers .{ext} but nothing decodes it"
            );
        }
    }

    #[test]
    fn a_missing_file_names_itself_instead_of_failing_silently() {
        let err = decode(Path::new("/nonexistent/clip.wav")).expect_err("must fail");
        assert!(err.contains("gone"), "{err}");
        assert!(err.contains("/nonexistent/clip.wav"), "{err}");
    }

    #[test]
    fn a_stereo_buffer_is_left_alone_and_surround_is_folded() {
        let stereo = [0.1, 0.2, 0.3, 0.4];
        assert_eq!(fold_to_stereo(&stereo, 2), stereo.to_vec());

        // One 4-channel frame: FL, FR, RL, RR.
        let quad = [1.0, 0.0, 1.0, 0.0];
        let folded = fold_to_stereo(&quad, 4);
        assert_eq!(folded.len(), 2, "four channels become two");
        assert!((folded[0] - 1.0).abs() < 1e-6, "left sums and stays in range");
        assert_eq!(folded[1], 0.0);
    }

    #[test]
    fn an_over_long_clip_is_capped_rather_than_swallowing_memory() {
        let rate = 48_000;
        let mut samples = vec![0.0f32; rate as usize * (MAX_SECONDS + 30)];
        truncate(&mut samples, rate, 1);
        assert_eq!(samples.len(), MAX_SECONDS * rate as usize);
        // A clip inside the cap is untouched.
        let mut short = vec![0.0f32; 4800];
        truncate(&mut short, rate, 1);
        assert_eq!(short.len(), 4800);
    }

    #[test]
    fn duration_follows_frames_and_rate_not_sample_count() {
        let pcm = Pcm {
            samples: vec![0.0; 96_000],
            rate: 48_000,
            channels: 2,
        };
        assert_eq!(pcm.frames(), 48_000);
        assert!((pcm.duration().as_secs_f64() - 1.0).abs() < 1e-9);
    }

    /// The in-process WAV path, exercised against a file we write here: it is
    /// the format the requirement is actually about, so "it decodes" should
    /// not be taken on faith.
    #[test]
    fn a_real_wav_file_decodes_without_any_system_tool() {
        let dir = std::env::temp_dir().join(format!(
            "sink-clip-decode-{}-{}",
            std::process::id(),
            crate::persistence::unix_now()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("tone.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).expect("writer");
        for i in 0..4410 {
            let v = (i as f32 * 0.05).sin();
            writer.write_sample((v * i16::MAX as f32) as i16).expect("sample");
        }
        writer.finalize().expect("finalize");

        let pcm = decode(&path).expect("decodes");
        assert_eq!(pcm.rate, 44_100, "the file's own rate is kept");
        assert_eq!(pcm.channels, 1);
        assert_eq!(pcm.frames(), 4410);
        let peak = pcm.samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(peak > 0.9 && peak <= 1.0, "samples land in [-1,1], peak {peak}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

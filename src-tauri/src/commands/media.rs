//! Media transport control over MPRIS, via the system `playerctl`.
//!
//! The OLED already reads now-playing data for its own screens; this is the
//! same source exposed as commands so the mixer — and the tablet remote — can
//! show what is playing and drive it.
//!
//! **Cover art does not travel as a path.** `mpris:artUrl` is an absolute path
//! chosen by whatever player is running, usually somewhere under a browser's
//! cache. Handing that path to a client and taking it back would be an
//! arbitrary file read wearing a cover-art costume, and no allowlist of
//! directories could be both safe and useful. Instead the backend resolves the
//! file itself and returns the *image*, downscaled and re-encoded, keyed by a
//! generation the client compares against. There is simply no parameter for a
//! path to travel through.

use std::process::Command;

use serde::Serialize;

/// One `playerctl metadata` call gets everything, so a status poll is one
/// fork+exec rather than six.
const FORMAT: &str =
    "{{title}}\t{{artist}}\t{{album}}\t{{status}}\t{{position}}\t{{mpris:length}}\t{{mpris:artUrl}}";

/// Longest edge of the cover we hand out. Album art arrives at anything from
/// 120 px to 3000 px; past this it is bytes on the wire nobody can see.
const ART_MAX_EDGE: u32 = 512;

/// A player as MPRIS names it, plus something readable for a picker.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MediaPlayer {
    /// The `playerctl -p` name, e.g. `chromium.instance1652439`.
    pub id: String,
    /// The part worth showing: `chromium`, `spotify`, `vlc`.
    pub label: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct MediaStatus {
    /// Which player this describes; `None` when nothing is running.
    pub player: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub position_us: u64,
    /// 0 when the player reports no length (live streams).
    pub length_us: u64,
    /// Changes when the cover changes. The client refetches the image only
    /// then, instead of dragging a few hundred kilobytes through every poll.
    pub art_generation: u64,
    /// False when `playerctl` is not installed, so the UI can say that rather
    /// than look broken.
    pub available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaArt {
    pub generation: u64,
    /// Always `image/jpeg`: everything is re-encoded on the way out.
    pub mime: String,
    pub base64: String,
}

/// Runs `playerctl`, optionally against one named player. `None` means
/// "whatever playerctl considers current".
fn playerctl(target: &Option<String>, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("playerctl");
    if let Some(p) = target {
        cmd.arg("-p").arg(p);
    }
    let out = cmd.args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A microsecond field from `playerctl metadata`. Some players pad or print a
/// decimal, so parse leniently rather than dropping the value to zero.
fn parse_us(raw: &str) -> u64 {
    let raw = raw.trim();
    if raw.is_empty() {
        return 0;
    }
    raw.parse::<u64>()
        .or_else(|_| raw.parse::<f64>().map(|v| v.max(0.0) as u64))
        .unwrap_or(0)
}

/// Stable-per-cover number. A hash of the URL is enough: players give a new
/// path (or a new query) whenever the artwork changes.
fn generation(url: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    if url.is_empty() {
        return 0;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    // Never 0: that is reserved for "no art", so a real cover cannot collide
    // with it and leave the client believing there is nothing to fetch.
    h.finish() | 1
}

/// Every MPRIS player currently on the bus.
#[tauri::command]
pub fn media_players() -> Vec<MediaPlayer> {
    playerctl(&None, &["-l"])
        .map(|out| {
            out.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(|id| MediaPlayer {
                    id: id.to_string(),
                    // `chromium.instance1652439` -> `chromium`
                    label: id.split('.').next().unwrap_or(id).to_string(),
                    })
                .collect()
        })
        .unwrap_or_default()
}

/// What the given player (or the current one) is doing right now.
#[tauri::command]
pub fn media_status(player: Option<String>) -> MediaStatus {
    let Some(raw) = playerctl(&player, &["metadata", "--format", FORMAT]) else {
        // Tell "no playerctl" apart from "playerctl, but nothing playing":
        // the first is a missing dependency the UI should name, the second is
        // an ordinary idle state.
        return MediaStatus {
            available: Command::new("playerctl").arg("--version").output().is_ok(),
            ..Default::default()
        };
    };
    let f: Vec<&str> = raw.split('\t').collect();
    let get = |i: usize| f.get(i).map(|s| s.trim()).unwrap_or_default();
    let art_url = get(6);
    MediaStatus {
        player: player.or_else(|| playerctl(&None, &["-l"])?.lines().next().map(str::to_string)),
        title: get(0).to_string(),
        artist: get(1).to_string(),
        album: get(2).to_string(),
        playing: get(3).eq_ignore_ascii_case("playing"),
        // Both are already microseconds. `playerctl position` — the separate
        // subcommand — prints *seconds*, so the two spellings of the same
        // value differ by a factor of a million; reading this one as seconds
        // put the playhead hours past the end of the track.
        position_us: parse_us(get(4)),
        length_us: parse_us(get(5)),
        art_generation: generation(art_url),
        available: true,
    }
}

/// The current cover, downscaled and re-encoded. `None` when there is no art
/// or it cannot be read — the client keeps whatever it had.
#[tauri::command]
pub fn media_art(player: Option<String>) -> Option<MediaArt> {
    let url = playerctl(&player, &["metadata", "--format", "{{mpris:artUrl}}"])?;
    let gen = generation(&url);
    if gen == 0 {
        return None;
    }
    let path = art_path(&url)?;
    // Sniff the content: browsers hand MPRIS an extensionless temp file, so
    // choosing a decoder by file name loses every browser-played track.
    let img = image::ImageReader::open(&path)
        .and_then(|r| r.with_guessed_format())
        .ok()?
        .decode()
        .ok()?;
    let img = if img.width().max(img.height()) > ART_MAX_EDGE {
        img.thumbnail(ART_MAX_EDGE, ART_MAX_EDGE)
    } else {
        img
    };
    let mut jpeg = Vec::new();
    img.to_rgb8()
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut jpeg, 82,
        ))
        .ok()?;
    Some(MediaArt {
        generation: gen,
        mime: "image/jpeg".to_string(),
        base64: base64(&jpeg),
    })
}

/// `file:///path` or a bare path to a local file. Anything remote is refused:
/// fetching an `http://` cover would turn a media poll into an outbound
/// request the user never asked for.
fn art_path(url: &str) -> Option<std::path::PathBuf> {
    if url.is_empty() {
        return None;
    }
    let raw = if url.starts_with('/') {
        percent_decode(url)
    } else {
        let rest = url.strip_prefix("file://")?;
        let decoded = percent_decode(rest);
        // "file:///path" keeps its leading slash; "file://host/path" is remote.
        if !decoded.starts_with('/') {
            return None;
        }
        decoded
    };
    Some(std::path::PathBuf::from(raw))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

/// Standard base64. Small enough not to be worth a dependency.
fn base64(bytes: &[u8]) -> String {
    const SET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(SET[(n >> (18 - i * 6) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn control(player: Option<String>, verb: &str) -> Result<(), String> {
    playerctl(&player, &[verb])
        .map(|_| ())
        .ok_or_else(|| format!("no media player answered `{verb}`"))
}

#[tauri::command]
pub fn media_play_pause(player: Option<String>) -> Result<(), String> {
    control(player, "play-pause")
}

#[tauri::command]
pub fn media_next(player: Option<String>) -> Result<(), String> {
    control(player, "next")
}

#[tauri::command]
pub fn media_previous(player: Option<String>) -> Result<(), String> {
    control(player, "previous")
}

/// Jump to an absolute position. Seconds, because that is what `playerctl`
/// takes; the UI works in microseconds like the rest of MPRIS.
#[tauri::command]
pub fn media_seek(player: Option<String>, position_us: u64) -> Result<(), String> {
    let secs = format!("{:.3}", position_us as f64 / 1e6);
    playerctl(&player, &["position", &secs])
        .map(|_| ())
        .ok_or_else(|| "the player refused to seek".to_string())
}

#[cfg(test)]
mod tests {
    /// The two ways playerctl reports position differ by a factor of a
    /// million: `metadata --format {{position}}` is microseconds, the
    /// `position` subcommand is seconds. Reading the first as the second put
    /// the playhead 38 hours into a 3-minute track and made the scrub bar and
    /// the elapsed time useless.
    #[test]
    fn positions_from_metadata_are_microseconds_already() {
        // A real reading: 139.2 s into a 168.8 s track.
        assert_eq!(parse_us("139227973"), 139_227_973);
        assert_eq!(parse_us("168855691"), 168_855_691);
        // Roughly two and a half minutes, not two and a half thousand hours.
        assert!(parse_us("139227973") / 1_000_000 < 200);
    }

    #[test]
    fn a_missing_or_odd_field_is_zero_rather_than_wrong() {
        assert_eq!(parse_us(""), 0);
        assert_eq!(parse_us("   "), 0);
        assert_eq!(parse_us("n/a"), 0);
        assert_eq!(parse_us("-5"), 0);
        // Some players print a decimal even in the microsecond field.
        assert_eq!(parse_us("1000.9"), 1000);
    }

    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_generation_is_stable_per_url_and_never_collides_with_no_art() {
        assert_eq!(generation(""), 0, "empty url means no art");
        assert_eq!(generation("file:///tmp/a"), generation("file:///tmp/a"));
        assert_ne!(generation("file:///tmp/a"), generation("file:///tmp/b"));
        assert_ne!(generation("file:///tmp/a"), 0);
    }

    #[test]
    fn art_paths_are_local_only() {
        assert_eq!(
            art_path("file:///tmp/cover.png"),
            Some("/tmp/cover.png".into())
        );
        // Browsers write extensionless temp files; still a local path.
        assert_eq!(
            art_path("file:///tmp/.org.chromium.Chromium.C8iXh8"),
            Some("/tmp/.org.chromium.Chromium.C8iXh8".into())
        );
        assert_eq!(art_path("file:///tmp/a%20b.png"), Some("/tmp/a b.png".into()));
        // A remote cover would make a media poll fetch from the network.
        assert_eq!(art_path("https://example.com/cover.jpg"), None);
        assert_eq!(art_path("file://host/share/cover.png"), None);
        assert_eq!(art_path(""), None);
    }

    #[test]
    fn a_player_id_reduces_to_something_worth_showing() {
        let p = MediaPlayer {
            id: "chromium.instance1652439".into(),
            label: "chromium".into(),
        };
        assert_eq!(p.id.split('.').next(), Some(p.label.as_str()));
    }
}


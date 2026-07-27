//! Shared plumbing for the JSON config stores: one load path and one save
//! path for every file under `$XDG_CONFIG_HOME/inari`.
//!
//! Each store used to repeat the same config_path/read/parse/warn/default
//! and ensure_private_dir/serialise/write_atomic pair. Duplicated, the
//! corruption behaviour drifted per file - which is how a torn buses.json
//! could be dropped more quietly than the rest. Routing everything through
//! [`parse_or_default`] makes that impossible: a file we cannot read is
//! always logged and always degrades to the store's default.

use std::path::{Path, PathBuf};

use log::warn;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// Schema version stamped into every store we write. Bump when a file's
/// shape changes in a way `#[serde(default)]` on the new fields alone
/// cannot paper over, i.e. when loading needs real migration code.
pub const SCHEMA_VERSION: u32 = 1;

/// A store's schema version. It defaults to [`SCHEMA_VERSION`] both when
/// the key is missing (files written before versioning existed *are* the
/// current schema) and when a store is built with `Default`, so stores can
/// keep deriving `Default`.
///
/// A version read from disk is kept and written back unchanged: an older
/// Inari that autosaves a newer file must not relabel it as its own
/// schema, or the newer one would stop recognising its own data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Version(pub u32);

impl Default for Version {
    fn default() -> Self {
        Self(SCHEMA_VERSION)
    }
}

/// Unknown top-level keys, carried through a load/save round-trip via
/// `#[serde(flatten)]`. Without it an older Inari that loads a file
/// written by a newer one and then autosaves silently deletes whatever it
/// did not understand - the user's work, from its own point of view.
pub type Extra = serde_json::Map<String, serde_json::Value>;

/// Path of `file` inside Sink's config directory.
pub fn config_path(file: &str) -> Result<PathBuf, SinkError> {
    let dir = dirs::config_dir()
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
    Ok(dir.join("inari").join(file))
}

/// Read `file`'s text. `None` means "nothing saved yet": no resolvable
/// config directory, or no file (first run, or the user wiped the config).
/// Stores that need to tell first run from corruption - to seed a legacy
/// default - branch on this; the rest use [`load`].
pub fn read(file: &str) -> Option<String> {
    std::fs::read_to_string(config_path(file).ok()?).ok()
}

/// Parse stored JSON, logging and degrading to the store's default on
/// anything unreadable. Corruption must never take the app down, and must
/// never be honoured silently either.
pub fn parse_or_default<T: DeserializeOwned + Default>(file: &str, raw: &str) -> T {
    match serde_json::from_str(raw) {
        Ok(value) => {
            warn_if_newer(file, raw);
            value
        }
        Err(e) => {
            warn!("ignoring malformed {file}: {e}");
            T::default()
        }
    }
}

/// The whole load path for stores that need no post-parse sanitization.
pub fn load<T: DeserializeOwned + Default>(file: &str) -> T {
    read(file)
        .map(|raw| parse_or_default(file, &raw))
        .unwrap_or_default()
}

/// Serialise `value` and write it into the config directory atomically,
/// creating the directory 0700 first.
pub fn save<T: Serialize + ?Sized>(file: &str, value: &T) -> Result<(), SinkError> {
    save_at(&config_path(file)?, value)
}

/// Like [`save`], for stores that build their own path (profiles and EQ
/// presets live in per-item files under a subdirectory).
pub fn save_at<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), SinkError> {
    if let Some(parent) = path.parent() {
        super::ensure_private_dir(parent)?;
    }
    let json = serde_json::to_string_pretty(value).map_err(|e| {
        SinkError::Config(format!("serialize {}: {e}", path.display()))
    })?;
    super::write_atomic(path, &json)?;
    Ok(())
}

/// A file from a newer Inari still loads - unknown fields are preserved
/// and written back - but say so once: it explains why a downgrade shows
/// fewer settings than the user just configured.
fn warn_if_newer(file: &str, raw: &str) {
    /// Reads just the version out of an otherwise unknown document.
    #[derive(Deserialize)]
    struct Peek {
        #[serde(default)]
        version: Version,
    }

    if let Ok(Peek { version }) = serde_json::from_str::<Peek>(raw) {
        if version.0 > SCHEMA_VERSION {
            warn!(
                "{file} was written by a newer Inari (schema {} > {SCHEMA_VERSION}); \
                 fields this version does not know are preserved as-is",
                version.0
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-in for a real store: version, a known field, and the
    /// catch-all every store carries.
    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Store {
        #[serde(default)]
        version: Version,
        #[serde(default)]
        label: String,
        #[serde(default, flatten)]
        extra: Extra,
    }

    #[test]
    fn version_defaults_to_current_when_absent() {
        // A file written before versioning existed is the current schema.
        let s: Store = parse_or_default("t.json", r#"{"label":"a"}"#);
        assert_eq!(s.version, Version(SCHEMA_VERSION));
        // …and a fresh store agrees, so Default and disk cannot disagree.
        assert_eq!(Store::default().version, Version(SCHEMA_VERSION));
        // The stamp is written back out.
        let json = serde_json::to_string(&s).expect("serializes");
        assert!(json.contains(r#""version":1"#), "{json}");
    }

    #[test]
    fn unknown_fields_survive_a_round_trip() {
        // A file from a newer Inari: higher version, fields we never heard
        // of. Loading and saving it must not eat any of that.
        let raw = r#"{"version":9,"label":"a","future":{"x":[1,2]},"flag":true}"#;
        let s: Store = parse_or_default("t.json", raw);
        assert_eq!(s.label, "a");
        assert_eq!(s.version, Version(9), "the file's own version is kept");
        let back: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&s).expect("serializes")).expect("value");
        assert_eq!(back["future"]["x"], serde_json::json!([1, 2]));
        assert_eq!(back["flag"], serde_json::json!(true));
        assert_eq!(back["version"], serde_json::json!(9));
    }

    #[test]
    fn corrupt_input_falls_back_to_default() {
        // Truncated, empty, and wrong-shaped files all degrade instead of
        // failing the load - and none of them is honoured silently.
        assert_eq!(parse_or_default::<Store>("t.json", "{ truncated"), Store::default());
        assert_eq!(parse_or_default::<Store>("t.json", ""), Store::default());
        assert_eq!(parse_or_default::<Store>("t.json", "[]"), Store::default());
    }

    #[test]
    fn save_at_round_trips_through_the_filesystem() {
        let dir = std::env::temp_dir().join(format!(
            "sink-json-{}-{}",
            std::process::id(),
            super::super::unix_now()
        ));
        let path = dir.join("store.json");
        let mut store = Store {
            label: "kept".into(),
            ..Store::default()
        };
        store.extra.insert("future".into(), serde_json::json!(7));

        save_at(&path, &store).expect("saves");
        let raw = std::fs::read_to_string(&path).expect("reads back");
        assert_eq!(parse_or_default::<Store>("store.json", &raw), store);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

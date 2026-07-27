//! Serving bytes to the tablet: the frontend bundle, and app icons.
//!
//! The bundle is not a second copy. Tauri already embeds `frontendDist` in the
//! binary and `AppHandle::asset_resolver()` reads those exact assets, so the
//! remote adds no megabytes and cannot ship a stale build. It also means CI
//! can run clippy without having built the frontend first, which an
//! `include_dir`-style embed would break.
//!
//! Icons are the other half. The desktop UI resolves them with
//! `convertFileSrc`, which produces `asset.localhost` URLs that only resolve
//! inside the Tauri webview - on a tablet those images would be blank. So the
//! remote serves icon files itself, restricted to the same directories
//! `tauri.conf.json`'s `assetProtocol` scope allows and nothing else.

use std::path::{Component, Path, PathBuf};

/// Extensions an icon may have. An icon path arrives from the client, and
/// "it is under an icon directory" is not on its own a reason to hand back
/// arbitrary bytes - a stray `.conf` or private key that happens to sit in an
/// icon theme is not an icon.
const ICON_EXTENSIONS: &[&str] = &["png", "svg", "svgz", "xpm", "jpg", "jpeg", "webp", "gif", "ico"];

/// Refuse to inline anything larger than this as an icon. Real icons are
/// kilobytes; the cap is what stops a symlinked disk image from being streamed
/// to a tablet.
pub const ICON_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Why an icon request was refused. The client is told which, but nothing
/// about what does exist on disk.
#[derive(Debug, PartialEq, Eq)]
pub enum IconReject {
    /// Not an absolute path, or it tries to walk out of wherever it starts.
    Malformed,
    /// Not an image extension we serve.
    NotAnIcon,
    /// Outside every directory the asset-protocol scope allows.
    OutOfScope,
}

/// The directories an icon may come from: the `assetProtocol` scope of
/// `tauri.conf.json`, kept in step by hand because the scope is only readable
/// from the webview side at runtime.
///
/// Icon themes and pixmaps, nothing else. Widening this list widens what any
/// device on the Wi-Fi can read out of the user's machine.
pub fn icon_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/usr/share/icons"),
        PathBuf::from("/usr/share/pixmaps"),
        PathBuf::from("/var/lib/flatpak/exports/share/icons"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".local/share/icons"));
        roots.push(home.join(".local/share/flatpak/exports/share/icons"));
    }
    roots
}

/// Everything that can be decided about an icon path without touching the
/// filesystem: it must be absolute, contain no `..`, and look like an image.
///
/// Split out from the scope check so the traversal rule is testable on paths
/// that do not exist - which is exactly the shape an attack has.
pub fn vet_icon_path(raw: &str) -> Result<&Path, IconReject> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(IconReject::Malformed);
    }
    // `..` is rejected outright rather than normalized away. Canonicalization
    // below would resolve it anyway, but a path that asks to climb out is a
    // request we have no reason to honour in any form.
    if path.components().any(|c| c == Component::ParentDir) {
        return Err(IconReject::Malformed);
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or(IconReject::NotAnIcon)?;
    if !ICON_EXTENSIONS.contains(&ext.as_str()) {
        return Err(IconReject::NotAnIcon);
    }
    Ok(path)
}

/// Is `path` inside one of `roots`?
///
/// Both sides must already be canonical: icon themes are full of symlinks, and
/// a symlink inside an icon directory pointing at `/etc/shadow` is only caught
/// by comparing where the link actually lands.
pub fn within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

/// Read an icon for a remote client, or say why not.
pub fn read_icon(raw: &str) -> Result<(Vec<u8>, &'static str), IconReject> {
    use std::io::Read;

    let path = vet_icon_path(raw)?;
    let real = path.canonicalize().map_err(|_| IconReject::OutOfScope)?;
    // Roots are canonicalized too (several are symlinks on a merged-/usr
    // system); ones that do not exist on this machine simply drop out.
    let roots: Vec<PathBuf> = icon_roots()
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .collect();
    if !within_roots(&real, &roots) {
        return Err(IconReject::OutOfScope);
    }
    // Opened once and asked about itself: `metadata(path)` followed by
    // `read(path)` is two lookups of a name that can change between them, so
    // the size the second one returns need not be the size the first one
    // approved.
    let file = std::fs::File::open(&real).map_err(|_| IconReject::OutOfScope)?;
    let meta = file.metadata().map_err(|_| IconReject::OutOfScope)?;
    if !meta.is_file() || meta.len() > ICON_MAX_BYTES {
        return Err(IconReject::OutOfScope);
    }
    // Read through the cap rather than trusting it: a file that grows after
    // the stat still cannot stream more than this to a tablet.
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(ICON_MAX_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|_| IconReject::OutOfScope)?;
    Ok((bytes, content_type(&real)))
}

/// Content type for a path, by extension.
///
/// The asset resolver reports its own mime type and that one is preferred;
/// this covers icons and the handful of cases where the resolver returns an
/// empty string.
pub fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" | "svgz" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "xpm" => "image/x-xpixmap",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        // Unknown types are handed over as opaque bytes rather than guessed
        // at: a wrong `text/html` would be an XSS vector on our own origin.
        _ => "application/octet-stream",
    }
}

/// Turn a request path into an asset-resolver key, or `None` if it is not one
/// we will look up.
///
/// The resolver is key-based and percent-decodes internally, so it would not
/// touch the filesystem regardless - but the client's string is checked here
/// anyway rather than trusted to stay that way. Decoding first is the point:
/// `%2e%2e%2f` is `../` and has to be caught after decoding, not before.
pub fn asset_key(request_path: &str) -> Option<String> {
    let decoded = percent_encoding::percent_decode_str(request_path)
        .decode_utf8()
        .ok()?;
    if decoded.contains("..") || decoded.contains('\0') || decoded.contains('\\') {
        return None;
    }
    let trimmed = decoded.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some("index.html".to_string());
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_absolute_image_paths_are_considered() {
        assert!(vet_icon_path("/usr/share/icons/hicolor/48x48/apps/firefox.png").is_ok());
        assert_eq!(vet_icon_path("usr/share/icons/x.png"), Err(IconReject::Malformed));
        assert_eq!(vet_icon_path("").unwrap_err(), IconReject::Malformed);
        assert_eq!(
            vet_icon_path("/usr/share/icons/../../etc/shadow.png").unwrap_err(),
            IconReject::Malformed,
            "traversal is refused before anything touches the filesystem"
        );
        assert_eq!(
            vet_icon_path("/usr/share/icons/theme/index.theme").unwrap_err(),
            IconReject::NotAnIcon
        );
        assert_eq!(
            vet_icon_path("/usr/share/icons/id_rsa").unwrap_err(),
            IconReject::NotAnIcon,
            "an extensionless file under an icon root is still not an icon"
        );
    }

    #[test]
    fn the_scope_is_the_asset_protocol_scope() {
        let roots = icon_roots();
        assert!(roots.contains(&PathBuf::from("/usr/share/icons")));
        assert!(roots.contains(&PathBuf::from("/usr/share/pixmaps")));
        // Nothing broader ever got added by accident.
        assert!(!roots.contains(&PathBuf::from("/usr/share")));
        assert!(!roots.contains(&PathBuf::from("/")));
    }

    #[test]
    fn paths_outside_every_root_are_refused() {
        let roots = vec![PathBuf::from("/usr/share/icons"), PathBuf::from("/usr/share/pixmaps")];
        assert!(within_roots(Path::new("/usr/share/icons/hicolor/app.png"), &roots));
        assert!(within_roots(Path::new("/usr/share/pixmaps/app.png"), &roots));
        assert!(!within_roots(Path::new("/etc/shadow"), &roots));
        assert!(!within_roots(Path::new("/home/user/.ssh/id_rsa.png"), &roots));
        assert!(
            !within_roots(Path::new("/usr/share/icons-evil/app.png"), &roots),
            "a sibling directory sharing a name prefix is not inside the root"
        );
    }

    #[test]
    fn a_real_file_outside_the_scope_is_refused() {
        // The end-to-end shape of the check, on a file that definitely exists.
        assert_eq!(read_icon("/etc/hostname").unwrap_err(), IconReject::NotAnIcon);
        assert_eq!(
            read_icon("/tmp/../etc/hosts.png").unwrap_err(),
            IconReject::Malformed
        );
    }

    #[test]
    fn asset_keys_cannot_climb_out() {
        assert_eq!(asset_key("/").as_deref(), Some("index.html"));
        assert_eq!(asset_key("").as_deref(), Some("index.html"));
        assert_eq!(asset_key("/assets/index-abc.js").as_deref(), Some("assets/index-abc.js"));
        assert_eq!(asset_key("/../../etc/passwd"), None);
        assert_eq!(
            asset_key("/%2e%2e%2f%2e%2e%2fetc%2fpasswd"),
            None,
            "percent-encoded traversal is decoded before it is judged"
        );
        assert_eq!(asset_key("/assets\\..\\x"), None);
    }

    #[test]
    fn content_types_cover_what_the_bundle_ships() {
        assert_eq!(content_type(Path::new("a/index.html")), "text/html; charset=utf-8");
        assert_eq!(content_type(Path::new("a/index-x.js")), "text/javascript; charset=utf-8");
        assert_eq!(content_type(Path::new("a/index-x.CSS")), "text/css; charset=utf-8");
        assert_eq!(content_type(Path::new("a/logo.svg")), "image/svg+xml");
        assert_eq!(content_type(Path::new("a/font.woff2")), "font/woff2");
        assert_eq!(content_type(Path::new("a/thing")), "application/octet-stream");
    }
}

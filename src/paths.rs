//! App-side half of the path pipeline. The ENGINE owns drop-spelling
//! classification (`abstracttui::input::paste::classify` — pure string
//! parsing, the cross-terminal corpus); this module owns what the
//! ruled split assigns the app: `~` expansion, existence checks, and
//! file-vs-directory classification against the REAL filesystem.
//!
//! `expand_path_spelling` additionally serves the `/attach <path>` and
//! `exec --attach` TYPED arguments, which accept spellings the drop
//! classifier deliberately refuses (relative paths — a typed flag is
//! explicit intent; quotes for shell-copied paths).

/// One drop-classified path resolved against the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    /// Expanded (`~` resolved), NOT canonicalized — attach canonicalizes
    /// so symlink semantics live in one place.
    pub path: String,
    pub is_dir: bool,
}

/// Expand ONE typed path spelling: strip matching outer quotes, decode
/// `file://` URLs, unescape `\ `, expand a leading `~`. Pure string →
/// string; no filesystem contact. (Engine-classified drop paths only
/// need the `~` half — classify already stripped quotes/escapes.)
pub fn expand_path_spelling(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            s = s[1..s.len() - 1].to_string();
            break;
        }
    }
    if let Some(rest) = s.strip_prefix("file://") {
        // file://host/path or file:///path — drop up to the first '/'.
        let path_part = match rest.find('/') {
            Some(ix) => &rest[ix..],
            None => rest,
        };
        s = percent_decode(path_part);
    }
    s = s.replace("\\ ", " ");
    expand_tilde(&s)
}

/// Expand a leading `~`/`~/` against `$HOME` (the app's side of the
/// engine split — classify returns `~/…` tokens as-is by contract).
pub fn expand_tilde(s: &str) -> String {
    if s == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return home;
        }
    } else if let Some(rest) = s.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    s.to_string()
}

/// Minimal percent-decoder (UTF-8 lossy on invalid sequences — a path
/// that decodes badly simply fails the existence gate downstream).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Existence gate over engine-classified drop paths: EVERY path must
/// exist (metadata-only stats, bounded by the classifier's own path
/// cap) or the whole drop refuses — one miss means "this paste was not
/// a drop after all" and the text inserts as today. Fifos/devices also
/// refuse (a send-time read of a fifo would hang the worker).
pub fn resolve_drop(paths: &[String]) -> Option<Vec<ResolvedPath>> {
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let expanded = expand_tilde(p);
        let meta = std::fs::metadata(&expanded).ok()?;
        let ft = meta.file_type();
        if ft.is_dir() {
            out.push(ResolvedPath {
                path: expanded,
                is_dir: true,
            });
        } else if ft.is_file() {
            out.push(ResolvedPath {
                path: expanded,
                is_dir: false,
            });
        } else {
            return None;
        }
    }
    Some(out)
}

/// Human size for chips/notices: 1-decimal KB/MB (bytes below 1 KB).
pub fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// End-to-end kind honesty (attachments design §4.5): what the MODEL
/// can actually do with this file, keyed on extension. `None` = no
/// caveat needed (text-like inlines for any model; PDF extracts
/// server-side).
pub fn kind_caveat(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    const TEXT_LIKE: &[&str] = &[
        "txt", "md", "markdown", "json", "yaml", "yml", "xml", "js", "ts", "jsx", "tsx", "py",
        "rs", "go", "java", "c", "h", "cpp", "hpp", "sh", "bash", "zsh", "toml", "ini", "cfg",
        "csv", "tsv", "html", "css", "sql", "rb", "php", "swift", "kt", "log", "tex", "rst", "pdf",
        "svg",
    ];
    // svg is deliberately TEXT-side: it uploads as application/xml and
    // inlines readably (the image modality would route it to raster
    // VLM decoders that reject it).
    //
    // Raster sets track core's canonical gate (c5574): the RELIABLE
    // subset is what vision providers actually accept; tif/tiff/bmp
    // pass core's gate + PIL decode but most provider image inputs
    // reject them — the caveat scopes confidence honestly instead of
    // promising sight core cannot guarantee downstream.
    const IMAGE_RELIABLE: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
    const IMAGE_FRAGILE: &[&str] = &["bmp", "tif", "tiff"];
    if TEXT_LIKE.contains(&ext.as_str()) {
        None
    } else if IMAGE_RELIABLE.contains(&ext.as_str()) {
        Some("images need a vision-capable model route")
    } else if IMAGE_FRAGILE.contains(&ext.as_str()) {
        Some("accepted, but many vision providers reject TIFF/BMP — PNG or JPEG is safest")
    } else {
        Some("the agent can list this file but likely cannot read its contents")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tempfile(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("acode-paths-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "x").unwrap();
        p.display().to_string()
    }

    #[test]
    fn spellings_expand() {
        assert_eq!(expand_path_spelling("\"/a/b c\""), "/a/b c");
        assert_eq!(expand_path_spelling("'/a/b'"), "/a/b");
        assert_eq!(expand_path_spelling("/a/My\\ File.txt"), "/a/My File.txt");
        assert_eq!(
            expand_path_spelling("file:///a/report%20final.pdf"),
            "/a/report final.pdf"
        );
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_path_spelling("~/x.txt"), format!("{home}/x.txt"));
        // Relative spellings pass through untouched (typed-arg lane).
        assert_eq!(expand_path_spelling("Cargo.toml"), "Cargo.toml");
    }

    #[test]
    fn resolve_drop_gates_on_existence_and_regularity() {
        let f = tempfile("plain.txt");
        let ok = resolve_drop(std::slice::from_ref(&f)).unwrap();
        assert_eq!(ok[0].path, f);
        assert!(!ok[0].is_dir);
        // One miss refuses the whole drop.
        assert!(resolve_drop(&[f.clone(), "/definitely/not/here".into()]).is_none());
        // Non-regular files refuse.
        assert!(resolve_drop(&["/dev/null".into()]).is_none());
        // Directories classify as dirs.
        let dir = std::env::temp_dir().display().to_string();
        assert!(resolve_drop(&[dir]).unwrap()[0].is_dir);
        // ~ expands before the stat (engine returns ~ tokens as-is).
        let home_rel = format!(
            "~/{}",
            std::path::Path::new(&f)
                .strip_prefix(std::env::var("HOME").unwrap())
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| String::new())
        );
        if home_rel != "~/" {
            assert!(resolve_drop(&[home_rel]).is_some());
        }
    }

    #[test]
    fn kind_caveats_are_honest() {
        assert_eq!(kind_caveat("notes.md"), None);
        assert_eq!(kind_caveat("report.PDF"), None);
        assert!(kind_caveat("photo.png").unwrap().contains("vision"));
        // Fragile rasters (core c5574): accepted at core's gate, but
        // provider image inputs mostly reject them — scoped confidence.
        assert!(kind_caveat("scan.tif").unwrap().contains("PNG or JPEG"));
        assert!(kind_caveat("scan.TIFF").unwrap().contains("PNG or JPEG"));
        assert!(kind_caveat("old.bmp").unwrap().contains("PNG or JPEG"));
        assert!(kind_caveat("data.zip").unwrap().contains("list"));
        assert!(kind_caveat("noext").unwrap().contains("list"));
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(500), "500 B");
        assert_eq!(human_size(12_700), "12.4 KB");
        assert_eq!(human_size(1_258_291), "1.2 MB");
    }
}

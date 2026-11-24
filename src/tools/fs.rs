//! File system tools, confined to configured root directories.
//!
//! Every path is resolved to a canonical absolute path (symlinks followed)
//! and must lie inside a canonical root. `..` components are rejected
//! outright, and a final symlink component is never written through.

use super::{arg_str, untrusted};
use crate::config::{Config, Paths};
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};

const MAX_READ: usize = 256 * 1024;
const MAX_LIST: usize = 500;
const MAX_WALK: usize = 20_000;



pub struct Sandbox {
    roots: Vec<PathBuf>,
    /// Alice's own data (config, secrets, database, other profiles) is never
    /// reachable, whatever the roots are, except the workspace directory.
    protected: Vec<PathBuf>,
    workspace: Option<PathBuf>,
}

/// The working folder (`tools.fs.workspace`, default `~/.alice/workspace`).
pub fn workspace_dir(cfg: &Config, paths: &Paths) -> PathBuf {
    match cfg.tools.fs.workspace.trim() {
        "" => paths.workspace(),
        w => expand_home(w),
    }
}

fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(h) = std::env::var_os("HOME") {
            return PathBuf::from(h).join(rest);
        }
    PathBuf::from(p)
}

impl Sandbox {
    pub fn new(cfg: &Config, paths: &Paths) -> Sandbox {
        let ws = workspace_dir(cfg, paths);
        let _ = std::fs::create_dir_all(&ws);
        let mut roots: Vec<PathBuf> = Vec::new();
        for r in std::iter::once(ws).chain(cfg.tools.fs.roots.iter().map(|r| expand_home(r))) {
            if let Ok(c) = std::fs::canonicalize(r)
                && !roots.contains(&c) {
                    roots.push(c);
                }
        }
        let protected = [crate::config::alice_root(), paths.home.clone()]
            .into_iter()
            .filter_map(|p| std::fs::canonicalize(p).ok())
            .collect();
        Sandbox { roots, protected, workspace: std::fs::canonicalize(paths.workspace()).ok() }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    fn inside(&self, p: &Path) -> bool {
        let in_workspace = self.workspace.as_ref().is_some_and(|w| p.starts_with(w));
        if !in_workspace && self.protected.iter().any(|d| p.starts_with(d)) {
            return false;
        }
        self.roots.iter().any(|r| p.starts_with(r))
    }

    fn candidate(&self, input: &str) -> Result<PathBuf, String> {
        if self.roots.is_empty() {
            return Err("no accessible directories configured".into());
        }
        let p = expand_home(input.trim());
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err("'..' is not allowed in paths".into());
        }
        Ok(if p.is_absolute() { p } else { self.roots[0].join(p) })
    }

    /// Resolve an existing path.
    pub fn existing(&self, input: &str) -> Result<PathBuf, String> {
        let c = self.candidate(input)?;
        let real = std::fs::canonicalize(&c).map_err(|_| format!("not found: {input}"))?;
        if !self.inside(&real) {
            return Err(format!("access denied: {input} is outside the allowed directories"));
        }
        Ok(real)
    }

    /// Resolve a path that may not exist yet. Its nearest existing ancestor
    /// must be inside a root, and the final component must not be a symlink.
    pub fn target(&self, input: &str) -> Result<PathBuf, String> {
        let c = self.candidate(input)?;
        let mut existing = c.as_path();
        let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
        while std::fs::symlink_metadata(existing).is_err() {
            match (existing.parent(), existing.file_name()) {
                (Some(parent), Some(name)) => {
                    rest.push(name);
                    existing = parent;
                }
                _ => return Err(format!("invalid path: {input}")),
            }
        }
        if rest.is_empty()
            && let Ok(m) = std::fs::symlink_metadata(existing)
                && m.file_type().is_symlink() {
                    // Allowed only if it resolves inside a root.
                    let real = std::fs::canonicalize(existing).map_err(|_| format!("broken symlink: {input}"))?;
                    if !self.inside(&real) {
                        return Err(format!("access denied: {input} is outside the allowed directories"));
                    }
                    return Ok(real);
                }
        let mut real = std::fs::canonicalize(existing).map_err(|e| e.to_string())?;
        if !self.inside(&real) {
            return Err(format!("access denied: {input} is outside the allowed directories"));
        }
        for name in rest.iter().rev() {
            real.push(name);
        }
        Ok(real)
    }

    fn display(&self, p: &Path) -> String {
        for r in &self.roots {
            if let Ok(rel) = p.strip_prefix(r) {
                return if rel.as_os_str().is_empty() { r.display().to_string() } else { r.join(rel).display().to_string() };
            }
        }
        p.display().to_string()
    }
}

pub async fn execute(cfg: &Config, paths: &Paths, name: &str, args: &Value) -> Result<String, String> {
    let sb = Sandbox::new(cfg, paths);
    let args = args.clone();
    let name = name.to_string();
    tokio::task::spawn_blocking(move || run(&sb, &name, &args)).await.map_err(|e| e.to_string())?
}

fn run(sb: &Sandbox, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "fs_list" => {
            let p = sb.existing(arg_str(args, "path")?)?;
            let mut entries: Vec<Value> = Vec::new();
            for e in std::fs::read_dir(&p).map_err(|e| e.to_string())?.flatten().take(MAX_LIST) {
                let Ok(m) = e.metadata() else { continue };
                let kind = if m.is_dir() { "dir" } else if m.file_type().is_symlink() { "symlink" } else { "file" };
                entries.push(json!({ "name": e.file_name().to_string_lossy(), "type": kind, "size": m.len() }));
            }
            entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            Ok(json!({ "path": sb.display(&p), "entries": entries }).to_string())
        }
        "fs_read" => {
            let input = arg_str(args, "path")?;
            let p = sb.existing(input)?;
            let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
            if bytes.iter().take(8192).any(|b| *b == 0) {
                return Err("binary file; not readable as text".into());
            }
            let text = String::from_utf8_lossy(&bytes);
            let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let limit = args.get("limit").and_then(|v| v.as_u64()).map(|v| v as usize);
            let mut out: String = match limit {
                Some(l) => text.lines().skip(offset).take(l).collect::<Vec<_>>().join("\n"),
                None if offset > 0 => text.lines().skip(offset).collect::<Vec<_>>().join("\n"),
                None => text.into_owned(),
            };
            if out.len() > MAX_READ {
                out = format!("{}\n[truncated: file larger than {} KB; use offset/limit]", crate::util::truncate(&out, MAX_READ), MAX_READ / 1024);
            }
            Ok(untrusted(&format!("file:{}", sb.display(&p)), &out))
        }
        "fs_search" => {
            let root = sb.existing(arg_str(args, "path")?)?;
            let name_q = args.get("name").and_then(|v| v.as_str()).map(|s| s.to_lowercase());
            let text_q = args.get("text").and_then(|v| v.as_str());
            if name_q.is_none() && text_q.is_none() {
                return Err("give `name` and/or `text`".into());
            }
            let mut hits = Vec::new();
            let mut stack = vec![root];
            let mut seen = 0usize;
            while let Some(dir) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&dir) else { continue };
                for e in rd.flatten() {
                    seen += 1;
                    if seen > MAX_WALK || hits.len() >= 200 {
                        break;
                    }
                    let Ok(ft) = e.file_type() else { continue };
                    let path = e.path();
                    if ft.is_symlink() {
                        continue; // never follow links during walks
                    }
                    if ft.is_dir() {
                        if sb.inside(&path) {
                            stack.push(path);
                        }
                        continue;
                    }
                    if !sb.inside(&path) {
                        continue;
                    }
                    let fname = e.file_name().to_string_lossy().to_lowercase();
                    if let Some(q) = &name_q
                        && !fname.contains(q.as_str()) {
                            continue;
                        }
                    if let Some(t) = text_q {
                        let Ok(m) = e.metadata() else { continue };
                        if m.len() > 2 * 1024 * 1024 {
                            continue;
                        }
                        let Ok(body) = std::fs::read_to_string(&path) else { continue };
                        if !body.contains(t) {
                            continue;
                        }
                    }
                    hits.push(sb.display(&path));
                }
            }
            Ok(json!({ "matches": hits, "truncated": seen > MAX_WALK || hits.len() >= 200 }).to_string())
        }
        "fs_write" => {
            let input = arg_str(args, "path")?;
            let content = arg_str(args, "content")?;
            let overwrite = args.get("overwrite").and_then(|v| v.as_bool()).unwrap_or(false);
            let p = sb.target(input)?;
            if p.is_dir() {
                return Err("path is a directory".into());
            }
            if p.exists() && !overwrite {
                return Err("file exists; set overwrite=true to replace it".into());
            }
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&p, content).map_err(|e| e.to_string())?;
            Ok(format!("wrote {} bytes to {}", content.len(), sb.display(&p)))
        }
        "fs_edit" => {
            let p = sb.existing(arg_str(args, "path")?)?;
            let old = arg_str(args, "old")?;
            let new = arg_str(args, "new")?;
            if old.is_empty() {
                return Err("`old` must not be empty".into());
            }
            let body = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            match body.matches(old).count() {
                0 => Err("`old` text not found".into()),
                1 => {
                    std::fs::write(&p, body.replacen(old, new, 1)).map_err(|e| e.to_string())?;
                    Ok(format!("edited {}", sb.display(&p)))
                }
                n => Err(format!("`old` text occurs {n} times; make it unique")),
            }
        }
        "fs_mkdir" => {
            let p = sb.target(arg_str(args, "path")?)?;
            std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
            Ok(format!("created {}", sb.display(&p)))
        }
        "fs_move" => {
            let from = sb.existing(arg_str(args, "from")?)?;
            let to = sb.target(arg_str(args, "to")?)?;
            if sb.roots().iter().any(|r| r == &from) {
                return Err("cannot move a root directory".into());
            }
            if to.exists() {
                return Err("destination exists".into());
            }
            std::fs::rename(&from, &to).map_err(|e| e.to_string())?;
            Ok(format!("moved {} -> {}", sb.display(&from), sb.display(&to)))
        }
        "fs_delete" => {
            let input = arg_str(args, "path")?;
            let c = sb.candidate(input)?;
            // Delete the link itself, never its target.
            let meta = std::fs::symlink_metadata(&c).map_err(|_| format!("not found: {input}"))?;
            let p = if meta.file_type().is_symlink() {
                let parent = c.parent().ok_or("invalid path")?;
                let real_parent = std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
                if !sb.inside(&real_parent) {
                    return Err(format!("access denied: {input}"));
                }
                std::fs::remove_file(&c).map_err(|e| e.to_string())?;
                return Ok(format!("deleted link {input}"));
            } else {
                sb.existing(input)?
            };
            if sb.roots().iter().any(|r| r == &p) {
                return Err("cannot delete a root directory".into());
            }
            if p.is_dir() {
                if args.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false) {
                    std::fs::remove_dir_all(&p).map_err(|e| e.to_string())?;
                } else {
                    std::fs::remove_dir(&p).map_err(|e| format!("{e} (set recursive=true for non-empty directories)"))?;
                }
            } else {
                std::fs::remove_file(&p).map_err(|e| e.to_string())?;
            }
            Ok(format!("deleted {}", sb.display(&p)))
        }
        _ => Err(format!("unknown tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Sandbox, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(dir.path().join("secret.txt"), "top secret").unwrap();
        let mut cfg = Config::default();
        cfg.tools.fs.workspace = root.display().to_string();
        let paths = Paths::at(dir.path().join("home"));
        let sb = Sandbox::new(&cfg, &paths);
        (dir, sb, paths)
    }

    #[test]
    fn rejects_traversal_and_outside_paths() {
        let (dir, sb, _) = setup();
        assert!(run(&sb, "fs_read", &json!({"path": "../secret.txt"})).unwrap_err().contains(".."));
        let abs = dir.path().join("secret.txt").display().to_string();
        assert!(run(&sb, "fs_read", &json!({"path": abs})).unwrap_err().contains("denied"));
        assert!(run(&sb, "fs_write", &json!({"path": "/etc/alice-test", "content": "x"})).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let (dir, sb, _) = setup();
        let root = &sb.roots()[0];
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), root.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(dir.path(), root.join("linkdir")).unwrap();
        assert!(run(&sb, "fs_read", &json!({"path": "link.txt"})).unwrap_err().contains("denied"));
        assert!(run(&sb, "fs_read", &json!({"path": "linkdir/secret.txt"})).unwrap_err().contains("denied"));
        assert!(run(&sb, "fs_write", &json!({"path": "link.txt", "content": "x", "overwrite": true})).unwrap_err().contains("denied"));
        assert!(run(&sb, "fs_write", &json!({"path": "linkdir/new.txt", "content": "x"})).unwrap_err().contains("denied"));
        // Deleting the link removes only the link.
        run(&sb, "fs_delete", &json!({"path": "link.txt"})).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("secret.txt")).unwrap(), "top secret");
    }

    #[test]
    fn alice_data_is_never_reachable() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let paths = Paths::at(&home);
        paths.ensure().unwrap();
        std::fs::write(paths.secrets(), "server.token = \"x\"").unwrap();
        let mut cfg = Config::default();
        // The user allows the whole parent directory, which contains Alice's data.
        cfg.tools.fs.roots = vec![dir.path().display().to_string()];
        let sb = Sandbox::new(&cfg, &paths);
        let secrets = paths.secrets().display().to_string();
        assert!(run(&sb, "fs_read", &json!({"path": secrets})).unwrap_err().contains("denied"));
        let conf = paths.config().display().to_string();
        assert!(run(&sb, "fs_write", &json!({"path": conf, "content": "x"})).unwrap_err().contains("denied"));
        assert!(run(&sb, "fs_delete", &json!({"path": home.display().to_string(), "recursive": true})).is_err());
        let found = run(&sb, "fs_search", &json!({"path": dir.path().display().to_string(), "name": "secrets"})).unwrap();
        assert!(!found.contains("secrets.toml"), "{found}");
        assert!(run(&sb, "fs_list", &json!({"path": dir.path().display().to_string()})).is_ok());
        // The workspace stays usable.
        let ws = paths.workspace().join("note.txt").display().to_string();
        run(&sb, "fs_write", &json!({"path": ws, "content": "ok"})).unwrap();
    }

    #[test]
    fn crud_inside_root() {
        let (_dir, sb, _) = setup();
        run(&sb, "fs_write", &json!({"path": "a/b.txt", "content": "hello world"})).unwrap();
        assert!(run(&sb, "fs_write", &json!({"path": "a/b.txt", "content": "x"})).is_err());
        assert!(run(&sb, "fs_read", &json!({"path": "a/b.txt"})).unwrap().contains("hello world"));
        run(&sb, "fs_edit", &json!({"path": "a/b.txt", "old": "world", "new": "alice"})).unwrap();
        assert!(run(&sb, "fs_search", &json!({"path": ".", "text": "alice"})).unwrap().contains("b.txt"));
        run(&sb, "fs_move", &json!({"from": "a/b.txt", "to": "a/c.txt"})).unwrap();
        assert!(run(&sb, "fs_delete", &json!({"path": "a"})).is_err());
        run(&sb, "fs_delete", &json!({"path": "a", "recursive": true})).unwrap();
        assert!(run(&sb, "fs_delete", &json!({"path": "."})).is_err());
    }
}

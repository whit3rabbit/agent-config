//! Source parsing and fetching: `owner/repo` shorthand, git URLs, and local
//! paths.
//!
//! Fetching shells out to the `git` binary (shallow clone into a tempdir) so
//! the crate needs no git library. Local sources are used in place and never
//! copied.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use crate::diag::{codes, CliResult, Diag};
use crate::lockfile::LockPackage;

/// Parsed source reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Normalized input as it will be recorded in the lockfile.
    pub display: String,
    /// Local directory or git repo.
    pub kind: SourceKind,
    /// Clone URL for git sources.
    pub url: Option<String>,
    /// Pinned branch/tag from `owner/repo@ref` syntax.
    pub git_ref: Option<String>,
    /// Subdirectory inside the repo (`owner/repo/subpath`).
    pub subpath: Option<String>,
}

/// Whether the source is a local directory or a git repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A directory on disk.
    Local,
    /// A git repository (fetched into a tempdir).
    Git,
}

/// A fetched, ready-to-install source directory. The tempdir (if any) lives
/// as long as this value.
pub struct ResolvedSource {
    /// The parsed source this was resolved from.
    pub source: Source,
    /// Directory containing the package content (after subpath resolution).
    pub dir: PathBuf,
    /// Resolved commit SHA for git sources (best-effort `None` for local
    /// paths outside a git repo).
    pub commit: Option<String>,
    /// Holds the clone alive for the lifetime of this value.
    _temp: Option<TempDir>,
}

impl ResolvedSource {
    /// Stable name for manifest-less sources.
    pub(crate) fn fallback_name(&self) -> String {
        let name = match &self.source.url {
            Some(url) => self
                .source
                .subpath
                .as_deref()
                .unwrap_or(url)
                .split(['?', '#'])
                .next()
                .unwrap_or_default()
                .trim_end_matches('/')
                .rsplit(['/', ':'])
                .next()
                .unwrap_or_default(),
            None => self
                .dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("package"),
        };
        crate::engine::sanitize_kebab(name.strip_suffix(".git").unwrap_or(name))
    }

    /// Relative paths of files under the resolved directory, sorted,
    /// excluding `.git` internals.
    pub fn tracked_files(&self) -> CliResult<Vec<PathBuf>> {
        let mut files = Vec::new();
        collect_files(&self.dir, &self.dir, &mut files)?;
        files.sort();
        Ok(files)
    }
}

/// Parse a source string.
///
/// Accepted forms:
///
/// - local paths (absolute, `./`, `../`, or `~/`), which must exist
/// - `owner/repo`, `owner/repo/subpath`, `owner/repo@ref` (GitHub shorthand)
/// - `https://…`/`http://…`/`git@host:…`/`.git` URLs (ref via `--ref`)
/// - `github:owner/repo` alias
pub fn parse(input: &str) -> CliResult<Source> {
    let input = input.trim();
    if input.is_empty() {
        return Err(Diag::new(codes::SOURCE_INVALID, "source must not be empty"));
    }

    // github: alias — normalize through the shorthand path.
    if let Some(rest) = input.strip_prefix("github:") {
        return shorthand(rest);
    }

    // URL-ish forms: any scheme, SSH syntax, or a `.git` suffix.
    if input.contains("://") || input.starts_with("git@") || input.ends_with(".git") {
        return Ok(Source {
            display: input.to_string(),
            kind: SourceKind::Git,
            url: Some(input.to_string()),
            git_ref: None,
            subpath: None,
        });
    }

    // Local path forms
    if input.starts_with('~') {
        let expanded = expand_tilde(input);
        return local_source(&expanded, input);
    }
    let as_path = Path::new(input);
    if as_path.is_absolute() || input.starts_with("./") || input.starts_with("../") {
        return local_source(as_path, input);
    }

    // owner/repo shorthand (2+ path segments, no scheme)
    if shorthand_like(input) {
        return shorthand(input);
    }

    // Fall back to treating it as a relative path that may exist.
    if as_path.exists() {
        return local_source(as_path, input);
    }

    Err(Diag::with_fix(
        codes::SOURCE_INVALID,
        format!("cannot interpret source {input:?}"),
        "use owner/repo, a git URL, or an existing local path",
    ))
}

fn shorthand_like(input: &str) -> bool {
    // `owner/repo`, `owner/repo/subpath`, with optional `@ref` suffix.
    let (path, _ref) = split_ref(input);
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    segments.len() >= 2
        && segments.len() <= 6
        && segments.iter().all(|s| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
}

fn split_ref(input: &str) -> (&str, Option<&str>) {
    // Only treat `@…` as a ref when it follows the path part (not an email or
    // a URL userinfo segment; shorthand never contains `:` or `://`).
    if let Some(pos) = input.rfind('@') {
        let (path, reference) = input.split_at(pos);
        let reference = &reference[1..];
        if !reference.is_empty() {
            return (path, Some(reference));
        }
    }
    (input, None)
}

fn shorthand(path_part: &str) -> CliResult<Source> {
    let (path, git_ref) = split_ref(path_part);
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return Err(Diag::with_fix(
            codes::SOURCE_INVALID,
            format!("shorthand {path_part:?} needs at least owner/repo"),
            "use owner/repo or owner/repo/subpath",
        ));
    }
    let url = format!("https://github.com/{}/{}", segments[0], segments[1]);
    let subpath = (segments.len() > 2).then(|| segments[2..].join("/"));
    let display = match (&git_ref, &subpath) {
        (Some(r), Some(s)) => format!("{}/{}@{}", segments[..2].join("/"), s, r),
        (Some(r), None) => format!("{}/{}@{}", segments[0], segments[1], r),
        (None, Some(s)) => format!("{}/{}", segments[..2].join("/"), s),
        (None, None) => format!("{}/{}", segments[0], segments[1]),
    };
    Ok(Source {
        display,
        kind: SourceKind::Git,
        url: Some(url),
        git_ref: git_ref.map(str::to_string),
        subpath,
    })
}

fn local_source(path: &Path, display: &str) -> CliResult<Source> {
    let canonical = path.canonicalize().map_err(|e| {
        Diag::with_fix(
            codes::SOURCE_INVALID,
            format!("local source {display:?} does not exist ({e})"),
            "check the path and try again",
        )
    })?;
    if !canonical.is_dir() {
        return Err(Diag::with_fix(
            codes::SOURCE_INVALID,
            format!("local source {display:?} is not a directory"),
            "point at the directory containing agent.config.toml",
        ));
    }
    Ok(Source {
        display: display.to_string(),
        kind: SourceKind::Local,
        url: None,
        git_ref: None,
        subpath: None,
    })
}

fn expand_tilde(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(input)
}

/// Resolve a source to a local directory, cloning git sources into a fresh
/// tempdir. `extra_ref` (from `--ref`) overrides a shorthand-pinned ref.
pub fn resolve(input: &str, extra_ref: Option<&str>) -> CliResult<ResolvedSource> {
    let mut source = parse(input)?;
    if source.kind == SourceKind::Git && extra_ref.is_some() {
        source.git_ref = extra_ref.map(str::to_string);
    }
    resolve_source(source)
}

/// Resolve the recorded source without reparsing its display name.
pub(crate) fn replay(pkg: &LockPackage) -> CliResult<ResolvedSource> {
    if let Some(url) = &pkg.url {
        return resolve_source(Source {
            display: pkg.source.clone(),
            kind: SourceKind::Git,
            url: Some(url.clone()),
            git_ref: pkg.git_ref.clone(),
            subpath: pkg.subpath.clone(),
        });
    }
    resolve(&pkg.source, pkg.git_ref.as_deref())
}

fn resolve_source(source: Source) -> CliResult<ResolvedSource> {
    match source.kind {
        SourceKind::Local => {
            let dir = if source.display.starts_with('~') {
                expand_tilde(&source.display)
            } else {
                PathBuf::from(&source.display)
            };
            let dir = dir.canonicalize().map_err(|e| {
                Diag::new(
                    codes::SOURCE_INVALID,
                    format!("cannot open {}: {e}", source.display),
                )
            })?;
            let commit = git_rev_parse(&dir).ok();
            Ok(ResolvedSource {
                source,
                dir,
                commit,
                _temp: None,
            })
        }
        SourceKind::Git => {
            let temp = TempDir::new()
                .map_err(|e| Diag::new(codes::IO, format!("cannot create tempdir: {e}")))?;
            let url = source.url.as_deref().expect("git sources carry a url");
            let git_ref = source.git_ref.as_deref();

            if let Some(r) =
                git_ref.filter(|r| r.len() == 40 && r.chars().all(|c| c.is_ascii_hexdigit()))
            {
                fetch_commit(url, r, temp.path())?;
            } else {
                let mut cmd = Command::new("git");
                cmd.arg("clone").arg("--quiet").arg("--depth").arg("1");
                if let Some(r) = git_ref {
                    cmd.arg("--branch").arg(r);
                }
                cmd.arg("--").arg(url).arg(temp.path());
                run_git(cmd, format!("clone {url}"))?;
            }
            let commit = git_rev_parse(temp.path()).map_err(|_| {
                Diag::new(
                    codes::GIT_FAILED,
                    "clone succeeded but HEAD could not be resolved",
                )
            })?;

            let dir = match &source.subpath {
                Some(sub) => {
                    let dir = package_path(temp.path(), Path::new(sub))?;
                    if !dir.is_dir() {
                        return Err(Diag::new(
                            codes::SOURCE_INVALID,
                            format!("subpath {sub:?} is not a directory"),
                        ));
                    }
                    dir
                }
                None => temp.path().to_path_buf(),
            };
            Ok(ResolvedSource {
                source,
                dir,
                commit: Some(commit),
                _temp: Some(temp),
            })
        }
    }
}

/// Resolve a package-supplied path before opening or traversing it.
pub(crate) fn package_path(root: &Path, relative: &Path) -> CliResult<PathBuf> {
    use std::path::Component;
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err(Diag::new(
            codes::INVALID_ENTRY,
            format!(
                "package path {} must be relative and stay inside the source",
                relative.display()
            ),
        ));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|e| Diag::new(codes::IO, format!("cannot open {}: {e}", root.display())))?;
    let path = root.join(relative);
    let canonical = path
        .canonicalize()
        .map_err(|e| Diag::new(codes::IO, format!("cannot open {}: {e}", path.display())))?;
    if !canonical.starts_with(canonical_root) {
        return Err(Diag::new(
            codes::INVALID_ENTRY,
            format!(
                "package path {} escapes the source directory",
                relative.display()
            ),
        ));
    }
    Ok(canonical)
}

pub(crate) fn read_capped(path: &Path) -> CliResult<Vec<u8>> {
    use crate::mapping::MAX_FILE_BYTES as LIMIT;
    use std::io::Read as _;
    let io_error = |e| Diag::new(codes::IO, format!("cannot read {}: {e}", path.display()));
    let metadata = std::fs::metadata(path).map_err(io_error)?;
    if !metadata.is_file() {
        return Err(Diag::new(
            codes::INVALID_ENTRY,
            format!("{} must be a regular file", path.display()),
        ));
    }
    let too_large = || {
        Diag::new(
            codes::FILE_TOO_LARGE,
            format!("{} exceeds the 8 MiB limit", path.display()),
        )
    };
    if metadata.len() > LIMIT {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(io_error)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > LIMIT {
        return Err(too_large());
    }
    Ok(bytes)
}

pub(crate) fn read_text(path: &Path) -> CliResult<String> {
    String::from_utf8(read_capped(path)?).map_err(|e| {
        Diag::new(
            codes::IO,
            format!("{} is not valid UTF-8: {e}", path.display()),
        )
    })
}

fn fetch_commit(url: &str, sha: &str, into: &Path) -> CliResult<()> {
    run_git(
        {
            let mut cmd = Command::new("git");
            cmd.arg("init").arg("--quiet").arg(into);
            cmd
        },
        "init clone dir",
    )?;
    run_git(
        {
            let mut cmd = Command::new("git");
            cmd.current_dir(into);
            cmd.arg("remote").arg("add").arg("origin").arg(url);
            cmd
        },
        "add remote",
    )?;
    run_git(
        {
            let mut cmd = Command::new("git");
            cmd.current_dir(into);
            cmd.arg("fetch")
                .arg("--quiet")
                .arg("--depth")
                .arg("1")
                .arg("origin")
                .arg(sha);
            cmd
        },
        format!("fetch {sha}"),
    )?;
    run_git(
        {
            let mut cmd = Command::new("git");
            cmd.current_dir(into);
            cmd.arg("checkout").arg("--quiet").arg("FETCH_HEAD");
            cmd
        },
        "checkout fetched commit",
    )?;
    Ok(())
}

fn git_rev_parse(dir: &Path) -> CliResult<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir);
    cmd.arg("rev-parse").arg("HEAD");
    let out = run_git(cmd, "rev-parse HEAD")?;
    Ok(out.trim().to_string())
}

fn run_git(mut cmd: Command, what: impl Into<String>) -> CliResult<String> {
    let what = what.into();
    let output = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Diag::with_fix(
                codes::GIT_FAILED,
                "the git executable was not found on PATH",
                "install git (https://git-scm.com) and try again",
            )
        } else {
            Diag::new(
                codes::GIT_FAILED,
                format!("git {what} failed to start: {e}"),
            )
        }
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        let detail = if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        };
        return Err(Diag::new(
            codes::GIT_FAILED,
            format!("git {what} failed{detail}"),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> CliResult<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| Diag::new(codes::IO, format!("cannot read {}: {e}", dir.display())))?
        .filter_map(|e| e.map(|e| e.path()).ok())
        .collect();
    entries.sort();
    for path in entries {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some(".git") {
                continue;
            }
            collect_files(root, &path, files)?;
        } else {
            files.push(
                path.strip_prefix(root)
                    .map(Path::to_path_buf)
                    .unwrap_or(path),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_shorthand() {
        let s = parse("vercel-labs/skills").expect("ok");
        assert_eq!(s.kind, SourceKind::Git);
        assert_eq!(
            s.url.as_deref(),
            Some("https://github.com/vercel-labs/skills")
        );
        assert_eq!(s.git_ref, None);
        assert_eq!(s.display, "vercel-labs/skills");

        let s = parse("owner/repo@v1.2.3").expect("ok");
        assert_eq!(s.git_ref.as_deref(), Some("v1.2.3"));
        assert_eq!(s.display, "owner/repo@v1.2.3");

        let s = parse("owner/repo/packages/toolkit").expect("ok");
        assert_eq!(s.subpath.as_deref(), Some("packages/toolkit"));

        let s = parse("github:owner/repo").expect("ok");
        assert_eq!(s.url.as_deref(), Some("https://github.com/owner/repo"));
        assert_eq!(s.display, "owner/repo");
    }

    #[test]
    fn parses_git_urls() {
        for input in [
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo",
            "git@github.com:owner/repo.git",
        ] {
            let s = parse(input).expect(input);
            assert_eq!(s.kind, SourceKind::Git);
            assert_eq!(s.url.as_deref(), Some(input));
        }
    }

    #[test]
    fn parses_local_paths() {
        let dir = tempfile::tempdir().unwrap();
        let s = parse(dir.path().to_str().unwrap()).expect("ok");
        assert_eq!(s.kind, SourceKind::Local);

        let s = parse(".").expect("cwd exists");
        assert_eq!(s.kind, SourceKind::Local);

        let err = parse("./does-not-exist-xyz").unwrap_err();
        assert_eq!(err.code, codes::SOURCE_INVALID);
    }

    #[test]
    fn rejects_single_segment() {
        let err = parse("just-a-word").unwrap_err();
        assert_eq!(err.code, codes::SOURCE_INVALID);
    }

    #[test]
    fn resolve_local_records_commit_when_in_repo() {
        let dir = tempfile::tempdir().unwrap();
        // A plain tempdir is not a repo; commit must be None, not an error.
        let resolved = resolve(dir.path().to_str().unwrap(), None).expect("ok");
        assert_eq!(resolved.commit, None);
    }

    #[test]
    fn resolve_clones_a_local_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        run_git(
            {
                let mut c = Command::new("git");
                c.current_dir(&repo);
                c.arg("init").arg("--quiet");
                c
            },
            "init",
        )
        .unwrap();
        std::fs::write(repo.join("agent.config.toml"), "[package]\nname = \"t\"\n").unwrap();
        run_git(
            {
                let mut c = Command::new("git");
                c.current_dir(&repo);
                c.arg("add").arg(".");
                c
            },
            "add",
        )
        .unwrap();
        run_git(
            {
                let mut c = Command::new("git");
                c.current_dir(&repo);
                c.arg("-c")
                    .arg("user.email=t@t")
                    .arg("-c")
                    .arg("user.name=t");
                c.arg("commit").arg("--quiet").arg("-m").arg("init");
                c
            },
            "commit",
        )
        .unwrap();

        let resolved = resolve(repo.to_str().unwrap(), None).expect("resolve");
        assert!(resolved.commit.is_some(), "cloned source records a commit");
        assert!(resolved.dir.join("agent.config.toml").exists());
    }
}

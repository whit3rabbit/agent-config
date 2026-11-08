//! Direct URL downloads (raw SKILL.md, zip / tar.gz archives), ported from
//! `src/download-source.ts`.

use std::path::{Path, PathBuf};

use skills_store::error::{diag, SkillsError};
use skills_store::frontmatter::parse_frontmatter;

use crate::archive::{extract_tar, read_zip_archive, ArchiveLimits};

const DEFAULT_DOWNLOAD_MAX_BYTES: usize = 10 * 1024 * 1024;
const DEFAULT_EXTRACT_MAX_BYTES: usize = 25 * 1024 * 1024;
const DEFAULT_EXTRACT_MAX_FILES: usize = 1000;
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

use std::time::Duration;

fn positive_env(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(fallback)
}

struct DownloadLimits {
    download_max_bytes: usize,
    extract_max_bytes: usize,
    extract_max_files: usize,
}

impl DownloadLimits {
    fn from_env() -> Self {
        Self {
            download_max_bytes: positive_env(
                "SKILLS_DOWNLOAD_MAX_BYTES",
                DEFAULT_DOWNLOAD_MAX_BYTES,
            ),
            extract_max_bytes: positive_env("SKILLS_EXTRACT_MAX_BYTES", DEFAULT_EXTRACT_MAX_BYTES),
            extract_max_files: positive_env("SKILLS_EXTRACT_MAX_FILES", DEFAULT_EXTRACT_MAX_FILES),
        }
    }
}

/// A downloaded source ready for discovery.
pub struct DownloadedSource {
    /// Root directory holding the skill content.
    pub root_dir: PathBuf,
    /// The whole temp tree (cleanup owns this).
    pub temp_dir: PathBuf,
    /// What the URL was.
    pub kind: &'static str,
}

fn is_valid_skill_markdown(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let fm = parse_frontmatter(&content);
    fm.data.get("name").is_some_and(|v| v.as_str().is_some())
        && fm
            .data
            .get("description")
            .is_some_and(|v| v.as_str().is_some())
}

/// Download `url` and unpack it into a temp dir (upstream `downloadSource`).
pub fn download_source(url: &str) -> Result<DownloadedSource, SkillsError> {
    let limits = DownloadLimits::from_env();
    let temp_path = std::env::temp_dir().join(format!(
        "skills-download-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&temp_path)
        .map_err(|err| SkillsError::new("SKILLS_IO", format!("tempdir: {err}")))?;
    let downloaded_file = temp_path.join("source.download");
    let extract_dir = temp_path.join("extract");

    let result = (|| -> Result<DownloadedSource, SkillsError> {
        let body = crate::http::download_capped(url, limits.download_max_bytes, FETCH_TIMEOUT)?;
        std::fs::write(&downloaded_file, &body)
            .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
        if body.is_empty() {
            return Err(diag::download_empty());
        }

        if is_valid_skill_markdown(&downloaded_file) {
            let skill_dir = temp_path.join("skill");
            std::fs::create_dir_all(&skill_dir)
                .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
            std::fs::write(skill_dir.join("SKILL.md"), &body)
                .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
            return Ok(DownloadedSource {
                root_dir: skill_dir,
                temp_dir: temp_path.clone(),
                kind: "skill-md",
            });
        }

        std::fs::create_dir_all(&extract_dir)
            .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
        let limits = ArchiveLimits {
            max_extracted_bytes: limits.extract_max_bytes,
            max_entries: limits.extract_max_files,
        };
        let header = &body[..body.len().min(512)];
        let is_zip = header.len() >= 2 && header[0] == 0x50 && header[1] == 0x4b;
        let is_gzip = header.len() >= 2 && header[0] == 0x1f && header[1] == 0x8b;

        let extracted: Result<bool, SkillsError> = (|| {
            if is_zip {
                let files = read_zip_archive(&body, limits)?;
                for (path, contents) in files {
                    let target = lexical_join(&extract_dir, &path);
                    if !target.starts_with(&extract_dir) {
                        return Err(diag::archive_unsafe_path(&path));
                    }
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
                    }
                    std::fs::write(&target, contents)
                        .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
                }
                Ok(true)
            } else {
                // tar (gzip or plain); a plain tar error means "not an archive".
                match extract_tar(&downloaded_file, &extract_dir, is_gzip, limits) {
                    Ok(()) => Ok(true),
                    Err(err) => {
                        if matches!(
                            err.code,
                            "SKILLS_ARCHIVE_UNSAFE_PATH"
                                | "SKILLS_ARCHIVE_ENCRYPTED_ENTRY"
                                | "SKILLS_ARCHIVE_LINK_ENTRY"
                                | "SKILLS_ARCHIVE_TOO_MANY_FILES"
                                | "SKILLS_ARCHIVE_TOO_LARGE"
                        ) {
                            return Err(err);
                        }
                        Ok(false)
                    }
                }
            }
        })();

        match extracted {
            Ok(true) => {
                let root_dir =
                    single_top_level_directory(&extract_dir).unwrap_or(extract_dir.clone());
                Ok(DownloadedSource {
                    root_dir,
                    temp_dir: temp_path.clone(),
                    kind: "archive",
                })
            }
            Ok(false) => Err(diag::download_not_a_skill()),
            Err(err) => Err(err),
        }
    })();

    match result {
        Ok(source) => Ok(source),
        Err(err) => {
            let _ = std::fs::remove_dir_all(&temp_path);
            Err(err)
        }
    }
}

fn single_top_level_directory(dir: &Path) -> Option<PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    let visible: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.file_name().to_string_lossy() != "__MACOSX")
        .collect();
    if visible.len() != 1 {
        return None;
    }
    let only = visible.into_iter().next()?;
    only.file_type()
        .ok()
        .filter(|t| t.is_dir())
        .map(|_| only.path())
}

fn lexical_join(base: &Path, relative: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

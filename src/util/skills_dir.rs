//! Shared install/uninstall logic for directory-scoped skills (Claude Code,
//! Google Antigravity).
//!
//! Layout written for each skill:
//!
//! ```text
//! <skills_root>/<name>/
//!   SKILL.md             (required: YAML frontmatter + markdown body)
//!   scripts/             (optional: caller-provided scripts; chmod 0755 if `executable`)
//!   references/          (optional: docs/templates)
//!   assets/              (optional: static files)
//! ```
//!
//! Ownership is tracked in `<skills_root>/.ai-hooker-skills.json` (same
//! schema as [`super::ownership`] uses for MCP).

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::HookerError;
use crate::integration::{InstallReport, UninstallReport};
use crate::spec::{SkillFrontmatter, SkillSpec};
use crate::util::{fs_atomic, ownership};

const LEDGER_FILE: &str = ".ai-hooker-skills.json";
const SKILL_MD: &str = "SKILL.md";
const KIND: &str = "skill";

/// Path to the per-scope ownership ledger living next to the skills root.
fn ledger_path(skills_root: &Path) -> PathBuf {
    // Sidecar file lives *next to* the skills_root directory. We use
    // `<skills_root>/<LEDGER_FILE>` so it travels with the skills set.
    skills_root.join(LEDGER_FILE)
}

fn skill_dir(skills_root: &Path, name: &str) -> PathBuf {
    skills_root.join(name)
}

/// Returns true if the ledger has an entry for `name`.
pub(crate) fn is_installed(skills_root: &Path, name: &str) -> Result<bool, HookerError> {
    ownership::contains(&ledger_path(skills_root), name)
}

/// Install (or update) a skill under `<skills_root>/<spec.name>/`. Records
/// ownership in the sidecar ledger.
pub(crate) fn install(skills_root: &Path, spec: &SkillSpec) -> Result<InstallReport, HookerError> {
    spec.validate()?;
    for asset in &spec.assets {
        validate_relative(&asset.relative_path)?;
    }

    let mut report = InstallReport::default();
    let dir = skill_dir(skills_root, &spec.name);

    let skill_md_path = dir.join(SKILL_MD);
    let skill_md = render_skill_md(&spec.frontmatter, &spec.body);
    let outcome = fs_atomic::write_atomic(&skill_md_path, skill_md.as_bytes(), false)?;
    record_outcome(&mut report, outcome);

    for asset in &spec.assets {
        let asset_path = dir.join(&asset.relative_path);
        let outcome = fs_atomic::write_atomic(&asset_path, &asset.bytes, false)?;
        if asset.executable {
            fs_atomic::chmod(&asset_path, 0o755)?;
        }
        record_outcome(&mut report, outcome);
    }

    let led = ledger_path(skills_root);
    let prior = ownership::owner_of(&led, &spec.name)?;
    let owner_changed = prior.as_deref() != Some(spec.owner_tag.as_str());

    if owner_changed || !report.created.is_empty() || !report.patched.is_empty() {
        ownership::record_install(&led, &spec.name, &spec.owner_tag)?;
    }

    if report.created.is_empty() && report.patched.is_empty() && !owner_changed {
        report.already_installed = true;
    }
    Ok(report)
}

/// Uninstall a skill. Refuses on owner mismatch / hand-installed skills.
pub(crate) fn uninstall(
    skills_root: &Path,
    name: &str,
    owner_tag: &str,
) -> Result<UninstallReport, HookerError> {
    SkillSpec::validate_name(name)?;

    let mut report = UninstallReport::default();
    let dir = skill_dir(skills_root, name);
    let led = ledger_path(skills_root);

    let on_disk = dir.exists();
    let in_ledger = ownership::contains(&led, name)?;

    if !on_disk && !in_ledger {
        report.not_installed = true;
        return Ok(report);
    }

    ownership::require_owner(&led, name, owner_tag, KIND, on_disk)?;

    if on_disk {
        fs::remove_dir_all(&dir).map_err(|e| HookerError::io(&dir, e))?;
        report.removed.push(dir);
    }

    ownership::record_uninstall(&led, name)?;

    if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
        report.not_installed = true;
    }
    Ok(report)
}

/// Reject absolute or `..`-containing relative paths so callers cannot
/// escape the skill directory via crafted asset paths.
fn validate_relative(p: &Path) -> Result<(), HookerError> {
    if p.is_absolute() {
        return Err(HookerError::Other(anyhow::anyhow!(
            "skill asset path must be relative (got {p:?})"
        )));
    }
    for comp in p.components() {
        match comp {
            Component::CurDir | Component::Normal(_) => {}
            _ => {
                return Err(HookerError::Other(anyhow::anyhow!(
                    "skill asset path must not contain `..` or root (got {p:?})"
                )))
            }
        }
    }
    Ok(())
}

fn record_outcome(report: &mut InstallReport, outcome: fs_atomic::WriteOutcome) {
    if outcome.no_change {
        return;
    }
    if outcome.existed {
        report.patched.push(outcome.path.clone());
    } else {
        report.created.push(outcome.path.clone());
    }
    if let Some(b) = outcome.backup {
        report.backed_up.push(b);
    }
}

/// Render `SKILL.md` from frontmatter + body. We keep the format minimal and
/// stable: triple-dashed YAML block, then a blank line, then the body.
fn render_skill_md(fm: &SkillFrontmatter, body: &str) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", yaml_escape_scalar(&fm.name)));
    out.push_str(&format!(
        "description: {}\n",
        yaml_escape_scalar(&fm.description)
    ));
    if let Some(tools) = &fm.allowed_tools {
        out.push_str("allowed-tools:\n");
        for t in tools {
            out.push_str(&format!("  - {}\n", yaml_escape_scalar(t)));
        }
    }
    out.push_str("---\n\n");
    out.push_str(body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Quote scalars that need it. Heuristic: quote on colon, leading dash,
/// leading whitespace, or any character outside the safe-bareword set.
fn yaml_escape_scalar(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s.starts_with(' ')
        || s.starts_with('-')
        || s.contains(':')
        || s.contains('#')
        || s.contains('"')
        || s.contains('\n');
    if needs_quote {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::SkillAsset;
    use tempfile::tempdir;

    fn basic_spec(name: &str, owner: &str) -> SkillSpec {
        SkillSpec::builder(name)
            .owner(owner)
            .description("Format Git commit messages.")
            .body("## Goal\nDo the thing.\n")
            .build()
    }

    #[test]
    fn install_creates_directory_with_skill_md() {
        let dir = tempdir().unwrap();
        install(dir.path(), &basic_spec("git-commit-formatter", "myapp")).unwrap();
        let md_path = dir.path().join("git-commit-formatter/SKILL.md");
        assert!(md_path.exists());
        let s = fs::read_to_string(&md_path).unwrap();
        assert!(s.starts_with("---\n"));
        assert!(s.contains("name: git-commit-formatter"));
        assert!(s.contains("description: Format Git commit messages."));
        assert!(s.contains("## Goal"));
    }

    #[test]
    fn install_records_ownership_in_ledger() {
        let dir = tempdir().unwrap();
        install(dir.path(), &basic_spec("alpha", "myapp")).unwrap();
        let led = ledger_path(dir.path());
        assert!(led.exists());
        assert_eq!(
            ownership::owner_of(&led, "alpha").unwrap().as_deref(),
            Some("myapp")
        );
    }

    #[test]
    fn install_idempotent_on_identical_content() {
        let dir = tempdir().unwrap();
        let s = basic_spec("alpha", "myapp");
        install(dir.path(), &s).unwrap();
        let r = install(dir.path(), &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn install_with_assets_writes_subdirs() {
        let dir = tempdir().unwrap();
        let spec = SkillSpec::builder("alpha")
            .owner("myapp")
            .description("desc")
            .body("body")
            .asset(SkillAsset {
                relative_path: PathBuf::from("scripts/run.sh"),
                bytes: b"#!/bin/sh\necho hi\n".to_vec(),
                executable: true,
            })
            .asset(SkillAsset {
                relative_path: PathBuf::from("references/cheatsheet.md"),
                bytes: b"# Cheatsheet\n".to_vec(),
                executable: false,
            })
            .build();
        install(dir.path(), &spec).unwrap();
        let script = dir.path().join("alpha/scripts/run.sh");
        assert!(script.exists());
        let _ref = dir.path().join("alpha/references/cheatsheet.md");
        assert!(_ref.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&script).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
    }

    #[test]
    fn install_rejects_absolute_asset_path() {
        let dir = tempdir().unwrap();
        let spec = SkillSpec::builder("alpha")
            .owner("myapp")
            .description("desc")
            .body("body")
            .asset(SkillAsset {
                relative_path: PathBuf::from("/etc/passwd"),
                bytes: b"oops".to_vec(),
                executable: false,
            })
            .build();
        let err = install(dir.path(), &spec).unwrap_err();
        assert!(matches!(err, HookerError::Other(_)));
    }

    #[test]
    fn install_rejects_dotdot_asset_path() {
        let dir = tempdir().unwrap();
        let spec = SkillSpec::builder("alpha")
            .owner("myapp")
            .description("desc")
            .body("body")
            .asset(SkillAsset {
                relative_path: PathBuf::from("../escape.txt"),
                bytes: b"oops".to_vec(),
                executable: false,
            })
            .build();
        let err = install(dir.path(), &spec).unwrap_err();
        assert!(matches!(err, HookerError::Other(_)));
    }

    #[test]
    fn uninstall_removes_directory_tree_and_ledger_entry() {
        let dir = tempdir().unwrap();
        let spec = SkillSpec::builder("alpha")
            .owner("myapp")
            .description("desc")
            .body("body")
            .asset(SkillAsset {
                relative_path: PathBuf::from("scripts/x.sh"),
                bytes: b"#!/bin/sh\n".to_vec(),
                executable: true,
            })
            .build();
        install(dir.path(), &spec).unwrap();
        uninstall(dir.path(), "alpha", "myapp").unwrap();
        assert!(!dir.path().join("alpha").exists());
        let led = ledger_path(dir.path());
        // Ledger should be removed entirely once empty.
        assert!(!led.exists());
    }

    #[test]
    fn uninstall_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        install(dir.path(), &basic_spec("alpha", "appA")).unwrap();
        let err = uninstall(dir.path(), "alpha", "appB").unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
        assert!(dir.path().join("alpha").exists());
    }

    #[test]
    fn uninstall_user_installed_skill_refused() {
        let dir = tempdir().unwrap();
        let user_skill = dir.path().join("user-skill");
        fs::create_dir_all(&user_skill).unwrap();
        fs::write(user_skill.join("SKILL.md"), "---\nname: user-skill\n---\n").unwrap();
        let err = uninstall(dir.path(), "user-skill", "myapp").unwrap_err();
        assert!(matches!(
            err,
            HookerError::NotOwnedByCaller { actual: None, .. }
        ));
    }

    #[test]
    fn uninstall_unknown_is_noop() {
        let dir = tempdir().unwrap();
        let r = uninstall(dir.path(), "ghost", "myapp").unwrap();
        assert!(r.not_installed);
    }

    #[test]
    fn frontmatter_with_allowed_tools_serializes() {
        let dir = tempdir().unwrap();
        let spec = SkillSpec::builder("alpha")
            .owner("myapp")
            .description("desc")
            .body("body")
            .allowed_tools(["bash", "edit"])
            .build();
        install(dir.path(), &spec).unwrap();
        let s = fs::read_to_string(dir.path().join("alpha/SKILL.md")).unwrap();
        assert!(s.contains("allowed-tools:\n  - bash\n  - edit\n"));
    }

    #[test]
    fn yaml_quoting_handles_colons_in_description() {
        let dir = tempdir().unwrap();
        let spec = SkillSpec::builder("alpha")
            .owner("myapp")
            .description("Title: subtitle.")
            .body("body")
            .build();
        install(dir.path(), &spec).unwrap();
        let s = fs::read_to_string(dir.path().join("alpha/SKILL.md")).unwrap();
        assert!(
            s.contains(r#"description: "Title: subtitle.""#),
            "got:\n{s}"
        );
    }
}

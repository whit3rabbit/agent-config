//! Richer install-state reporting for hooks, MCP servers, and skills.
//!
//! Where `is_installed` returns a single boolean, [`StatusReport`] captures the
//! distinct realities a planner needs to act on: present-and-owned-by-us,
//! present-but-owned-by-someone-else, present-without-any-ledger-claim,
//! ledger-claim-without-any-on-disk-presence, parse-failed config, etc.
//!
//! Each [`Integration`](crate::Integration), [`McpSurface`](crate::McpSurface),
//! and [`SkillSurface`](crate::SkillSurface) provides a `*_status` method that
//! returns one of these reports. The legacy `is_*_installed` methods are kept
//! as compatibility wrappers — they collapse to `true` for both
//! [`InstallStatus::InstalledOwned`] and [`InstallStatus::InstalledOtherOwner`].

use std::path::{Path, PathBuf};

use crate::error::AgentConfigError;
use crate::util::{fs_atomic, md_block};

/// What kind of install the [`StatusReport`] describes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlanTarget {
    /// A hook entry identified by its consumer tag.
    Hook {
        /// The consumer tag the report was queried for.
        tag: String,
    },
    /// An MCP server identified by name.
    Mcp {
        /// The MCP server name the report was queried for.
        name: String,
    },
    /// A skill identified by name.
    Skill {
        /// The skill name the report was queried for.
        name: String,
    },
    /// An instruction identified by name.
    Instruction {
        /// The instruction name the report was queried for.
        name: String,
    },
}

/// High-level installation state.
///
/// Each variant maps to a single concrete combination of (harness-config
/// presence, agent-config ledger ownership). Callers can match on this directly
/// to choose between install, repair, or skip.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstallStatus {
    /// Not present in either the harness config or the ownership ledger.
    Absent,
    /// Recorded under the caller's owner tag and present in the harness config.
    InstalledOwned {
        /// Owner tag recorded in the ledger. Equals the caller's expected
        /// owner.
        owner: String,
    },
    /// Recorded under a different owner. The caller cannot uninstall this
    /// without forcing a steal; new installs must use a different name.
    InstalledOtherOwner {
        /// Owner tag recorded in the ledger.
        owner: String,
    },
    /// Present in the harness config but no ledger entry claims it. Likely
    /// hand-installed by the user or installed by a tool that does not
    /// participate in the agent-config ownership protocol.
    PresentUnowned,
    /// Recorded in the ledger but missing from the harness config. The most
    /// common cause is that the user (or another tool) deleted the entry
    /// directly without going through `uninstall`.
    LedgerOnly {
        /// Owner tag recorded in the ledger.
        owner: String,
    },
    /// On-disk state is structurally inconsistent in a way that does not fit
    /// the other variants — duplicate entries, an unparseable config, an
    /// incomplete skill directory, etc. Callers should typically treat this
    /// as "needs manual repair before install".
    Drifted {
        /// One or more concrete drift reasons.
        issues: Vec<DriftIssue>,
    },
    /// State could not be determined (e.g., a probe encountered a soft I/O
    /// failure). Reserved for non-fatal cases. Hard errors propagate as
    /// [`AgentConfigError`] instead.
    Unknown,
}

/// One concrete reason a [`StatusReport`] is in the
/// [`InstallStatus::Drifted`] state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DriftIssue {
    /// Ownership ledger contains an entry, but the corresponding config,
    /// directory, or file entry is missing.
    LedgerOnly {
        /// The ledger path that contains the stale entry.
        path: PathBuf,
        /// Owner recorded for the entry, when available.
        owner: Option<String>,
    },
    /// Config, directory, or file entry exists without a matching ownership
    /// ledger entry.
    ConfigOnly {
        /// The unowned config, directory, or file path.
        path: PathBuf,
    },
    /// The ledger owner does not match the owner the caller asked to validate
    /// against.
    OwnerMismatch {
        /// Expected owner tag.
        expected: String,
        /// Actual owner tag from the ledger.
        actual: Option<String>,
        /// Ledger path that recorded the owner.
        path: Option<PathBuf>,
    },
    /// The harness config exists but cannot be parsed or has an unsupported
    /// shape for validation.
    MalformedConfig {
        /// The malformed config path.
        path: PathBuf,
        /// Parser or shape error.
        reason: String,
    },
    /// The agent-config ownership ledger exists but is not valid ledger JSON.
    MalformedLedger {
        /// The malformed ledger path.
        path: PathBuf,
        /// Parser or shape error.
        reason: String,
    },
    /// A backup already exists from an earlier first-touch write.
    BackupCollision {
        /// The existing backup path.
        path: PathBuf,
    },
    /// A backup that validation expected to be present is missing.
    MissingBackup {
        /// The expected backup path.
        path: PathBuf,
    },
    /// A backup exists even though the validated install state does not need
    /// it.
    StaleBackup {
        /// The stale backup path.
        path: PathBuf,
    },
    /// A directory-backed surface is not laid out as expected.
    UnexpectedDirectoryShape {
        /// The path with the unexpected shape.
        path: PathBuf,
        /// Human-readable shape problem.
        reason: String,
    },
    /// A skill directory exists but the required `SKILL.md` manifest is
    /// missing.
    SkillMissingSkillMd {
        /// The skill directory.
        dir: PathBuf,
        /// The expected manifest path.
        missing: PathBuf,
    },
    /// A skill file or symlink resolves outside the skill directory.
    SkillAssetEscapesRoot {
        /// The escaping asset path.
        path: PathBuf,
        /// The skill directory that should contain all assets.
        root: PathBuf,
    },
    /// A known unsupported surface has files present on disk.
    UnsupportedButPresent {
        /// The unsupported path that exists.
        path: PathBuf,
    },
    /// A skill directory exists but the required `SKILL.md` manifest is
    /// missing.
    SkillIncomplete {
        /// The skill directory.
        dir: PathBuf,
        /// The expected manifest path.
        missing: PathBuf,
    },
    /// An instruction file exists but the content hash does not match the
    /// ledger record.
    InstructionContentDrift {
        /// The instruction file path.
        path: PathBuf,
    },
    /// The harness config exists but cannot be parsed (malformed JSON, JSONC,
    /// JSON5, TOML, or YAML).
    InvalidConfig {
        /// The unparseable file.
        path: PathBuf,
        /// The parser error message.
        reason: String,
    },
    /// The same name appears in the harness config more than once.
    /// Defensive — most config formats reject this on parse, but YAML and
    /// JSONC accept duplicate keys silently in some readers.
    MultipleEntries {
        /// The duplicated name.
        name: String,
        /// The number of times it appears.
        count: usize,
    },
}

/// Advisory observations that do not change the [`InstallStatus`] but may
/// matter to a planner.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatusWarning {
    /// A `<config>.bak` file exists. Usually means a previous install was
    /// rolled back or interrupted; the backup is still on disk.
    BackupExists {
        /// The backup path.
        path: PathBuf,
    },
}

/// Per-file check used to populate [`StatusReport::files`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathStatus {
    /// File or directory does not exist.
    Missing {
        /// The probed path.
        path: PathBuf,
    },
    /// File or directory exists.
    Exists {
        /// The probed path.
        path: PathBuf,
    },
    /// File exists but is not in a valid shape (parse failure, wrong type,
    /// missing required member, etc.).
    Invalid {
        /// The probed path.
        path: PathBuf,
        /// Human-readable reason.
        reason: String,
    },
}

/// Full report returned by `Integration::status`, `McpSurface::mcp_status`,
/// and `SkillSurface::skill_status`.
#[must_use]
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StatusReport {
    /// What the report describes.
    pub target: PlanTarget,
    /// High-level state — the field most callers will branch on.
    pub status: InstallStatus,
    /// Path to the harness config the agent inspects (e.g.
    /// `~/.claude/settings.json`, `~/.codex/config.toml`). `None` for
    /// surfaces with no single canonical config file.
    pub config_path: Option<PathBuf>,
    /// Path to the agent-config ownership ledger that backs this surface.
    /// `None` for hook surfaces that embed `_agent_config_tag` markers
    /// directly into the harness config (no separate ledger).
    pub ledger_path: Option<PathBuf>,
    /// Per-file status for paths the report inspected. Useful for rendering
    /// "what would change" planners without re-probing.
    pub files: Vec<PathStatus>,
    /// Advisory observations.
    pub warnings: Vec<StatusWarning>,
}

/// Internal: presence of a single named entry in a harness config file.
///
/// Each `*_status` probe maps the underlying file format (JSON, JSONC, JSON5,
/// TOML, YAML) into this enum so the [`StatusReport`] assembly logic stays
/// uniform.
#[derive(Debug, Clone)]
pub(crate) enum ConfigPresence {
    /// File missing or entry not present.
    Absent,
    /// Entry present exactly once.
    Single,
    /// Entry present more than once (some formats permit duplicates).
    Duplicate {
        /// Number of occurrences.
        count: usize,
    },
    /// Config exists but cannot be parsed. Carries the parser message so the
    /// caller can surface it via [`DriftIssue::InvalidConfig`].
    Invalid {
        /// Human-readable parse error.
        reason: String,
    },
}

impl StatusReport {
    /// Build a report for an MCP server. `recorded_owner` should be
    /// `ownership::owner_of(ledger_path, name)` from the agent.
    pub(crate) fn for_mcp(
        name: &str,
        config_path: PathBuf,
        ledger_path: PathBuf,
        presence: ConfigPresence,
        expected_owner: &str,
        recorded_owner: Option<String>,
    ) -> Self {
        let target = PlanTarget::Mcp {
            name: name.to_string(),
        };
        Self::assemble(
            target,
            Some(config_path),
            Some(ledger_path),
            presence,
            expected_owner,
            recorded_owner,
            Vec::new(),
        )
    }

    /// Build a report for a hook entry that embeds `_agent_config_tag` into the
    /// harness config (no ledger). This shape covers
    /// claude/cursor/gemini/codex/copilot/opencode/windsurf hooks.
    ///
    /// `present_in_config` is true when the array contains an object with
    /// `_agent_config_tag` equal to the caller's tag. Parse failures should be
    /// passed via [`ConfigPresence::Invalid`].
    pub(crate) fn for_tagged_hook(
        tag: &str,
        config_path: PathBuf,
        presence: ConfigPresence,
    ) -> Self {
        let target = PlanTarget::Hook {
            tag: tag.to_string(),
        };
        // For tagged hooks, the tag is the owner; treat presence as
        // owned-by-tag, absence as Absent.
        let mut files = Vec::new();
        let mut warnings = Vec::new();
        let status = match presence {
            ConfigPresence::Single => {
                files.push(PathStatus::Exists {
                    path: config_path.clone(),
                });
                InstallStatus::InstalledOwned {
                    owner: tag.to_string(),
                }
            }
            ConfigPresence::Duplicate { count } => {
                files.push(PathStatus::Exists {
                    path: config_path.clone(),
                });
                InstallStatus::Drifted {
                    issues: vec![DriftIssue::MultipleEntries {
                        name: tag.to_string(),
                        count,
                    }],
                }
            }
            ConfigPresence::Invalid { reason } => {
                files.push(PathStatus::Invalid {
                    path: config_path.clone(),
                    reason: reason.clone(),
                });
                InstallStatus::Drifted {
                    issues: vec![DriftIssue::InvalidConfig {
                        path: config_path.clone(),
                        reason,
                    }],
                }
            }
            ConfigPresence::Absent => {
                if config_path.exists() {
                    files.push(PathStatus::Exists {
                        path: config_path.clone(),
                    });
                } else {
                    files.push(PathStatus::Missing {
                        path: config_path.clone(),
                    });
                }
                check_backup(&config_path, &mut warnings);
                InstallStatus::Absent
            }
        };
        Self {
            target,
            status,
            config_path: Some(config_path),
            ledger_path: None,
            files,
            warnings,
        }
    }

    /// Build a report for a hook surface backed by a per-tag file
    /// (`<root>/<rules_dir>/<tag>.md`, used by cline/roo/antigravity rules).
    pub(crate) fn for_file_hook(tag: &str, file_path: PathBuf) -> Self {
        let target = PlanTarget::Hook {
            tag: tag.to_string(),
        };
        let exists = file_path.exists();
        let mut files = Vec::new();
        let mut warnings = Vec::new();
        let status = if exists {
            files.push(PathStatus::Exists {
                path: file_path.clone(),
            });
            InstallStatus::InstalledOwned {
                owner: tag.to_string(),
            }
        } else {
            files.push(PathStatus::Missing {
                path: file_path.clone(),
            });
            check_backup(&file_path, &mut warnings);
            InstallStatus::Absent
        };
        Self {
            target,
            status,
            config_path: Some(file_path),
            ledger_path: None,
            files,
            warnings,
        }
    }

    /// Build a report for a prompt-rule hook stored as an AGENT-CONFIG fenced
    /// markdown block inside a shared file.
    pub(crate) fn for_markdown_block_hook(
        tag: &str,
        file_path: PathBuf,
    ) -> Result<Self, AgentConfigError> {
        let target = PlanTarget::Hook {
            tag: tag.to_string(),
        };
        let exists = file_path.exists();
        let mut files = Vec::new();
        let mut warnings = Vec::new();

        let status = if exists {
            let host = fs_atomic::read_to_string_or_empty(&file_path)?;
            if md_block::malformed(&host, tag) {
                files.push(PathStatus::Invalid {
                    path: file_path.clone(),
                    reason: "malformed agent-config markdown fence".into(),
                });
                InstallStatus::Drifted {
                    issues: vec![DriftIssue::MalformedConfig {
                        path: file_path.clone(),
                        reason: "malformed agent-config markdown fence".into(),
                    }],
                }
            } else {
                files.push(PathStatus::Exists {
                    path: file_path.clone(),
                });
                if md_block::contains(&host, tag) {
                    InstallStatus::InstalledOwned {
                        owner: tag.to_string(),
                    }
                } else {
                    InstallStatus::Absent
                }
            }
        } else {
            files.push(PathStatus::Missing {
                path: file_path.clone(),
            });
            check_backup(&file_path, &mut warnings);
            InstallStatus::Absent
        };

        Ok(Self {
            target,
            status,
            config_path: Some(file_path),
            ledger_path: None,
            files,
            warnings,
        })
    }

    /// Build a report for a skill. The skill is "present in config" when its
    /// directory exists; the report adds [`DriftIssue::SkillIncomplete`] when
    /// the directory exists but lacks `SKILL.md`.
    pub(crate) fn for_skill(
        name: &str,
        skill_dir: PathBuf,
        manifest_path: PathBuf,
        ledger_path: PathBuf,
        expected_owner: &str,
        recorded_owner: Option<String>,
    ) -> Self {
        let target = PlanTarget::Skill {
            name: name.to_string(),
        };
        let dir_exists = skill_dir.exists();
        let manifest_exists = manifest_path.exists();
        let mut extra_drift = Vec::new();
        let presence = if dir_exists {
            if !manifest_exists {
                extra_drift.push(DriftIssue::SkillIncomplete {
                    dir: skill_dir.clone(),
                    missing: manifest_path.clone(),
                });
            }
            ConfigPresence::Single
        } else {
            ConfigPresence::Absent
        };

        let mut report = Self::assemble(
            target,
            Some(skill_dir.clone()),
            Some(ledger_path),
            presence,
            expected_owner,
            recorded_owner,
            extra_drift,
        );

        // Replace the auto-generated PathStatus for skill_dir with finer-grained
        // entries, and add a manifest-level entry.
        report.files.clear();
        if dir_exists {
            report.files.push(PathStatus::Exists {
                path: skill_dir.clone(),
            });
            report.files.push(if manifest_exists {
                PathStatus::Exists {
                    path: manifest_path,
                }
            } else {
                PathStatus::Missing {
                    path: manifest_path,
                }
            });
        } else {
            report.files.push(PathStatus::Missing { path: skill_dir });
            report.files.push(PathStatus::Missing {
                path: manifest_path,
            });
        }
        report
    }

    /// Build a report for an instruction. The instruction is "present in
    /// config" when its file exists on disk.
    pub(crate) fn for_instruction(
        name: &str,
        instruction_path: PathBuf,
        ledger_path: PathBuf,
        presence: ConfigPresence,
        expected_owner: &str,
        recorded_owner: Option<String>,
    ) -> Self {
        let target = PlanTarget::Instruction {
            name: name.to_string(),
        };
        Self::assemble(
            target,
            Some(instruction_path),
            Some(ledger_path),
            presence,
            expected_owner,
            recorded_owner,
            Vec::new(),
        )
    }

    /// Common assembly for ledger-backed surfaces (MCP, skills).
    /// `extra_drift` is folded into a `Drifted` status when non-empty,
    /// otherwise the ledger/config combination determines the variant.
    fn assemble(
        target: PlanTarget,
        config_path: Option<PathBuf>,
        ledger_path: Option<PathBuf>,
        presence: ConfigPresence,
        expected_owner: &str,
        recorded_owner: Option<String>,
        mut extra_drift: Vec<DriftIssue>,
    ) -> Self {
        let mut files = Vec::new();
        let mut warnings = Vec::new();

        if let Some(p) = config_path.as_ref() {
            files.push(if p.exists() {
                PathStatus::Exists { path: p.clone() }
            } else {
                PathStatus::Missing { path: p.clone() }
            });
        }
        if let Some(p) = ledger_path.as_ref() {
            files.push(if p.exists() {
                PathStatus::Exists { path: p.clone() }
            } else {
                PathStatus::Missing { path: p.clone() }
            });
        }

        // Map (presence, recorded_owner) into InstallStatus, deferring drift
        // when callers have already accumulated reasons (e.g. SkillIncomplete).
        let mut status = match (&presence, recorded_owner.as_deref()) {
            (ConfigPresence::Invalid { reason }, _) => {
                if let Some(p) = config_path.as_ref() {
                    if let Some(slot) = files
                        .iter_mut()
                        .find(|f| matches!(f, PathStatus::Exists { path } if path == p))
                    {
                        *slot = PathStatus::Invalid {
                            path: p.clone(),
                            reason: reason.clone(),
                        };
                    }
                }
                let mut issues = std::mem::take(&mut extra_drift);
                issues.push(DriftIssue::InvalidConfig {
                    path: config_path.clone().unwrap_or_default(),
                    reason: reason.clone(),
                });
                InstallStatus::Drifted { issues }
            }
            (ConfigPresence::Duplicate { count }, _) => {
                let target_name = match &target {
                    PlanTarget::Hook { tag } => tag.clone(),
                    PlanTarget::Mcp { name }
                    | PlanTarget::Skill { name }
                    | PlanTarget::Instruction { name } => name.clone(),
                };
                let mut issues = std::mem::take(&mut extra_drift);
                issues.push(DriftIssue::MultipleEntries {
                    name: target_name,
                    count: *count,
                });
                InstallStatus::Drifted { issues }
            }
            (ConfigPresence::Single, Some(owner)) if owner == expected_owner => {
                InstallStatus::InstalledOwned {
                    owner: owner.to_string(),
                }
            }
            (ConfigPresence::Single, Some(owner)) => InstallStatus::InstalledOtherOwner {
                owner: owner.to_string(),
            },
            (ConfigPresence::Single, None) => InstallStatus::PresentUnowned,
            (ConfigPresence::Absent, Some(owner)) => InstallStatus::LedgerOnly {
                owner: owner.to_string(),
            },
            (ConfigPresence::Absent, None) => InstallStatus::Absent,
        };

        // Fold any accumulated extra drift into a Drifted status that
        // wasn't already escalated by the match arms above.
        if !extra_drift.is_empty() {
            let mut issues = extra_drift;
            if let InstallStatus::Drifted { issues: existing } = &mut status {
                std::mem::swap(existing, &mut issues);
                existing.extend(issues);
            } else {
                status = InstallStatus::Drifted { issues };
            }
        }

        if matches!(status, InstallStatus::Absent) {
            if let Some(p) = config_path.as_ref() {
                check_backup(p, &mut warnings);
            }
        }

        Self {
            target,
            status,
            config_path,
            ledger_path,
            files,
            warnings,
        }
    }
}

/// Push a `BackupExists` warning if `<path>.bak` is on disk.
fn check_backup(path: &Path, warnings: &mut Vec<StatusWarning>) {
    let mut bak = path.to_path_buf();
    let name = bak
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    let mut name = name.into_string().unwrap_or_default();
    if name.is_empty() {
        return;
    }
    name.push_str(".bak");
    bak.set_file_name(name);
    if bak.exists() {
        warnings.push(StatusWarning::BackupExists { path: bak });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn for_mcp_owned_when_owner_matches() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("mcp.json");
        let led = dir.path().join(".agent-config-mcp.json");
        std::fs::write(&cfg, b"{}").unwrap();
        std::fs::write(&led, b"{}").unwrap();
        let r = StatusReport::for_mcp(
            "github",
            cfg.clone(),
            led,
            ConfigPresence::Single,
            "myapp",
            Some("myapp".into()),
        );
        assert!(matches!(
            r.status,
            InstallStatus::InstalledOwned { ref owner } if owner == "myapp"
        ));
        assert_eq!(
            r.target,
            PlanTarget::Mcp {
                name: "github".into()
            }
        );
    }

    #[test]
    fn for_mcp_other_owner_when_recorded_differs() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("mcp.json");
        let led = dir.path().join(".agent-config-mcp.json");
        let r = StatusReport::for_mcp(
            "github",
            cfg,
            led,
            ConfigPresence::Single,
            "myapp",
            Some("otherapp".into()),
        );
        assert!(matches!(
            r.status,
            InstallStatus::InstalledOtherOwner { ref owner } if owner == "otherapp"
        ));
    }

    #[test]
    fn for_mcp_present_unowned_when_no_ledger_record() {
        let dir = tempdir().unwrap();
        let r = StatusReport::for_mcp(
            "github",
            dir.path().join("mcp.json"),
            dir.path().join("ledger.json"),
            ConfigPresence::Single,
            "myapp",
            None,
        );
        assert!(matches!(r.status, InstallStatus::PresentUnowned));
    }

    #[test]
    fn for_mcp_ledger_only_when_config_absent() {
        let dir = tempdir().unwrap();
        let r = StatusReport::for_mcp(
            "github",
            dir.path().join("mcp.json"),
            dir.path().join("ledger.json"),
            ConfigPresence::Absent,
            "myapp",
            Some("myapp".into()),
        );
        assert!(matches!(
            r.status,
            InstallStatus::LedgerOnly { ref owner } if owner == "myapp"
        ));
    }

    #[test]
    fn for_mcp_absent_when_neither_present() {
        let dir = tempdir().unwrap();
        let r = StatusReport::for_mcp(
            "github",
            dir.path().join("mcp.json"),
            dir.path().join("ledger.json"),
            ConfigPresence::Absent,
            "myapp",
            None,
        );
        assert!(matches!(r.status, InstallStatus::Absent));
    }

    #[test]
    fn for_mcp_drifted_on_invalid_config() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("mcp.json");
        std::fs::write(&cfg, b"{not valid").unwrap();
        let r = StatusReport::for_mcp(
            "github",
            cfg.clone(),
            dir.path().join("ledger.json"),
            ConfigPresence::Invalid {
                reason: "expected `:` at line 1".into(),
            },
            "myapp",
            None,
        );
        let issues = match &r.status {
            InstallStatus::Drifted { issues } => issues,
            other => panic!("expected Drifted, got {other:?}"),
        };
        assert!(matches!(issues[0], DriftIssue::InvalidConfig { .. }));
    }

    #[test]
    fn for_skill_incomplete_when_manifest_missing() {
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("alpha");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let manifest = skill_dir.join("SKILL.md");
        let r = StatusReport::for_skill(
            "alpha",
            skill_dir,
            manifest,
            dir.path().join("ledger.json"),
            "myapp",
            Some("myapp".into()),
        );
        let issues = match &r.status {
            InstallStatus::Drifted { issues } => issues,
            other => panic!("expected Drifted, got {other:?}"),
        };
        assert!(matches!(issues[0], DriftIssue::SkillIncomplete { .. }));
    }

    #[test]
    fn backup_warning_emitted_when_bak_exists() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("mcp.json");
        std::fs::write(dir.path().join("mcp.json.bak"), b"{}").unwrap();
        let r = StatusReport::for_mcp(
            "github",
            cfg,
            dir.path().join("ledger.json"),
            ConfigPresence::Absent,
            "myapp",
            None,
        );
        assert!(r
            .warnings
            .iter()
            .any(|w| matches!(w, StatusWarning::BackupExists { .. })));
    }

    #[test]
    fn tagged_hook_owned_when_present() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        std::fs::write(&cfg, b"{}").unwrap();
        let r = StatusReport::for_tagged_hook("alpha", cfg, ConfigPresence::Single);
        assert!(matches!(
            r.status,
            InstallStatus::InstalledOwned { ref owner } if owner == "alpha"
        ));
        assert!(r.ledger_path.is_none());
    }

    #[test]
    fn tagged_hook_drifted_on_invalid_config() {
        let dir = tempdir().unwrap();
        let r = StatusReport::for_tagged_hook(
            "alpha",
            dir.path().join("settings.json"),
            ConfigPresence::Invalid {
                reason: "broken".into(),
            },
        );
        assert!(matches!(r.status, InstallStatus::Drifted { .. }));
    }

    #[test]
    fn file_hook_present_when_path_exists() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("alpha.md");
        std::fs::write(&p, b"x").unwrap();
        let r = StatusReport::for_file_hook("alpha", p);
        assert!(matches!(r.status, InstallStatus::InstalledOwned { .. }));
    }

    #[test]
    fn file_hook_absent_when_missing() {
        let dir = tempdir().unwrap();
        let r = StatusReport::for_file_hook("alpha", dir.path().join("alpha.md"));
        assert!(matches!(r.status, InstallStatus::Absent));
    }
}

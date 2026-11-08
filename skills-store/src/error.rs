//! User-facing errors, rendered exactly like the upstream `skills` CLI's
//! diagnostics: `[SKILLS_CODE] why` followed by optional `fix:` lines.

use std::fmt;

/// A user-facing error or warning with a stable diagnostic code.
///
/// `Display` reproduces the upstream ANSI rendering: a bold-red `[CODE]`
/// prefix, the `why` message, and any `fix` guidance on `├▶` / `╰▶` lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsError {
    /// Stable diagnostic code, e.g. `SKILLS_INVALID_AGENTS`.
    pub code: &'static str,
    /// Why this failed.
    pub why: String,
    /// Actionable instructions, rendered on its own line.
    pub fix: Option<String>,
}

impl SkillsError {
    /// Build an error from a code and message.
    pub fn new(code: &'static str, why: impl Into<String>) -> Self {
        Self {
            code,
            why: why.into(),
            fix: None,
        }
    }

    /// Attach fix guidance.
    #[must_use]
    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    /// The plain (ANSI-free) rendering, matching `formatError` without colors.
    #[must_use]
    pub fn plain(&self) -> String {
        match &self.fix {
            Some(fix) => format!("[{}] {}\n╰▶ fix: {}", self.code, self.why, fix),
            None => format!("[{}] {}", self.code, self.why),
        }
    }

    /// Render with the upstream ANSI decoration (bold-red code, dim fix marker).
    #[must_use]
    pub fn ansi(&self) -> String {
        let header = format!("\x1b[1m\x1b[31m[{}]\x1b[0m\x1b[0m {}", self.code, self.why);
        match &self.fix {
            Some(fix) => format!("{header}\n\x1b[2m╰▶\x1b[0m \x1b[2mfix:\x1b[0m {fix}"),
            None => header,
        }
    }
}

impl fmt::Display for SkillsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.plain())
    }
}

impl std::error::Error for SkillsError {}

/// Diagnostic constructors. Messages mirror `src/diagnostics.ts` verbatim.
pub mod diag {
    use super::SkillsError;

    /// Helper: `Available skills:` list used by several fixes.
    fn available_skills(names: &[String]) -> String {
        if names.is_empty() {
            String::new()
        } else {
            let mut out = String::from("Available skills:");
            for name in names {
                out.push_str(&format!("\n  - {name}"));
            }
            out
        }
    }

    /// `SKILLS_INVALID_AGENTS`
    pub fn invalid_agents(invalid: &[String], valid: &[String]) -> SkillsError {
        SkillsError::new(
            "SKILLS_INVALID_AGENTS",
            format!("Invalid agents: {}", invalid.join(", ")),
        )
        .with_fix(format!("Valid agents: {}", valid.join(", ")))
    }

    /// `SKILLS_OPTION_REQUIRES_VALUE`
    pub fn option_requires_value(option: &str, value: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_OPTION_REQUIRES_VALUE",
            format!("{option} requires {value}"),
        )
    }

    /// `SKILLS_UNKNOWN_OPTION`
    pub fn unknown_option(option: &str) -> SkillsError {
        SkillsError::new("SKILLS_UNKNOWN_OPTION", format!("Unknown option: {option}"))
    }

    /// `SKILLS_INVALID_OPTION_VALUE`
    pub fn invalid_option_value(option: &str, expected: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_INVALID_OPTION_VALUE",
            format!("{option} must be {expected}"),
        )
    }

    /// `SKILLS_DUPLICATE_OPTION`
    pub fn duplicate_option(option: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_DUPLICATE_OPTION",
            format!("Only one {option} value can be provided"),
        )
    }

    /// `SKILLS_REMOVE_ALL_WITH_NAMES`
    pub fn remove_all_with_names(example: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_REMOVE_ALL_WITH_NAMES",
            "Cannot combine --all with specific skill names.",
        )
        .with_fix(format!(
            "Use `skills remove --all` to remove every skill, or omit --all to remove only the named skills.\
             \n   Example: skills remove {example} -y"
        ))
    }

    /// `SKILLS_PROMPT_REQUIRES_TTY`
    pub fn prompt_requires_tty() -> SkillsError {
        SkillsError::new(
            "SKILLS_PROMPT_REQUIRES_TTY",
            "Interactive prompt required but stdin is not a TTY. Nothing was installed. \
             Use --agent <name> (or --agent '*') and -y to run non-interactively.",
        )
    }

    /// `SKILLS_MISSING_ARGUMENT`
    pub fn missing_argument(name: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_MISSING_ARGUMENT",
            format!("Missing required argument: {name}"),
        )
    }

    /// `SKILLS_MULTIPLE_SOURCES`
    pub fn multiple_sources(received: &[String]) -> SkillsError {
        SkillsError::new(
            "SKILLS_MULTIPLE_SOURCES",
            format!(
                "Expected one source, received {}: {}",
                received.len(),
                received.join(", ")
            ),
        )
    }

    /// `SKILLS_UNKNOWN_COMMAND`
    pub fn unknown_command(command: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_UNKNOWN_COMMAND",
            format!("Unknown command: {command}"),
        )
        .with_fix("Run skills --help for usage.")
    }

    /// `SKILLS_REMOVE_GLOBALLY_TRACKED`
    pub fn remove_globally_tracked(skills: &[String]) -> SkillsError {
        SkillsError::new(
            "SKILLS_REMOVE_GLOBALLY_TRACKED",
            format!("Tracked in the global lock: {}", skills.join(", ")),
        )
        .with_fix(format!(
            "Remove globally with: skills remove {} -g",
            skills.join(" ")
        ))
    }

    /// `SKILLS_UNSAFE_SUBPATH`
    pub fn unsafe_subpath(subpath: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_UNSAFE_SUBPATH",
            format!(
                "Unsafe subpath: \"{subpath}\" contains path traversal segments. \
                 Subpaths must not contain \"..\" components."
            ),
        )
    }

    /// `SKILLS_SUBPATH_OUTSIDE_REPO`
    pub fn subpath_outside_repo(subpath: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_SUBPATH_OUTSIDE_REPO",
            format!(
                "Invalid subpath: \"{subpath}\" resolves outside the repository directory. \
                 Subpath must not contain \"..\" segments that escape the base path."
            ),
        )
    }

    /// `SKILLS_NO_MATCHING_SKILLS`
    pub fn no_matching_skills(requested: &[String], available: &[String]) -> SkillsError {
        let mut err = SkillsError::new(
            "SKILLS_NO_MATCHING_SKILLS",
            format!("No matching skills found for: {}", requested.join(", ")),
        );
        let fix = available_skills(available);
        if !fix.is_empty() {
            err = err.with_fix(fix);
        }
        err
    }

    /// `SKILLS_NO_VALID_SKILLS`
    pub fn no_valid_skills() -> SkillsError {
        SkillsError::new(
            "SKILLS_NO_VALID_SKILLS",
            "No valid skills found. Skills require a SKILL.md with name and description.",
        )
    }

    /// `SKILLS_MULTIPLE_SKILLS_IN_SOURCE`
    pub fn multiple_skills_in_source(names: &[String]) -> SkillsError {
        let mut why =
            String::from("This source contains multiple skills. Specify exactly one skill:");
        for name in names {
            why.push_str(&format!("\n  - {name}"));
        }
        SkillsError::new("SKILLS_MULTIPLE_SKILLS_IN_SOURCE", why)
    }

    /// `SKILLS_AMBIGUOUS_SKILL_SELECTOR`
    pub fn ambiguous_skill_selector(selector: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_AMBIGUOUS_SKILL_SELECTOR",
            format!("Skill selector \"{selector}\" matched multiple skills."),
        )
    }

    /// `SKILLS_CONFLICTING_SKILL_SELECTORS`
    pub fn conflicting_skill_selectors(from_source: &str, from_option: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_CONFLICTING_SKILL_SELECTORS",
            format!(
                "Conflicting skill selectors: source selects \"{from_source}\" but --skill selects \"{from_option}\". Provide one selector."
            ),
        )
    }

    /// `SKILLS_LOCAL_PATH_NOT_FOUND`
    pub fn local_path_not_found(path: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_LOCAL_PATH_NOT_FOUND",
            format!("Local path does not exist: {path}"),
        )
    }

    /// `SKILLS_WELLKNOWN_SCOPE_NOT_FOUND`
    pub fn wellknown_scope_not_found(scope_path: &str, root_url: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_WELLKNOWN_SCOPE_NOT_FOUND",
            format!(
                "No skills found for the scoped path '{scope_path}' on {root_url}. \
                 Not falling back to the root skills index because that would install every skill the host publishes. \
                 Check the URL, or run 'skills add {root_url}' to install from the root index."
            ),
        )
    }

    /// `SKILLS_WELLKNOWN_DIGEST_MISMATCH`
    pub fn wellknown_digest_mismatch(skill: &str, expected: &str, actual: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_WELLKNOWN_DIGEST_MISMATCH",
            format!("Digest mismatch for skill '{skill}': expected {expected}, received {actual}."),
        )
    }

    /// `SKILLS_WELLKNOWN_INDEX_NOT_FOUND`
    pub fn wellknown_index_not_found() -> SkillsError {
        SkillsError::new(
            "SKILLS_WELLKNOWN_INDEX_NOT_FOUND",
            "No skills found at this URL. Make sure the server has a /.well-known/agent-skills/index.json or /.well-known/skills/index.json file.",
        )
    }

    /// `SKILLS_SKILL_SKIPPED`
    pub fn skill_skipped(path: &str, reason: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_SKILL_SKIPPED",
            format!("⚠ Skipped {path} — {reason}"),
        )
    }

    /// `SKILLS_UNSUPPORTED_GIT_TRANSPORT`
    pub fn unsupported_git_transport(transport: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_UNSUPPORTED_GIT_TRANSPORT",
            format!("Unsupported Git transport: {transport}"),
        )
    }

    /// `SKILLS_GIT_CLONE_TIMEOUT`
    pub fn git_clone_timeout(seconds: u64) -> SkillsError {
        SkillsError::new(
            "SKILLS_GIT_CLONE_TIMEOUT",
            format!(
                "Clone timed out after {seconds}s. Common causes:\n\
                 \x20 - Large repository: raise the timeout with SKILLS_CLONE_TIMEOUT_MS=600000 (10m)\n\
                 \x20 - Slow network: retry, or clone manually and pass the local path to 'skills add'\n\
                 \x20 - Private repo without credentials: ensure auth is configured\n\
                 \x20\x20\x20\x20 - For SSH: ssh-add -l (to check loaded keys)\n\
                 \x20\x20\x20\x20 - For HTTPS: gh auth status (if using GitHub CLI)"
            ),
        )
    }

    /// `SKILLS_GIT_AUTH_FAILED`
    pub fn git_auth_failed(url: &str, ssh_url: Option<&str>, host: &str) -> SkillsError {
        let why = match ssh_url {
            Some(ssh_url) => format!(
                "Authentication failed for {url}.\n\
                 \x20 - For private repos, ensure you have access\n\
                 \x20 - Retry with SSH: npx skills add {ssh_url}\n\
                 \x20 - Check access with: gh auth status -h {host} or ssh -T git@{host}"
            ),
            None => format!(
                "Authentication failed for {url}.\n\
                 \x20 - For private repos, ensure you have access\n\
                 \x20 - For SSH: Check your keys with 'ssh -T git@github.com'\n\
                 \x20 - For HTTPS: Run 'gh auth login' or configure git credentials"
            ),
        };
        SkillsError::new("SKILLS_GIT_AUTH_FAILED", why)
    }

    /// `SKILLS_GIT_SAML_SSO_BLOCKED`
    pub fn git_saml_sso_blocked(url: &str, ssh_url: &str, host: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_GIT_SAML_SSO_BLOCKED",
            format!(
                "GitHub blocked HTTPS access to {url} because the organization enforces SAML SSO.\n\
                 \x20 skills tried your existing git credentials and available fallbacks, but none succeeded.\n\
                 \x20 - Re-authorize your GitHub credentials/app for that org's SSO policy\n\
                 \x20 - Or rerun with SSH: npx skills add {ssh_url}\n\
                 \x20 - Verify access with: gh auth status -h {host} or ssh -T git@{host}"
            ),
        )
    }

    /// `SKILLS_GIT_CLONE_FAILED`
    pub fn git_clone_failed(url: &str, detail: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_GIT_CLONE_FAILED",
            format!("Failed to clone {url}: {detail}"),
        )
    }

    /// `SKILLS_DOWNLOAD_HTTP_ERROR`
    pub fn download_http_error(status: u16) -> SkillsError {
        SkillsError::new(
            "SKILLS_DOWNLOAD_HTTP_ERROR",
            format!("Download failed with HTTP {status}"),
        )
    }

    /// `SKILLS_DOWNLOAD_TOO_LARGE`
    pub fn download_too_large(max_bytes: usize) -> SkillsError {
        SkillsError::new(
            "SKILLS_DOWNLOAD_TOO_LARGE",
            format!("Download is larger than {max_bytes} bytes. Set SKILLS_DOWNLOAD_MAX_BYTES to override."),
        )
    }

    /// `SKILLS_DOWNLOAD_NO_BODY`
    pub fn download_no_body() -> SkillsError {
        SkillsError::new("SKILLS_DOWNLOAD_NO_BODY", "Download response has no body")
    }

    /// `SKILLS_DOWNLOAD_EMPTY`
    pub fn download_empty() -> SkillsError {
        SkillsError::new("SKILLS_DOWNLOAD_EMPTY", "Downloaded URL is empty")
    }

    /// `SKILLS_DOWNLOAD_NOT_A_SKILL`
    pub fn download_not_a_skill() -> SkillsError {
        SkillsError::new(
            "SKILLS_DOWNLOAD_NOT_A_SKILL",
            "Downloaded URL is not a valid SKILL.md file or supported archive",
        )
    }

    /// `SKILLS_DOWNLOAD_NOT_LOCKED`
    pub fn download_not_locked(url: &str, skills: &[String]) -> SkillsError {
        SkillsError::new(
            "SKILLS_DOWNLOAD_NOT_LOCKED",
            format!(
                "Not recording {} in the lock file: {url} has a query string or credentials, which may be secret",
                skills.join(", ")
            ),
        )
        .with_fix("To let `skills update` fetch it again, serve it from a URL without them.")
    }

    /// `SKILLS_ARCHIVE_INVALID`
    pub fn archive_invalid(reason: &str) -> SkillsError {
        SkillsError::new("SKILLS_ARCHIVE_INVALID", reason)
    }

    /// `SKILLS_ARCHIVE_MULTI_DISK_ZIP`
    pub fn archive_multi_disk_zip() -> SkillsError {
        SkillsError::new(
            "SKILLS_ARCHIVE_MULTI_DISK_ZIP",
            "Multi-disk zip archives are not supported",
        )
    }

    /// `SKILLS_ARCHIVE_UNSAFE_PATH`
    pub fn archive_unsafe_path(path: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_ARCHIVE_UNSAFE_PATH",
            format!("Archive contains unsafe path: {path}"),
        )
    }

    /// `SKILLS_ARCHIVE_ENCRYPTED_ENTRY`
    pub fn archive_encrypted_entry() -> SkillsError {
        SkillsError::new(
            "SKILLS_ARCHIVE_ENCRYPTED_ENTRY",
            "Encrypted zip entries are not supported",
        )
    }

    /// `SKILLS_ARCHIVE_LINK_ENTRY`
    pub fn archive_link_entry() -> SkillsError {
        SkillsError::new(
            "SKILLS_ARCHIVE_LINK_ENTRY",
            "Archive links are not supported",
        )
    }

    /// `SKILLS_ARCHIVE_TOO_MANY_FILES`
    pub fn archive_too_many_files(
        count: usize,
        max: usize,
        override_env: Option<&str>,
    ) -> SkillsError {
        let mut why = format!("Archive contains too many files ({count}). Maximum is {max}.");
        if let Some(env) = override_env {
            why.push_str(&format!(" Set {env} to override."));
        }
        SkillsError::new("SKILLS_ARCHIVE_TOO_MANY_FILES", why)
    }

    /// `SKILLS_ARCHIVE_TOO_LARGE`
    pub fn archive_too_large(max_bytes: usize, override_env: Option<&str>) -> SkillsError {
        let mut why = format!("Archive extracts to more than {max_bytes} bytes.");
        if let Some(env) = override_env {
            why.push_str(&format!(" Set {env} to override."));
        }
        SkillsError::new("SKILLS_ARCHIVE_TOO_LARGE", why)
    }

    /// `SKILLS_ARCHIVE_UNSUPPORTED_COMPRESSION`
    pub fn archive_unsupported_compression(method: u16) -> SkillsError {
        SkillsError::new(
            "SKILLS_ARCHIVE_UNSUPPORTED_COMPRESSION",
            format!("Unsupported zip compression method: {method}"),
        )
    }

    /// `SKILLS_ARCHIVE_UNSUPPORTED_FORMAT`
    pub fn archive_unsupported_format() -> SkillsError {
        SkillsError::new(
            "SKILLS_ARCHIVE_UNSUPPORTED_FORMAT",
            "Unsupported archive format",
        )
    }

    /// `SKILLS_ARCHIVE_MISSING_SKILL_MD`
    pub fn archive_missing_skill_md() -> SkillsError {
        SkillsError::new(
            "SKILLS_ARCHIVE_MISSING_SKILL_MD",
            "Archive missing root SKILL.md",
        )
    }

    /// `SKILLS_WELLKNOWN_ARCHIVE_UNSAFE_PATH`
    pub fn wellknown_archive_unsafe_path(path: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_WELLKNOWN_ARCHIVE_UNSAFE_PATH",
            format!("Unsafe archive path: {path}"),
        )
    }

    /// `SKILLS_WELLKNOWN_ARCHIVE_TOO_LARGE`
    pub fn wellknown_archive_too_large() -> SkillsError {
        SkillsError::new(
            "SKILLS_WELLKNOWN_ARCHIVE_TOO_LARGE",
            "Archive exceeds maximum unpacked size",
        )
    }

    /// `SKILLS_WELLKNOWN_ARCHIVE_TOO_MANY_FILES`
    pub fn wellknown_archive_too_many_files() -> SkillsError {
        SkillsError::new(
            "SKILLS_WELLKNOWN_ARCHIVE_TOO_MANY_FILES",
            "Archive contains too many files",
        )
    }

    /// `SKILLS_UNSAFE_SKILL_NAME`
    pub fn unsafe_skill_name() -> SkillsError {
        SkillsError::new(
            "SKILLS_UNSAFE_SKILL_NAME",
            "Invalid skill name: potential path traversal detected",
        )
    }

    /// `SKILLS_CLEANUP_OUTSIDE_TEMP_DIR`
    pub fn cleanup_outside_temp_dir() -> SkillsError {
        SkillsError::new(
            "SKILLS_CLEANUP_OUTSIDE_TEMP_DIR",
            "Attempted to clean up directory outside of temp directory",
        )
    }

    /// `SKILLS_BROKEN_SYMLINK`
    pub fn broken_symlink(path: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_BROKEN_SYMLINK",
            format!("Skipping broken symlink: {path}"),
        )
    }

    /// `SKILLS_LOCK_MISSING_SOURCE_URL`
    pub fn lock_missing_source_url(skill: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_LOCK_MISSING_SOURCE_URL",
            format!(
                "Cannot restore {skill}: skills-lock.json is missing sourceUrl for this generic Git source"
            ),
        )
    }

    /// `SKILLS_INVALID_LOCAL_LOCK`
    pub fn invalid_local_lock(path: &str, reason: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_INVALID_LOCAL_LOCK",
            format!("Invalid {path}: {reason}. Fix or delete the file, then retry."),
        )
    }

    /// `SKILLS_SYMLINK_OUTSIDE_SKILL`
    pub fn symlink_outside_skill(path: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_SYMLINK_OUTSIDE_SKILL",
            format!("Skipping symlink that points outside the skill directory: {path}"),
        )
    }

    /// `SKILLS_SYMLINK_CYCLE`
    pub fn symlink_cycle(path: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_SYMLINK_CYCLE",
            format!("Skipping symlink that creates a directory cycle: {path}"),
        )
    }

    /// `SKILLS_NOTION_CLI_MISSING`
    pub fn notion_cli_missing() -> SkillsError {
        SkillsError::new(
            "SKILLS_NOTION_CLI_MISSING",
            "Notion CLI (ntn) is required. Install it from:\nhttps://developers.notion.com/cli/get-started/overview\nThen run `ntn login`.",
        )
    }

    /// `SKILLS_USE_AGENT_UNSUPPORTED`
    pub fn use_agent_unsupported(agent: &str, supported: &[String]) -> SkillsError {
        SkillsError::new(
            "SKILLS_USE_AGENT_UNSUPPORTED",
            format!("Running {agent} is not supported yet."),
        )
        .with_fix(format!(
            "Supported agents for skills use --agent: {}",
            supported.join(", ")
        ))
    }

    /// `SKILLS_USE_AGENT_COMMAND_NOT_FOUND`
    pub fn use_agent_command_not_found(agent: &str, command: &str) -> SkillsError {
        SkillsError::new(
            "SKILLS_USE_AGENT_COMMAND_NOT_FOUND",
            format!("Could not launch {agent}: command not found: {command}"),
        )
    }

    /// `SKILLS_USE_AGENT_WILDCARD`
    pub fn use_agent_wildcard() -> SkillsError {
        SkillsError::new(
            "SKILLS_USE_AGENT_WILDCARD",
            "skills use --agent does not support '*'; specify exactly one agent.",
        )
    }

    /// `SKILLS_USE_MULTIPLE_AGENTS`
    pub fn use_multiple_agents() -> SkillsError {
        SkillsError::new(
            "SKILLS_USE_MULTIPLE_AGENTS",
            "skills use --agent accepts exactly one agent.",
        )
    }

    /// `SKILLS_USE_NO_MATCHING_SKILL`
    pub fn use_no_matching_skill(selector: &str, available: &[String]) -> SkillsError {
        let mut err = SkillsError::new(
            "SKILLS_USE_NO_MATCHING_SKILL",
            format!("No matching skill found for: {selector}"),
        );
        let fix = available_skills(available);
        if !fix.is_empty() {
            err = err.with_fix(fix);
        }
        err
    }
}

//! Git clone plumbing ported from `src/git.ts`: depth-1 clones with ref and
//! commit-SHA fallbacks, `gh` CLI retry for GitHub HTTPS auth failures, and
//! SSH fallback — shelling out to `git` with the upstream env hardening.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use skills_store::error::{diag, SkillsError};

const DEFAULT_CLONE_TIMEOUT_MS: u64 = 300_000;
const ALLOWED_GIT_PROTOCOLS: &str = "https:http:ssh:git:file";

fn clone_timeout() -> Duration {
    let Some(raw) = std::env::var("SKILLS_CLONE_TIMEOUT_MS").ok().filter(|v| !v.is_empty())
    else {
        return Duration::from_millis(DEFAULT_CLONE_TIMEOUT_MS);
    };
    raw.parse::<u64>()
        .ok()
        .map(Duration::from_millis)
        .filter(|d| !d.is_zero())
        .unwrap_or_else(|| Duration::from_millis(DEFAULT_CLONE_TIMEOUT_MS))
}

/// Whether a ref is a full 40-char Git commit SHA.
#[must_use]
pub fn is_commit_sha(ref_: &str) -> bool {
    ref_.len() == 40 && ref_.chars().all(|c| c.is_ascii_hexdigit())
}

/// Whether an error message means the requested ref is missing upstream.
fn is_missing_ref_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    (lower.contains("remote branch") && lower.contains("not found in upstream origin"))
        || lower.contains("couldn't find remote ref")
        || lower.contains("upload-pack: not our ref")
}

fn is_github_sso_auth_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("saml sso")
        || lower.contains("enforced sso")
        || lower.contains("enabled or enforced saml")
        || lower.contains("re-authorize the oauth application")
}

fn is_auth_failure(message: &str) -> bool {
    message.contains("Authentication failed")
        || message.contains("could not read Username")
        || message.contains("Permission denied")
        || message.contains("Repository not found")
        || message.contains("requested URL returned error: 403")
        || is_github_sso_auth_error(message)
}

#[derive(Clone)]
struct GitHubRepoInfo {
    ssh_url: String,
}

fn parse_github_repo_url(url: &str) -> Option<GitHubRepoInfo> {
    if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        if !skills_store::paths::is_github_host(host) {
            return None;
        }
        let path = path.trim_end_matches(".git").trim_end_matches('/');
        let (owner, repo) = path.split_once('/')?;
        return Some(GitHubRepoInfo { ssh_url: format!("git@{host}:{owner}/{repo}.git") });
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let (host, path) = crate::source_parser::host_and_path(url)?;
    if !skills_store::paths::is_github_host(&host) {
        return None;
    }
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() != 2 {
        return None;
    }
    let repo = segments[1].trim_end_matches(".git");
    Some(GitHubRepoInfo { ssh_url: format!("git@{host}:{}/{repo}.git", segments[0]) })
}

fn is_github_https_clone_url(url: &str) -> bool {
    url.starts_with("https://")
        && crate::source_parser::host_and_path(url)
            .map(|(host, _)| skills_store::paths::is_github_host(&host))
            .unwrap_or(false)
}

/// Extra `-c` config plus env, mirroring the upstream simple-git client.
fn git_command() -> Command {
    let mut command = Command::new("git");
    for config in [
        "filter.lfs.required=false",
        "filter.lfs.smudge=",
        "filter.lfs.clean=",
        "filter.lfs.process=",
        "core.longpaths=true",
    ] {
        command.arg("-c").arg(config);
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", ALLOWED_GIT_PROTOCOLS)
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .env("LC_ALL", "C");
    command
}

fn reset_temp_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::create_dir_all(dir);
}

/// Run a command with a hard timeout; returns (exit ok, combined output).
fn run_with_timeout(mut command: Command, timeout: Duration) -> (bool, String) {
    command.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let Ok(mut child) = command.spawn() else {
        return (false, "failed to spawn command".to_string());
    };
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        use std::io::Read;
        let mut out = Vec::new();
        let mut err = Vec::new();
        if let Some(mut stdout) = stdout_pipe {
            let _ = stdout.read_to_end(&mut out);
        }
        if let Some(mut stderr) = stderr_pipe {
            let _ = stderr.read_to_end(&mut err);
        }
        (String::from_utf8_lossy(&out).to_string(), String::from_utf8_lossy(&err).to_string())
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let (out, err) = out_handle.join().unwrap_or_default();
                return (status.success(), format!("{out}{err}"));
            }
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out_handle.join();
                    return (false, "timed out".to_string());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => {
                let _ = out_handle.join();
                return (false, err.to_string());
            }
        }
    }
}

fn clone_at_sha(url: &str, sha: &str, temp_dir: &Path) -> Result<(), String> {
    let run = |args: &[&str]| -> (bool, String) {
        let mut command = git_command();
        command.current_dir(temp_dir).args(args);
        run_with_timeout(command, Duration::from_secs(120))
    };
    let (ok1, o1) = run(&["init"]);
    if !ok1 {
        return Err(o1);
    }
    let (ok2, o2) = run(&["remote", "add", "origin", url]);
    if !ok2 {
        return Err(o2);
    }
    let (ok3, o3) = run(&["fetch", "--depth", "1", "origin", sha]);
    if !ok3 {
        return Err(o3);
    }
    let (ok4, o4) = run(&["checkout", "FETCH_HEAD"]);
    if !ok4 {
        return Err(o4);
    }
    Ok(())
}

fn try_gh_clone(repo: &GitHubRepoInfo, temp_dir: &Path, ref_: Option<&str>) -> Result<(), String> {
    let mut clone_target = repo
        .ssh_url
        .trim_start_matches("git@")
        .split(':')
        .next_back()
        .map(|s| s.trim_end_matches(".git").to_string())
        .unwrap_or_default();
    let host = repo
        .ssh_url
        .strip_prefix("git@")
        .and_then(|rest| rest.split_once(':'))
        .map(|(host, _)| host.to_string())
        .unwrap_or_else(|| "github.com".to_string());
    let gh_status = Command::new("gh")
        .args(["auth", "status", "-h", &host])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output();
    let Ok(status) = gh_status else {
        return Err("gh unavailable".to_string());
    };
    let status_text = format!(
        "{}{}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
    if let Some(line) = status_text
        .lines()
        .find(|line| line.to_lowercase().contains("git operations protocol:"))
    {
        if line.to_lowercase().contains("ssh") {
            clone_target = repo.ssh_url.clone();
        }
    }

    let mut args: Vec<String> = vec![
        "repo".into(),
        "clone".into(),
        clone_target,
        temp_dir.to_string_lossy().to_string(),
        "--".into(),
        "--depth=1".into(),
    ];
    if let Some(ref_) = ref_ {
        args.push("--branch".into());
        args.push(ref_.to_string());
    }
    let mut command = Command::new("gh");
    command
        .args(&args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", ALLOWED_GIT_PROTOCOLS);
    let (success, output) = run_with_timeout(command, clone_timeout());
    if success {
        Ok(())
    } else {
        Err(output)
    }
}

fn fresh_temp_dir() -> Result<PathBuf, SkillsError> {
    let dir = std::env::temp_dir().join(format!("skills-{}", unique_suffix()));
    std::fs::create_dir_all(&dir)
        .map_err(|err| SkillsError::new("SKILLS_IO", format!("tempdir: {err}")))?;
    Ok(dir)
}

fn unique_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{pid:x}{nanos:x}")
}

/// Clone `url` (optionally `ref`) into a fresh temp dir and return its path.
#[allow(clippy::too_many_lines)]
pub fn clone_repo(url: &str, ref_: Option<&str>) -> Result<PathBuf, SkillsError> {
    if url.to_lowercase().starts_with("ext::") {
        return Err(diag::unsupported_git_transport("ext"));
    }

    let clone_dir = fresh_temp_dir()?;
    let repo = parse_github_repo_url(url);
    let ref_can_be_sha = ref_.is_some_and(is_commit_sha);
    let _ = &ref_;

    let mut clone_args: Vec<String> =
        vec!["clone".into(), "--depth".into(), "1".into()];
    if let Some(ref_) = ref_ {
        clone_args.push("--branch".into());
        clone_args.push(ref_.to_string());
    }
    clone_args.push(url.to_string());
    clone_args.push(clone_dir.to_string_lossy().to_string());

    let mut command = git_command();
    let arg_refs: Vec<&str> = clone_args.iter().map(String::as_str).collect();
    command.args(arg_refs);
    let (success, output) = run_with_timeout(command, clone_timeout());
    if success {
        return Ok(clone_dir);
    }

    let message = output.trim().to_string();
    if ref_can_be_sha && is_missing_ref_error(&message) {
        if let Some(ref_) = ref_ {
            reset_temp_dir(&clone_dir);
            if clone_at_sha(url, ref_, &clone_dir).is_ok() {
                return Ok(clone_dir);
            }
        }
    }

    let is_timeout = message.contains("timed out");
    let is_auth = is_auth_failure(&message);

    if is_timeout {
        let _ = std::fs::remove_dir_all(&clone_dir);
        return Err(diag::git_clone_timeout(clone_timeout().as_secs()));
    }

    if is_auth && repo.is_some() && is_github_https_clone_url(url) {
        let repo = repo.clone().unwrap();
        reset_temp_dir(&clone_dir);
        if try_gh_clone(&repo, &clone_dir, ref_).is_ok() {
            return Ok(clone_dir);
        }
        reset_temp_dir(&clone_dir);
        let mut ssh_args: Vec<String> =
            vec!["clone".into(), "--depth".into(), "1".into()];
        if let Some(ref_) = ref_ {
            ssh_args.push("--branch".into());
            ssh_args.push(ref_.to_string());
        }
        ssh_args.push(repo.ssh_url.clone());
        ssh_args.push(clone_dir.to_string_lossy().to_string());
        let mut ssh_command = git_command();
        ssh_command.env(
            "GIT_SSH_COMMAND",
            std::env::var("GIT_SSH_COMMAND")
                .unwrap_or_else(|_| "ssh -o BatchMode=yes".to_string()),
        );
        let arg_refs: Vec<&str> = ssh_args.iter().map(String::as_str).collect();
        ssh_command.args(arg_refs);
        let (ssh_ok, ssh_message) = run_with_timeout(ssh_command, clone_timeout());
        if ssh_ok {
            return Ok(clone_dir);
        }
        if let (true, Some(ref_)) = (ref_can_be_sha && is_missing_ref_error(&ssh_message), ref_) {
            reset_temp_dir(&clone_dir);
            if clone_at_sha(&repo.ssh_url, ref_, &clone_dir).is_ok() {
                return Ok(clone_dir);
            }
        }
    }

    let _ = std::fs::remove_dir_all(&clone_dir);
    if is_auth {
        let host = repo
            .as_ref()
            .and_then(|repo| repo.ssh_url.strip_prefix("git@"))
            .and_then(|rest| rest.split_once(':'))
            .map(|(host, _)| host.to_string())
            .unwrap_or_else(|| "github.com".to_string());
        if let Some(repo) = repo {
            if is_github_sso_auth_error(&message) {
                return Err(diag::git_saml_sso_blocked(url, &repo.ssh_url, &host));
            }
            return Err(diag::git_auth_failed(url, Some(&repo.ssh_url), &host));
        }
        return Err(diag::git_auth_failed(url, None, &host));
    }
    Err(diag::git_clone_failed(url, &message))
}

/// The commit checked out in a cloned repository, or `None`.
#[must_use]
pub fn get_head_commit(repo_dir: &Path) -> Option<String> {
    rev_parse(repo_dir, "HEAD^{commit}")
}

/// Resolve the Git tree object hash for a skill folder path (matches the
/// GitHub Trees API SHA).
#[allow(dead_code)] // used when re-hashing cloned trees during update
#[must_use]
pub fn get_git_tree_hash(repo_dir: &Path, skill_path: &str) -> Option<String> {
    let normalized = skill_path.replace('\\', "/");
    let folder_path = match normalized.rfind('/') {
        Some(index) => &normalized[..index],
        None => "",
    };
    let revision = if folder_path.is_empty() {
        "HEAD^{tree}".to_string()
    } else {
        format!("HEAD:{folder_path}")
    };
    rev_parse(repo_dir, &revision)
}

fn rev_parse(repo_dir: &Path, revision: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["rev-parse", "--verify", "--end-of-options", revision])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(hash.to_lowercase())
    } else {
        None
    }
}

/// Remove a temp clone directory, refusing paths outside the temp dir
/// (upstream `cleanupTempDir`).
pub fn cleanup_temp_dir(dir: &Path) -> Result<(), SkillsError> {
    let normalized = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let tmp = std::fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|_| std::env::temp_dir());
    if !normalized.starts_with(&tmp) {
        return Err(diag::cleanup_outside_temp_dir());
    }
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}

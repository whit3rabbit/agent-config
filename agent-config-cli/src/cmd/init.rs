//! `agentconfig init` — scaffold an `agent.config.toml` in the cwd.

use clap::Args as ClapArgs;

use crate::diag::{codes, CliResult, Diag};
use crate::manifest;

use super::fail;

/// Arguments for `init`.
#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Optional package name; defaults to the current directory's name.
    pub name: Option<String>,
}

/// Entry point for `init`.
pub fn run(args: &Args) -> i32 {
    match init(args) {
        Ok(code) => code,
        Err(diag) => fail(diag),
    }
}

/// The scaffold written by `init`.
pub const SCAFFOLD: &str = r#"# agent-config package manifest.
# Docs: every [[hooks]] / [[instructions]] / [[mcp]] / [[skills]] entry
# installs into the harnesses you target with `agentconfig add <source>`.
[package]
name = "PKG_NAME"
version = "0.1.0"
description = "What this package installs and when to use it."

# A command hook. `command` is an argv array (POSIX-quoted by the library);
# a plain string is treated as trusted raw shell.
#[[hooks]]
#tag = "guard-secrets"
#command = ["scripts/guard.sh", "--strict"]
#event = "PreToolUse"        # default; see docs for the full event list
#matcher = "Bash"            # "*"|tool name|regex|[any of...]
#timeout = 30                # seconds
#rules = "Optional prompt rules injected next to the hook."

# Prompt rules / standalone instructions, placed wherever each harness
# expects them (inline block, referenced file, or rules-dir file).
#[[instructions]]
#name = "code-style"
#file = "STYLE.md"           # or body = "inline markdown"
#placement = "auto"          # auto | inline | referenced | standalone

# An MCP server. stdio entries take command + optional env; http/sse take
# url + optional headers. Env placeholders like "${VAR}" keep secrets out
# of the manifest.
#[[mcp]]
#name = "context7"
#command = ["npx", "-y", "@upstash/context7-mcp"]
#env = { API_KEY = "${CONTEXT7_API_KEY}" }

# An Agent Skills folder (SKILL.md + assets) or an inline skill.
#[[skills]]
#path = "skills/my-skill"
"#;

fn init(args: &Args) -> CliResult<i32> {
    let path = std::env::current_dir()
        .map_err(|e| {
            Diag::new(
                codes::IO,
                format!("cannot resolve the working directory: {e}"),
            )
        })?
        .join(manifest::MANIFEST_FILE);
    if path.exists() {
        return Err(Diag::with_fix(
            codes::INIT_EXISTS,
            format!("{} already exists", path.display()),
            "edit the existing manifest instead",
        ));
    }
    let name = args.name.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string()))
            .map(|n| crate::engine::sanitize_kebab(&n))
            .unwrap_or_else(|| "my-package".to_string())
    });
    std::fs::write(&path, SCAFFOLD.replace("PKG_NAME", &name))
        .map_err(|e| Diag::new(codes::IO, format!("cannot write {}: {e}", path.display())))?;
    println!("scaffolded {}", path.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_replaces_package_name() {
        let out = SCAFFOLD.replace("PKG_NAME", "acme");
        assert!(out.contains("name = \"acme\""));
        assert!(!out.contains("PKG_NAME"));
    }

    #[test]
    fn scaffold_is_valid_toml_after_uncommenting() {
        // Sanity: the scaffold's active lines (everything uncommented) parse.
        let out = SCAFFOLD.replace("PKG_NAME", "acme");
        let active: String = out
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        crate::manifest::parse(&active).expect("active scaffold lines parse");
    }
}

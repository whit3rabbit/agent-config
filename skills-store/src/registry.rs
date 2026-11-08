//! The agent registry: every agent the canonical skills store can target,
//! mirroring `src/agents.ts` of the upstream `skills` CLI (order included —
//! it drives interactive lists and detection order).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::paths;

/// Per-agent configuration.
pub struct AgentConfig {
    /// Registry id used in `--agent` flags and lock files, e.g. `claude-code`.
    pub name: &'static str,
    /// Human-facing name, e.g. `Claude Code`.
    pub display_name: &'static str,
    /// Project skills directory relative to the project root.
    pub skills_dir: &'static str,
    /// Behavioral flags from the upstream registry.
    pub flags: AgentFlags,
    detect: fn(&DetectContext) -> bool,
    global_dir: fn(&DetectContext) -> Option<PathBuf>,
}

/// Optional registry flags (all default `false`).
#[derive(Clone, Copy, Default)]
pub struct AgentFlags {
    /// Hidden from the universal agents list (Replit and `universal`).
    pub hide_in_universal_list: bool,
    /// Displayed in the interactive locked section (several agents hide).
    pub hide_in_universal_prompt: bool,
    /// Universal agent that does NOT read `~/.agents/skills` for global installs.
    pub global_reads_canonical_false: bool,
    /// Automatic project installs may create this agent's missing skills root.
    pub create_project_skills_dir_by_default: bool,
}

impl AgentConfig {
    /// Whether this agent's project skills dir is the canonical `.agents/skills`.
    #[must_use]
    pub fn is_universal(&self) -> bool {
        self.skills_dir == ".agents/skills"
    }

    /// Universal + reads `~/.agents/skills` for global installs.
    #[must_use]
    pub fn reads_canonical_globally(&self) -> bool {
        self.is_universal() && !self.flags.global_reads_canonical_false
    }

    /// Whether this agent appears in the interactive universal prompt.
    #[must_use]
    pub fn visible_in_universal_prompt(&self) -> bool {
        self.is_universal()
            && self.flags.show_in_universal_list()
            && !self.flags.hide_in_universal_prompt
    }

    /// Detect whether the agent is installed on this machine (or in `cwd` for
    /// agents detected by project markers).
    #[must_use]
    pub fn detect_installed(&self, ctx: &DetectContext) -> bool {
        (self.detect)(ctx)
    }

    /// The agent's global skills directory, or `None` when it does not support
    /// global installation.
    #[must_use]
    pub fn global_skills_dir(&self, ctx: &DetectContext) -> Option<PathBuf> {
        (self.global_dir)(ctx)
    }
}

impl AgentFlags {
    fn show_in_universal_list(&self) -> bool {
        // Upstream default is true; Replit and `universal` opt out.
        !self.hide_in_universal_list
    }
}

/// Snapshot of the environment used for detection and directory resolution.
#[derive(Debug, Clone)]
pub struct DetectContext {
    /// Project directory (usually the process cwd).
    pub cwd: PathBuf,
    /// User home directory.
    pub home: PathBuf,
    /// XDG configuration home (`$XDG_CONFIG_HOME` or `~/.config`).
    pub config_home: PathBuf,
}

impl DetectContext {
    /// Build a context from the current process environment.
    #[must_use]
    pub fn from_env(cwd: &Path) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
            home: paths::home_dir(),
            config_home: paths::config_home(),
        }
    }

    fn home_join(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }
}

// ─── Detection helpers ──────────────────────────────────────────────────────

fn exists(p: &Path) -> bool {
    p.exists()
}

fn package_json_has_dependency(package_json_path: &Path, dependency_name: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(package_json_path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    ["dependencies", "devDependencies"].iter().any(|field| {
        value
            .get(field)
            .and_then(|v| v.get(dependency_name))
            .is_some_and(|v| !v.is_null())
    })
}

fn openclaw_global_skills_dir(ctx: &DetectContext) -> PathBuf {
    for dir in [".openclaw", ".clawdbot", ".moltbot"] {
        if exists(&ctx.home.join(dir)) {
            return ctx.home.join(dir).join("skills");
        }
    }
    ctx.home.join(".openclaw").join("skills")
}

// ─── Registry ───────────────────────────────────────────────────────────────

macro_rules! agents {
    ($( $name:literal, $display:literal, $skills_dir:literal, $flags:expr, $detect:expr, $global:expr );* $(;)?) => {
        static AGENTS: OnceLock<Vec<AgentConfig>> = OnceLock::new();
        fn build_agents() -> Vec<AgentConfig> {
            vec![
                $( AgentConfig {
                    name: $name,
                    display_name: $display,
                    skills_dir: $skills_dir,
                    flags: $flags,
                    detect: $detect,
                    global_dir: $global,
                }, )*
            ]
        }
    };
}

agents! {
    "aider-desk", "AiderDesk", ".aider-desk/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".aider-desk")),
        |c| Some(c.home_join(".aider-desk/skills"));
    "amp", "Amp", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.config_home.join("amp")),
        |c| Some(c.config_home.join("agents").join("skills"));
    "antigravity", "Antigravity", ".agents/skills",
        AgentFlags { global_reads_canonical_false: true, hide_in_universal_prompt: true, ..Default::default() },
        |c| exists(&c.home_join(".gemini/antigravity")),
        |c| Some(c.home_join(".gemini/config/skills"));
    "antigravity-cli", "Antigravity CLI", ".agents/skills",
        AgentFlags { global_reads_canonical_false: true, hide_in_universal_prompt: true, ..Default::default() },
        |c| exists(&c.home_join(".gemini/antigravity-cli")),
        |c| Some(c.home_join(".gemini/config/skills"));
    "astrbot", "AstrBot", "data/skills", AgentFlags::default(),
        |c| exists(&c.cwd.join("data/skills")) || exists(&c.home_join(".astrbot")),
        |c| Some(c.home_join(".astrbot/data/skills"));
    "autohand-code", "Autohand Code CLI", ".autohand/skills", AgentFlags::default(),
        |_c| exists(&paths::autohand_home()),
        |_c| Some(paths::autohand_home().join("skills"));
    "augment", "Augment", ".augment/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".augment")),
        |c| Some(c.home_join(".augment/skills"));
    "bob", "IBM Bob", ".bob/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".bob")),
        |c| Some(c.home_join(".bob/skills"));
    "claude-code", "Claude Code", ".claude/skills",
        AgentFlags { create_project_skills_dir_by_default: true, ..Default::default() },
        |_c| exists(&paths::claude_home()),
        |_c| Some(paths::claude_home().join("skills"));
    "openclaw", "OpenClaw", "skills", AgentFlags::default(),
        |c| exists(&c.home_join(".openclaw")) || exists(&c.home_join(".clawdbot")) || exists(&c.home_join(".moltbot")),
        |c| Some(openclaw_global_skills_dir(c));
    "cline", "Cline", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".cline")),
        |c| Some(c.home_join(".agents/skills"));
    "codearts-agent", "CodeArts Agent", ".codeartsdoer/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".codeartsdoer")),
        |c| Some(c.home_join(".codeartsdoer/skills"));
    "codebuddy", "CodeBuddy", ".codebuddy/skills", AgentFlags::default(),
        |c| exists(&c.cwd.join(".codebuddy")) || exists(&c.home_join(".codebuddy")),
        |c| Some(c.home_join(".codebuddy/skills"));
    "codemaker", "Codemaker", ".codemaker/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".codemaker")),
        |c| Some(c.home_join(".codemaker/skills"));
    "codestudio", "Code Studio", ".codestudio/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".codestudio")),
        |c| Some(c.home_join(".codestudio/skills"));
    "codex", "Codex", ".agents/skills", AgentFlags::default(),
        |_c| exists(&paths::codex_home()) || exists(Path::new("/etc/codex")),
        |c| Some(c.home_join(".agents/skills"));
    "command-code", "Command Code", ".commandcode/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".commandcode")),
        |c| Some(c.home_join(".commandcode/skills"));
    "continue", "Continue", ".continue/skills", AgentFlags::default(),
        |c| exists(&c.cwd.join(".continue")) || exists(&c.home_join(".continue")),
        |c| Some(c.home_join(".continue/skills"));
    "cortex", "Cortex Code", ".cortex/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".snowflake/cortex")),
        |c| Some(c.home_join(".snowflake/cortex/skills"));
    "crush", "Crush", ".crush/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".config/crush")),
        |c| Some(c.home_join(".config/crush/skills"));
    "cursor", "Cursor", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".cursor")),
        |c| Some(c.home_join(".cursor/skills"));
    "deepagents", "Deep Agents", ".agents/skills",
        AgentFlags { hide_in_universal_prompt: true, ..Default::default() },
        |c| exists(&c.home_join(".deepagents")),
        |c| Some(c.home_join(".deepagents/agent/skills"));
    "devin", "Devin for Terminal", ".devin/skills", AgentFlags::default(),
        |c| exists(&c.config_home.join("devin")),
        |c| Some(c.config_home.join("devin/skills"));
    "dexto", "Dexto", ".agents/skills",
        AgentFlags { hide_in_universal_prompt: true, ..Default::default() },
        |c| exists(&c.home_join(".dexto")),
        |c| Some(c.home_join(".agents/skills"));
    "droid", "Droid", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".factory")),
        |c| Some(c.home_join(".factory/skills"));
    "eve", "Eve", "agent/skills", AgentFlags::default(),
        |c| exists(&c.cwd.join("agent")) && package_json_has_dependency(&c.cwd.join("package.json"), "eve"),
        |_| None;
    "firebender", "Firebender", ".agents/skills",
        AgentFlags { hide_in_universal_prompt: true, ..Default::default() },
        |c| exists(&c.home_join(".firebender")),
        |c| Some(c.home_join(".firebender/skills"));
    "forgecode", "ForgeCode", ".forge/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".forge")),
        |c| Some(c.home_join(".forge/skills"));
    "fx", "fx", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".fx")),
        |c| Some(c.home_join(".agents/skills"));
    "gemini-cli", "Gemini CLI", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".gemini")),
        |c| Some(c.home_join(".gemini/skills"));
    "github-copilot", "GitHub Copilot", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".copilot")),
        |c| Some(c.home_join(".copilot/skills"));
    "goose", "Goose", ".goose/skills", AgentFlags::default(),
        |c| exists(&c.config_home.join("goose")),
        |c| Some(c.config_home.join("goose/skills"));
    "grok", "Grok Build", ".grok/skills", AgentFlags::default(),
        |_c| exists(&paths::grok_home()),
        |_c| Some(paths::grok_home().join("skills"));
    "hermes-agent", "Hermes Agent", ".hermes/skills", AgentFlags::default(),
        |_c| exists(&paths::hermes_home()),
        |_c| Some(paths::hermes_home().join("skills"));
    "inference-sh", "inference.sh", ".inferencesh/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".inferencesh")),
        |c| Some(c.home_join(".inferencesh/skills"));
    "jazz", "Jazz", ".jazz/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".jazz")) || exists(&c.cwd.join(".jazz")),
        |c| Some(c.home_join(".jazz/skills"));
    "junie", "Junie", ".junie/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".junie")),
        |c| Some(c.home_join(".junie/skills"));
    "iflow-cli", "iFlow CLI", ".iflow/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".iflow")),
        |c| Some(c.home_join(".iflow/skills"));
    "kilo", "Kilo Code", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".kilo")) || exists(&c.home_join(".kilocode")),
        |c| Some(c.home_join(".kilo/skills"));
    "kimchi", "Kimchi", ".kimchi/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".config/kimchi")),
        |c| Some(c.home_join(".config/kimchi/harness/skills"));
    "kimi-code-cli", "Kimi Code CLI", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".kimi-code")) || exists(&c.home_join(".kimi")),
        |c| Some(c.home_join(".agents/skills"));
    "kiro-cli", "Kiro CLI", ".kiro/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".kiro")),
        |c| Some(c.home_join(".kiro/skills"));
    "kode", "Kode", ".kode/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".kode")),
        |c| Some(c.home_join(".kode/skills"));
    "lingma", "Lingma", ".lingma/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".lingma")),
        |c| Some(c.home_join(".lingma/skills"));
    "loaf", "Loaf", ".agents/skills",
        AgentFlags { hide_in_universal_prompt: true, ..Default::default() },
        |c| exists(&c.home_join(".loaf")),
        |c| Some(c.home_join(".agents/skills"));
    "mcpjam", "MCPJam", ".mcpjam/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".mcpjam")),
        |c| Some(c.home_join(".mcpjam/skills"));
    "minimax-code", "MiniMax Code", ".minimax/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".minimax")) || exists(Path::new("/Applications/MiniMax Code.app")),
        |c| Some(c.home_join(".minimax/skills"));
    "mistral-vibe", "Mistral Vibe", ".vibe/skills", AgentFlags::default(),
        |_c| exists(&paths::vibe_home()),
        |_c| Some(paths::vibe_home().join("skills"));
    "moxby", "Moxby", ".moxby/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".moxby")),
        |c| Some(c.home_join(".moxby/skills"));
    "mux", "Mux", ".mux/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".mux")),
        |c| Some(c.home_join(".mux/skills"));
    "opencode", "OpenCode", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.config_home.join("opencode")),
        |c| Some(c.config_home.join("opencode/skills"));
    "openhands", "OpenHands", ".openhands/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".openhands")),
        |c| Some(c.home_join(".openhands/skills"));
    "ona", "Ona", ".ona/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".ona")),
        |c| Some(c.home_join(".ona/skills"));
    "pi", "Pi", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".pi/agent")),
        |c| Some(c.home_join(".agents/skills"));
    "posit-assistant", "Posit Assistant", ".posit/assistant/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".posit/assistant")) || exists(&c.home_join(".positai")),
        |c| Some(c.home_join(".posit/assistant/skills"));
    "qoder", "Qoder", ".qoder/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".qoder")),
        |c| Some(c.home_join(".qoder/skills"));
    "qoder-cn", "Qoder CN", ".qoder/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".qoder-cn")),
        |c| Some(c.home_join(".qoder-cn/skills"));
    "qwen-code", "Qwen Code", ".qwen/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".qwen")),
        |c| Some(c.home_join(".qwen/skills"));
    "replit", "Replit", ".agents/skills",
        AgentFlags { hide_in_universal_list: true, ..Default::default() },
        |c| exists(&c.cwd.join(".replit")),
        |c| Some(c.config_home.join("agents/skills"));
    "reasonix", "Reasonix", ".reasonix/skills", AgentFlags::default(),
        |_c| exists(&paths::reasonix_home()),
        |_c| Some(paths::reasonix_home().join("skills"));
    "rovodev", "Rovo Dev", ".rovodev/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".rovodev")),
        |c| Some(c.home_join(".rovodev/skills"));
    "roo", "Roo Code", ".roo/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".roo")),
        |c| Some(c.home_join(".roo/skills"));
    "sarvam-code", "Sarvam Code", ".agents/skills",
        AgentFlags { hide_in_universal_prompt: true, ..Default::default() },
        |_c| exists(&paths::sarvam_home()),
        |c| Some(c.home_join(".agents/skills"));
    "tabnine-cli", "Tabnine CLI", ".tabnine/agent/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".tabnine")),
        |c| Some(c.home_join(".tabnine/agent/skills"));
    "terramind", "Terramind", ".terramind/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".terramind")),
        |c| Some(c.home_join(".terramind/skills"));
    "tinycloud", "Tinycloud", ".tinycloud/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".tinycloud")),
        |c| Some(c.home_join(".tinycloud/skills"));
    "trae", "Trae", ".trae/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".trae")),
        |c| Some(c.home_join(".trae/skills"));
    "trae-cn", "Trae CN", ".trae/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".trae-cn")),
        |c| Some(c.home_join(".trae-cn/skills"));
    "warp", "Warp", ".agents/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".warp")),
        |c| Some(c.home_join(".agents/skills"));
    "windsurf", "Windsurf", ".windsurf/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".codeium/windsurf")),
        |c| Some(c.home_join(".codeium/windsurf/skills"));
    "zed", "Zed", ".agents/skills", AgentFlags::default(),
        |c| {
            exists(&c.config_home.join("zed"))
                || paths::env_trimmed("APPDATA").is_some_and(|app| exists(&PathBuf::from(app).join("Zed")))
                || paths::flatpak_config_home().is_some_and(|f| exists(&f.join("zed")))
        },
        |c| Some(c.home_join(".agents/skills"));
    "zcode", "ZCode", ".zcode/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".zcode")) || exists(Path::new("/Applications/ZCode.app")),
        |c| Some(c.home_join(".zcode/skills"));
    "zencoder", "Zencoder", ".zencoder/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".zencoder")),
        |c| Some(c.home_join(".zencoder/skills"));
    "zenflow", "Zenflow", ".zencoder/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".zencoder")),
        |c| Some(c.home_join(".zencoder/skills"));
    "neovate", "Neovate", ".neovate/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".neovate")),
        |c| Some(c.home_join(".neovate/skills"));
    "pochi", "Pochi", ".pochi/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".pochi")),
        |c| Some(c.home_join(".pochi/skills"));
    "promptscript", "PromptScript", ".agents/skills",
        AgentFlags { hide_in_universal_prompt: true, ..Default::default() },
        |c| exists(&c.cwd.join(".promptscript")) || exists(&c.cwd.join("promptscript.yaml")),
        |_| None;
    "adal", "AdaL", ".adal/skills", AgentFlags::default(),
        |c| exists(&c.home_join(".adal")),
        |c| Some(c.home_join(".adal/skills"));
    "universal", "Universal", ".agents/skills",
        AgentFlags { hide_in_universal_list: true, ..Default::default() },
        |_| false,
        |c| Some(c.config_home.join("agents/skills"));
}

/// Every registered agent in upstream definition order, including the neutral
/// `universal` target (never detected).
#[must_use]
pub fn agents() -> &'static [AgentConfig] {
    AGENTS.get_or_init(build_agents)
}

/// Look up an agent by registry id.
#[must_use]
pub fn agent_by_id(name: &str) -> Option<&'static AgentConfig> {
    agents().iter().find(|a| a.name == name)
}

/// Agents whose project skills dir is `.agents/skills` and that participate in
/// the universal list (upstream `getUniversalAgents`).
#[must_use]
pub fn universal_agents() -> Vec<&'static AgentConfig> {
    agents().iter().filter(|a| a.is_universal() && a.flags.show_in_universal_list()).collect()
}

/// The subset of universal agents shown in the interactive locked section.
#[must_use]
pub fn visible_universal_agents() -> Vec<&'static AgentConfig> {
    agents().iter().filter(|a| a.visible_in_universal_prompt()).collect()
}

/// Agents whose project skills dir is not `.agents/skills`.
#[must_use]
pub fn non_universal_agents() -> Vec<&'static AgentConfig> {
    agents().iter().filter(|a| !a.is_universal()).collect()
}

/// Agents detected on this machine / project.
#[must_use]
pub fn detect_installed_agents(cwd: &Path) -> Vec<&'static AgentConfig> {
    let ctx = DetectContext::from_env(cwd);
    agents().iter().filter(|a| a.detect_installed(&ctx)).collect()
}

/// Legacy project skill dirs that repositories still publish to, searched
/// during discovery (upstream `LEGACY_AGENT_PROJECT_SKILL_DIRS`).
pub const LEGACY_AGENT_PROJECT_SKILL_DIRS: &[&str] = &[
    ".cline/skills",
    ".codex/skills",
    ".factory/skills",
    ".github/skills",
    ".kilo/skills",
    ".kilocode/skills",
    ".opencode/skills",
    ".pi/skills",
];

/// Project skill dirs searched during discovery, sorted (upstream
/// `AGENT_PROJECT_SKILL_DIRS`; the root `skills/` dir is handled separately).
#[must_use]
pub fn agent_project_skill_dirs() -> &'static [String] {
    static DIRS: OnceLock<Vec<String>> = OnceLock::new();
    DIRS.get_or_init(|| {
        let mut dirs: Vec<String> = agents()
            .iter()
            .map(|a| a.skills_dir.to_string())
            .chain(LEGACY_AGENT_PROJECT_SKILL_DIRS.iter().map(|d| (*d).to_string()))
            .filter(|d| d != "skills")
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_shape() {
        // Upstream agents.ts registers 79 entries including `universal`.
        assert_eq!(agents().len(), 79);
        for agent in agents() {
            assert!(!agent.display_name.is_empty());
            assert!(!agent.skills_dir.is_empty());
        }
        assert_eq!(agent_by_id("claude-code").unwrap().display_name, "Claude Code");
        assert!(agent_by_id("eve").unwrap().global_skills_dir(&DetectContext::from_env(Path::new("."))).is_none());
        assert!(agent_by_id("promptscript").unwrap().global_skills_dir(&DetectContext::from_env(Path::new("."))).is_none());
    }

    #[test]
    fn universal_partition() {
        let universal = universal_agents();
        let non_universal = non_universal_agents();
        // getUniversalAgents excludes Replit and the neutral `universal` target.
        assert_eq!(universal.len() + non_universal.len() + 2, 79);
        assert!(universal.iter().all(|a| a.is_universal()));
        assert!(non_universal.iter().any(|a| a.name == "cursor" || a.name == "claude-code"));
        // cursor: universal project and reads ~/.agents/skills globally too.
        let cursor = agent_by_id("cursor").unwrap();
        assert!(cursor.is_universal() && cursor.reads_canonical_globally());
        let amp = agent_by_id("amp").unwrap();
        assert!(amp.reads_canonical_globally());
        let antigravity = agent_by_id("antigravity").unwrap();
        assert!(!antigravity.reads_canonical_globally());
    }

    #[test]
    fn project_skill_dirs_sorted() {
        let dirs = agent_project_skill_dirs();
        assert!(dirs.contains(&".agents/skills".to_string()));
        assert!(dirs.contains(&".github/skills".to_string()));
        assert!(!dirs.contains(&"skills".to_string()));
        let mut sorted: Vec<String> = dirs.to_vec();
        sorted.sort();
        assert_eq!(sorted, dirs.to_vec());
    }
}

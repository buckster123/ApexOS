use std::collections::HashMap;
use std::path::Path;
use serde::Deserialize;

// ── config types (loaded from policy.toml) ────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Every tool call requires explicit approval.
    #[default]
    Suggest,
    /// Reads and edits inside the workspace auto-approve; mutations outside or
    /// command execution ask. Mirrors Claude Code `acceptEdits`.
    AutoEdit,
    /// Nothing asks — full-send. Mirrors `bypassPermissions`.
    Yolo,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Rule {
    /// Auto-approve regardless of mode (overridden by Yolo).
    Allow,
    /// Always ask (overridden by Yolo).
    Ask,
    /// Auto if path is inside AGENTD_WORKSPACE, else ask.
    /// Path check is deferred to keyboard; currently treated as Allow in
    /// AutoEdit mode and Ask in Suggest mode.
    Workspace,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision { Allow, Ask }

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SubagentsConfig {
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
    #[serde(default = "default_inherit_mode")]
    pub inherit_mode: bool,
}

fn default_max_depth()      -> u32  { 4 }
fn default_max_concurrent() -> u32  { 16 }
fn default_inherit_mode()   -> bool { true }

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub rules: HashMap<String, Rule>,
    #[serde(default)]
    pub subagents: SubagentsConfig,
}

impl PolicyConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {}", path.display(), e))?;
        toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("policy.toml parse error: {}", e))
    }
}

// ── pure rule evaluation ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PolicyEngine {
    pub config: PolicyConfig,
}

impl PolicyEngine {
    pub fn new(config: PolicyConfig) -> Self { Self { config } }

    /// Evaluate whether `tool_name` may proceed without user confirmation.
    /// Returns `Decision::Allow` or `Decision::Ask`.
    pub fn check(&self, tool_name: &str) -> Decision {
        if self.config.mode == Mode::Yolo {
            return Decision::Allow;
        }
        let rule = self.find_rule(tool_name);
        self.apply_rule(rule)
    }

    fn find_rule(&self, tool_name: &str) -> Option<&Rule> {
        // Exact match wins over wildcard.
        if let Some(r) = self.config.rules.get(tool_name) {
            return Some(r);
        }
        for (pattern, rule) in &self.config.rules {
            if matches_wildcard(pattern, tool_name) {
                return Some(rule);
            }
        }
        None
    }

    fn apply_rule(&self, rule: Option<&Rule>) -> Decision {
        match rule {
            None                  => Decision::Ask,   // unknown tool → safe default
            Some(Rule::Allow)     => Decision::Allow,
            Some(Rule::Ask)       => Decision::Ask,
            Some(Rule::Workspace) => match self.config.mode {
                Mode::AutoEdit => Decision::Allow, // path check deferred
                _              => Decision::Ask,
            },
        }
    }
}

/// Pattern `"prefix.*"` matches any `"prefix.<something>"`.
fn matches_wildcard(pattern: &str, tool: &str) -> bool {
    if pattern == tool {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        tool.starts_with(&format!("{prefix}."))
    } else {
        false
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(mode: Mode, rules: &[(&str, Rule)]) -> PolicyEngine {
        PolicyEngine::new(PolicyConfig {
            mode,
            rules: rules.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
            ..Default::default()
        })
    }

    #[test]
    fn yolo_allows_everything() {
        let e = engine(Mode::Yolo, &[("shell.exec", Rule::Ask)]);
        assert_eq!(e.check("shell.exec"), Decision::Allow);
        assert_eq!(e.check("anything"),   Decision::Allow);
    }

    #[test]
    fn suggest_allow_rule_passes() {
        let e = engine(Mode::Suggest, &[("fs.read", Rule::Allow)]);
        assert_eq!(e.check("fs.read"), Decision::Allow);
    }

    #[test]
    fn suggest_ask_rule_blocks() {
        let e = engine(Mode::Suggest, &[("shell.exec", Rule::Ask)]);
        assert_eq!(e.check("shell.exec"), Decision::Ask);
    }

    #[test]
    fn suggest_unknown_tool_blocks() {
        let e = engine(Mode::Suggest, &[]);
        assert_eq!(e.check("unknown.tool"), Decision::Ask);
    }

    #[test]
    fn wildcard_matches_prefixed_tools() {
        let e = engine(Mode::Suggest, &[("cerebro.*", Rule::Allow)]);
        assert_eq!(e.check("cerebro.recall"), Decision::Allow);
        assert_eq!(e.check("cerebro.store"),  Decision::Allow);
        assert_eq!(e.check("cerebro"),        Decision::Ask);  // bare name, no dot
        assert_eq!(e.check("cerebro_other"),  Decision::Ask);  // wrong separator
    }

    #[test]
    fn exact_match_wins_over_wildcard() {
        let e = engine(Mode::Suggest, &[
            ("cerebro.*",    Rule::Allow),
            ("cerebro.exec", Rule::Ask),
        ]);
        assert_eq!(e.check("cerebro.exec"),   Decision::Ask);
        assert_eq!(e.check("cerebro.recall"), Decision::Allow);
    }

    #[test]
    fn auto_edit_workspace_rule_allows() {
        let e = engine(Mode::AutoEdit, &[("fs.write", Rule::Workspace)]);
        assert_eq!(e.check("fs.write"), Decision::Allow);
    }

    #[test]
    fn suggest_workspace_rule_blocks() {
        let e = engine(Mode::Suggest, &[("fs.write", Rule::Workspace)]);
        assert_eq!(e.check("fs.write"), Decision::Ask);
    }

    #[test]
    fn loads_default_policy_config() {
        let cfg = PolicyConfig::default();
        assert_eq!(cfg.mode, Mode::Suggest);
        assert!(cfg.rules.is_empty());
    }

    #[test]
    fn parses_policy_toml() {
        let toml = r#"
mode = "auto-edit"

[rules]
"fs.read"   = "allow"
"shell.exec" = "ask"
"cerebro.*" = "allow"
"fs.write"  = "workspace"

[subagents]
max_depth       = 3
max_concurrent  = 8
inherit_mode    = false
"#;
        let cfg: PolicyConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.mode, Mode::AutoEdit);
        assert_eq!(cfg.rules["fs.read"],    Rule::Allow);
        assert_eq!(cfg.rules["shell.exec"], Rule::Ask);
        assert_eq!(cfg.rules["cerebro.*"],  Rule::Allow);
        assert_eq!(cfg.rules["fs.write"],   Rule::Workspace);
        assert_eq!(cfg.subagents.max_depth, 3);
        assert_eq!(cfg.subagents.max_concurrent, 8);
        assert!(!cfg.subagents.inherit_mode);
    }
}

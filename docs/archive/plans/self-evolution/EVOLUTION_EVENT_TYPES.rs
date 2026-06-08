// ─────────────────────────────────────────────────────────────────────────────
// TARGET FILE: agentd/crates/core/src/types.rs
//
// These are ADDITIONS to the existing file. Drop each section in the
// appropriate place — the existing SessionId / ActionId / Event / etc.
// all stay untouched.
//
// ALSO REQUIRED: agentd/crates/plugins/src/policy.rs
//   - Remove the local `Mode` enum definition
//   - Add: `use apexos_core::PolicyMode;`
//   - Replace every `Mode::Suggest` → `PolicyMode::Suggest`, etc.
//   - Replace every `Mode` type annotation → `PolicyMode`
//   - The `PolicyConfig.mode` field type changes from `Mode` to `PolicyMode`
// ─────────────────────────────────────────────────────────────────────────────

// ── Add alongside SessionId and ActionId ──────────────────────────────────────

/// Unique identifier for an evolution proposal/apply/rollback triple.
/// u64 sequential, consistent with ActionId(u64) and SessionId(u64).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EvolutionId(pub u64);

/// Policy mode — moved here from plugins::policy so EvolutionProposal
/// (which lives in core) can reference it without a circular dep.
///
/// plugins::policy imports this via `use apexos_core::PolicyMode;`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyMode {
    #[default]
    Suggest,
    AutoEdit,
    Yolo,
}

/// Which runtime subsystem to reload. Used by HotReloadSubsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subsystem {
    Plugins,
    Policy,
    Agent,   // reloads soul.md system prompt Arc
    Gateway, // no-op today (stateless), reserved
}

/// Discrete, auditable change proposals. Each variant maps to exactly one
/// config artifact and one hot-reload action — no raw TOML patches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvolutionProposal {
    /// Add a new MCP server entry to /etc/agentd/plugins.toml and hot-reload.
    RegisterMcpServer {
        name:    String,
        command: String,
        env:     std::collections::HashMap<String, String>,
        reason:  String,
    },
    /// Remove an MCP server from /etc/agentd/plugins.toml and stop its process.
    UnregisterMcpServer {
        name:   String,
        reason: String,
    },
    /// Change a tool's approval mode in /etc/agentd/policy.toml.
    UpdatePolicyRule {
        tool_pattern: String,    // exact name or "prefix.*" wildcard
        new_mode:     PolicyMode,
        reason:       String,
    },
    /// Propose a patch to the agent's own system prompt (/etc/agentd/soul.md).
    /// `patch` is the full replacement content (not a diff) for simplicity and
    /// easy rollback — the pre-patch content is snapshotted in Cerebro.
    UpdateSystemPrompt {
        content: String,
        reason:  String,
    },
    /// Reload a subsystem in-place without structural config change.
    /// Useful after manual edits to config files outside the agent.
    HotReloadSubsystem {
        subsystem: Subsystem,
    },
}

// ── Add to the existing Event enum in types.rs ───────────────────────────────
//
// Insert these variants in the "// ── system ──" section alongside Error:

//     /// Agent has proposed a structural change. Goes through policy engine.
//     /// `evolution.*` rule namespace; default mode is suggest.
//     EvolutionProposed {
//         id:          EvolutionId,
//         proposal:    EvolutionProposal,
//         proposed_by: SessionId,
//     },
//
//     /// An EvolutionProposed was approved and applied.
//     /// patch_summary is a human-readable description of what changed.
//     EvolutionApplied {
//         id:            EvolutionId,
//         proposal:      EvolutionProposal,
//         patch_summary: String,
//         applied_by:    Option<SessionId>,   // None = auto (yolo mode)
//     },
//
//     /// A previously applied evolution was rolled back.
//     EvolutionRolledBack {
//         evolution_id:   EvolutionId,
//         reason:         String,
//         rolled_back_by: Option<SessionId>,
//     },

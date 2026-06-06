use serde::{Deserialize, Serialize};

// ── ID newtypes (cheap, copyable, type-safe) ───────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActionId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginId(pub String);

// ── The central event enum ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    // ── from frontends (intents) ──────────────────────────
    UserPrompt   { session: SessionId, text: String },
    UserApproval { session: SessionId, action: ActionId, granted: bool },
    UserCancel   { session: SessionId },

    // ── from the agent loop ───────────────────────────────
    AgentText     { session: SessionId, delta: String },
    AgentThinking { session: SessionId, delta: String },
    ToolRequested { session: SessionId, call: ToolCall },
    TurnComplete  { session: SessionId },

    // ── from the plugin supervisor ────────────────────────
    ToolResult { session: SessionId, call: ActionId, output: ToolOutput },
    PluginUp   { plugin: PluginId, tools: Vec<ToolSpec> },
    PluginDown { plugin: PluginId, reason: String },

    // ── from the policy engine ────────────────────────────
    ApprovalPending { session: SessionId, call: ToolCall },

    // ── system ────────────────────────────────────────────
    Error { session: Option<SessionId>, message: String },
}

// ── Tool call / result ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id:   ActionId,
    pub tool: String,
    pub args: serde_json::Value,
    /// Set by the policy engine, not the agent.
    pub needs_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub ok:      bool,
    pub content: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name:         String,
    pub description:  String,
    pub input_schema: serde_json::Value,
}

// ── Agent context — every session is one of these ─────────────────────────
//
// parent == None     -> root session, output streams to a frontend
// parent == Some(id) -> child session, TurnComplete -> ToolResult to parent

#[derive(Debug, Clone)]
pub struct AgentContext {
    pub id:      SessionId,
    pub parent:  Option<SessionId>,
    pub history: Vec<Message>,
    pub spawned: Vec<SessionId>,
}

impl AgentContext {
    pub fn root(id: SessionId) -> Self {
        Self { id, parent: None, history: Vec::new(), spawned: Vec::new() }
    }
    pub fn child(id: SessionId, parent: SessionId) -> Self {
        Self { id, parent: Some(parent), history: Vec::new(), spawned: Vec::new() }
    }
    pub fn is_root(&self) -> bool { self.parent.is_none() }
}

// ── Conversation message (maps to the Anthropic messages API) ──────────────
//
// Assistant MUST carry thinking blocks alongside text/tool_use — they must be
// replayed across tool round-trips or the API rejects the continuation.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    User      { content: Vec<ContentBlock> },
    Assistant { content: Vec<ContentBlock> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text     { text: String },
    Thinking { thinking: String, signature: String },
    ToolUse  { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: serde_json::Value, is_error: bool },
}

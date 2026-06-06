// crates/core/src/state.rs
//
// SystemState + the `apply` transition function. This is the ONE piece of real
// logic in the `core` crate, and the whole of build-order step 1. It is a PURE
// function over (state, event) — no async, no I/O — so it is exhaustively
// unit-testable. Get the transitions right here and the rest of the system has
// a trustworthy source of truth to lean on.

use std::collections::HashMap;
use crate::types::*;

#[derive(Debug, Default)]
pub struct SystemState {
    /// The whole agent tree, held FLAT. Parent links reconstruct hierarchy.
    pub sessions: HashMap<SessionId, AgentContext>,
    /// Currently-registered tools, keyed by tool name → owning plugin.
    pub tools:    HashMap<String, PluginId>,
    /// Live plugins and the tools each advertises.
    pub plugins:  HashMap<PluginId, Vec<ToolSpec>>,
    /// Tool calls awaiting human approval (policy emitted ApprovalPending).
    pub pending_approvals: HashMap<ActionId, SessionId>,
}

impl SystemState {
    /// Fold one event into canonical state. PURE — call from the bus only.
    pub fn apply(&mut self, event: &Event) {
        match event {
            // ── session lifecycle ──────────────────────────────────────────
            // NOTE: session creation is implicit on first UserPrompt for a new
            // SessionId. Sub-agent sessions are created by the agent task
            // manager (which builds the child AgentContext); if you prefer an
            // explicit SessionOpened event, add it to Event and handle here.
            Event::UserPrompt { session, text } => {
                let ctx = self.sessions
                    .entry(*session)
                    .or_insert_with(|| AgentContext::root(*session));
                ctx.history.push(Message::User {
                    content: vec![ContentBlock::Text { text: text.clone() }],
                });
            }

            Event::UserCancel { session } => {
                // Cancellation cascade is DRIVEN elsewhere (the task manager
                // walks `spawned` and aborts child tasks). Here we only record
                // intent / clean state if you keep a per-session status flag.
                let _ = session;
            }

            // ── agent streaming ────────────────────────────────────────────
            // AgentText / AgentThinking are transient UI deltas. The canonical
            // assistant turn is committed by the turn engine into history when
            // the turn completes; we do NOT accumulate deltas into state here
            // (avoids double-bookkeeping). Keep these as pure fan-out.
            Event::AgentText { .. } | Event::AgentThinking { .. } => {}

            // ── tool flow ──────────────────────────────────────────────────
            Event::ToolRequested { .. } => {
                // No state change by default — dispatch/approval handle it.
                // (If you track in-flight tool calls for the UI, record here.)
            }

            Event::ApprovalPending { session, call } => {
                self.pending_approvals.insert(call.id, *session);
            }

            Event::UserApproval { action, .. } => {
                self.pending_approvals.remove(action);
                // The actual dispatch-on-approve is driven by the policy/
                // dispatch task watching this event; state just forgets it.
            }

            Event::ToolResult { session, call, output } => {
                let _ = (call, output);
                // If this session is a CHILD and the result corresponds to its
                // TurnComplete, routing to the parent is handled by the task
                // manager (see TurnComplete). Tool results from plugins are
                // appended to history by the turn engine, not here.
                let _ = session;
            }

            // ── multi-agent routing hook ───────────────────────────────────
            Event::TurnComplete { session } => {
                // The ROUTING DECISION lives in the task manager, but state can
                // assert the invariant: a child's completion means its result
                // should become a ToolResult in the parent's history.
                if let Some(ctx) = self.sessions.get(session) {
                    if let Some(_parent) = ctx.parent {
                        // task manager: deliver child's final output as a
                        // ToolResult to `_parent`. (Left to the async layer.)
                    }
                }
            }

            // ── plugin lifecycle ───────────────────────────────────────────
            Event::PluginUp { plugin, tools } => {
                for spec in tools {
                    self.tools.insert(spec.name.clone(), plugin.clone());
                }
                self.plugins.insert(plugin.clone(), tools.clone());
            }

            Event::PluginDown { plugin, .. } => {
                // Remove the plugin and all tools it owned.
                self.tools.retain(|_, owner| owner != plugin);
                self.plugins.remove(plugin);
            }

            Event::Error { .. } => { /* surfaced to UI + log; no state change */ }
        }
    }

    // ── helpers the async layer leans on ──────────────────────────────────

    /// Register a child session created by the task manager (agent.spawn).
    pub fn register_child(&mut self, child: SessionId, parent: SessionId) {
        self.sessions.insert(child, AgentContext::child(child, parent));
        if let Some(p) = self.sessions.get_mut(&parent) {
            p.spawned.push(child);
        }
    }

    /// Collect a session and all its transitive descendants (cancel cascade).
    pub fn subtree(&self, root: SessionId) -> Vec<SessionId> {
        let mut out = vec![root];
        let mut i = 0;
        while i < out.len() {
            if let Some(ctx) = self.sessions.get(&out[i]) {
                out.extend(ctx.spawned.iter().copied());
            }
            i += 1;
        }
        out
    }
}

// ── UNIT TESTS — build step 1 is "make these (and more) pass" ───────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str) -> ToolSpec {
        ToolSpec { name: name.into(), description: String::new(),
                   input_schema: serde_json::json!({}) }
    }

    #[test]
    fn user_prompt_creates_root_session_and_appends_history() {
        let mut s = SystemState::default();
        s.apply(&Event::UserPrompt { session: SessionId(1), text: "hi".into() });
        let ctx = s.sessions.get(&SessionId(1)).unwrap();
        assert!(ctx.is_root());
        assert_eq!(ctx.history.len(), 1);
    }

    #[test]
    fn plugin_up_registers_tools_then_down_removes_them() {
        let mut s = SystemState::default();
        let pid = PluginId("cerebro".into());
        s.apply(&Event::PluginUp { plugin: pid.clone(),
            tools: vec![spec("cerebro.recall"), spec("cerebro.store")] });
        assert_eq!(s.tools.len(), 2);
        assert_eq!(s.tools.get("cerebro.recall"), Some(&pid));

        s.apply(&Event::PluginDown { plugin: pid.clone(), reason: "exit".into() });
        assert!(s.tools.is_empty());
        assert!(s.plugins.is_empty());
    }

    #[test]
    fn subtree_collects_transitive_children() {
        let mut s = SystemState::default();
        s.apply(&Event::UserPrompt { session: SessionId(1), text: "root".into() });
        s.register_child(SessionId(2), SessionId(1));
        s.register_child(SessionId(3), SessionId(1));
        s.register_child(SessionId(4), SessionId(2));   // grandchild
        let mut tree = s.subtree(SessionId(1));
        tree.sort_by_key(|s| s.0);
        assert_eq!(tree, vec![SessionId(1), SessionId(2), SessionId(3), SessionId(4)]);
    }

    #[test]
    fn approval_pending_then_resolved_clears_state() {
        let mut s = SystemState::default();
        let call = ToolCall { id: ActionId(7), tool: "shell.exec".into(),
            args: serde_json::json!({"cmd":"ls"}), needs_approval: true };
        s.apply(&Event::ApprovalPending { session: SessionId(1), call });
        assert_eq!(s.pending_approvals.len(), 1);
        s.apply(&Event::UserApproval { session: SessionId(1), action: ActionId(7), granted: true });
        assert!(s.pending_approvals.is_empty());
    }
}

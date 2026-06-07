# agent crate — turn engine

> Load this when working on the `agent` crate (build-order step 4).

## Status
- [x] Anthropic streaming client wired (raw HTTP + SSE, no SDK)
- [x] Thinking-block retention across tool round-trips
- [x] Semaphore capping concurrent API calls
- [x] Tool request → bus event → tool result → continue loop
- [x] `Provider` trait for multi-provider support (OpenRouter slot-in ready)
- [x] `soul.md` system prompt loaded at startup into `Arc<RwLock<String>>`; hot-reload-ready (Phase 2)

## File layout
```
crates/agent/src/
  provider.rs   — Provider trait + Chunk enum
  anthropic.rs  — AnthropicProvider (SSE streaming)
  turn.rs       — TurnEngine + run_turn()
```

## Critical footgun: thinking blocks
Assistant thinking blocks MUST be retained in the message history across tool
round-trips. Dropping them causes the API to reject the continuation.
Pattern: append the full assistant turn (text + thinking + tool_use blocks)
before appending tool_result, then loop.

With Opus 4.7+ and `thinking: {type: "adaptive"}` (default display "omitted"),
the thinking TEXT is empty but the SIGNATURE is always present and must be kept.

## Streaming events (Anthropic SSE)
- `content_block_start` → init block state by index
- `content_block_delta` with `thinking_delta` → accumulate thinking text
- `content_block_delta` with `signature_delta` → accumulate signature (end of thinking block)
- `content_block_delta` with `text_delta` → accumulate text
- `content_block_delta` with `input_json_delta` → accumulate tool input JSON
- `content_block_stop` → emit complete `ThinkingBlock`, `TextBlock`, or `ToolUse` chunk
- `message_stop` → emit `Done`

## Provider trait
```rust
#[async_trait]
pub trait Provider: Send + Sync {
    async fn messages_stream(
        &self,
        history: &[Message],
        tools: &[ToolSpec],
        system: Option<&str>,
    ) -> anyhow::Result<ChunkStream>;
}
```
To add OpenRouter: implement `OpenAICompatProvider`, pass to `TurnEngine::new`. Zero turn loop changes.

## Concurrency
`tokio::sync::Semaphore` on `TurnEngine`. Default `max_concurrent = 16`.
Permit acquired before each streaming call, released when the round-trip completes.

## Tool dispatch timing
Subscribe to broadcast BEFORE emitting `ToolRequested` to avoid missing results
that arrive before the receiver is set up.

## System prompt (soul.md)

`TurnEngine.system` is `Arc<RwLock<String>>` (not `Option<String>`).
- `new(provider, max, Some(soul_content))` — wraps in Arc, ready to hot-swap
- `system_arc()` — returns clone of the Arc; Phase 2 evolution handler writes here
- `with_system(None)` — child inherits parent's Arc (sub-agents share soul)
- `with_system(Some(s))` — child gets isolated Arc (explicit override, unaffected by hot-reloads)

Load order: `AGENTD_SOUL` env → `/etc/agentd/soul.md` → `config/soul.md` (dev) → empty (no system prompt).

`run_turn` reads the string each call: `.read().await.clone()` → pass as `Option<&str>` to `messages_stream`.

## main.rs wiring
- `Arc<RwLock<HashMap<PluginId, Vec<ToolSpec>>>>` — updated on PluginUp/PluginDown
- `Arc<Mutex<HashMap<SessionId, Vec<Message>>>>` — per-session history
- Agent listener task spawned before supervisor to catch early PluginUp events
- `UserPrompt` → clone history + tools → `tokio::spawn(run_turn(...))`

## Reference
- `docs/reference/core_loops.rs` — Loop 2 (agent turn engine shape)

## Notes
- `reqwest::Client` is cheaply cloneable (Arc-backed); safe to keep one in `AnthropicProvider`
- SSE buffer: processes complete lines from byte chunks; handles chunk boundaries in mid-line
- Integration test (ignored): `cargo test -p apexos-agent -- --include-ignored integration_live_stream`

# plugins crate — MCP-over-stdio supervisor

> Load this when working on the `plugins` crate (build-order step 3).

## Status
- [x] MCP handshake implemented
- [x] Subprocess spawn + stdin/stdout wiring
- [x] Tool registry populated from server capabilities (66 tools from CerebroCortex)
- [x] Restart policy (always / on-failure)
- [x] Tested against real CerebroCortex binary

## CerebroCortex entrypoint (dev)
`/home/andre/Projects/CerebroCortex/cerebro-mcp`
Startup time: ~2–3 seconds (loads sentence-transformers embeddings).

## Resolved
- **MCP framing**: newline-delimited JSON (one JSON object per line + `\n`). No Content-Length headers. Protocol version `"2024-11-05"` negotiated.
- **tokio::process::Command**: async spawn is fine; stdin/stdout are async read/write. `kill_on_drop(true)` prevents subprocess leaks.

## Reference
- `docs/reference/core_loops.rs` — Loop 3 (plugin supervisor shape)
- `agentd/config/plugins.toml` — plugin declarations

## Notes
- Drain child stderr in a background task immediately after spawn — if it fills the OS pipe buffer, the child blocks and the handshake hangs
- `McpClient::attach` takes `&mut Child` and calls `.take()` on stdin/stdout; the `Child` value can then be moved into a watcher task for `.wait()`
- Server notifications (no `id` field) are silently dropped — extend reader task if notification handling is needed
- `PluginId` needs `Display` impl (added to `core/types.rs`) — format strings use `{id}` not `{:?}`
- CerebroCortex startup: ~6s in integration test (loads sentence-transformers). The binary at `/home/andre/Projects/CerebroCortex/cerebro-mcp` runs the venv Python automatically.
- Integration tests: run with `cargo test -p apexos-plugins -- --include-ignored cerebro --nocapture`

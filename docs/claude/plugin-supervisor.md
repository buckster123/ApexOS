# plugins crate — MCP-over-stdio supervisor

> Load this when working on the `plugins` crate (build-order step 3).

## Status
- [ ] MCP handshake implemented
- [ ] Subprocess spawn + stdin/stdout wiring
- [ ] Tool registry populated from server capabilities
- [ ] Restart policy (always / on-failure)
- [ ] Tested against real CerebroCortex binary

## CerebroCortex entrypoint (dev)
`/home/andre/Projects/CerebroCortex/cerebro-mcp`
Startup time: ~2–3 seconds (loads sentence-transformers embeddings).

## Deferred — resolve at keyboard
- Exact MCP framing: initialize request/response format vs real binary
- Whether to use `tokio::process::Command` or `std::process::Command` + blocking thread

## Reference
- `docs/reference/core_loops.rs` — Loop 3 (plugin supervisor shape)
- `agentd/config/plugins.toml` — plugin declarations

## Notes
<!-- Fill in: MCP protocol quirks, subprocess lifecycle gotchas, restart policy edge cases -->

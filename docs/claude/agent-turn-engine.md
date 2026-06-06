# agent crate — turn engine

> Load this when working on the `agent` crate (build-order step 4).

## Status
- [ ] Anthropic streaming client wired
- [ ] Thinking-block retention across tool round-trips
- [ ] Semaphore capping concurrent API calls
- [ ] Tool request → bus event → tool result → continue loop

## Critical footgun: thinking blocks
Assistant thinking blocks MUST be retained in the message history across tool
round-trips. Dropping them causes the API to reject the continuation.
Pattern: append the full assistant turn (text + thinking + tool_use blocks)
before appending tool_result, then loop.

## Concurrency
`tokio::sync::Semaphore` on the inference client. Cap = `max_concurrent` from
`policy.toml [subagents]`. Default: 16.

## Reference
- `docs/reference/core_loops.rs` — Loop 2 (agent turn engine shape)

## Notes
<!-- Fill in: streaming quirks, API error handling patterns, semaphore sizing -->

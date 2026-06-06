# gateway crate — WebSocket server

> Load this when working on the `gateway` crate (build-order step 2).

## Status
- [x] axum WebSocket endpoint at ws://localhost:8787
- [x] Intent parsing: UserPrompt, UserApproval, UserCancel → Event on bus
- [x] State stream: broadcast::Receiver → WebSocket send loop
- [x] Multiple simultaneous connections supported

## Protocol
Stateless by design — the gateway holds NO authoritative state.
- **Client → server:** JSON intent objects (`{type: "user_prompt", text: "..."}`)
- **Server → client:** JSON `Event` stream (same type as on the internal bus)

## Reference
- `docs/reference/core_loops.rs` — Bus shape (what gateway subscribes to)
- `docs/reference/core_types.rs` — Event variants the frontend receives

## Notes
- axum 0.8: `axum::serve()` returns `Serve` (implements `IntoFuture`, NOT `Future`) — must `.await` inside an async block when spawning
- `bcast.subscribe()` must happen synchronously at the top of `handle_socket` before any tasks are spawned, to guarantee no events are missed
- `Message::Text` takes `String` in axum 0.8 (`json.into()` works from `String`)
- Integration tests need a brief `tokio::time::sleep(20ms)` after `connect_async` to let `handle_socket` subscribe before the first event fires

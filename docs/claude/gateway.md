# gateway crate — WebSocket server

> Load this when working on the `gateway` crate (build-order step 2).

## Status
- [ ] axum WebSocket endpoint at ws://localhost:8787
- [ ] Intent parsing: UserPrompt, UserApproval, UserCancel → Event on bus
- [ ] State stream: broadcast::Receiver → WebSocket send loop
- [ ] Multiple simultaneous connections supported

## Protocol
Stateless by design — the gateway holds NO authoritative state.
- **Client → server:** JSON intent objects (`{type: "user_prompt", text: "..."}`)
- **Server → client:** JSON `Event` stream (same type as on the internal bus)

## Reference
- `docs/reference/core_loops.rs` — Bus shape (what gateway subscribes to)
- `docs/reference/core_types.rs` — Event variants the frontend receives

## Notes
<!-- Fill in: axum version, WebSocket upgrade pattern, reconnect behavior -->

# gateway crate — WebSocket server + HTTP API

> Load this when working on the `gateway` crate (build-order step 2 + step 9).

## Status
- [x] axum WebSocket endpoint at ws://localhost:8787/ws
- [x] Intent parsing: UserPrompt, UserApproval, UserCancel → Event on bus
- [x] State stream: broadcast::Receiver → WebSocket send loop
- [x] Multiple simultaneous connections supported
- [x] Static UI files served via custom tokio::fs handler (GET /, /style.css, /app.js)
- [x] GET /api/status — returns `{"api_key_set": bool}`
- [x] POST /api/key — updates shared api_key Arc<RwLock<String>>, persists to /var/lib/agentd/.api_key

## Protocol
Stateless by design — the gateway holds NO authoritative state.
- **Client → server:** JSON intent objects (`{type: "user_prompt", text: "..."}`)
- **Server → client:** JSON `Event` stream (same type as on the internal bus)

## GatewayState fields
| Field | Type | Purpose |
|-------|------|---------|
| `bus` | `BusHandle` | emit inbound intents onto the bus |
| `bcast` | `broadcast::Sender<Event>` | subscribe for outbound events |
| `api_key` | `Arc<RwLock<String>>` | shared with AnthropicProvider; browser UI updates take effect immediately |
| `ui_dir` | `PathBuf` | resolved from `AGENTD_UI` env var; passed as state so the static handler can read from it |

## Static file serving
`tower-http`'s `ServeDir` returns 404 inside `systemd`'s `ProtectSystem=strict` mount
namespace even when files are readable (async fs I/O fails in that namespace).
Fix: explicit `fallback(static_handler)` that uses `tokio::fs::read` directly.
Only three filenames are served (`index.html`, `style.css`, `app.js`) — no path traversal possible.

UI files must live under `ReadWritePaths=` in the service:
```
AGENTD_UI=/var/lib/agentd/ui
```

## Reference
- `docs/reference/core_loops.rs` — Bus shape (what gateway subscribes to)
- `docs/reference/core_types.rs` — Event variants the frontend receives

## Notes
- axum 0.8: `axum::serve()` returns `Serve` (implements `IntoFuture`, NOT `Future`) — must `.await` inside an async block when spawning
- `bcast.subscribe()` must happen synchronously at the top of `handle_socket` before any tasks are spawned, to guarantee no events are missed
- `Message::Text` takes `String` in axum 0.8 (`json.into()` works from `String`)
- Integration tests need a brief `tokio::time::sleep(20ms)` after `connect_async` to let `handle_socket` subscribe before the first event fires
- `serve()` no longer takes `ui_dir` parameter — it lives in `GatewayState`

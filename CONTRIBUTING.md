# Contributing to ApexOS

Thanks for your interest. This project moves fast and welcomes contributors.
Read `ARCHITECTURE.md` first — it explains the system design and the patterns
you'll be working within.

---

## Prerequisites

- **Rust** — stable toolchain (`rustup update stable`)
- **Raspberry Pi 5** — target hardware; 8GB RAM recommended. The system is
  designed for Pi 5 but the daemon will build and run on x86 Linux for development.
- **ffmpeg** — needed for audio tools and voice I/O
- **Python 3.11+** — for CerebroCortex (memory system MCP plugin)

Optional but useful on the Pi:
- espeak-ng (TTS), whisper.cpp (STT), piper (neural TTS)
- vastai CLI (`pip install vastai`) for GPU rental features

---

## Dev environment

### Local (x86) build — fast iteration, no Pi needed

```bash
git clone https://github.com/buckster123/ApexOS
cd ApexOS

# Build the daemon
cd agentd && cargo build

# Build the MCP tools
cd ../tools && cargo build
```

Run tests:
```bash
cd agentd && cargo test         # 42 tests, 0 failures expected
```

Set env and run locally:
```bash
export ANTHROPIC_API_KEY=sk-...
export AGENTD_UI_DIR=$(pwd)/ui
cd agentd && cargo run
# Open http://localhost:8787
```

### Pi deploy

See `docs/install.md` for the full first-time setup.

For iterating on an already-running Pi:

```bash
# On your machine — commit and push first, then:
ssh apexos@<pi-ip>

# On the Pi:
cd ~/ApexOS && git pull
cd agentd && ~/.cargo/bin/cargo build --release   # ~1 min on Pi 5
cd ../tools && ~/.cargo/bin/cargo build --release # ~8s

# Hot-swap (stop → copy → start):
sudo systemctl stop agentd
sudo cp agentd/target/release/agentd /usr/local/bin/agentd
sudo cp tools/target/release/apexos-tools /usr/local/bin/apexos-tools
sudo cp ui/* /var/lib/agentd/ui/   # use absolute paths, ~ expands to root under sudo
sudo systemctl start agentd
```

**Never scp x86 binaries to the Pi** — always build on the Pi (aarch64).

---

## Common contribution patterns

### Adding a new agent-callable tool

There are two kinds: **MCP tools** (in `apexos-tools`, separate process) and
**virtual tools** (in `agentd` supervisor, native Rust).

**MCP tool** (simpler — add to the existing `apexos-tools` server):

1. Add tool spec JSON to `list()` in `tools/crates/apexos-tools/src/tools.rs`
2. Add match arm to `call()` in the same file
3. Implement the function — synchronous, shells out via `std::process::Command`
4. Build `tools/` workspace, deploy `apexos-tools` binary
5. No agentd restart needed — supervisor auto-restarts plugin processes

**Virtual tool** (needs agentd rebuild — for tools that need bus access or shared state):

1. Add `{name}_spec()` in `agentd/crates/agentd/src/main.rs`
2. Register it in `gather_tools()`
3. Add handler in `agentd/crates/plugins/src/supervisor.rs` under the `tools/call` match
4. Full rebuild + redeploy of agentd required

### Adding a gateway API route

In `agentd/crates/gateway/src/lib.rs`:

1. Add the route to the `Router` in `router()`
2. Write an async handler fn (`async fn my_handler(...) -> impl IntoResponse`)
3. Add `#[derive(Deserialize)]` structs for request bodies/query params
4. Update `agentd/crates/gateway/tests/echo.rs` if `GatewayState` gains a new field

### Adding a desktop UI window

1. **`ui/desktop.html`** — add start menu button + `#win-{id}-content` div
2. **`ui/desktop-app.js`** — add entry to `WIN_DEFAULTS` (title, dimensions), hook in `openWin()` and `onclose`
3. **`ui/desktop-style.css`** — add `.{id}-win` and component styles

No build step. Deploy with `sudo cp ui/* /var/lib/agentd/ui/`.

### Adding a new inference backend

The `RoutingProvider` in `agentd/crates/agent/src/provider.rs` delegates to
either `AnthropicProvider` or `OaiProvider` based on `backend_arc`. Any
OpenAI-compatible API (Together, Groq, LM Studio, etc.) works via `OaiProvider`
without code changes — just set `AGENTD_BACKEND=ollama` and `AGENTD_OAI_BASE_URL`.

---

## Testing

```bash
cd agentd && cargo test
```

Tests live in:
- `agentd/crates/core/src/state.rs` — unit tests for `apply()`
- `agentd/crates/agent/src/` — agent turn engine tests
- `agentd/crates/plugins/src/policy.rs` — policy engine tests
- `agentd/crates/gateway/tests/echo.rs` — WebSocket integration tests

For UI changes, manual testing in a browser is required. Run agentd locally
with `AGENTD_UI_DIR=./ui` and open `http://localhost:8787/desktop.html`.

---

## Code conventions

**Rust:**
- No comments unless the why is non-obvious
- No `unwrap()` in production paths — use `?` or explicit error handling
- `tokio::spawn` for background work; keep handlers non-blocking
- New `Event` variants must be handled in `core/src/state.rs` `apply()` (even as no-ops)
- Virtual tools live in `supervisor.rs`; main.rs only has specs + wiring

**JavaScript (UI):**
- Vanilla JS — no framework, no build step
- `async/await` for all API calls
- New windows follow the `WIN_DEFAULTS` + `openWin` / `onclose` pattern

**Git:**
- Commit message format: `implement {thing}`, `fix {thing}`, `add {thing}` — imperative, lowercase
- Tests pass → commit → push (no WIP commits on main)
- Docs update in the same commit as the code it describes

---

## PR process

1. Fork and branch from `main`
2. Make your change with tests
3. `cargo test` passes in `agentd/`
4. Open a PR — describe what you changed and why
5. For Pi-specific features, note if you've tested on hardware or only x86

Small, focused PRs are preferred. A single well-tested feature beats a large diff.

---

## Project structure for first-time contributors

If this is your first time in the codebase, the suggested reading order:

1. `README.md` — one-page overview
2. `ARCHITECTURE.md` — system design and key patterns
3. `CLAUDE.md` — current build state and locked decisions (updated with every step)
4. `agentd/crates/core/src/types.rs` — the Event enum and core types
5. `agentd/crates/gateway/src/lib.rs` — all API routes (good map of what exists)
6. `docs/claude/` — subsystem-specific deep dives (load only what you need)

The `docs/archive/` folder has historical planning docs that explain the *why*
behind major design decisions if you want more context.

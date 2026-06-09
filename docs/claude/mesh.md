# ApexOS Mesh — Multi-Pi Colony Architecture

Design document for Step 35. Load this when working on mesh discovery,
bootstrap, peer routing, or cross-node A2A.

---

## Vision

Each Pi runs a full **agent colony** — one main `agentd`, N sub-agents,
one council. Colonies connect into a mesh. The mesh is:

- **Self-propagating** — a new Pi on the LAN gets bootstrapped by the
  nearest colony with no human steps beyond plugging it in
- **Decentralised** — no central coordinator; any node can bootstrap any
  other; Cerebro is per-node by default, synced on council events
- **Autonomy-gated** — bootstrap requires human approval in `suggest`
  mode, fires automatically in `yolo`
- **Arbitrarily scalable** — add a Pi, add a colony; ~$80/node

---

## Node Identity

Every node needs a **stable, human-readable ID** that survives IP changes
(DHCP). Use the system hostname: `apex-kitchen`, `apex-garage`, etc.

```
node_id  = hostname of the Pi  (set during install)
ws_url   = ws://<ip>:8787/ws   (resolved at runtime via mDNS or peers.toml)
```

`node_id` is set once at install time via `hostnamectl set-hostname apex-<name>`.
Avahi then advertises it as `apex-kitchen.local` — stable across reboots and
IP changes as long as the hostname doesn't change.

---

## Component Map

```
┌─── apex-main (Pi 5) ──────────────────────────────────────────┐
│  agentd                                                         │
│    ├── discovery_task  (60s mDNS poll → PeerSeen events)        │
│    ├── peer_registry   (peers.toml, hot-reload)                 │
│    ├── bootstrap_node  (virtual tool → SSH sequence)            │
│    └── mesh_router     (cross-node send_to_agent forwarding)    │
│  Cerebro (local)                                                │
└───────────────────────────────────────────────────────────────┘
         │ peers.toml WS endpoints │
┌─── apex-kitchen (Pi 5) ─────┐   ┌─── apex-garage (Pi 4) ──────┐
│  full colony                 │   │  full colony                  │
│  own Cerebro                 │   │  own Cerebro                  │
└──────────────────────────────┘   └───────────────────────────────┘
```

---

## 1. mDNS Advertisement

Every node registers an Avahi service on startup so other nodes can find it
without any config.

**`/etc/avahi/services/apexos.service`** (deployed by `install.sh`):

```xml
<?xml version="1.0" standalone='no'?>
<!DOCTYPE service-group SYSTEM "avahi-service.dtd">
<service-group>
  <name replace-wildcards="yes">ApexOS %h</name>
  <service>
    <type>_apexos._tcp</type>
    <port>8787</port>
    <txt-record>node_id=%h</txt-record>
    <txt-record>version=1</txt-record>
  </service>
</service-group>
```

`%h` = hostname. The `version=1` TXT record lets future nodes refuse
bootstrap of incompatible versions.

**Query** — any node can discover peers via:
```bash
avahi-browse -rpt _apexos._tcp
```
Output is parseable: `hostname;ip;port` per line.

**GET /api/mesh/nodes** — gateway runs `avahi-browse` and returns JSON:
```json
[{ "node_id": "apex-kitchen", "ip": "192.168.0.201", "port": 8787,
   "ws_url": "ws://192.168.0.201:8787/ws", "known": true }]
```
`known: true` = already in `peers.toml`.

---

## 2. Peer Registry

**`/etc/agentd/peers.toml`** — hot-reloadable (same mechanism as `policy.toml`):

```toml
[[peer]]
node_id = "apex-kitchen"
ws_url  = "ws://192.168.0.201:8787/ws"
role    = "full"       # full | sensor | thin

[[peer]]
node_id = "apex-garage"
ws_url  = "ws://192.168.0.202:8787/ws"
role    = "full"
```

### New Event variants (in `core/src/types.rs`):

```rust
PeerRegistered { node_id: String, ws_url: String, role: String },
PeerLost       { node_id: String },
PeerSeen       { node_id: String, ip: String },   // mDNS discovery hit
```

`PeerSeen` triggers the autonomy gate. `PeerRegistered` fires when a peer
is written to `peers.toml` (either by bootstrap_node or manual `POST /api/mesh/peers`).

### New API routes:

```
GET  /api/mesh/nodes        → avahi-browse result (discovered + known)
GET  /api/mesh/peers        → peers.toml contents as JSON
POST /api/mesh/peers        → add peer { node_id, ws_url, role }
DELETE /api/mesh/peers/:id  → remove peer (writes peers.toml)
```

---

## 3. Bootstrap Virtual Tool

`bootstrap_node` is a **virtual tool** (handled in `agent/src/virtual_tools.rs`
alongside `agent_spawn`, `schedule_task`, etc.).

### Input schema:

```json
{
  "ip":        "192.168.0.203",
  "ssh_user":  "apexos",          // default: "apexos"
  "ssh_pass":  "...",
  "api_key":   "sk-ant-...",      // optional: inject Anthropic key
  "node_name": "apex-office",     // sets hostname; default: apex-<last-ip-octet>
  "role":      "full"             // full | sensor
}
```

### Execution sequence (all steps via `run_command` with sshpass):

```
Step 1: CHECK — is apexos already installed?
  ssh: systemctl is-active agentd 2>/dev/null && echo "exists"
  → if "exists": skip to step 6 (re-register only)

Step 2: PREP — ensure git + avahi + curl on target
  ssh: apt-get install -y git curl avahi-daemon avahi-utils

Step 3: CLONE — get the repo
  ssh: git clone https://github.com/buckster123/ApexOS /home/$ssh_user/ApexOS
    or: git -C /home/$ssh_user/ApexOS pull  (if already cloned)

Step 4: INSTALL — run install.sh in background, tail log
  ssh: bash /home/$ssh_user/ApexOS/install.sh &> /tmp/apex-install.log &
  poll: tail -1 /tmp/apex-install.log  (every 30s, up to 30min)
  done signal: grep "install complete" /tmp/apex-install.log

Step 5: CONFIGURE
  ssh: echo "$api_key" | sudo tee /etc/agentd/env  (if api_key provided)
  ssh: hostnamectl set-hostname $node_name
  ssh: systemctl enable --now agentd

Step 6: VERIFY
  ssh: curl -sf http://localhost:8787/api/status | grep -q "ok"

Step 7: REGISTER
  local: POST /api/mesh/peers { node_id, ws_url, role }
  local: store_memory in Cerebro (tag: mesh, node_id, ip)
  bus: emit PeerRegistered
```

### Async problem & solution

`run_command` has a 30s timeout; a full Rust build takes 5-15min on Pi 5.

**Solution**: step 4 runs `install.sh` backgrounded (`&`), then polls with
short commands every 30s. Each poll is a separate `run_command` call (fast).
The virtual tool manages its own polling loop in a `tokio::spawn` task,
emitting progress events on the bus:

```rust
Event::ToolProgress { session, call_id, message: "bootstrap: building agentd (2/5)..." }
```

Frontend shows these as live status updates in the tool call card.

### Idempotency

Step 1 detects an existing install and skips to re-registration. Running
`bootstrap_node` twice on the same node is safe: it re-verifies, re-registers
the peer, and updates `peers.toml` without changing the remote system.

---

## 4. Autonomy Gate (Discovery Loop)

Background task in `main.rs` alongside the sensor wakeup loop:

```rust
tokio::spawn(async move {
    let mut known: HashSet<String> = load_peers().map(|p| p.node_id).collect();
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let nodes = query_avahi().await;          // runs avahi-browse
        for node in nodes {
            if known.contains(&node.node_id) { continue; }
            known.insert(node.node_id.clone());
            bus.emit(Event::PeerSeen { node_id: node.node_id.clone(), ip: node.ip.clone() }).await;

            let prompt = format!(
                "New ApexOS node detected on LAN: {} at {}. \
                 Bootstrap and add to mesh? Use bootstrap_node tool if yes.",
                node.node_id, node.ip
            );
            bus.emit(Event::UserPrompt { session: SessionId(0), text: prompt }).await;
            // In suggest mode the agent sees this and decides.
            // In yolo mode the agent will call bootstrap_node immediately.
        }
    }
});
```

`known` is seeded from `peers.toml` at startup so already-registered nodes
don't trigger re-prompt.

**Security note**: mDNS is LAN-only and unauthenticated. The `version=1`
TXT record is a weak signal. Subnet guard: only emit `PeerSeen` for IPs in
the same /24 as the local eth0 IP. This prevents bootstrap storms across
routed networks. Configurable via `MESH_SUBNET_GUARD=192.168.0.0/24` env var.

---

## 5. Cross-Node A2A Routing

Current `send_to_agent` targets a local `session_id` (u64). Extend with
optional `node` field:

```json
{ "session_id": 0, "node": "apex-kitchen", "message": "hello from main" }
```

Gateway resolver:

```rust
if let Some(node_id) = args["node"].as_str() {
    // Look up ws_url from peers.toml
    // POST to http://{peer_ip}:8787/api/sessions/0/message
    // → injects into root session on target node
}
```

Uses HTTP POST (not WS) for fire-and-forget reliability. The target's
existing `/api/sessions/{id}/message` route handles it unchanged.

**Root session (0)** is the target by default when no session_id is given.
Agents can query `/api/sessions/active` on the peer to find a specific session.

---

## 6. New env vars

```
MESH_AUTO_BOOTSTRAP=false      # true = auto-bootstrap in yolo, false = prompt
MESH_SUBNET_GUARD=             # CIDR to restrict auto-discovery (default: local /24)
MESH_DISCOVERY_INTERVAL=60     # seconds between mDNS polls
PEERS_TOML=/etc/agentd/peers.toml
```

---

## 7. install.sh additions

To make a fresh Pi mesh-ready:

```bash
# Install avahi
apt-get install -y avahi-daemon avahi-utils

# Drop service file
cat > /etc/avahi/services/apexos.service << 'EOF'
<?xml version="1.0" standalone='no'?>
<!DOCTYPE service-group SYSTEM "avahi-service.dtd">
<service-group>
  <name replace-wildcards="yes">ApexOS %h</name>
  <service>
    <type>_apexos._tcp</type>
    <port>8787</port>
    <txt-record>node_id=%h</txt-record>
    <txt-record>version=1</txt-record>
  </service>
</service-group>
EOF

systemctl enable --now avahi-daemon

# Create empty peers.toml if missing
[ -f /etc/agentd/peers.toml ] || echo "# ApexOS mesh peers" > /etc/agentd/peers.toml
chown agentd:agentd /etc/agentd/peers.toml
```

Add `peers.toml` to `ReadWritePaths` in `agentd.service`.

---

## 8. Repo layout additions

```
agentd/
  crates/
    agentd/src/
      mesh.rs          # PeerRegistry, discovery loop, avahi query
    agent/src/
      virtual_tools/
        bootstrap_node.rs   # SSH bootstrap sequence + polling
    gateway/src/
      mesh_routes.rs   # /api/mesh/* handlers
/etc/agentd/
  peers.toml           # peer registry (new config file)
deploy/
  apexos.avahi.service # Avahi service advertisement file
```

---

## 9. Build phases

| Phase | What | Testable independently |
|-------|------|----------------------|
| 35a | Avahi service file + `install.sh` additions + `GET /api/mesh/nodes` (avahi-browse wrapper) | Yes — `curl /api/mesh/nodes` returns discovered peers |
| 35b | `peers.toml` struct + `PeerRegistry` hot-reload + `GET/POST/DELETE /api/mesh/peers` | Yes — add peer, verify event on bus |
| 35c | `bootstrap_node` virtual tool — SSH sequence, polling loop, `ToolProgress` events | Yes — bootstrap a second Pi from agent prompt |
| 35d | Discovery loop — mDNS poll, `PeerSeen` event, autonomy gate + `MESH_AUTO_BOOTSTRAP` | Yes — plug in Pi, watch log |
| 35e | Cross-node A2A — `send_to_agent` with `node:` field, HTTP proxy to peer | Yes — send message from main to kitchen node |

Phases are independent. 35a and 35b have zero risk. 35c is the exciting one.
35d and 35e can follow after 35c is proven.

---

## 10. Open edges

| Edge | Resolution |
|------|-----------|
| **SSH key rotation after bootstrap** | After step 5, agent can `ssh-copy-id` its own key so future SSH doesn't need plaintext password. Low priority — LAN only. |
| **API key propagation** | bootstrap_node takes optional `api_key` arg. If omitted, agent prompts human. Never stored in Cerebro or logs — only written to `/etc/agentd/env` on target (chmod 600). |
| **Peer goes offline** | Discovery loop detects disappearance after 3 missed polls → emits `PeerLost`, sets peer status `offline` in peers.toml (doesn't remove). Agent notified. |
| **Build time variability** | Pi 5 ~5min, Pi 4 ~15min, Zero 2W ~45min. Poll interval in step 4 auto-adjusts: 30s checks, 30min hard timeout. Timeout emits error event. |
| **install.sh doesn't exist yet** | README references it. Need to write it. Can be extracted from CLAUDE.md deploy workflow. |
| **Circular bootstrap** | Node A bootstraps Node B, Node B sees Node A via mDNS, tries to bootstrap Node A. Guard: skip bootstrap if `node_id` already in `peers.toml`. |
| **Cross-node council** | Deferred. A2A (35e) is the prerequisite. Council can spawn personas on peer nodes as a later step once routing is proven. |

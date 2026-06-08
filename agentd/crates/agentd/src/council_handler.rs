use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use apexos_core::{BusHandle, CouncilAgentDef, Event, SessionId, ActionId, ToolOutput};
use apexos_agent::run_council;

/// Message type: (calling session, tool call id, raw convene_council args)
pub type CouncilMsg = (SessionId, ActionId, serde_json::Value);

/// Spawn the council handler task.
///
/// Receives `convene_council` tool calls from the supervisor and runs them
/// as isolated tokio tasks so multiple councils can run concurrently.
pub fn spawn_council_handler(
    mut rx:          mpsc::Receiver<CouncilMsg>,
    bcast:           broadcast::Sender<Event>,
    bus:             BusHandle,
    anthropic_key:   Arc<RwLock<String>>,
    oai_api_key:     Arc<RwLock<String>>,
    oai_base_url:    Arc<RwLock<String>>,
    backend_arc:     Arc<RwLock<String>>,
    model_arc:       Arc<RwLock<String>>,
) {
    tokio::spawn(async move {
        // council id counter — simple sequential, sufficient for uniqueness within a run
        let mut counter: u64 = 0;

        while let Some((session, call_id, args)) = rx.recv().await {
            counter += 1;
            let council_id = format!("c{counter}");

            let topic = args["topic"].as_str().unwrap_or("").to_owned();
            let max_rounds = args["max_rounds"].as_u64().unwrap_or(3) as u32;
            let consensus_threshold = args["consensus_threshold"].as_f64().unwrap_or(0.7) as f32;

            // Parse agents array — supports both string IDs and objects
            let agent_defs: Vec<CouncilAgentDef> = match args["agents"].as_array() {
                Some(arr) => arr.iter().filter_map(parse_agent_def).collect(),
                None => {
                    let bus_c = bus.clone();
                    let call_id_c = call_id;
                    tokio::spawn(async move {
                        bus_c.emit(Event::ToolResult {
                            session,
                            call: call_id_c,
                            output: ToolOutput {
                                ok: false,
                                content: serde_json::json!("convene_council: 'agents' must be an array"),
                            },
                        }).await;
                    });
                    continue;
                }
            };

            if agent_defs.is_empty() {
                let bus_c = bus.clone();
                let call_id_c = call_id;
                tokio::spawn(async move {
                    bus_c.emit(Event::ToolResult {
                        session,
                        call: call_id_c,
                        output: ToolOutput {
                            ok: false,
                            content: serde_json::json!("convene_council: at least one agent required"),
                        },
                    }).await;
                });
                continue;
            }

            // Clone arcs for the spawned task
            let ant_key   = Arc::clone(&anthropic_key);
            let oai_key   = Arc::clone(&oai_api_key);
            let oai_url   = Arc::clone(&oai_base_url);
            let bus_c     = bus.clone();
            let bcast_c   = bcast.clone();

            // butt_in channel — gateway can inject human messages mid-council
            // (capacity 4; unread messages accumulate until next round checks them)
            let (_butt_in_tx, butt_in_rx) = mpsc::channel::<String>(4);

            let default_backend = backend_arc.read().await.clone();
            let default_model   = model_arc.read().await.clone();

            tokio::spawn(async move {
                let synthesis = run_council(
                    council_id,
                    topic,
                    agent_defs,
                    max_rounds,
                    consensus_threshold,
                    ant_key,
                    oai_key,
                    oai_url,
                    default_backend,
                    default_model,
                    bus_c.clone(),  // _bus arg
                    bcast_c,
                    butt_in_rx,
                ).await;

                bus_c.emit(Event::ToolResult {
                    session,
                    call: call_id,
                    output: ToolOutput {
                        ok:      true,
                        content: serde_json::json!(synthesis),
                    },
                }).await;
            });
        }
    });
}

fn parse_agent_def(v: &serde_json::Value) -> Option<CouncilAgentDef> {
    if let Some(id) = v.as_str() {
        // Native shorthand: "AZOTH" → use native persona (council.rs resolves it)
        return Some(CouncilAgentDef {
            id:      id.to_owned(),
            persona: String::new(),  // empty → council engine uses native_persona()
            backend: None,
            model:   None,
            color:   None,
        });
    }
    if let Some(obj) = v.as_object() {
        let id = obj.get("id")?.as_str()?.to_owned();
        let persona = obj.get("persona")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_owned();
        return Some(CouncilAgentDef {
            id,
            persona,
            backend: obj.get("backend").and_then(|b| b.as_str()).map(str::to_owned),
            model:   obj.get("model").and_then(|m| m.as_str()).map(str::to_owned),
            color:   obj.get("color").and_then(|c| c.as_str()).map(str::to_owned),
        });
    }
    None
}

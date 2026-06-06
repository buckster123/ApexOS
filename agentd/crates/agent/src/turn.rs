use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Semaphore};
use apexos_core::{
    ActionId, ContentBlock, Event, BusHandle, Message, SessionId,
    ToolCall, ToolOutput, ToolSpec,
};
use futures_util::StreamExt;
use crate::provider::{Chunk, Provider};

pub struct TurnEngine {
    pub provider: Arc<dyn Provider>,
    pub sem:      Arc<Semaphore>,
    pub system:   Option<String>,
}

impl TurnEngine {
    pub fn new(
        provider: impl Provider + 'static,
        max_concurrent: usize,
        system: Option<String>,
    ) -> Self {
        Self {
            provider: Arc::new(provider),
            sem:      Arc::new(Semaphore::new(max_concurrent)),
            system,
        }
    }
}

/// Run one assistant turn (streaming + tool round-trips).
///
/// Returns the full updated history so the caller can persist it.
pub async fn run_turn(
    session: SessionId,
    mut history: Vec<Message>,
    bus: BusHandle,
    bcast: broadcast::Sender<Event>,
    tools: Vec<ToolSpec>,
    engine: Arc<TurnEngine>,
) -> anyhow::Result<Vec<Message>> {
    // Per-turn ActionId counter (simple; unique within this turn).
    let mut next_id: u64 = 1;
    // Maps our ActionId back to the Anthropic string id for tool_result blocks.
    let mut id_map: HashMap<ActionId, String> = HashMap::new();

    loop {
        // Bound concurrent API calls — released when _permit drops at end of loop body.
        let _permit = engine.sem.acquire().await?;

        let mut stream = engine.provider
            .messages_stream(&history, &tools, engine.system.as_deref())
            .await?;

        let mut assistant_blocks: Vec<ContentBlock> = Vec::new();
        let mut pending_tools:    Vec<ToolCall>     = Vec::new();

        while let Some(chunk) = stream.next().await {
            match chunk? {
                Chunk::TextDelta(t) => {
                    bus.emit(Event::AgentText { session, delta: t }).await;
                }
                Chunk::ThinkingDelta(t) => {
                    bus.emit(Event::AgentThinking { session, delta: t }).await;
                }
                Chunk::TextBlock(text) => {
                    assistant_blocks.push(ContentBlock::Text { text });
                }
                // CRITICAL: thinking blocks MUST be retained with their signature
                // or the API rejects the next turn in a tool-use loop.
                Chunk::ThinkingBlock { thinking, signature } => {
                    assistant_blocks.push(ContentBlock::Thinking { thinking, signature });
                }
                Chunk::ToolUse { id: api_id, name, input } => {
                    let action_id = ActionId(next_id);
                    next_id += 1;
                    id_map.insert(action_id, api_id.clone());
                    assistant_blocks.push(ContentBlock::ToolUse {
                        id:    api_id,
                        name:  name.clone(),
                        input: input.clone(),
                    });
                    pending_tools.push(ToolCall {
                        id:              action_id,
                        tool:            name,
                        args:            input,
                        needs_approval:  false,
                    });
                }
                Chunk::Done => break,
            }
        }

        // Commit full assistant turn — text + thinking + tool_use all together.
        history.push(Message::Assistant { content: assistant_blocks });

        if pending_tools.is_empty() {
            bus.emit(Event::TurnComplete { session }).await;
            return Ok(history);
        }

        // Subscribe before emitting ToolRequested so we can't miss results.
        let mut rx = bcast.subscribe();

        for call in &pending_tools {
            bus.emit(Event::ToolRequested { session, call: call.clone() }).await;
        }

        let tool_results = collect_tool_results(&mut rx, session, &pending_tools, &id_map).await?;
        history.push(Message::User { content: tool_results });
        // _permit drops here — reacquired at top of loop for next round-trip.
    }
}

async fn collect_tool_results(
    rx:      &mut broadcast::Receiver<Event>,
    session: SessionId,
    pending: &[ToolCall],
    id_map:  &HashMap<ActionId, String>,
) -> anyhow::Result<Vec<ContentBlock>> {
    let mut remaining: HashMap<ActionId, ()> = pending.iter().map(|c| (c.id, ())).collect();
    let mut results:   HashMap<ActionId, ToolOutput> = HashMap::new();

    while !remaining.is_empty() {
        match rx.recv().await {
            Ok(Event::ToolResult { session: s, call: action_id, output }) if s == session => {
                if remaining.remove(&action_id).is_some() {
                    results.insert(action_id, output);
                }
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return Err(anyhow::anyhow!("bus closed while awaiting tool results")),
        }
    }

    let mut out = Vec::with_capacity(pending.len());
    for call in pending {
        let output = results.remove(&call.id).unwrap_or(ToolOutput {
            ok:      false,
            content: serde_json::json!("missing result"),
        });
        let api_id = id_map.get(&call.id).cloned().unwrap_or_default();
        out.push(ContentBlock::ToolResult {
            tool_use_id: api_id,
            content:     output.content,
            is_error:    !output.ok,
        });
    }
    Ok(out)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChunkStream, Provider};
    use apexos_core::{Bus, SystemState};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use tokio::sync::Mutex;

    struct MockProvider {
        responses: Arc<Mutex<VecDeque<Vec<Chunk>>>>,
    }

    impl MockProvider {
        fn with_responses(resps: Vec<Vec<Chunk>>) -> Self {
            Self { responses: Arc::new(Mutex::new(VecDeque::from(resps))) }
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn messages_stream(
            &self,
            _history: &[Message],
            _tools: &[ToolSpec],
            _system: Option<&str>,
        ) -> anyhow::Result<ChunkStream> {
            let chunks = self.responses.lock().await
                .pop_front()
                .unwrap_or_default();
            let stream = futures_util::stream::iter(chunks.into_iter().map(Ok));
            Ok(Box::pin(stream))
        }
    }

    #[tokio::test]
    async fn text_only_turn_completes() {
        let (bus, handle, bcast) = Bus::new(SystemState::default());
        tokio::spawn(bus.run());

        let provider = MockProvider::with_responses(vec![vec![
            Chunk::TextDelta("hello".into()),
            Chunk::TextBlock("hello".into()),
            Chunk::Done,
        ]]);

        let engine = Arc::new(TurnEngine::new(provider, 1, None));
        let session = SessionId(1);
        let history = vec![Message::User {
            content: vec![ContentBlock::Text { text: "hi".into() }],
        }];

        let updated = run_turn(session, history, handle, bcast, vec![], engine)
            .await
            .unwrap();

        assert_eq!(updated.len(), 2, "user + assistant");
        match &updated[1] {
            Message::Assistant { content } => {
                assert!(content.iter().any(|b| matches!(b, ContentBlock::Text { text } if text == "hello")));
            }
            _ => panic!("expected assistant turn"),
        }
    }

    #[tokio::test]
    async fn thinking_blocks_retained_in_history() {
        let (bus, handle, bcast) = Bus::new(SystemState::default());
        tokio::spawn(bus.run());

        let provider = MockProvider::with_responses(vec![vec![
            Chunk::ThinkingDelta("reasoning...".into()),
            Chunk::ThinkingBlock { thinking: "reasoning...".into(), signature: "SIG".into() },
            Chunk::TextBlock("answer".into()),
            Chunk::Done,
        ]]);

        let engine = Arc::new(TurnEngine::new(provider, 1, None));
        let session = SessionId(1);
        let history = vec![Message::User {
            content: vec![ContentBlock::Text { text: "think".into() }],
        }];

        let updated = run_turn(session, history, handle, bcast, vec![], engine)
            .await
            .unwrap();

        match &updated[1] {
            Message::Assistant { content } => {
                assert!(content.iter().any(|b| {
                    matches!(b, ContentBlock::Thinking { signature, .. } if signature == "SIG")
                }), "thinking block with signature must be in assistant history");
            }
            _ => panic!("expected assistant turn"),
        }
    }

    #[tokio::test]
    async fn tool_round_trip() {
        let (bus, handle, bcast) = Bus::new(SystemState::default());
        tokio::spawn(bus.run());

        // First response: requests a tool call.
        // Second response: final text after tool result.
        let provider = MockProvider::with_responses(vec![
            vec![
                Chunk::ToolUse {
                    id:    "tid1".into(),
                    name:  "test.tool".into(),
                    input: serde_json::json!({"x": 1}),
                },
                Chunk::Done,
            ],
            vec![
                Chunk::TextBlock("done".into()),
                Chunk::Done,
            ],
        ]);

        let engine = Arc::new(TurnEngine::new(provider, 1, None));
        let session = SessionId(1);
        let history = vec![Message::User {
            content: vec![ContentBlock::Text { text: "use tool".into() }],
        }];

        // Simulate a tool executor: listen for ToolRequested, emit ToolResult.
        let bus_sim = handle.clone();
        let mut rx_sim = bcast.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = rx_sim.recv().await {
                if let Event::ToolRequested { session, call } = event {
                    bus_sim.emit(Event::ToolResult {
                        session,
                        call: call.id,
                        output: ToolOutput {
                            ok:      true,
                            content: serde_json::json!("result"),
                        },
                    }).await;
                }
            }
        });

        let updated = run_turn(session, history, handle, bcast, vec![], engine)
            .await
            .unwrap();

        // history: user → assistant(tool_use) → user(tool_result) → assistant(text)
        assert_eq!(updated.len(), 4);
    }
}

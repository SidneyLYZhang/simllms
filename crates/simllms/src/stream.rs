use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures_core::Stream;
use tokio::time::Sleep;

use crate::error::{ApiError, Error, TransportError};
use crate::reply::Usage;

/// What a streaming step delivered. The uniform sequence every provider is
/// normalized into: text deltas, reasoning deltas, and a final chunk
/// carrying finish reason and Usage.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Delta {
    Text { text: String },
    Reasoning { text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Chunk {
    pub index: u64,
    pub delta: Option<Delta>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
}

enum SseEvent {
    Data(String),
    Done,
}

/// Extracts the next complete SSE event from the buffer, draining consumed
/// bytes. `None` means no complete event yet. Comment lines, `event:` and
/// `id:` fields are ignored; multi-line data payloads are joined.
fn next_event(buffer: &mut Vec<u8>) -> Option<SseEvent> {
    loop {
        let (end, consumed) = find_event_end(buffer)?;
        let bytes: Vec<u8> = buffer.drain(..end + consumed).collect();
        let text = String::from_utf8_lossy(&bytes);
        let mut data_lines = Vec::new();
        for line in text.split('\n') {
            let line = line.trim_end_matches('\r');
            if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
            }
        }
        if data_lines.is_empty() {
            continue; // comment-only or event-only frame
        }
        let data = data_lines.join("\n");
        if data.trim() == "[DONE]" {
            return Some(SseEvent::Done);
        }
        return Some(SseEvent::Data(data));
    }
}

/// Finds the blank line that terminates an SSE event: `\n\n` or `\r\n\r\n`
/// (a lone `\r\r` also counts, defensively). Returns (event end, delimiter len).
fn find_event_end(buf: &[u8]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (pat, len) in [
        (&b"\n\n"[..], 2usize),
        (&b"\r\n\r\n"[..], 4usize),
        (&b"\r\r"[..], 2usize),
    ] {
        if let Some(pos) = find_sub(buf, pat) {
            if best.is_none() || pos < best.unwrap().0 {
                best = Some((pos, len));
            }
        }
    }
    best
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Normalizes one `data:` payload into the steps it carries. A provider may
/// pack reasoning and text into the same event, so one event yields a
/// sequence: reasoning first, then text — our fixed order, not the field
/// order on the wire, so a UI can keep its thinking pane collapsed while the
/// answer begins. A field the provider left empty carries nothing and is not
/// a step: reasoning models send `content: ""` on every reasoning event, and
/// those must not arrive as an empty answer. The event's finish reason and
/// Usage ride the last step, and every step of one event carries the same
/// choice index. A provider error event surfaces as `Api` with status 0 (no
/// HTTP status exists mid-stream); the raw event JSON is preserved as evidence.
fn parse_event(data: &str) -> Result<Vec<Chunk>, Error> {
    let v: serde_json::Value = serde_json::from_str(data).map_err(|e| {
        Error::Api(ApiError {
            status: 0,
            provider_message: format!("malformed stream event: {e}"),
            raw_body: data.to_string(),
        })
    })?;

    if let Some(err) = v.get("error") {
        let message = err
            .pointer("/message")
            .and_then(|m| m.as_str())
            .unwrap_or("provider sent a stream error event");
        return Err(Error::Api(ApiError {
            status: 0,
            provider_message: message.to_string(),
            raw_body: data.to_string(),
        }));
    }

    let choice = v.pointer("/choices/0");
    let delta = choice.and_then(|c| c.get("delta"));
    let text = delta
        .and_then(|d| d.get("content"))
        .and_then(|t| t.as_str());
    let reasoning = delta
        .and_then(|d| d.get("reasoning_content").or_else(|| d.get("reasoning")))
        .and_then(|t| t.as_str());

    let mut steps: Vec<Option<Delta>> = Vec::new();
    if let Some(step) = reasoning.filter(|value| !value.is_empty()) {
        steps.push(Some(Delta::Reasoning {
            text: step.to_string(),
        }));
    }
    if let Some(step) = text.filter(|value| !value.is_empty()) {
        steps.push(Some(Delta::Text {
            text: step.to_string(),
        }));
    }
    if steps.is_empty() {
        steps.push(None);
    }

    let usage = v.get("usage").filter(|u| u.is_object()).and_then(|u| {
        Some(Usage {
            prompt_tokens: u.get("prompt_tokens").and_then(|x| x.as_u64()),
            completion_tokens: u.get("completion_tokens").and_then(|x| x.as_u64()),
            total_tokens: u.get("total_tokens").and_then(|x| x.as_u64()),
        })
    });
    let finish_reason = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|f| f.as_str())
        .map(str::to_string);
    let index = choice
        .and_then(|c| c.get("index"))
        .and_then(|i| i.as_u64())
        .unwrap_or(0);

    let mut chunks: Vec<Chunk> = steps
        .into_iter()
        .map(|step| Chunk {
            index,
            delta: step,
            finish_reason: None,
            usage: None,
        })
        .collect();
    if let Some(last) = chunks.last_mut() {
        last.finish_reason = finish_reason;
        last.usage = usage;
    }

    Ok(chunks)
}

/// The stream returned by `chat_stream`. Implements `futures_core::Stream`
/// and exposes an inherent `next()`; dropping it cancels the request.
///
/// Timeouts: the first event must arrive within the first-token timeout and
/// every subsequent event within the idle timeout, or the stream yields a
/// `Transport` error and ends.
pub struct ChatStream {
    byte_stream: Pin<Box<dyn Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
    buffer: Vec<u8>,
    remainder: std::collections::VecDeque<Chunk>,
    created_at: Instant,
    first_token_timeout: Duration,
    idle_timeout: Duration,
    got_first: bool,
    last_event: Instant,
    sleep: Option<Pin<Box<Sleep>>>,
    eof: bool,
    finished: bool,
}

impl ChatStream {
    pub(crate) fn new(
        response: reqwest::Response,
        first_token_timeout: Duration,
        idle_timeout: Duration,
    ) -> Self {
        ChatStream {
            byte_stream: Box::pin(response.bytes_stream()),
            buffer: Vec::new(),
            remainder: std::collections::VecDeque::new(),
            created_at: Instant::now(),
            first_token_timeout,
            idle_timeout,
            got_first: false,
            last_event: Instant::now(),
            sleep: None,
            eof: false,
            finished: false,
        }
    }

    /// Convenience for callers who do not use `Stream`: `while let Some(c) =
    /// stream.next().await { ... }`.
    pub async fn next(&mut self) -> Option<Result<Chunk, Error>> {
        std::future::poll_fn(|cx| Pin::new(&mut *self).poll_next(cx)).await
    }

    fn timeout_error(&self) -> Error {
        Error::Transport(TransportError {
            message: if self.got_first {
                format!(
                    "no stream event within idle timeout of {:?}",
                    self.idle_timeout
                )
            } else {
                format!(
                    "no first stream event within timeout of {:?}",
                    self.first_token_timeout
                )
            },
        })
    }
}

impl Stream for ChatStream {
    type Item = Result<Chunk, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            // Deliver the steps already made from this event before framing
            // another one, and before `finished` is consulted: an event's
            // remainder is owed to the caller even when the next frame is
            // `[DONE]`, EOF or an error.
            if let Some(chunk) = this.remainder.pop_front() {
                return Poll::Ready(Some(Ok(chunk)));
            }
            if this.finished {
                return Poll::Ready(None);
            }
            if let Some(event) = next_event(&mut this.buffer) {
                this.last_event = Instant::now();
                this.got_first = true;
                this.sleep = None;
                match event {
                    SseEvent::Done => {
                        this.finished = true;
                        return Poll::Ready(None);
                    }
                    SseEvent::Data(data) => match parse_event(&data) {
                        Ok(chunks) => {
                            this.remainder.extend(chunks);
                            continue;
                        }
                        Err(e) => {
                            this.finished = true;
                            return Poll::Ready(Some(Err(e)));
                        }
                    },
                }
            }
            if this.eof {
                this.finished = true;
                return Poll::Ready(None);
            }

            let deadline = if this.got_first {
                this.last_event + this.idle_timeout
            } else {
                this.created_at + this.first_token_timeout
            };
            let deadline_at = tokio::time::Instant::from_std(deadline);
            let now = tokio::time::Instant::now();
            if now >= deadline_at {
                this.finished = true;
                let e = this.timeout_error();
                return Poll::Ready(Some(Err(e)));
            }
            match this.sleep.as_mut() {
                Some(s) if s.deadline() != deadline_at => {
                    *s = Box::pin(tokio::time::sleep_until(deadline_at));
                }
                None => this.sleep = Some(Box::pin(tokio::time::sleep_until(deadline_at))),
                _ => {}
            }
            if let Poll::Ready(()) = Pin::new(this.sleep.as_mut().expect("sleep set")).poll(cx) {
                this.finished = true;
                let e = this.timeout_error();
                return Poll::Ready(Some(Err(e)));
            }

            match this.byte_stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => this.buffer.extend_from_slice(&bytes),
                Poll::Ready(Some(Err(e))) => {
                    this.finished = true;
                    return Poll::Ready(Some(Err(Error::Transport(TransportError {
                        message: e.to_string(),
                    }))));
                }
                Poll::Ready(None) => {
                    this.eof = true;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain_events(body: &str) -> Vec<SseEvent> {
        let mut buffer = body.as_bytes().to_vec();
        let mut events = Vec::new();
        while let Some(e) = next_event(&mut buffer) {
            events.push(e);
        }
        events
    }

    #[test]
    fn parses_data_events_and_done() {
        let body = "data: {\"a\":1}\n\ndata: [DONE]\n\n";
        let events = drain_events(body);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], SseEvent::Data(d) if d == "{\"a\":1}"));
        assert!(matches!(&events[1], SseEvent::Done));
    }

    #[test]
    fn handles_crlf_and_comments_and_multiline_data() {
        let body =
            ": keep-alive\r\nevent: delta\r\ndata: {\"x\":\r\ndata: 1}\r\n\r\ndata: [DONE]\r\n\r\n";
        let events = drain_events(body);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], SseEvent::Data(d) if d == "{\"x\":\n1}"));
    }

    #[test]
    fn incomplete_event_stays_buffered() {
        let mut buffer = b"data: {\"partial".to_vec();
        assert!(next_event(&mut buffer).is_none());
        buffer.extend_from_slice(b"}\"\n\n".as_slice());
        assert!(matches!(next_event(&mut buffer), Some(SseEvent::Data(_))));
        assert!(buffer.is_empty());
    }

    /// An event that carries one field normalizes into exactly one step.
    fn one_step(data: &str) -> Chunk {
        let mut chunks = parse_event(data).expect("event must parse");
        assert_eq!(
            chunks.len(),
            1,
            "a single-field event yields exactly one step"
        );
        chunks.remove(0)
    }

    #[test]
    fn parses_text_reasoning_usage_and_finish() {
        let text = one_step(
            "{\"choices\":[{\"index\":3,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}],\"usage\":null}",
        );
        assert_eq!(text.index, 3);
        assert_eq!(text.delta, Some(Delta::Text { text: "hi".into() }));

        let reasoning = one_step(
            "{\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking\"}}],\"usage\":null}",
        );
        assert_eq!(
            reasoning.delta,
            Some(Delta::Reasoning {
                text: "thinking".into()
            })
        );

        let final_chunk = one_step(
            "{\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":9,\"total_tokens\":16}}",
        );
        assert_eq!(final_chunk.finish_reason.as_deref(), Some("stop"));
        assert_eq!(final_chunk.usage.map(|u| u.total_tokens), Some(Some(16)));
    }

    #[test]
    fn one_event_yields_one_step_per_field_it_carries() {
        let cases: Vec<(serde_json::Value, Vec<Option<Delta>>)> = vec![
            (
                serde_json::json!({ "reasoning_content": "reasoning" }),
                vec![Some(Delta::Reasoning {
                    text: "reasoning".into(),
                })],
            ),
            (
                serde_json::json!({ "content": "text" }),
                vec![Some(Delta::Text {
                    text: "text".into(),
                })],
            ),
            (
                serde_json::json!({ "reasoning_content": "reasoning", "content": "text" }),
                vec![
                    Some(Delta::Reasoning {
                        text: "reasoning".into(),
                    }),
                    Some(Delta::Text {
                        text: "text".into(),
                    }),
                ],
            ),
            // The shape reasoning models send while thinking: an empty answer
            // field rides every reasoning event, and carries no step.
            (
                serde_json::json!({ "reasoning_content": "reasoning", "content": "" }),
                vec![Some(Delta::Reasoning {
                    text: "reasoning".into(),
                })],
            ),
            (
                serde_json::json!({ "reasoning_content": "", "content": "text" }),
                vec![Some(Delta::Text {
                    text: "text".into(),
                })],
            ),
            (
                serde_json::json!({ "reasoning_content": "", "content": "" }),
                vec![None],
            ),
            (serde_json::json!({ "role": "assistant" }), vec![None]),
        ];
        for (fields, expected) in cases {
            let event = serde_json::to_string(&serde_json::json!({
                "choices": [{ "index": 0, "delta": fields }]
            }))
            .expect("fixture is valid JSON");
            let chunks = parse_event(&event).expect("event must parse");
            assert_eq!(
                chunks.iter().map(|c| c.delta.clone()).collect::<Vec<_>>(),
                expected,
                "for the event {event}"
            );
        }
    }

    #[test]
    fn one_event_with_reasoning_and_text_yields_both_steps() {
        // Written with the answer field first on the wire: the step order is
        // ours, never the provider's field order.
        let chunks = parse_event(
            "{\"choices\":[{\"index\":2,\"delta\":{\"reasoning_content\":\"thinking\",\"content\":\"answer\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}",
        )
        .expect("an event carrying both is legal");
        assert_eq!(
            chunks.iter().map(|c| c.delta.clone()).collect::<Vec<_>>(),
            vec![
                Some(Delta::Reasoning {
                    text: "thinking".into()
                }),
                Some(Delta::Text {
                    text: "answer".into()
                }),
            ]
        );
        assert!(
            chunks.iter().all(|c| c.index == 2),
            "steps of one event share the choice index"
        );
        assert_eq!(chunks[0].finish_reason, None);
        assert_eq!(chunks[1].finish_reason.as_deref(), Some("stop"));
        assert_eq!(chunks[0].usage, None);
        assert_eq!(
            chunks[1].usage.as_ref().and_then(|u| u.total_tokens),
            Some(3)
        );
    }

    #[test]
    fn provider_error_event_maps_to_api() {
        let err = parse_event("{\"error\":{\"message\":\"quota exceeded\"}}")
            .expect_err("error event must fail");
        match err {
            Error::Api(e) => {
                assert_eq!(e.provider_message, "quota exceeded");
                assert_eq!(e.raw_body, "{\"error\":{\"message\":\"quota exceeded\"}}");
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }
}

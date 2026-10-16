//! Streaming example: talks to a local mock SSE server when no key is
//! present, or to the live Moonshot API when MOONSHOT_API_KEY is set.
//!
//! Run with: cargo run --example chat_stream

use simllms::{Client, Message, Provider, RetryPolicy};

#[tokio::main]
async fn main() {
    match std::env::var("MOONSHOT_API_KEY") {
        Ok(key) => {
            let client = Client::new(Provider::Moonshot, "kimi-k2", key);
            let mut stream = client
                .chat_stream(vec![Message::text("用三句话介绍月之暗面。")])
                .await
                .expect("live stream failed");
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(c) => {
                        if let Some(simllms::stream::Delta::Text { text }) = c.delta {
                            print!("{text}");
                        }
                        if c.finish_reason.is_some() {
                            println!("\n[finish] {}", c.finish_reason.unwrap());
                        }
                    }
                    Err(e) => {
                        eprintln!("stream error: {e}");
                        break;
                    }
                }
            }
        }
        Err(_) => {
            let mut server = mockito::Server::new_async().await;
            let sse = concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Moon\"}}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"shot is an LLM provider.\"}}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":6,\"total_tokens\":14}}\n\n",
                "data: [DONE]\n\n",
            );
            let mock = server
                .mock("POST", "/chat/completions")
                .match_body(mockito::Matcher::PartialJson(
                    serde_json::json!({ "stream": true, "stream_options": { "include_usage": true } }),
                ))
                .with_status(200)
                .with_header("content-type", "text/event-stream")
                .with_body(sse)
                .create();
            let client = Client::new(Provider::Moonshot, "kimi-k2", "mock-key")
                .with_base_url(server.url())
                .with_retry(RetryPolicy::disabled());
            let mut stream = client
                .chat_stream(vec![Message::text("hello")])
                .await
                .expect("mock stream failed");
            let mut text = String::new();
            let mut usage = None;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.expect("chunk");
                match chunk.delta {
                    Some(simllms::stream::Delta::Text { text: t }) => text.push_str(&t),
                    _ => {}
                }
                if let Some(u) = chunk.usage {
                    usage = u.total_tokens;
                }
            }
            mock.assert();
            println!("{text} (against the local mock server — set MOONSHOT_API_KEY to go live)");
            println!("[total tokens] {usage:?}");
        }
    }
}

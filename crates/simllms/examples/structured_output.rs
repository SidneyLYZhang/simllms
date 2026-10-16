//! Structured output example: native response_format only (ADR-0004 — the
//! schema is never pasted into the prompt). Talks to a local mock server
//! when no key is present, or to live OpenRouter when OPENROUTER_API_KEY is
//! set.
//!
//! Run with: cargo run --example structured_output

use simllms::{Client, Message, Options, Provider, RetryPolicy};

#[tokio::main]
async fn main() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "capital": { "type": "string" },
            "population_millions": { "type": "number" }
        },
        "required": ["capital", "population_millions"],
        "additionalProperties": false
    });

    match std::env::var("OPENROUTER_API_KEY") {
        Ok(key) => {
            let client = Client::new(Provider::OpenRouter, "openai/gpt-4o-mini", key);
            let reply = client
                .chat_with(
                    vec![Message::text("法国的首都是哪座城市？人口约多少？返回 JSON。")],
                    Options {
                        response_schema: Some(schema),
                        ..Default::default()
                    },
                )
                .await
                .expect("live structured chat failed");
            println!("{}", reply.content);
        }
        Err(_) => {
            let mut server = mockito::Server::new_async().await;
            let mock = server
                .mock("POST", "/chat/completions")
                .match_body(mockito::Matcher::PartialJson(serde_json::json!({
                    "response_format": {
                        "type": "json_schema",
                        "json_schema": { "name": "response", "strict": true }
                    }
                })))
                .with_status(200)
                .with_body(
                    serde_json::json!({
                        "choices": [{ "message": { "role": "assistant", "content": "{\"capital\":\"Paris\",\"population_millions\":2.1}" } }],
                        "usage": { "prompt_tokens": 22, "completion_tokens": 14, "total_tokens": 36 }
                    })
                    .to_string(),
                )
                .create();
            let client = Client::new(Provider::OpenRouter, "vendor/model", "mock-key")
                .with_base_url(server.url())
                .with_retry(RetryPolicy::disabled());
            let reply = client
                .chat_with(
                    vec![Message::text("法国的首都是哪座城市？")],
                    Options {
                        response_schema: Some(schema),
                        ..Default::default()
                    },
                )
                .await
                .expect("mock structured chat failed");
            mock.assert();
            println!(
                "{} (against the local mock server — set OPENROUTER_API_KEY to go live)",
                reply.content
            );
        }
    }
}

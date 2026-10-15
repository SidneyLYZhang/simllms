//! Chat example: talks to a local mock server when no key is present, or to
//! the live DeepSeek API when DEEPSEEK_API_KEY is set.
//!
//! Run with: cargo run --example deepseek_chat

use simllms::{Client, Message, Provider, RetryPolicy};

#[tokio::main]
async fn main() {
    match std::env::var("DEEPSEEK_API_KEY") {
        Ok(key) => {
            let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", key);
            let reply = client
                .chat(vec![Message::text("用一句话介绍 deepseek 是什么。")])
                .await
                .expect("live chat failed");
            println!("{}", reply.content);
            if let Some(usage) = reply.usage {
                println!("[usage] {usage:?}");
            }
        }
        Err(_) => {
            let mut server = mockito::Server::new_async().await;
            let body = serde_json::json!({
                "choices": [{ "message": { "role": "assistant", "content": "(mock reply) DeepSeek is an LLM provider." } }],
                "usage": { "prompt_tokens": 9, "completion_tokens": 12, "total_tokens": 21 }
            });
            let mock = server
                .mock("POST", "/chat/completions")
                .with_status(200)
                .with_body(body.to_string())
                .create();
            let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "mock-key")
                .with_base_url(server.url())
                .with_retry(RetryPolicy::disabled());
            let reply = client
                .chat(vec![Message::text("hello")])
                .await
                .expect("mock chat failed");
            mock.assert();
            println!("{} (against the local mock server — set DEEPSEEK_API_KEY to go live)", reply.content);
            println!("[usage] {:?}", reply.usage);
        }
    }
}

//! Pointing simllms at a self-hosted OpenAI-compatible gateway.
//!
//! Run with: cargo run --example custom_gateway
//!
//! Starts a local mock gateway and chats through `Provider::Custom` — the
//! same path a vLLM / One-API / corporate-gateway deployment takes.

use simllms::{Client, Message, Provider, RetryPolicy};

#[tokio::main]
async fn main() {
    let mut server = mockito::Server::new_async().await;
    let body = serde_json::json!({
        "choices": [{ "message": { "role": "assistant", "content": "(gateway) hello from your own model" } }],
        "usage": { "prompt_tokens": 5, "completion_tokens": 8, "total_tokens": 13 }
    });
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_body(body.to_string())
        .create();

    let client = Client::new(Provider::Custom, "my-selfhosted-model", "gateway-key")
        .with_base_url(server.url())
        .with_default_header("x-deployment", "example")
        .with_retry(RetryPolicy::disabled());

    let warns = client.warnings(&[Message::text("hello")], &Default::default());
    println!(
        "gate warnings: {warns:?} (gateway models are unknown to the Catalog — warn-and-pass)"
    );

    let reply = client
        .chat(vec![Message::text("hello")])
        .await
        .expect("gateway chat failed");
    mock.assert();
    println!("{}", reply.content);
    println!("[usage] {:?}", reply.usage);
}

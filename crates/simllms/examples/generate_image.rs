//! Image generation example: the same call normalizes three provider wire
//! shapes (Zhipu sync URL, OpenRouter sync b64, DashScope submit-then-poll).
//! Talks to a local mock server when no key is present, or to live Zhipu
//! when ZHIPU_API_KEY is set.
//!
//! Run with: cargo run --example generate_image

use simllms::{Client, ImageRequest, Provider, RetryPolicy};

fn show(reply: &simllms::ImageReply) {
    for image in &reply.images {
        match (&image.url, &image.data) {
            (Some(url), _) => println!("image url: {url}"),
            (None, Some(data)) => println!("image bytes: {} received", data.len()),
            _ => println!("image: (empty)"),
        }
        if let Some(prompt) = &image.revised_prompt {
            println!("revised prompt: {prompt}");
        }
    }
}

#[tokio::main]
async fn main() {
    match std::env::var("ZHIPU_API_KEY") {
        Ok(key) => {
            let client = Client::new(Provider::Zhipu, "cogview-4", key);
            let reply = client
                .generate_image(ImageRequest {
                    prompt: "水彩风格的中国龙".into(),
                    size: Some("1024x1024".into()),
                    ..ImageRequest::new("水彩风格的中国龙")
                })
                .await
                .expect("live image generation failed");
            show(&reply);
        }
        Err(_) => {
            let mut server = mockito::Server::new_async().await;
            let mock = server
                .mock("POST", "/images/generations")
                .with_status(200)
                .with_body(
                    serde_json::json!({
                        "data": [{ "url": "https://img.example/dragon.png" }]
                    })
                    .to_string(),
                )
                .create();
            let client = Client::new(Provider::Zhipu, "cogview-4", "mock-key")
                .with_base_url(server.url())
                .with_retry(RetryPolicy::disabled());
            let reply = client
                .generate_image(ImageRequest::new("水彩风格的中国龙"))
                .await
                .expect("mock image generation failed");
            mock.assert();
            println!("(against the local mock server — set ZHIPU_API_KEY to go live)");
            show(&reply);
        }
    }
}

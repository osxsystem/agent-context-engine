use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};

use super::super::{Provider, VoyageClient};

struct CountingServer {
    base_url: String,
    requests: Arc<AtomicUsize>,
}

async fn counting_server(failing_requests: usize) -> CountingServer {
    let requests = Arc::new(AtomicUsize::new(0));
    let handler_requests = requests.clone();
    let app = Router::new().route(
        "/v1/embeddings",
        post(move |Json(body): Json<serde_json::Value>| {
            let requests = handler_requests.clone();
            async move {
                let call = requests.fetch_add(1, Ordering::SeqCst) + 1;
                if call <= failing_requests {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({"error": "rejected"})),
                    )
                        .into_response();
                }
                let text_len = body["input"][0].as_str().map_or(0, str::len);
                Json(serde_json::json!({
                    "data": [{"index": 0, "embedding": [call as f32, text_len as f32, 0.5]}]
                }))
                .into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    CountingServer {
        base_url: format!("http://{addr}/v1"),
        requests,
    }
}

fn client(
    server: &CountingServer,
    provider: Provider,
    model: &str,
    dimensions: Option<u32>,
) -> VoyageClient {
    VoyageClient::new_for_provider(
        provider,
        model.to_string(),
        vec!["test-key".to_string()],
        Some(&server.base_url),
        dimensions,
    )
    .unwrap()
}

#[tokio::test]
async fn repeated_query_returns_the_same_vector_without_a_second_request() {
    let server = counting_server(0).await;
    let client = client(&server, Provider::Voyage, "m", None);

    let first = client
        .embed_query("find the auth middleware")
        .await
        .unwrap();
    let second = client
        .embed_query("find the auth middleware")
        .await
        .unwrap();
    let third = client
        .embed_query("find the auth middleware")
        .await
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(first, third);
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cache_is_shared_by_clones_of_one_client() {
    let server = counting_server(0).await;
    let client = client(&server, Provider::Voyage, "m", None);
    let clone = client.clone();

    let first = client.embed_query("q").await.unwrap();
    let second = clone.embed_query("q").await.unwrap();

    assert_eq!(first, second);
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn different_text_misses_the_cache() {
    let server = counting_server(0).await;
    let client = client(&server, Provider::Voyage, "m", None);

    let a = client.embed_query("alpha").await.unwrap();
    let b = client.embed_query("alpha ").await.unwrap();
    let c = client.embed_query("Alpha").await.unwrap();

    assert_eq!(server.requests.load(Ordering::SeqCst), 3);
    assert_ne!(a, b);
    assert_ne!(a, c);
}

#[tokio::test]
async fn different_model_misses_the_cache() {
    let server = counting_server(0).await;
    let small = client(&server, Provider::Voyage, "model-small", None);
    let large = client(&server, Provider::Voyage, "model-large", None);

    small.embed_query("same text").await.unwrap();
    large.embed_query("same text").await.unwrap();

    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn different_dimensions_miss_the_cache() {
    let server = counting_server(0).await;
    let narrow = client(&server, Provider::OpenAI, "m", Some(256));
    let wide = client(&server, Provider::OpenAI, "m", Some(512));

    narrow.embed_query("same text").await.unwrap();
    wide.embed_query("same text").await.unwrap();

    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn shared_clients_with_the_same_settings_share_one_cache() {
    let server = counting_server(0).await;
    let make = || {
        VoyageClient::shared_for_provider(
            Provider::Voyage,
            "shared-cache-model".to_string(),
            vec!["shared-cache-key".to_string()],
            Some(&server.base_url),
            None,
        )
        .unwrap()
    };

    let first = make().embed_query("per request lookup").await.unwrap();
    let second = make().embed_query("per request lookup").await.unwrap();

    assert_eq!(first, second);
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_query_is_not_cached() {
    let server = counting_server(1).await;
    let client = client(&server, Provider::Voyage, "m", None);

    assert!(client.embed_query("retry me").await.is_err());
    let recovered = client.embed_query("retry me").await.unwrap();
    let repeated = client.embed_query("retry me").await.unwrap();

    assert_eq!(recovered, repeated);
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

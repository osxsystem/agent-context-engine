//! Wire tests for the Jev reranker: a mock TypeSafe `/v1/systemone` endpoint
//! captures every request and answers with canned relevance probabilities, so
//! request construction, response parsing, ordering, concurrency bounds and
//! degradation all run against real serialization and a real HTTP round trip.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::post};
use context_engine_rs::config::LlmConfig;
use context_engine_rs::query::jev::{JevReranker, MAX_CONCURRENT_REQUESTS};
use context_engine_rs::query::merger::MergeChunk;
use context_engine_rs::query::reranker::{RerankOutput, RerankRequest, Reranker};
use serde_json::{Value, json};
use tokio::net::TcpListener;

/// One request as the mock saw it.
#[derive(Debug, Clone)]
struct Captured {
    authorization: Option<String>,
    body: Value,
}

/// A mock `/v1/systemone`: answers `relevance` with the probability mapped to
/// the request's `state.file_path`, optionally after a delay, and records every
/// request plus the peak number in flight at once.
#[derive(Clone, Default)]
struct MockJev {
    probabilities: Arc<HashMap<String, f64>>,
    delay: Duration,
    status: Option<StatusCode>,
    /// Statuses a key answers with, one per request, before it succeeds.
    key_statuses: Arc<Mutex<HashMap<String, VecDeque<StatusCode>>>>,
    captured: Arc<Mutex<Vec<Captured>>>,
    in_flight: Arc<AtomicUsize>,
    peak_in_flight: Arc<AtomicUsize>,
}

impl MockJev {
    fn answering(probabilities: &[(&str, f64)]) -> Self {
        Self {
            probabilities: Arc::new(
                probabilities
                    .iter()
                    .map(|(f, p)| ((*f).to_owned(), *p))
                    .collect(),
            ),
            ..Self::default()
        }
    }

    /// `key` answers its next requests with `statuses`, in order.
    fn with_key_statuses(self, key: &str, statuses: &[StatusCode]) -> Self {
        self.key_statuses
            .lock()
            .unwrap()
            .insert(format!("Bearer {key}"), statuses.iter().copied().collect());
        self
    }

    /// Serves the mock on an ephemeral port; returns its base URL.
    async fn serve(&self) -> String {
        let mock = self.clone();
        let app = Router::new().route(
            "/v1/systemone",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let mock = mock.clone();
                async move { mock.answer(headers, body).await }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let addr = listener.local_addr().expect("mock addr");
        tokio::spawn(async move { axum::serve(listener, app).await.expect("mock server") });
        format!("http://{addr}")
    }

    async fn answer(&self, headers: HeaderMap, body: Value) -> Response {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_in_flight.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);

        let file = body["state"]["file_path"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let authorization = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let scripted = authorization.as_ref().and_then(|a| {
            self.key_statuses
                .lock()
                .unwrap()
                .get_mut(a)
                .and_then(VecDeque::pop_front)
        });
        self.captured.lock().unwrap().push(Captured {
            authorization,
            body,
        });
        if let Some(status) = scripted.or(self.status) {
            return (status, Json(json!({"error": "nope"}))).into_response();
        }
        let p = self.probabilities.get(&file).copied().unwrap_or(0.0);
        Json(json!({
            "model": "jev-1.13.0",
            "answers": {"relevance": {"type": "noul", "noul": p}},
            "usage": {"input_tokens": 120, "output_tokens": 1}
        }))
        .into_response()
    }

    fn captured(&self) -> Vec<Captured> {
        self.captured.lock().unwrap().clone()
    }

    /// The key each request carried, in arrival order.
    fn keys_used(&self) -> Vec<String> {
        self.captured()
            .into_iter()
            .map(|c| {
                c.authorization
                    .unwrap_or_default()
                    .trim_start_matches("Bearer ")
                    .to_owned()
            })
            .collect()
    }
}

/// Far more rate-limit answers than any retry schedule will ask for.
const ALWAYS_LIMITED: [StatusCode; 8] = [StatusCode::TOO_MANY_REQUESTS; 8];

/// A reranker whose retry pause is short enough for tests.
fn jev_with_keys(base: String, keys: &[&str]) -> JevReranker {
    JevReranker::new(&jev_config(Some(base), keys)).with_retry_pause(Duration::from_millis(10))
}

fn chunk(file: &str, symbol: &str) -> MergeChunk {
    MergeChunk {
        file: file.to_owned(),
        line_start: 10,
        line_end: 12,
        score: 0.5,
        content: format!("fn {symbol}() {{}}"),
        symbol: Some(symbol.to_owned()),
        symbol_fqn: None,
        symbol_kind: Some("function".to_owned()),
    }
}

fn jev_config(base_url: Option<String>, keys: &[&str]) -> LlmConfig {
    LlmConfig {
        // The LLM provider stays configured with its own key: a Jev failure
        // must never be papered over by quietly reranking through it instead.
        provider: "google".to_owned(),
        api_keys: vec!["google-key".to_owned()],
        rerank_provider: Some("jev".to_owned()),
        jev_api_keys: keys.iter().map(|k| (*k).to_owned()).collect(),
        jev_base_url: base_url,
        ..LlmConfig::default()
    }
}

async fn rerank(reranker: &JevReranker, query: &str, chunks: &[MergeChunk]) -> RerankOutput {
    let numbered: Vec<Option<String>> = chunks
        .iter()
        .map(|c| Some(format!("10: {}", c.content)))
        .collect();
    let caller_stats = vec![None; chunks.len()];
    let spans = RerankRequest::no_spans(chunks.len());
    reranker
        .rerank(RerankRequest {
            query,
            chunks,
            numbered: &numbered,
            caller_stats: &caller_stats,
            min_prune_lines: 16,
            candidate_spans: &spans,
        })
        .await
}

#[tokio::test]
async fn jev_orders_by_relevance_probability_and_serializes_the_request() {
    let mock = MockJev::answering(&[("a.rs", 0.2), ("b.rs", 0.9), ("c.rs", 0.5)]);
    let base = mock.serve().await;
    let reranker = JevReranker::new(&jev_config(Some(base), &["ts-key"]));
    let chunks = [
        chunk("a.rs", "alpha"),
        chunk("b.rs", "beta"),
        chunk("c.rs", "gamma"),
    ];

    let out = rerank(&reranker, "how are sessions restored", &chunks).await;

    assert_eq!(out.skip_reason, None);
    assert!(!out.fallback_used);
    assert_eq!(out.reranked_indices, vec![1, 2, 0]);
    assert_eq!(
        out.relevance,
        vec![0.2, 0.9, 0.5],
        "aligned with input chunks"
    );
    assert_eq!(
        out.line_selections,
        vec![None, None, None],
        "chunks come back whole"
    );

    let requests = mock.captured();
    assert_eq!(requests.len(), 3, "one request per candidate");
    let beta = requests
        .iter()
        .find(|r| r.body["state"]["file_path"] == "b.rs")
        .expect("request for b.rs");
    assert_eq!(beta.authorization.as_deref(), Some("Bearer ts-key"));
    assert_eq!(beta.body["model"], "jev-latest");
    assert_eq!(beta.body["state"]["query"], "how are sessions restored");
    assert_eq!(beta.body["state"]["symbol_name"], "beta");
    assert_eq!(beta.body["state"]["symbol_kind"], "function");
    assert_eq!(beta.body["state"]["code"], "10: fn beta() {}");
    let relevance = &beta.body["questions"]["relevance"];
    assert_eq!(relevance["type"], "noul");
    assert!(
        relevance["instructions"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    assert!(
        relevance["criteria"]["true"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    assert!(
        relevance["criteria"]["false"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
}

/// `rerank_model` belongs to the LLM path (chat uses it too), so whatever it
/// holds, Jev asks for the vendor's stable alias.
#[tokio::test]
async fn jev_sends_the_stable_alias_whatever_rerank_model_holds() {
    let mock = MockJev::answering(&[]);
    let base = mock.serve().await;
    let config = LlmConfig {
        rerank_model: "jev-preview".to_owned(),
        ..jev_config(Some(format!("{base}/v1/")), &["ts-key"])
    };

    rerank(&JevReranker::new(&config), "q", &[chunk("a.rs", "alpha")]).await;

    let requests = mock.captured();
    assert_eq!(
        requests.len(),
        1,
        "base URL given as …/v1/ still reaches the endpoint"
    );
    assert_eq!(requests[0].body["model"], "jev-latest");
}

#[tokio::test]
async fn jev_bounds_concurrent_requests() {
    let mock = MockJev {
        delay: Duration::from_millis(50),
        ..MockJev::answering(&[])
    };
    let base = mock.serve().await;
    let reranker = JevReranker::new(&jev_config(Some(base), &["ts-key"]));
    let chunks: Vec<MergeChunk> = (0..MAX_CONCURRENT_REQUESTS * 3)
        .map(|i| chunk(&format!("f{i}.rs"), "s"))
        .collect();

    let out = rerank(&reranker, "q", &chunks).await;

    assert_eq!(out.skip_reason, None);
    assert_eq!(mock.captured().len(), chunks.len());
    let peak = mock.peak_in_flight.load(Ordering::SeqCst);
    assert!(
        peak <= MAX_CONCURRENT_REQUESTS,
        "peak {peak} exceeds the bound"
    );
    assert!(
        peak > 1,
        "requests should fan out concurrently, peak was {peak}"
    );
}

#[tokio::test]
async fn jev_without_a_key_keeps_similarity_order_and_says_why() {
    let mock = MockJev::answering(&[("a.rs", 0.1), ("b.rs", 0.9)]);
    let base = mock.serve().await;
    let reranker = JevReranker::new(&jev_config(Some(base), &[]));

    let out = rerank(
        &reranker,
        "q",
        &[chunk("a.rs", "alpha"), chunk("b.rs", "beta")],
    )
    .await;

    assert_eq!(out.reranked_indices, vec![0, 1]);
    assert!(out.relevance.is_empty());
    let reason = out.skip_reason.expect("skip reason");
    assert!(reason.contains("TypeSafe API key"), "{reason}");
    assert!(mock.captured().is_empty(), "nothing is sent without a key");
}

#[tokio::test]
async fn jev_error_status_keeps_similarity_order_and_says_why() {
    let mock = MockJev {
        status: Some(StatusCode::UNAUTHORIZED),
        ..MockJev::answering(&[])
    };
    let base = mock.serve().await;
    let reranker = jev_with_keys(base, &["bad-key"]);

    let out = rerank(
        &reranker,
        "q",
        &[chunk("a.rs", "alpha"), chunk("b.rs", "beta")],
    )
    .await;

    assert_eq!(out.reranked_indices, vec![0, 1]);
    assert!(out.fallback_used);
    let reason = out.skip_reason.expect("skip reason");
    assert!(reason.contains("Jev") && reason.contains("401"), "{reason}");
}

#[tokio::test]
async fn jev_transport_failure_keeps_similarity_order_and_says_why() {
    // Bind then drop: nothing listens on this port any more.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let dead = format!("http://{}", listener.local_addr().expect("addr"));
    drop(listener);
    let reranker = jev_with_keys(dead, &["ts-key"]);

    let out = rerank(
        &reranker,
        "q",
        &[chunk("a.rs", "alpha"), chunk("b.rs", "beta")],
    )
    .await;

    assert_eq!(out.reranked_indices, vec![0, 1]);
    assert!(out.fallback_used);
    assert!(out.skip_reason.expect("skip reason").contains("Jev"));
}

/// The provider both retrieval tools build from settings: selecting `jev`
/// routes through Jev, and its ordering is what comes back.
#[tokio::test]
async fn settings_selecting_jev_rank_through_jev() {
    use context_engine_rs::query::reranker::RerankProvider;

    let mock = MockJev::answering(&[("a.rs", 0.1), ("b.rs", 0.8)]);
    let base = mock.serve().await;
    let provider = RerankProvider::from_settings(&jev_config(Some(base), &["ts-key"]));
    assert!(
        provider.llm_client().is_none(),
        "Jev selected: no LLM client in play"
    );

    let chunks = [chunk("a.rs", "alpha"), chunk("b.rs", "beta")];
    let numbered = vec![None, None];
    let spans = RerankRequest::no_spans(2);
    let out = provider
        .rerank(RerankRequest {
            query: "q",
            chunks: &chunks,
            numbered: &numbered,
            caller_stats: &[None, None],
            min_prune_lines: 16,
            candidate_spans: &spans,
        })
        .await;

    assert_eq!(out.reranked_indices, vec![1, 0]);
    assert_eq!(out.relevance, vec![0.1, 0.8]);
    assert_eq!(mock.captured().len(), 2);
}

/// A Jev failure degrades to similarity order; it is never answered by the
/// configured LLM provider instead (that provider has a key here).
#[tokio::test]
async fn jev_selected_without_a_key_never_substitutes_the_llm_provider() {
    use context_engine_rs::query::reranker::RerankProvider;

    let provider = RerankProvider::from_settings(&jev_config(None, &[]));
    assert!(provider.llm_client().is_none());
    let chunks = [chunk("a.rs", "alpha"), chunk("b.rs", "beta")];
    let spans = RerankRequest::no_spans(2);
    let out = provider
        .rerank(RerankRequest {
            query: "q",
            chunks: &chunks,
            numbered: &[None, None],
            caller_stats: &[None, None],
            min_prune_lines: 16,
            candidate_spans: &spans,
        })
        .await;

    assert_eq!(out.reranked_indices, vec![0, 1]);
    assert!(
        out.skip_reason
            .expect("skip reason")
            .contains("TypeSafe API key")
    );
}

/// One deadline bounds the whole fan-out: a stalled endpoint degrades the
/// query promptly instead of holding it for every request's own timeout.
#[tokio::test]
async fn jev_past_its_deadline_keeps_similarity_order_and_says_why() {
    let mock = MockJev {
        delay: Duration::from_secs(5),
        ..MockJev::answering(&[("a.rs", 0.1), ("b.rs", 0.9)])
    };
    let base = mock.serve().await;
    let reranker = JevReranker::new(&jev_config(Some(base), &["ts-key"]))
        .with_deadline(Duration::from_millis(200));

    let started = std::time::Instant::now();
    let out = rerank(
        &reranker,
        "q",
        &[chunk("a.rs", "alpha"), chunk("b.rs", "beta")],
    )
    .await;

    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(out.reranked_indices, vec![0, 1]);
    assert!(out.fallback_used);
    let reason = out.skip_reason.expect("skip reason");
    assert!(
        reason.contains("Jev") && reason.contains("deadline"),
        "{reason}"
    );
}

#[tokio::test]
async fn jev_with_no_candidates_sends_nothing() {
    let mock = MockJev::answering(&[]);
    let base = mock.serve().await;
    let reranker = JevReranker::new(&jev_config(Some(base), &["ts-key"]));

    let out = rerank(&reranker, "q", &[]).await;

    assert!(out.reranked_indices.is_empty());
    assert_eq!(out.skip_reason, None);
    assert!(mock.captured().is_empty());
}

#[tokio::test]
async fn jev_rotates_keys_round_robin_across_requests() {
    let mock = MockJev::answering(&[]);
    let base = mock.serve().await;
    let reranker = jev_with_keys(base, &["k1", "k2", "k3"]);
    let chunks: Vec<MergeChunk> = (0..6).map(|i| chunk(&format!("f{i}.rs"), "s")).collect();

    let out = rerank(&reranker, "q", &chunks).await;

    assert_eq!(out.skip_reason, None);
    let mut used = mock.keys_used();
    used.sort();
    assert_eq!(used, ["k1", "k1", "k2", "k2", "k3", "k3"]);
}

#[tokio::test]
async fn jev_retries_a_rate_limited_request_on_a_different_key() {
    let mock = MockJev::answering(&[("a.rs", 0.7)]).with_key_statuses("limited", &ALWAYS_LIMITED);
    let base = mock.serve().await;
    let reranker = jev_with_keys(base, &["limited", "fresh"]);

    let out = rerank(&reranker, "q", &[chunk("a.rs", "alpha")]).await;

    assert_eq!(out.skip_reason, None);
    assert_eq!(out.relevance, vec![0.7]);
    assert_eq!(mock.keys_used(), ["limited", "fresh"]);
}

/// Every key fails the first pass; after the pause only the key that did not
/// report a limit is tried again.
#[tokio::test]
async fn jev_second_pass_skips_keys_that_reported_a_limit() {
    let mock = MockJev::answering(&[("a.rs", 0.4)])
        .with_key_statuses("limited", &ALWAYS_LIMITED)
        .with_key_statuses("flaky", &[StatusCode::BAD_GATEWAY]);
    let base = mock.serve().await;
    let reranker = jev_with_keys(base, &["limited", "flaky"]);

    let out = rerank(&reranker, "q", &[chunk("a.rs", "alpha")]).await;

    assert_eq!(out.skip_reason, None);
    assert_eq!(out.relevance, vec![0.4]);
    assert_eq!(mock.keys_used(), ["limited", "flaky", "flaky"]);
}

#[tokio::test]
async fn jev_with_every_key_rate_limited_keeps_similarity_order_and_says_why() {
    let mock = MockJev::answering(&[("a.rs", 0.9)])
        .with_key_statuses("spent-1", &ALWAYS_LIMITED)
        .with_key_statuses("spent-2", &ALWAYS_LIMITED);
    let base = mock.serve().await;
    let reranker = jev_with_keys(base, &["spent-1", "spent-2"]);

    let out = rerank(&reranker, "q", &[chunk("a.rs", "alpha")]).await;

    assert_eq!(out.reranked_indices, vec![0]);
    assert!(out.fallback_used);
    assert!(out.relevance.is_empty());
    let reason = out.skip_reason.expect("skip reason");
    assert!(reason.contains("rate-limited on 2 of 2"), "{reason}");
    assert_eq!(
        mock.keys_used(),
        ["spent-1", "spent-2"],
        "no retry pass once every key reported a limit"
    );
}

/// One key limited, the other failing otherwise: the reason still names the
/// rate limit, alongside the error that came last.
#[tokio::test]
async fn jev_with_some_keys_rate_limited_names_the_limit_in_its_reason() {
    let mock = MockJev::answering(&[("a.rs", 0.9)])
        .with_key_statuses("capped", &ALWAYS_LIMITED)
        .with_key_statuses("broken", &[StatusCode::BAD_GATEWAY; 2]);
    let base = mock.serve().await;
    let reranker = jev_with_keys(base, &["capped", "broken"]);

    let out = rerank(&reranker, "q", &[chunk("a.rs", "alpha")]).await;

    assert!(out.fallback_used);
    let reason = out.skip_reason.expect("skip reason");
    assert!(
        reason.contains("rate-limited on 1 of 2") && reason.contains("502"),
        "{reason}"
    );
    assert_eq!(mock.keys_used(), ["capped", "broken", "broken"]);
}

/// Settings build a fresh reranker for every query; rotation still carries on
/// from where the previous query left it.
#[tokio::test]
async fn jev_rotation_carries_on_across_queries() {
    let mock = MockJev::answering(&[]);
    let base = mock.serve().await;

    for _ in 0..2 {
        let reranker = jev_with_keys(base.clone(), &["query-1", "query-2"]);
        rerank(&reranker, "q", &[chunk("a.rs", "alpha")]).await;
    }

    assert_eq!(mock.keys_used(), ["query-1", "query-2"]);
}

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::embedding::InputType;

const VOYAGE_ENDPOINT: &str = "https://api.voyageai.com/v1/embeddings";

/// Well-known message used to mark an embed aborted because its cancellation
/// token fired (as opposed to a real provider failure). Callers use
/// [`is_cancel_error`] to distinguish the two.
const EMBED_CANCELLED_MSG: &str = "embedding cancelled by token";

/// Returns true if `err` was produced by an embed call that aborted because its
/// [`CancellationToken`] fired, rather than a genuine provider error. Lets the
/// indexing pipeline report user-cancel as `Cancelled` instead of `EmbeddingFailed`.
pub fn is_cancel_error(err: &anyhow::Error) -> bool {
    err.to_string() == EMBED_CANCELLED_MSG
}
pub const MAX_BATCH_SIZE: usize = 128;
/// Byte-size cap for the sum of input texts in a single batch. VoyageAI's
/// per-batch token limit is 1M for voyage-4-lite. Worst-case for minified code
/// is ~2 bytes/token; 1.5 MB / 2 = 750K tokens — 25% headroom under the 1M limit.
const MAX_BATCH_BYTES: usize = 1_500_000;

/// Embedding provider selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Voyage,
    Gemini,
}

impl Provider {
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "voyage" => Ok(Provider::Voyage),
            "gemini" | "google" => Ok(Provider::Gemini),
            other => bail!(
                "unknown embedding provider {:?}; expected \"voyage\" or \"gemini\"",
                other
            ),
        }
    }
}

/// Resolve the embeddings URL from an optional user-supplied base.
///
/// Normalization rules (mirrors `llm::openai::chat_url`):
///   * `None`, empty, or whitespace-only → `VOYAGE_ENDPOINT`.
///   * Trim whitespace, then strip a trailing `/`.
///   * If the path already ends in `/embeddings`, keep it as-is.
///   * Otherwise append `/embeddings`.
pub fn voyage_url(base: Option<&str>) -> String {
    let raw = match base {
        Some(s) => s.trim(),
        None => "",
    };
    if raw.is_empty() {
        return VOYAGE_ENDPOINT.to_owned();
    }
    let trimmed = raw.trim_end_matches('/');
    if trimmed.ends_with("/embeddings") {
        trimmed.to_owned()
    } else {
        format!("{trimmed}/embeddings")
    }
}

fn gemini_embed_url(model: &str, key: &str) -> String {
    format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:batchEmbedContents?key={}",
        model, key
    )
}

// ─── Voyage request / response shapes ─────────────────────────────────────

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
    input_type: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

// ─── Gemini request / response shapes ─────────────────────────────────────

#[derive(Serialize)]
struct GeminiEmbedRequest<'a> {
    requests: Vec<GeminiEmbedContentRequest<'a>>,
}

#[derive(Serialize)]
struct GeminiEmbedContentRequest<'a> {
    model: String,
    content: GeminiContent<'a>,
    #[serde(rename = "taskType")]
    task_type: &'a str,
}

#[derive(Serialize)]
struct GeminiContent<'a> {
    parts: Vec<GeminiPart<'a>>,
}

#[derive(Serialize)]
struct GeminiPart<'a> {
    text: &'a str,
}

#[derive(Deserialize)]
struct GeminiEmbedResponse {
    embeddings: Vec<GeminiEmbedding>,
}

#[derive(Deserialize)]
struct GeminiEmbedding {
    values: Vec<f32>,
}

// ─── Client ───────────────────────────────────────────────────────────────

/// Multi-provider embedding client (Voyage, Gemini) with round-robin key
/// rotation and retry on 429.
#[derive(Clone)]
pub struct EmbedClient {
    inner: Arc<EmbedInner>,
}

struct EmbedInner {
    http: Client,
    /// Tighter-timeout client for user-facing query embedding (30s vs 120s).
    query_http: Client,
    provider: Provider,
    model: String,
    api_keys: Vec<String>,
    /// Resolved embeddings endpoint URL (Voyage only).
    endpoint: String,
    /// Round-robin cursor — atomically advanced on each batch call.
    key_cursor: AtomicUsize,
}

impl EmbedClient {
    /// Create a new client. Returns `Err` if `api_keys` is empty or `provider`
    /// is not a recognised provider string.
    pub fn new(
        provider: &str,
        model: String,
        api_keys: Vec<String>,
        base_url: Option<&str>,
    ) -> Result<Self> {
        if api_keys.is_empty() {
            bail!("embedding client requires at least one API key");
        }
        let provider = Provider::from_str(provider)?;
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("build reqwest client")?;
        let query_http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("build query reqwest client")?;
        let endpoint = voyage_url(base_url);
        Ok(Self {
            inner: Arc::new(EmbedInner {
                http,
                query_http,
                provider,
                model,
                api_keys,
                endpoint,
                key_cursor: AtomicUsize::new(0),
            }),
        })
    }

    /// Return the configured embedding model name.
    pub fn model(&self) -> &str {
        &self.inner.model
    }

    /// Maximum number of texts per batch for the configured provider.
    pub fn max_batch_size(&self) -> usize {
        match self.inner.provider {
            Provider::Voyage => 128,
            Provider::Gemini => 100,
        }
    }

    /// Embed a single query string with bounded retry.
    ///
    /// Uses `input_type: "query"`. On 429 from all keys, waits 2 s and retries
    /// once. A second 429 wave returns `Err`. Non-429 errors return `Err` immediately.
    pub async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let texts = vec![text.to_string()];
        let n_keys = self.inner.api_keys.len();
        let start_cursor = self.inner.key_cursor.fetch_add(1, Ordering::Relaxed) % n_keys;

        // First pass — try each key once (30s timeout per attempt).
        for offset in 0..n_keys {
            let key_idx = (start_cursor + offset) % n_keys;
            let key = &self.inner.api_keys[key_idx];
            match self
                .try_embed_query_with_key(key, &texts, InputType::Query)
                .await
            {
                Ok(mut embeddings) => {
                    return embeddings.pop().ok_or_else(|| {
                        anyhow::anyhow!("embedding provider returned empty embeddings")
                    });
                }
                Err(EmbedError::RateLimited) => {
                    warn!(
                        key_index = key_idx,
                        "VoyageAI 429 on query embed — trying next key"
                    );
                }
                Err(EmbedError::Other(e)) => return Err(e),
            }
        }

        // All keys 429 — one backoff attempt (2 s), then return Err.
        warn!("all embedding provider keys rate-limited on query embed; backing off 2s");
        tokio::time::sleep(Duration::from_secs(2)).await;

        for offset in 0..n_keys {
            let key_idx = (start_cursor + offset) % n_keys;
            let key = &self.inner.api_keys[key_idx];
            match self
                .try_embed_query_with_key(key, &texts, InputType::Query)
                .await
            {
                Ok(mut embeddings) => {
                    return embeddings.pop().ok_or_else(|| {
                        anyhow::anyhow!("embedding provider returned empty embeddings")
                    });
                }
                Err(EmbedError::RateLimited) => continue,
                Err(EmbedError::Other(e)) => return Err(e),
            }
        }

        anyhow::bail!("embedding provider query embed still rate-limited after backoff")
    }

    /// Embed texts in batches respecting both count and byte-size limits
    /// per request. Returns one Vec<f32> per input.
    ///
    /// `cancel` is an optional [`CancellationToken`]: when it fires during the
    /// indefinite 429 backoff loop, the call aborts promptly with an error for
    /// which [`is_cancel_error`] returns true.
    pub async fn embed(
        &self,
        texts: &[String],
        input_type: InputType,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<Vec<f32>>> {
        let mut all_embeddings = Vec::with_capacity(texts.len());
        for batch in byte_aware_batches(texts, self.max_batch_size()) {
            let embeddings = self.embed_batch(batch, input_type, cancel).await?;
            all_embeddings.extend(embeddings);
        }
        Ok(all_embeddings)
    }

    /// Embed a batch of texts. Public so the pipeline can drive batching
    /// manually and report per-batch progress between awaits.
    ///
    /// `cancel` is an optional [`CancellationToken`]. The first pass over keys
    /// and the indefinite exponential-backoff retry both observe it: the backoff
    /// sleep races against `cancel.cancelled()`, and the token is checked between
    /// key attempts. On cancellation the call returns an error for which
    /// [`is_cancel_error`] returns true, so the pipeline can tell a user-cancel
    /// apart from a genuine provider failure. The bounded-retry semantics are
    /// otherwise unchanged: indefinite retry on a 429-storm, immediate return on
    /// any non-429 error.
    pub async fn embed_batch(
        &self,
        texts: &[String],
        input_type: InputType,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<Vec<f32>>> {
        let n_keys = self.inner.api_keys.len();
        let start_cursor = self.inner.key_cursor.fetch_add(1, Ordering::Relaxed) % n_keys;

        // Try each key once before falling back to exponential backoff.
        for offset in 0..n_keys {
            // Bail out if cancellation arrived between attempts.
            if let Some(ct) = cancel
                && ct.is_cancelled()
            {
                return Err(cancelled_error());
            }
            let key_idx = (start_cursor + offset) % n_keys;
            let key = &self.inner.api_keys[key_idx];

            match self.try_embed_with_key(key, texts, input_type).await {
                Ok(embeddings) => return Ok(embeddings),
                Err(EmbedError::RateLimited) => {
                    warn!(key_index = key_idx, "VoyageAI 429 — trying next key");
                }
                // Non-429 error: abort immediately, old data untouched.
                Err(EmbedError::Other(e)) => return Err(e),
            }
        }

        // All keys returned 429 — exponential backoff, retry indefinitely.
        // The sleep races against cancellation so a user-cancel during a 429
        // storm aborts promptly instead of hanging forever.
        let mut delay_secs: u64 = 2;
        loop {
            warn!(
                delay_secs = delay_secs,
                "all VoyageAI keys rate-limited; backing off"
            );
            let sleep = tokio::time::sleep(Duration::from_secs(delay_secs));
            match cancel {
                Some(ct) => {
                    tokio::select! {
                        _ = sleep => {}
                        _ = ct.cancelled() => return Err(cancelled_error()),
                    }
                }
                None => sleep.await,
            }

            for offset in 0..n_keys {
                if let Some(ct) = cancel
                    && ct.is_cancelled()
                {
                    return Err(cancelled_error());
                }
                let key_idx = (start_cursor + offset) % n_keys;
                let key = &self.inner.api_keys[key_idx];
                match self.try_embed_with_key(key, texts, input_type).await {
                    Ok(embeddings) => {
                        info!("VoyageAI embed succeeded after backoff");
                        return Ok(embeddings);
                    }
                    Err(EmbedError::RateLimited) => continue,
                    Err(EmbedError::Other(e)) => return Err(e),
                }
            }

            delay_secs = (delay_secs * 2).min(60);
        }
    }

    async fn try_embed_with_key(
        &self,
        key: &str,
        texts: &[String],
        input_type: InputType,
    ) -> std::result::Result<Vec<Vec<f32>>, EmbedError> {
        self.try_embed_with_key_using(&self.inner.http, key, texts, input_type)
            .await
    }

    async fn try_embed_query_with_key(
        &self,
        key: &str,
        texts: &[String],
        input_type: InputType,
    ) -> std::result::Result<Vec<Vec<f32>>, EmbedError> {
        self.try_embed_with_key_using(&self.inner.query_http, key, texts, input_type)
            .await
    }

    async fn try_embed_with_key_using(
        &self,
        client: &Client,
        key: &str,
        texts: &[String],
        input_type: InputType,
    ) -> std::result::Result<Vec<Vec<f32>>, EmbedError> {
        match self.inner.provider {
            Provider::Voyage => {
                let body = EmbedRequest {
                    model: &self.inner.model,
                    input: texts,
                    input_type: input_type.as_str(),
                };

                let response = client
                    .post(&self.inner.endpoint)
                    .bearer_auth(key)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| EmbedError::Other(e.into()))?;

                let status = response.status();

                if status.as_u16() == 429 {
                    return Err(EmbedError::RateLimited);
                }

                if !status.is_success() {
                    let text = response.text().await.unwrap_or_default();
                    return Err(EmbedError::Other(anyhow::anyhow!(
                        "VoyageAI error {}: {}",
                        status,
                        text
                    )));
                }

                let resp: EmbedResponse = response
                    .json()
                    .await
                    .map_err(|e| EmbedError::Other(e.into()))?;

                Ok(resp.data.into_iter().map(|d| d.embedding).collect())
            }
            Provider::Gemini => {
                let task_type = match input_type {
                    InputType::Document => "RETRIEVAL_DOCUMENT",
                    InputType::Query => "RETRIEVAL_QUERY",
                };
                let requests: Vec<GeminiEmbedContentRequest> = texts
                    .iter()
                    .map(|t| GeminiEmbedContentRequest {
                        model: format!("models/{}", self.inner.model),
                        content: GeminiContent {
                            parts: vec![GeminiPart { text: t.as_str() }],
                        },
                        task_type,
                    })
                    .collect();
                let body = GeminiEmbedRequest { requests };
                let url = gemini_embed_url(&self.inner.model, key);

                let response = client
                    .post(&url)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| EmbedError::Other(e.into()))?;

                let status = response.status();

                if status.as_u16() == 429 {
                    return Err(EmbedError::RateLimited);
                }

                if !status.is_success() {
                    let text = response.text().await.unwrap_or_default();
                    return Err(EmbedError::Other(anyhow::anyhow!(
                        "Gemini embedding error {}: {}",
                        status,
                        text
                    )));
                }

                let resp: GeminiEmbedResponse = response
                    .json()
                    .await
                    .map_err(|e| EmbedError::Other(e.into()))?;

                Ok(resp.embeddings.into_iter().map(|e| e.values).collect())
            }
        }
    }
}

/// Split texts into sub-slices where each batch has at most `max_count`
/// texts AND the sum of `text.len()` stays under `MAX_BATCH_BYTES`. A single
/// text exceeding the byte cap is sent alone (the provider will truncate or
/// reject at the token level, but it won't poison the whole batch).
fn byte_aware_batches(texts: &[String], max_count: usize) -> Vec<&[String]> {
    let mut batches = Vec::new();
    let mut start = 0;
    while start < texts.len() {
        let mut end = start;
        let mut batch_bytes = 0usize;
        while end < texts.len()
            && end - start < max_count
            && (batch_bytes + texts[end].len() <= MAX_BATCH_BYTES || end == start)
        {
            batch_bytes += texts[end].len();
            end += 1;
        }
        batches.push(&texts[start..end]);
        start = end;
    }
    batches
}

enum EmbedError {
    RateLimited,
    Other(anyhow::Error),
}

/// Build the well-known cancellation error surfaced from `embed`/`embed_batch`.
/// Pairs with [`is_cancel_error`].
fn cancelled_error() -> anyhow::Error {
    anyhow::anyhow!(EMBED_CANCELLED_MSG)
}

/// Backward-compatible type alias.
pub type VoyageClient = EmbedClient;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use std::time::Duration;

    // ─── Mock-server harness for embed_batch cancellation / backoff tests ───

    /// A handler that always responds 429. Used to simulate a Voyage free-tier
    /// 429-storm where every key is rate-limited indefinitely.
    async fn always_429() -> axum::http::StatusCode {
        axum::http::StatusCode::TOO_MANY_REQUESTS
    }

    /// A valid Voyage-shaped success body for a single 3-dim embedding.
    fn success_body() -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "data": [ { "embedding": [0.1f32, 0.2f32, 0.3f32] } ]
        }))
    }

    /// Spawn a local HTTP server on 127.0.0.1:0 with the given router and return
    /// its base URL (e.g. `http://127.0.0.1:54321`). The server runs until the
    /// test process exits (the spawned task is detached).
    async fn spawn_server(router: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        format!("http://{addr}")
    }

    /// REGRESSION: a 429-storm must not hang `embed_batch` forever. With a
    /// cancellation token, cancelling mid-backoff returns an error identifying
    /// cancellation rather than parking indefinitely.
    #[tokio::test]
    async fn embed_batch_cancellation_unblocks_during_429_storm() {
        let base =
            spawn_server(axum::Router::new().route("/embeddings", axum::routing::post(always_429)))
                .await;

        let client = EmbedClient::new(
            "voyage",
            "voyage-4-lite".to_string(),
            vec!["k1".to_string(), "k2".to_string()],
            Some(&base),
        )
        .unwrap();

        let token = CancellationToken::new();
        let token_for_cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            token_for_cancel.cancel();
        });

        let texts = vec!["hello world".to_string()];
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            client.embed_batch(&texts, InputType::Document, Some(&token)),
        )
        .await;

        // Did not time out (i.e. did not hang).
        let inner = result.expect("embed_batch hung past 5s — cancellation did not unblock it");
        // Returned an error that is identifiable as cancellation.
        let err = inner.expect_err("embed_batch should error on cancellation, not succeed");
        assert!(
            is_cancel_error(&err),
            "expected a cancellation error, got: {err:#}"
        );
    }

    /// The non-cancelled path still works: a server that 429s the first key
    /// attempt then succeeds returns embeddings. Two keys mean the second
    /// (successful) attempt happens in the first key-rotation pass — no backoff
    /// sleep involved, so the test is fast and deterministic.
    #[tokio::test]
    async fn embed_batch_succeeds_after_first_key_429_no_sleep() {
        // First request → 429, every subsequent request → 200.
        let counter = Arc::new(AtomicU32::new(0));
        let counter_for_handler = counter.clone();
        let handler = move || {
            let counter = counter_for_handler.clone();
            async move {
                let n = counter.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    axum::http::StatusCode::TOO_MANY_REQUESTS.into_response()
                } else {
                    success_body().into_response()
                }
            }
        };

        use axum::response::IntoResponse;
        let base =
            spawn_server(axum::Router::new().route("/embeddings", axum::routing::post(handler)))
                .await;

        let client = EmbedClient::new(
            "voyage",
            "voyage-4-lite".to_string(),
            vec!["k1".to_string(), "k2".to_string()],
            Some(&base),
        )
        .unwrap();

        let token = CancellationToken::new();
        let texts = vec!["hello".to_string()];
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            client.embed_batch(&texts, InputType::Document, Some(&token)),
        )
        .await
        .expect("embed_batch should not hang on the success-after-one-429 path");

        let embeddings = result.expect("embed_batch should succeed after key rotation");
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0], vec![0.1f32, 0.2f32, 0.3f32]);
        // First key 429'd, second key succeeded — exactly 2 requests, no backoff.
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    /// A `None` cancel token preserves the original semantics on the happy path:
    /// embeddings come back without any token plumbing.
    #[tokio::test]
    async fn embed_batch_none_token_succeeds() {
        let base = spawn_server(axum::Router::new().route(
            "/embeddings",
            axum::routing::post(|| async { success_body() }),
        ))
        .await;

        let client = EmbedClient::new(
            "voyage",
            "voyage-4-lite".to_string(),
            vec!["k1".to_string()],
            Some(&base),
        )
        .unwrap();

        let texts = vec!["hi".to_string()];
        let embeddings = client
            .embed_batch(&texts, InputType::Document, None)
            .await
            .expect("embed_batch with no token should succeed");
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0], vec![0.1f32, 0.2f32, 0.3f32]);
    }

    #[test]
    fn voyage_url_default_when_none() {
        assert_eq!(voyage_url(None), VOYAGE_ENDPOINT);
    }

    #[test]
    fn voyage_url_default_when_blank() {
        assert_eq!(voyage_url(Some("")), VOYAGE_ENDPOINT);
        assert_eq!(voyage_url(Some("   ")), VOYAGE_ENDPOINT);
        assert_eq!(voyage_url(Some("\t\n")), VOYAGE_ENDPOINT);
    }

    #[test]
    fn voyage_url_appends_to_base() {
        assert_eq!(
            voyage_url(Some("https://my-proxy.com/v1")),
            "https://my-proxy.com/v1/embeddings"
        );
        assert_eq!(
            voyage_url(Some("http://localhost:8080/api/v1")),
            "http://localhost:8080/api/v1/embeddings"
        );
    }

    #[test]
    fn voyage_url_strips_trailing_slash() {
        assert_eq!(
            voyage_url(Some("https://my-proxy.com/v1/")),
            "https://my-proxy.com/v1/embeddings"
        );
    }

    #[test]
    fn voyage_url_accepts_full_form() {
        assert_eq!(
            voyage_url(Some("https://my-proxy.com/v1/embeddings")),
            "https://my-proxy.com/v1/embeddings"
        );
    }

    #[test]
    fn voyage_url_accepts_full_form_trailing_slash() {
        assert_eq!(
            voyage_url(Some("https://my-proxy.com/v1/embeddings/")),
            "https://my-proxy.com/v1/embeddings"
        );
    }

    #[test]
    fn byte_aware_batches_splits_by_size() {
        // 600 KB each → only 2 fit in 1.5 MB cap (1.2 MB < 1.5 MB, 1.8 MB > 1.5 MB)
        let texts: Vec<String> = (0..5).map(|_| "x".repeat(600_000)).collect();
        let batches = byte_aware_batches(&texts, MAX_BATCH_SIZE);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), 2);
        assert_eq!(batches[1].len(), 2);
        assert_eq!(batches[2].len(), 1);
    }

    #[test]
    fn byte_aware_batches_respects_count_limit() {
        let texts: Vec<String> = (0..200).map(|_| "short".to_string()).collect();
        let batches = byte_aware_batches(&texts, 128);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 128);
        assert_eq!(batches[1].len(), 72);
    }

    #[test]
    fn byte_aware_batches_oversized_single_text_sent_alone() {
        let texts: Vec<String> = vec!["x".repeat(3_000_000), "small".to_string()];
        let batches = byte_aware_batches(&texts, MAX_BATCH_SIZE);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 1);
        assert_eq!(batches[1].len(), 1);
    }

    #[test]
    fn gemini_url_contains_model_and_key() {
        let url = gemini_embed_url("gemini-embedding-004", "test-key-123");
        assert!(url.contains("models/gemini-embedding-004:batchEmbedContents"));
        assert!(url.contains("key=test-key-123"));
        assert!(url.starts_with("https://generativelanguage.googleapis.com"));
    }

    #[test]
    fn provider_parse_voyage() {
        assert_eq!(Provider::from_str("voyage").unwrap(), Provider::Voyage);
        assert_eq!(Provider::from_str("VOYAGE").unwrap(), Provider::Voyage);
        assert_eq!(Provider::from_str("").unwrap(), Provider::Voyage);
        assert_eq!(Provider::from_str("   ").unwrap(), Provider::Voyage);
    }

    #[test]
    fn provider_parse_gemini() {
        assert_eq!(Provider::from_str("gemini").unwrap(), Provider::Gemini);
        assert_eq!(Provider::from_str("GEMINI").unwrap(), Provider::Gemini);
        assert_eq!(Provider::from_str("google").unwrap(), Provider::Gemini);
        assert_eq!(Provider::from_str("Google").unwrap(), Provider::Gemini);
    }

    #[test]
    fn provider_parse_unknown_errors() {
        assert!(Provider::from_str("openai").is_err());
        assert!(Provider::from_str("anthropic").is_err());
    }
}

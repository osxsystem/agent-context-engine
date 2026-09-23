//! Reranking with TypeSafe's Jev, a System One model: instead of generating a
//! JSON ordering, it answers a typed yes/no ("noul") question with a calibrated
//! probability. Each candidate chunk gets one request asking whether it
//! implements or defines what the query asks for; candidates sort by the
//! returned probability.
//!
//! Wire contract (`POST {base}/v1/systemone`, `Authorization: Bearer <key>`):
//! `{model, state, questions: {relevance: {type: "noul", ...}}}` answered by
//! `{answers: {relevance: {type: "noul", noul: <0..1>}}, usage: {...}}`.
//!
//! Narrowing is not done here yet: every chunk comes back whole.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use futures::{StreamExt, TryStreamExt};
use serde_json::{Value, json};

use crate::config::LlmConfig;
use crate::llm::keys::{KeyRing, RateLimited};
use crate::query::reranker::{RerankOutput, RerankRequest, Reranker};

/// TypeSafe's public API; overridden by `llm.jev_base_url`.
pub const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
/// The vendor's stable alias. `llm.rerank_model` is not consulted: the LLM path
/// and chat share it, so a Jev name there would break them.
pub const MODEL: &str = "jev-latest";
/// Requests in flight at once for one rerank. A query fans out one request per
/// candidate (~30), so this is what keeps a single query from bursting through
/// the per-minute quota.
pub const MAX_CONCURRENT_REQUESTS: usize = 8;

/// The whole fan-out must finish within this, or the query degrades to
/// similarity order. It also bounds a client that failed to build with its
/// per-request timeouts.
pub const RERANK_DEADLINE: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

const RELEVANCE_INSTRUCTIONS: &str = "Does this code implement or define what the query asks for?";
const RELEVANCE_TRUE: &str =
    "The code implements, defines, or directly answers what the query asks for.";
const RELEVANCE_FALSE: &str = "The code is unrelated to the query, or only mentions, calls, \
                               or tests the thing asked for without being it.";

pub struct JevReranker {
    http: reqwest::Client,
    url: String,
    deadline: Duration,
    /// Empty when no TypeSafe key is configured; reranking then degrades.
    keys: KeyRing,
}

impl JevReranker {
    pub fn new(config: &LlmConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            http,
            url: systemone_url(config.jev_base_url.as_deref()),
            deadline: RERANK_DEADLINE,
            // Rebuilt for every query, so rotation state must outlive it.
            keys: KeyRing::shared(
                config
                    .jev_api_keys
                    .iter()
                    .map(|k| k.trim().to_owned())
                    .filter(|k| !k.is_empty())
                    .collect(),
            ),
        }
    }

    /// Replaces the pause before rate-limited requests retry on other keys.
    pub fn with_retry_pause(mut self, pause: Duration) -> Self {
        self.keys = self.keys.with_pause(pause);
        self
    }

    /// Replaces [`RERANK_DEADLINE`] for this reranker.
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// The request body for one candidate.
    fn request_body(&self, query: &str, req: &RerankRequest<'_>, i: usize) -> Value {
        let chunk = &req.chunks[i];
        let code = req.numbered[i].as_deref().unwrap_or(&chunk.content);
        json!({
            "model": MODEL,
            "state": {
                "query": query,
                "file_path": chunk.file,
                "symbol_name": chunk.symbol,
                "symbol_kind": chunk.symbol_kind,
                "code": code,
            },
            "questions": {
                "relevance": {
                    "type": "noul",
                    "instructions": RELEVANCE_INSTRUCTIONS,
                    "criteria": { "true": RELEVANCE_TRUE, "false": RELEVANCE_FALSE },
                },
            },
        })
    }

    /// Sends one candidate's request, starting on the next key round-robin and
    /// moving to other keys when it fails.
    async fn ask(&self, body: &Value) -> Result<(f64, Value)> {
        self.keys
            .send("Jev request", |key| async move {
                self.ask_with_key(&key, body).await
            })
            .await
    }

    /// Sends one request; returns the relevance probability and the raw reply.
    async fn ask_with_key(&self, key: &str, body: &Value) -> Result<(f64, Value)> {
        let resp = self
            .http
            .post(&self.url)
            .bearer_auth(key)
            .json(body)
            .send()
            .await
            .context("request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!(
                "HTTP {status}: {}",
                text.chars().take(200).collect::<String>()
            );
        }
        let reply: Value = resp.json().await.context("response is not JSON")?;
        let p = reply["answers"]["relevance"]["noul"]
            .as_f64()
            .context("response has no answers.relevance.noul probability")?;
        Ok((p, reply))
    }
}

impl Reranker for JevReranker {
    async fn rerank(&self, req: RerankRequest<'_>) -> RerankOutput {
        let start = Instant::now();
        let n = req.chunks.len();
        let similarity_order =
            |skip_reason: String, fallback_used: bool, raw_request| RerankOutput {
                reranked_indices: (0..n).collect(),
                line_selections: vec![None; n],
                raw_request,
                raw_response: String::new(),
                elapsed_ms: start.elapsed().as_millis() as u64,
                fallback_used,
                skip_reason: Some(skip_reason),
                relevance: Vec::new(),
            };

        if n == 0 {
            return RerankOutput {
                reranked_indices: Vec::new(),
                line_selections: Vec::new(),
                raw_request: String::new(),
                raw_response: String::new(),
                elapsed_ms: 0,
                fallback_used: false,
                skip_reason: None,
                relevance: Vec::new(),
            };
        }

        if self.keys.is_empty() {
            return similarity_order(
                "no TypeSafe API key configured for the Jev reranker".to_owned(),
                false,
                String::new(),
            );
        }

        let bodies: Vec<Value> = (0..n)
            .map(|i| self.request_body(req.query, &req, i))
            .collect();
        let raw_request = Value::Array(bodies.clone()).to_string();
        // `buffered` keeps results in candidate order and caps how many
        // requests are in flight; the first failure stops the rest.
        // Futures are lazy: building them all up front sends nothing.
        let requests: Vec<_> = bodies.iter().map(|body| self.ask(body)).collect();
        let fan_out = futures::stream::iter(requests)
            .buffered(MAX_CONCURRENT_REQUESTS)
            .try_collect::<Vec<(f64, Value)>>();
        let answers = match tokio::time::timeout(self.deadline, fan_out).await {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                let reason = match e.downcast_ref::<RateLimited>() {
                    Some(r) => format!(
                        "Jev reranking was rate-limited on {} of {} TypeSafe keys; last error: {:#}",
                        r.limited, r.keys, r.last
                    ),
                    None => format!("Jev request failed: {e:#}"),
                };
                return similarity_order(reason, true, raw_request);
            }
            Err(_) => {
                return similarity_order(
                    format!("Jev reranking missed its {:?} deadline", self.deadline),
                    true,
                    raw_request,
                );
            }
        };

        let (relevance, replies): (Vec<f64>, Vec<Value>) = answers.into_iter().unzip();
        let mut reranked_indices: Vec<usize> = (0..n).collect();
        // Stable: equal probabilities keep their similarity order.
        reranked_indices.sort_by(|&a, &b| relevance[b].total_cmp(&relevance[a]));
        RerankOutput {
            reranked_indices,
            line_selections: vec![None; n],
            raw_request,
            raw_response: Value::Array(replies).to_string(),
            elapsed_ms: start.elapsed().as_millis() as u64,
            fallback_used: false,
            skip_reason: None,
            relevance,
        }
    }
}

/// `…/v1/systemone` from a base URL given bare, as `…/v1`, or in full.
fn systemone_url(base: Option<&str>) -> String {
    let base = base
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .unwrap_or(DEFAULT_BASE_URL);
    let base = base.trim_end_matches('/');
    let base = base.strip_suffix("/v1/systemone").unwrap_or(base);
    let base = base.strip_suffix("/v1").unwrap_or(base);
    format!("{base}/v1/systemone")
}

#[cfg(test)]
mod tests {
    use super::systemone_url;

    #[test]
    fn systemone_url_accepts_every_base_form() {
        for base in [
            "http://127.0.0.1:9000",
            "http://127.0.0.1:9000/",
            "http://127.0.0.1:9000/v1",
            "http://127.0.0.1:9000/v1/systemone",
        ] {
            assert_eq!(
                systemone_url(Some(base)),
                "http://127.0.0.1:9000/v1/systemone",
                "{base}"
            );
        }
        assert_eq!(systemone_url(None), "https://api.typesafe.ai/v1/systemone");
        assert_eq!(
            systemone_url(Some("  ")),
            "https://api.typesafe.ai/v1/systemone"
        );
    }
}

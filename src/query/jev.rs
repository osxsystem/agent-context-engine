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
//! The same request narrows the chunk: one `span_<n>` question per symbol span
//! the engine found inside it. Spans clearing [`SPAN_THRESHOLD`] become the
//! chunk's line ranges; with none, the chunk comes back whole. Spans are real
//! symbol boundaries, so the LLM path's range padding and sanitising do not
//! apply here.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use futures::{StreamExt, TryStreamExt};
use serde_json::{Value, json};

use crate::config::LlmConfig;
use crate::llm::keys::{KeyRing, RateLimited};
use crate::query::reranker::{RerankOutput, RerankRequest, Reranker, SymbolSpan};

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

/// A span whose probability reaches this is selected.
pub const SPAN_THRESHOLD: f64 = 0.5;

const RELEVANCE_INSTRUCTIONS: &str = "Does this code implement or define what the query asks for?";
const RELEVANCE_TRUE: &str =
    "The code implements, defines, or directly answers what the query asks for.";
const RELEVANCE_FALSE: &str = "The code is unrelated to the query, or only mentions, calls, \
                               or tests the thing asked for without being it.";
const SPAN_TRUE: &str = "This symbol is where the code answering the query lives.";
const SPAN_FALSE: &str = "This symbol is not needed to answer the query; the answer is \
                          elsewhere in the code or not in it at all.";

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

    /// The request body for one candidate: the relevance question plus one
    /// question per span in `spans`, all over the same state.
    fn request_body(
        &self,
        query: &str,
        req: &RerankRequest<'_>,
        i: usize,
        spans: &[SymbolSpan],
    ) -> Value {
        let chunk = &req.chunks[i];
        let code = req.numbered[i].as_deref().unwrap_or(&chunk.content);
        let mut body = json!({
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
        });
        for (j, span) in spans.iter().enumerate() {
            let kind = span.kind.as_deref().unwrap_or("symbol");
            body["questions"][span_key(j)] = json!({
                "type": "noul",
                "instructions": format!(
                    "Is the {kind} `{}` (lines {}-{}) part of what the query asks for?",
                    span.name, span.line_start, span.line_end
                ),
                "criteria": { "true": SPAN_TRUE, "false": SPAN_FALSE },
            });
        }
        body
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

        let spans: Vec<&[SymbolSpan]> = (0..n).map(|i| narrowing_spans(&req, i)).collect();
        let bodies: Vec<Value> = (0..n)
            .map(|i| self.request_body(req.query, &req, i, spans[i]))
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
        // Selections align with the ranked order, as the engine reads them.
        let line_selections = reranked_indices
            .iter()
            .map(|&i| selected_ranges(spans[i], &replies[i]))
            .collect();
        RerankOutput {
            reranked_indices,
            line_selections,
            raw_request,
            raw_response: Value::Array(replies).to_string(),
            elapsed_ms: start.elapsed().as_millis() as u64,
            fallback_used: false,
            skip_reason: None,
            relevance,
        }
    }
}

/// The spans to ask about for chunk `i`: none when the chunk is under the
/// prune floor, since it is short enough to show whole.
fn narrowing_spans<'a>(req: &'a RerankRequest<'_>, i: usize) -> &'a [SymbolSpan] {
    let chunk = &req.chunks[i];
    if chunk.line_end.saturating_sub(chunk.line_start) < req.min_prune_lines {
        return &[];
    }
    req.candidate_spans.get(i).map_or(&[], Vec::as_slice)
}

fn span_key(j: usize) -> String {
    format!("span_{j}")
}

/// The line ranges of the spans whose answer clears [`SPAN_THRESHOLD`], a
/// span nested in another selected one folded into it; `None` returns the
/// chunk whole. A missing answer counts as not selected.
fn selected_ranges(spans: &[SymbolSpan], reply: &Value) -> Option<Vec<(u32, u32)>> {
    let mut picked: Vec<(u32, u32)> = spans
        .iter()
        .enumerate()
        .filter(|(j, _)| {
            reply["answers"][span_key(*j)]["noul"]
                .as_f64()
                .is_some_and(|p| p >= SPAN_THRESHOLD)
        })
        .map(|(_, s)| (s.line_start, s.line_end))
        .collect();
    picked.sort_unstable();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for (start, end) in picked {
        match ranges.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => ranges.push((start, end)),
        }
    }
    (!ranges.is_empty()).then_some(ranges)
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

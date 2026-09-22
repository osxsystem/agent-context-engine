//! Rerank A/B: measure what the rerank stage actually does to ranking quality.
//!
//! The chunk benchmark deliberately queries with `rerank: false`, so until now
//! nothing in this repo had measured the rerank stage at all.
//!
//! **One request, two rankings.** A single `rerank: true` query returns both
//! `pre_rerank_results` (the merge order the reranker was handed) and `results`
//! (what it returned), over the *identical* candidate set. Scoring both sides of
//! one response beats issuing two requests: no embedding jitter, no index drift,
//! and no candidate-set difference to explain away, so the delta is attributable
//! to the reranker and nothing else.
//!
//! Correctness is not redefined here. Both rankings are scored by
//! `scoring::RecallTally`, the same type the chunk benchmark scores its single
//! ranking with, over cases from the same `eval::derive_eval_set`.

use std::collections::BTreeMap;

use crate::eval::derive_eval_set;
use crate::scoring::{RankingScore, RecallTally, ScoreDeltas};
use crate::{EVAL_LIMIT, QueryResultRow};

/// Nearest-rank percentiles over the per-query rerank-stage latencies.
///
/// `rank = ceil(p/100 * N)` on an ascending sort, 1-based — the method the shell
/// benches already document and use, so a p50 here is comparable to theirs.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone)]
pub struct LatencySummary {
    pub samples: usize,
    pub min_ms: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub max_ms: u64,
}

impl LatencySummary {
    fn from_samples(mut xs: Vec<u64>) -> Self {
        if xs.is_empty() {
            return Self::default();
        }
        xs.sort_unstable();
        let n = xs.len();
        let at = |p: f64| -> u64 {
            let rank = ((p * n as f64).ceil() as usize).clamp(1, n);
            xs[rank - 1]
        };
        Self {
            samples: n,
            min_ms: xs[0],
            p50_ms: at(0.50),
            p95_ms: at(0.95),
            max_ms: xs[n - 1],
        }
    }
}

/// What the server was configured to rerank with, recorded so an artifact stays
/// interpretable months later.
///
/// Read best-effort from `/api/config`. That endpoint returns the whole settings
/// object including API keys; only these three fields are declared here, with no
/// flatten and no catch-all, so serde drops the rest and no key reaches disk.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone)]
pub struct RerankerDescriptor {
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Agentic RAG routes reranking through a tool-calling loop instead of the
    /// single-shot reranker. A decision model cannot call tools, so an A/B taken
    /// with this `true` is not comparing alternatives.
    pub agentic_rag: Option<bool>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct RerankAbReport {
    pub label: String,
    pub timestamp_unix: u64,
    pub repo: String,
    pub server: String,
    pub top_k: u64,
    pub eval_cases: u64,
    /// Cases whose request and decode both succeeded. Compare against
    /// `eval_cases` before trusting a delta.
    pub queries_scored: u64,
    pub reranker: RerankerDescriptor,
    /// Ranking as the reranker received it — the no-rerank control.
    pub baseline: RankingScore,
    /// Ranking as the reranker returned it.
    pub candidate: RankingScore,
    pub deltas: ScoreDeltas,
    /// Latency of the rerank stage, over queries where it actually ran.
    pub rerank_ms: LatencySummary,
    /// Queries where the reranker degraded instead of ranking. These are
    /// excluded from `rerank_ms` — a skipped rerank costs almost no time, so
    /// counting it would drag p50 down and make a broken reranker look fast.
    pub rerank_skipped: u64,
    pub skip_reasons: BTreeMap<String, u64>,
    pub reproduce_cmd: String,
}

// ─── Wire shapes ────────────────────────────────────────────────────────────

#[derive(serde::Deserialize, Default)]
struct Timing {
    /// Deliberately `Option`: an absent field means "this server did not report
    /// it", which must not silently become a 0 ms sample in a p50 that gates a
    /// decision.
    rerank_ms: Option<u64>,
}

#[derive(serde::Deserialize)]
struct RerankInfoRow {
    #[serde(default)]
    skip_reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct RerankQueryResp {
    #[serde(default)]
    results: Vec<QueryResultRow>,
    #[serde(default)]
    pre_rerank_results: Vec<QueryResultRow>,
    #[serde(default)]
    timing: Timing,
    #[serde(default)]
    rerank: Option<RerankInfoRow>,
}

/// Narrow view of `/api/config` — see `RerankerDescriptor` on key safety.
#[derive(serde::Deserialize)]
struct ConfigResp {
    #[serde(default)]
    llm: Option<LlmConfigView>,
}

#[derive(serde::Deserialize)]
struct LlmConfigView {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    rerank_model: Option<String>,
    #[serde(default)]
    agentic_rag: Option<bool>,
}

// ─── Run ────────────────────────────────────────────────────────────────────

pub fn run(repo: &str, server: &str, out: &str, label: &str, top_k: u64, compare: Option<&str>) {
    let cases = derive_eval_set(repo, EVAL_LIMIT);
    if cases.is_empty() {
        eprintln!(
            "[chunk_bench] ERROR: no retrieval cases could be derived from {repo}. The eval set \
             needs documented functions or methods; without them there is nothing to score."
        );
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let client = reqwest::Client::new();

    let mut pre = RecallTally::default();
    let mut post = RecallTally::default();
    let mut rerank_samples: Vec<u64> = Vec::new();
    let mut rerank_skipped = 0u64;
    let mut skip_reasons: BTreeMap<String, u64> = BTreeMap::new();

    let descriptor = rt.block_on(fetch_descriptor(&client, server));
    if descriptor.agentic_rag == Some(true) {
        eprintln!(
            "[chunk_bench] WARNING: the server has agentic_rag ENABLED. Reranking is routed \
             through a tool-calling loop, not the single-shot reranker. A decision model cannot \
             call tools, so these numbers are NOT a fair baseline for a single-shot reranker \
             comparison. Set agentic_rag=false and re-run."
        );
    }

    rt.block_on(async {
        for case in &cases {
            let exp_file_norm = case.file.to_lowercase();
            let body = serde_json::json!({
                "query": case.query,
                "repo": repo,
                "top_k": top_k,
                "rerank": true,
            });
            let resp = match client
                .post(format!("{server}/api/query"))
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[chunk_bench] query error for {:?}: {e}", case.symbol);
                    continue;
                }
            };
            let parsed: RerankQueryResp = match resp.json().await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("[chunk_bench] decode error for {:?}: {e}", case.symbol);
                    continue;
                }
            };

            let skipped = parsed.rerank.as_ref().and_then(|r| r.skip_reason.clone());
            match skipped {
                Some(reason) => {
                    rerank_skipped += 1;
                    *skip_reasons.entry(reason).or_insert(0) += 1;
                }
                // Only time reranks that actually happened.
                None => {
                    if let Some(ms) = parsed.timing.rerank_ms {
                        rerank_samples.push(ms);
                    }
                }
            }

            let exp = (case.line_start, case.line_end);
            pre.observe(&exp_file_norm, exp, &parsed.pre_rerank_results);
            post.observe(&exp_file_norm, exp, &parsed.results);
        }
    });

    let baseline = pre.finish("no-rerank (merge order)");
    let candidate = post.finish(label);
    let deltas = ScoreDeltas::between(&candidate, &baseline);
    let queries_scored = post.observed();

    let report = RerankAbReport {
        label: label.to_owned(),
        timestamp_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        repo: repo.to_owned(),
        server: server.to_owned(),
        top_k,
        eval_cases: cases.len() as u64,
        queries_scored,
        reranker: descriptor,
        baseline,
        candidate,
        deltas,
        rerank_ms: LatencySummary::from_samples(rerank_samples),
        rerank_skipped,
        skip_reasons,
        reproduce_cmd: format!(
            "cargo run --release --bin chunk_bench -- {repo} {server} {out} --rerank-ab --label {label} --top-k {top_k}"
        ),
    };

    if report.queries_scored == 0 {
        eprintln!("[chunk_bench] ERROR: no queries were scored; the numbers below are vacuous.");
    }
    if report.rerank_skipped > 0 {
        eprintln!(
            "[chunk_bench] WARNING: the reranker degraded on {}/{} scored queries ({:?}). The \
             deltas measure an absent reranker, not a bad one.",
            report.rerank_skipped, report.queries_scored, report.skip_reasons
        );
    }

    let json = serde_json::to_string_pretty(&report).expect("serialize report");
    std::fs::write(out, &json).unwrap_or_else(|e| panic!("write {out}: {e}"));
    println!("{json}");

    if let Some(prior_path) = compare {
        match compare_runs(prior_path, &report) {
            Ok(text) => println!("\n{text}"),
            Err(e) => eprintln!("[chunk_bench] compare failed for {prior_path}: {e}"),
        }
    }
}

async fn fetch_descriptor(client: &reqwest::Client, server: &str) -> RerankerDescriptor {
    let Ok(resp) = client.get(format!("{server}/api/config")).send().await else {
        return RerankerDescriptor::default();
    };
    let Ok(cfg) = resp.json::<ConfigResp>().await else {
        return RerankerDescriptor::default();
    };
    match cfg.llm {
        Some(l) => RerankerDescriptor {
            provider: l.provider,
            model: l.rerank_model,
            agentic_rag: l.agentic_rag,
        },
        None => RerankerDescriptor::default(),
    }
}

/// Cross-run comparison: this run's reranker row against a previously recorded
/// artifact's. This is the two-reranker axis — run once per reranker, then diff.
///
/// Runs are only comparable when they scored the same cases against the same
/// index, so mismatched repo, top_k or case count is reported rather than
/// quietly differenced.
fn compare_runs(prior_path: &str, current: &RerankAbReport) -> Result<String, String> {
    let raw = std::fs::read_to_string(prior_path).map_err(|e| e.to_string())?;
    let prior: RerankAbReport = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    let mut warnings = Vec::new();
    if prior.repo != current.repo {
        warnings.push(format!("repo differs: {} vs {}", current.repo, prior.repo));
    }
    if prior.top_k != current.top_k {
        warnings.push(format!("top_k differs: {} vs {}", current.top_k, prior.top_k));
    }
    if prior.queries_scored != current.queries_scored {
        warnings.push(format!(
            "scored-query count differs: {} vs {}",
            current.queries_scored, prior.queries_scored
        ));
    }
    if prior.reranker.provider == current.reranker.provider
        && prior.reranker.model == current.reranker.model
    {
        warnings.push(format!(
            "both runs used the same reranker ({:?}/{:?}) — this is not an A/B",
            current.reranker.provider, current.reranker.model
        ));
    }

    let d = ScoreDeltas::between(&current.candidate, &prior.candidate);
    let mut out = format!(
        "=== cross-run deltas: {} (current) − {} (prior) ===\n\
         recall@1   {:+.4}   ({:.4} vs {:.4})\n\
         recall@5   {:+.4}   ({:.4} vs {:.4})\n\
         recall@10  {:+.4}   ({:.4} vs {:.4})\n\
         mean_iou   {:+.4}   ({:.4} vs {:.4})\n\
         rerank p50 {:+}ms   ({}ms vs {}ms)",
        current.label,
        prior.label,
        d.recall_at_1,
        current.candidate.recall_at_1,
        prior.candidate.recall_at_1,
        d.recall_at_5,
        current.candidate.recall_at_5,
        prior.candidate.recall_at_5,
        d.recall_at_10,
        current.candidate.recall_at_10,
        prior.candidate.recall_at_10,
        d.mean_iou,
        current.candidate.mean_iou,
        prior.candidate.mean_iou,
        current.rerank_ms.p50_ms as i64 - prior.rerank_ms.p50_ms as i64,
        current.rerank_ms.p50_ms,
        prior.rerank_ms.p50_ms,
    );
    for w in warnings {
        out.push_str(&format!("\nWARNING: {w}"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_nearest_rank() {
        // n=10: p50 rank = ceil(0.50*10) = 5 -> xs[4] = 5.
        //       p95 rank = ceil(0.95*10) = 10 -> xs[9] = 10.
        let s = LatencySummary::from_samples((1..=10).collect());
        assert_eq!(s.samples, 10);
        assert_eq!((s.min_ms, s.p50_ms, s.p95_ms, s.max_ms), (1, 5, 10, 10));
    }

    #[test]
    fn percentiles_handle_unsorted_input_and_single_sample() {
        let s = LatencySummary::from_samples(vec![90, 10, 50]);
        assert_eq!((s.min_ms, s.p50_ms, s.max_ms), (10, 50, 90));
        let one = LatencySummary::from_samples(vec![42]);
        assert_eq!((one.samples, one.p50_ms, one.p95_ms), (1, 42, 42));
    }

    #[test]
    fn empty_samples_do_not_panic() {
        let s = LatencySummary::from_samples(vec![]);
        assert_eq!(s.samples, 0);
        assert_eq!(s.p50_ms, 0);
    }
}

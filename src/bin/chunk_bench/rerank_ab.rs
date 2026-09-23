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
use crate::{EVAL_LIMIT, QueryResultRow, warm_up};

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
    pub no_rerank: RankingScore,
    /// Ranking as the reranker returned it.
    pub reranked: RankingScore,
    pub deltas: ScoreDeltas,
    /// Latency of the rerank stage, over queries where it actually ran.
    pub rerank_ms: LatencySummary,
    /// Queries where the reranker degraded instead of ranking. These are
    /// excluded from `rerank_ms` — a skipped rerank costs almost no time, so
    /// counting it would drag p50 down and make a broken reranker look fast.
    pub rerank_skipped: u64,
    pub skip_reasons: BTreeMap<String, u64>,
    /// Queries with no candidates to rank, so the rerank stage never ran. Still
    /// scored (a miss is a miss), but excluded from `rerank_ms` like skips.
    #[serde(default)]
    pub rerank_not_run: u64,
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

// ─── Guards ─────────────────────────────────────────────────────────────────

/// Why this server configuration cannot produce a meaningful A/B, if it can't.
///
/// Agentic RAG routes reranking through a tool-calling loop, and a decision
/// model cannot call tools, so a run taken with it on compares nothing. This was
/// once a warning; a warning did not stop an artifact being written from exactly
/// that run, so it is a refusal now. An unreadable config (`None`) is allowed —
/// an older server must stay benchmarkable.
fn config_refusal(d: &RerankerDescriptor) -> Option<String> {
    (d.agentic_rag == Some(true)).then(|| {
        "the server has agentic_rag ENABLED: reranking runs through a tool-calling loop, not \
         the single-shot reranker, so the numbers would not compare rerankers. Set \
         agentic_rag=false and re-run."
            .to_owned()
    })
}

/// What the rerank stage did for one query.
#[derive(Debug, PartialEq)]
enum RerankOutcome<'a> {
    /// Ranked real candidates. The only case that yields a latency sample; the
    /// sample itself stays optional, see `Timing::rerank_ms`.
    Ran(Option<u64>),
    /// Degraded to merge order, with the reason.
    Skipped(&'a str),
    /// Had nothing to rank. Both the engine (empty vector search) and the
    /// reranker (empty candidate list) return early at ~0 ms with no skip
    /// reason, so without this case they would pass for instant reranks.
    NotRun,
}

fn rerank_outcome(resp: &RerankQueryResp) -> RerankOutcome<'_> {
    if resp.pre_rerank_results.is_empty() {
        return RerankOutcome::NotRun;
    }
    match resp.rerank.as_ref() {
        None => RerankOutcome::NotRun,
        Some(RerankInfoRow {
            skip_reason: Some(reason),
        }) => RerankOutcome::Skipped(reason),
        Some(_) => RerankOutcome::Ran(resp.timing.rerank_ms),
    }
}

/// Longest skip-reason prefix kept as a `skip_reasons` key.
const SKIP_BUCKET_CHARS: usize = 120;

/// Collapse a skip reason to a stable, readable bucket key.
///
/// Transport failures carry the upstream error body verbatim, including quota
/// countdowns and request ids, so keying on the raw string gives every failure
/// its own bucket and a report full of kilobyte-long JSON keys. The first line,
/// whitespace-collapsed and cut at `SKIP_BUCKET_CHARS`, names the failure kind.
fn skip_bucket(reason: &str) -> String {
    let first_line = reason.lines().next().unwrap_or("");
    let collapsed = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= SKIP_BUCKET_CHARS {
        return collapsed;
    }
    let mut cut: String = collapsed.chars().take(SKIP_BUCKET_CHARS).collect();
    cut.push('…');
    cut
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
    let mut rerank_not_run = 0u64;
    let mut skip_reasons: BTreeMap<String, u64> = BTreeMap::new();

    let descriptor = rt.block_on(fetch_descriptor(&client, server));
    // Recall@10 is scored against the returned top-k; asked for fewer than ten,
    // it would silently equal recall@top_k while still being labelled @10.
    if top_k < 10 {
        eprintln!("[chunk_bench] ERROR: --top-k must be at least 10 for recall@10 to mean anything");
        std::process::exit(2);
    }
    if let Some(why) = config_refusal(&descriptor) {
        eprintln!("[chunk_bench] ERROR: refusing to run: {why}");
        std::process::exit(2);
    }

    rt.block_on(async {
        warm_up(&client, server, repo).await;
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

            match rerank_outcome(&parsed) {
                // A reranker that cannot serve the first query is almost always
                // down for the whole run (quota, key, endpoint), and each failed
                // attempt can cost the full retry budget. Stop before spending
                // minutes on an artifact that measures an absent reranker.
                RerankOutcome::Skipped(reason) if post.observed() == 0 => {
                    eprintln!(
                        "[chunk_bench] ERROR: the reranker skipped the first scored query; nothing \
                         written. Reason: {reason}"
                    );
                    std::process::exit(3);
                }
                RerankOutcome::Skipped(reason) => {
                    rerank_skipped += 1;
                    *skip_reasons.entry(skip_bucket(reason)).or_insert(0) += 1;
                }
                // Only time reranks that actually happened.
                RerankOutcome::Ran(ms) => rerank_samples.extend(ms),
                RerankOutcome::NotRun => rerank_not_run += 1,
            }

            let exp = (case.line_start, case.line_end);
            pre.observe(&exp_file_norm, exp, &parsed.pre_rerank_results);
            post.observe(&exp_file_norm, exp, &parsed.results);
        }
    });

    let no_rerank = pre.finish("no-rerank (merge order)");
    let reranked = post.finish(label);
    let deltas = ScoreDeltas::between(&reranked, &no_rerank);
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
        no_rerank,
        reranked,
        deltas,
        rerank_ms: LatencySummary::from_samples(rerank_samples),
        rerank_skipped,
        skip_reasons,
        rerank_not_run,
        reproduce_cmd: format!(
            "cargo run --release --bin chunk_bench -- {repo} {server} {out} --rerank-ab --label {label} --top-k {top_k}"
        ),
    };

    if report.queries_scored == 0 {
        eprintln!("[chunk_bench] ERROR: no queries were scored; the numbers below are vacuous.");
    }
    if report.rerank_skipped > 0 {
        eprintln!(
            "[chunk_bench] WARNING: the reranker degraded on {}/{} scored queries ({:?}). On \
             those queries the deltas measure an absent reranker, not a bad one.",
            report.rerank_skipped, report.queries_scored, report.skip_reasons
        );
    }
    if report.rerank_not_run > 0 {
        eprintln!(
            "[chunk_bench] WARNING: {}/{} scored queries had no candidates, so the reranker \
             never ran on them.",
            report.rerank_not_run, report.queries_scored
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

    let d = ScoreDeltas::between(&current.reranked, &prior.reranked);
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
        current.reranked.recall_at_1,
        prior.reranked.recall_at_1,
        d.recall_at_5,
        current.reranked.recall_at_5,
        prior.reranked.recall_at_5,
        d.recall_at_10,
        current.reranked.recall_at_10,
        prior.reranked.recall_at_10,
        d.mean_iou,
        current.reranked.mean_iou,
        prior.reranked.mean_iou,
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

    fn resp(pre: usize, rerank_ms: Option<u64>, rerank: Option<Option<&str>>) -> RerankQueryResp {
        let rows = (0..pre)
            .map(|i| QueryResultRow {
                file: "/r/a.rs".to_owned(),
                line_start: i as u32 + 1,
                line_end: i as u32 + 1,
            })
            .collect();
        RerankQueryResp {
            results: vec![],
            pre_rerank_results: rows,
            timing: Timing { rerank_ms },
            rerank: rerank.map(|r| RerankInfoRow {
                skip_reason: r.map(str::to_owned),
            }),
        }
    }

    #[test]
    fn a_rerank_over_candidates_is_a_latency_sample() {
        let r = resp(10, Some(900), Some(None));
        assert_eq!(rerank_outcome(&r), RerankOutcome::Ran(Some(900)));
    }

    #[test]
    fn a_skip_reason_is_a_skip() {
        let r = resp(10, Some(18_000), Some(Some("quota")));
        assert_eq!(rerank_outcome(&r), RerankOutcome::Skipped("quota"));
    }

    #[test]
    fn no_candidates_means_the_reranker_never_ran() {
        // The engine returns early with `rerank: None` and 0 ms when vector
        // search finds nothing; counted as a sample, that 0 ms drags p50 down.
        assert_eq!(rerank_outcome(&resp(0, Some(0), None)), RerankOutcome::NotRun);
        // The reranker itself also returns early, without a skip reason, when
        // handed an empty candidate list.
        assert_eq!(rerank_outcome(&resp(0, Some(0), Some(None))), RerankOutcome::NotRun);
    }

    #[test]
    fn agentic_rag_on_refuses_the_run() {
        let d = RerankerDescriptor {
            agentic_rag: Some(true),
            ..Default::default()
        };
        assert!(config_refusal(&d).is_some());
    }

    #[test]
    fn agentic_rag_off_or_unknown_is_allowed() {
        let off = RerankerDescriptor {
            agentic_rag: Some(false),
            ..Default::default()
        };
        assert!(config_refusal(&off).is_none());
        // An unreadable /api/config must not block a run against an older server.
        assert!(config_refusal(&RerankerDescriptor::default()).is_none());
    }

    #[test]
    fn skip_bucket_keeps_short_reasons_verbatim() {
        assert_eq!(
            skip_bucket("TypeSafe key missing"),
            "TypeSafe key missing"
        );
    }

    #[test]
    fn skip_bucket_groups_errors_that_differ_only_in_their_tail() {
        // Upstream error bodies embed countdowns and request ids, so two
        // failures of the same kind must land in the same bucket.
        let prefix = "LLM request failed: OpenAI API returned HTTP 503 Service Unavailable: \
                      {\"error\":{\"message\":\"[antigravity/gemini-3-flash] [429]";
        let a = format!("{prefix}\n  Resets in 119h56m45s");
        let b = format!("{prefix}\n  Resets in 119h12m03s");
        assert_eq!(skip_bucket(&a), skip_bucket(&b));
        assert!(skip_bucket(&a).chars().count() <= SKIP_BUCKET_CHARS + 1);
        assert!(!skip_bucket(&a).contains('\n'));
    }

    #[test]
    fn empty_samples_do_not_panic() {
        let s = LatencySummary::from_samples(vec![]);
        assert_eq!(s.samples, 0);
        assert_eq!(s.p50_ms, 0);
    }
}

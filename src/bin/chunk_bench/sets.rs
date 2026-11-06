//! The multi-set, multi-arm retrieval run behind `scripts/retrieval_bench.sh`.
//!
//! One run scores several case sets (doc, ident, mixed by template, para)
//! against one warm index, once per arm. An arm is a name plus JSON merged into
//! the `/api/query` body, so a retrieval change that ships behind a request flag
//! can be A/B-ed on the same server, case by case, with nothing else varying.
//!
//! A query that fails or comes back "warming" is counted on its own and never
//! scored as a miss, because a dead server and a bad ranking must not look alike.

use std::time::Instant;

use serde_json::{Map, Value, json};

use crate::eval::{
    CaseSet, EvalCase, IDENT_LIMIT, ParaProvenance, derive_doc_set, derive_ident_set,
    derive_mixed_set, derive_para_set, para_provenance,
};
use crate::scoring::{
    Paired, RankingDetail, RankingScore, RecallTally, lists_differing, paired_at,
};
use crate::{EVAL_LIMIT, QueryOutcome, query_once, warm_up};

/// A named variation of the `/api/query` body.
#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct Arm {
    pub name: String,
    pub body: Map<String, Value>,
}

impl Arm {
    /// Parse `NAME=JSON`. The JSON must be an object that leaves `query` and
    /// `repo` alone, since those are the case itself.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let (name, json) = spec
            .split_once('=')
            .ok_or_else(|| format!("--arm expects NAME=JSON, got {spec:?}"))?;
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            return Err(format!("--arm name {name:?} must be letters, digits, _ - ."));
        }
        let Value::Object(body) = serde_json::from_str::<Value>(json)
            .map_err(|e| format!("--arm {name}: invalid JSON: {e}"))?
        else {
            return Err(format!("--arm {name}: JSON must be an object"));
        };
        if let Some(key) = ["query", "repo"].into_iter().find(|k| body.contains_key(*k)) {
            return Err(format!("--arm {name}: {key:?} belongs to the case, not the arm"));
        }
        Ok(Self {
            name: name.to_owned(),
            body,
        })
    }
}

/// Which case sets a run scores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetGroup {
    Doc,
    Ident,
    Mixed,
    Para,
}

impl SetGroup {
    pub const ALL: [SetGroup; 4] = [Self::Doc, Self::Ident, Self::Mixed, Self::Para];

    pub fn parse_list(list: &str) -> Result<Vec<Self>, String> {
        let mut out = Vec::new();
        for name in list.split(',').filter(|n| !n.is_empty()) {
            let group = match name {
                "doc" => Self::Doc,
                "ident" => Self::Ident,
                "mixed" => Self::Mixed,
                "para" => Self::Para,
                other => return Err(format!("--sets: unknown set {other:?} (doc, ident, mixed, para)")),
            };
            if !out.contains(&group) {
                out.push(group);
            }
        }
        if out.is_empty() {
            return Err("--sets needs at least one of doc, ident, mixed, para".to_owned());
        }
        Ok(out)
    }
}

/// Derive the requested sets, in the fixed order doc, ident, mixed, para.
fn build_sets(repo: &str, wanted: &[SetGroup]) -> Vec<CaseSet> {
    let wants = |g| wanted.contains(&g);
    let mut sets = Vec::new();
    if wants(SetGroup::Doc) {
        sets.push(derive_doc_set(repo, EVAL_LIMIT));
    }
    if wants(SetGroup::Ident) || wants(SetGroup::Mixed) {
        let ident = derive_ident_set(repo, IDENT_LIMIT);
        if wants(SetGroup::Mixed) {
            let mixed = derive_mixed_set(&ident);
            if wants(SetGroup::Ident) {
                sets.push(ident);
            }
            sets.extend(mixed);
        } else {
            sets.push(ident);
        }
    }
    if wants(SetGroup::Para) {
        sets.push(derive_para_set(repo));
    }
    sets
}

/// One arm's results over one set.
pub(crate) struct ArmRun {
    pub(crate) tally: RecallTally,
    pub(crate) failed: u64,
    pub(crate) warming: u64,
}

impl ArmRun {
    pub(crate) fn new() -> Self {
        Self {
            tally: RecallTally::default(),
            failed: 0,
            warming: 0,
        }
    }

    /// Fold one query outcome in. Only a ranking is scored. A failure or a
    /// "warming" answer is counted and kept out of the denominator.
    pub(crate) fn record(&mut self, case: &EvalCase, arm: &str, outcome: QueryOutcome) {
        match outcome {
            QueryOutcome::Ranked(rows) => self.tally.observe(
                &case.file.to_lowercase(),
                (case.line_start, case.line_end),
                &rows,
            ),
            QueryOutcome::Warming => {
                self.warming += 1;
                self.tally.observe_unscored();
            }
            QueryOutcome::Failed(why) => {
                eprintln!("[chunk_bench] arm {arm}: query {:?} failed: {why}", case.query);
                self.failed += 1;
                self.tally.observe_unscored();
            }
        }
    }
}

#[derive(serde::Serialize, Debug)]
struct StaleCase {
    file: String,
    symbol: String,
    line_start: u32,
}

#[derive(serde::Serialize, Debug)]
struct ArmReport {
    arm: String,
    score: RankingScore,
    #[serde(flatten)]
    detail: RankingDetail,
    queries_failed: u64,
    queries_warming: u64,
}

/// The second arm against the first, per cutoff.
#[derive(serde::Serialize, Debug)]
struct PairedReport {
    baseline: String,
    candidate: String,
    at_1: Paired,
    at_5: Paired,
    /// Cases whose result list differs between the two arms. Two arms with the
    /// same request still differ on near-ties, because the query embedding comes
    /// from a live API that is not bit-stable, so read this against that floor.
    lists_differing: usize,
}

#[derive(serde::Serialize, Debug)]
struct SetReport {
    set: String,
    cases: usize,
    stale: Vec<StaleCase>,
    arms: Vec<ArmReport>,
    paired: Option<PairedReport>,
}

#[derive(serde::Serialize, Debug)]
pub struct SetsReport {
    label: String,
    repo: String,
    server: String,
    top_k: u64,
    arms: Vec<Arm>,
    sets: Vec<SetReport>,
    pub queries_failed: u64,
    pub queries_warming: u64,
    wall_secs: f64,
    para_provenance: Option<ParaProvenance>,
}

fn request_body(case: &EvalCase, repo: &str, top_k: u64, arm: &Arm) -> Value {
    let mut body = json!({
        "query": case.query,
        "repo": repo,
        "top_k": top_k,
        "rerank": false,
    });
    let object = body.as_object_mut().expect("body is an object");
    for (key, value) in &arm.body {
        object.insert(key.clone(), value.clone());
    }
    body
}

fn score_set(
    client: &reqwest::Client,
    rt: &tokio::runtime::Runtime,
    server: &str,
    repo: &str,
    top_k: u64,
    arms: &[Arm],
    set: &CaseSet,
) -> SetReport {
    let mut runs: Vec<ArmRun> = arms.iter().map(|_| ArmRun::new()).collect();
    rt.block_on(async {
        for case in &set.cases {
            for (arm, run) in arms.iter().zip(runs.iter_mut()) {
                let body = request_body(case, repo, top_k, arm);
                run.record(case, &arm.name, query_once(client, server, &body).await);
            }
        }
    });

    let details: Vec<RankingDetail> = runs.iter().map(|r| r.tally.detail()).collect();
    let paired = match details.as_slice() {
        [first, second, ..] => Some(PairedReport {
            baseline: arms[0].name.clone(),
            candidate: arms[1].name.clone(),
            at_1: paired_at(&first.ranks, &second.ranks, 1),
            at_5: paired_at(&first.ranks, &second.ranks, 5),
            lists_differing: lists_differing(&first.case_hashes, &second.case_hashes),
        }),
        _ => None,
    };
    let arm_reports = arms
        .iter()
        .zip(runs)
        .zip(details)
        .map(|((arm, run), detail)| ArmReport {
            arm: arm.name.clone(),
            score: run.tally.finish(&arm.name),
            detail,
            queries_failed: run.failed,
            queries_warming: run.warming,
        })
        .collect();
    SetReport {
        set: set.name.clone(),
        cases: set.cases.len(),
        stale: set
            .stale
            .iter()
            .map(|c| StaleCase {
                file: c.file.clone(),
                symbol: c.symbol.clone(),
                line_start: c.line_start,
            })
            .collect(),
        arms: arm_reports,
        paired,
    }
}

/// Run every requested set against `server` once per arm and return the report.
pub fn run(
    repo: &str,
    server: &str,
    label: &str,
    wanted: &[SetGroup],
    arms: Vec<Arm>,
    top_k: u64,
) -> SetsReport {
    let sets = build_sets(repo, wanted);
    let started = Instant::now();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let client = reqwest::Client::new();
    rt.block_on(warm_up(&client, server, repo));

    let reports: Vec<SetReport> = sets
        .iter()
        .map(|set| {
            eprintln!(
                "[chunk_bench] scoring {} ({} cases, {} arms) ...",
                set.name,
                set.cases.len(),
                arms.len()
            );
            score_set(&client, &rt, server, repo, top_k, &arms, set)
        })
        .collect();

    let total = |pick: fn(&ArmReport) -> u64| {
        reports
            .iter()
            .flat_map(|s| &s.arms)
            .map(pick)
            .sum::<u64>()
    };
    SetsReport {
        label: label.to_owned(),
        repo: repo.to_owned(),
        server: server.to_owned(),
        top_k,
        queries_failed: total(|a| a.queries_failed),
        queries_warming: total(|a| a.queries_warming),
        sets: reports,
        arms,
        wall_secs: started.elapsed().as_secs_f64(),
        para_provenance: wanted.contains(&SetGroup::Para).then(para_provenance),
    }
}

impl SetsReport {
    /// Cases scored across all sets, one count per set not per arm.
    pub fn case_count(&self) -> usize {
        self.sets.iter().map(|s| s.cases).sum()
    }

    /// One table per set, then the failure line a run must end on.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "chunk_bench sets: {} on {} ({} arms, top_k {}, {:.0}s)\n",
            self.label,
            self.repo,
            self.arms.len(),
            self.top_k,
            self.wall_secs
        ));
        for set in &self.sets {
            out.push_str(&format!("\n== {} ({} cases", set.set, set.cases));
            if !set.stale.is_empty() {
                out.push_str(&format!(", {} stale and unscored", set.stale.len()));
            }
            out.push_str(") ==\n");
            if set.cases == 0 {
                out.push_str("(nothing to score)\n");
                continue;
            }
            out.push_str(&format!(
                "{:<14} {:>6} {:>6} {:>6} {:>6} {:>8} {:>6} {:>7}\n",
                "arm", "R@1", "R@5", "R@10", "MRR", "lines@5", "failed", "warming"
            ));
            for a in &set.arms {
                out.push_str(&format!(
                    "{:<14} {:>6.3} {:>6.3} {:>6.3} {:>6.3} {:>8.1} {:>6} {:>7}\n",
                    a.arm,
                    a.score.recall_at_1,
                    a.score.recall_at_5,
                    a.score.recall_at_10,
                    a.detail.mrr,
                    a.detail.lines_at_5,
                    a.queries_failed,
                    a.queries_warming
                ));
            }
            if let Some(p) = &set.paired {
                let line = |cut: &str, c: &Paired| {
                    format!("R@{cut} +{}/-{}/={}", c.wins, c.losses, c.ties)
                };
                out.push_str(&format!(
                    "paired {} -> {}: {}  {}\n",
                    p.baseline,
                    p.candidate,
                    line("1", &p.at_1),
                    line("5", &p.at_5)
                ));
                out.push_str(&format!(
                    "result lists: {} of {} cases differ between {} and {}; hashes {}\n",
                    p.lists_differing,
                    set.cases,
                    p.baseline,
                    p.candidate,
                    set.arms
                        .iter()
                        .map(|a| format!("{}={}", a.arm, a.detail.result_hash))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
        }
        out.push_str(&format!(
            "\nqueries_failed={} queries_warming={}\n",
            self.queries_failed, self.queries_warming
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QueryResultRow;

    fn case() -> EvalCase {
        EvalCase {
            query: "q".into(),
            file: "/r/a.rs".into(),
            symbol: "s".into(),
            line_start: 10,
            line_end: 20,
        }
    }

    fn hit_row() -> QueryResultRow {
        QueryResultRow {
            file: "/r/a.rs".into(),
            line_start: 12,
            line_end: 18,
            relevance: None,
        }
    }

    #[test]
    fn a_failed_query_is_counted_and_never_scored_as_a_miss() {
        let mut run = ArmRun::new();
        run.record(&case(), "a", QueryOutcome::Ranked(vec![hit_row()]));
        run.record(&case(), "a", QueryOutcome::Failed("HTTP 502".into()));
        assert_eq!(run.failed, 1);
        assert_eq!(run.tally.observed(), 1);
        assert_eq!(run.tally.finish("a").recall_at_1, 1.0);
    }

    #[test]
    fn a_warming_answer_is_counted_apart_from_failures() {
        let mut run = ArmRun::new();
        run.record(&case(), "a", QueryOutcome::Warming);
        assert_eq!((run.failed, run.warming), (0, 1));
        assert_eq!(run.tally.observed(), 0);
    }

    #[test]
    fn arm_parses_name_and_object() {
        let arm = Arm::parse(r#"on={"symbol_lookup":true}"#).unwrap();
        assert_eq!(arm.name, "on");
        assert_eq!(arm.body["symbol_lookup"], json!(true));
        assert!(Arm::parse("base={}").unwrap().body.is_empty());
    }

    #[test]
    fn arm_rejects_bad_specs() {
        assert!(Arm::parse("no-equals").is_err());
        assert!(Arm::parse("=({})").is_err());
        assert!(Arm::parse("bad name={}").is_err());
        assert!(Arm::parse("a=[1]").is_err(), "an arm is an object");
        assert!(Arm::parse("a={oops").is_err());
        assert!(Arm::parse(r#"a={"query":"x"}"#).is_err());
        assert!(Arm::parse(r#"a={"repo":"/x"}"#).is_err());
    }

    #[test]
    fn arm_body_overrides_the_default_request() {
        let arm = Arm::parse(r#"a={"top_k":30,"symbol_lookup":false}"#).unwrap();
        let body = request_body(&case(), "/r", 10, &arm);
        assert_eq!(body["top_k"], 30);
        assert_eq!(body["symbol_lookup"], false);
        assert_eq!(body["rerank"], false);
        assert_eq!(body["query"], "q");
    }

    #[test]
    fn set_list_parses_dedupes_and_rejects_unknowns() {
        assert_eq!(
            SetGroup::parse_list("ident,doc,ident").unwrap(),
            [SetGroup::Ident, SetGroup::Doc]
        );
        assert!(SetGroup::parse_list("doc,nope").is_err());
        assert!(SetGroup::parse_list("").is_err());
    }
}

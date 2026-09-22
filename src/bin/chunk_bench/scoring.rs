//! The one definition of retrieval correctness in this benchmark.
//!
//! Both consumers score through `RecallTally`: the chunk benchmark's single
//! ranking (`run_retrieval`) and the rerank A/B's pre- and post-rerank rankings
//! (`rerank_ab`). That matters more than the usual don't-repeat-yourself reason
//! — two copies of the hit rule would let one artifact's recall drift from
//! another's after a one-character edit, and the drift would look like a result.

use crate::QueryResultRow;

/// Intersection-over-union of two inclusive line ranges.
pub fn iou(a: (u32, u32), b: (u32, u32)) -> f64 {
    let inter_start = a.0.max(b.0);
    let inter_end = a.1.min(b.1);
    if inter_start > inter_end {
        return 0.0;
    }
    let inter = (inter_end - inter_start + 1) as f64;
    let union = ((a.1 - a.0 + 1) + (b.1 - b.0 + 1)) as f64 - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// Recall and IoU for one ranking, scored against the eval ground truth.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone)]
pub struct RankingScore {
    pub label: String,
    pub recall_at_1: f64,
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub mean_iou: f64,
}

/// `candidate − baseline` on every metric a `RankingScore` carries. Positive
/// means the candidate ranking put the expected symbol higher.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone)]
pub struct ScoreDeltas {
    pub recall_at_1: f64,
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub mean_iou: f64,
}

impl ScoreDeltas {
    pub fn between(candidate: &RankingScore, baseline: &RankingScore) -> Self {
        Self {
            recall_at_1: candidate.recall_at_1 - baseline.recall_at_1,
            recall_at_5: candidate.recall_at_5 - baseline.recall_at_5,
            recall_at_10: candidate.recall_at_10 - baseline.recall_at_10,
            mean_iou: candidate.mean_iou - baseline.mean_iou,
        }
    }
}

/// Running recall/IoU tally across eval cases.
#[derive(Default)]
pub struct RecallTally {
    r1: u64,
    r5: u64,
    r10: u64,
    iou_sum: f64,
    n: u64,
}

impl RecallTally {
    /// Score one ranking for one eval case.
    ///
    /// Hit rule: a result counts only when its file path matches the expected
    /// file (compared lowercased with forward slashes, so a Windows-indexed path
    /// still matches) and its line range overlaps the expected range by at least
    /// one line. The hit rank is the first such result. IoU takes the best
    /// overlap among all same-file results, which is why a poor result ranking
    /// above a good one costs rank but not IoU.
    ///
    /// Every observed case lands in the denominator whether or not it hit, so a
    /// case dropped before this point (unresolved symbol, failed request)
    /// silently shrinks the sample rather than counting as a miss.
    pub fn observe(&mut self, exp_file_norm: &str, exp: (u32, u32), ranking: &[QueryResultRow]) {
        let (exp_start, exp_end) = exp;
        let mut best_iou = 0.0f64;
        let mut hit_rank: Option<usize> = None;
        for (rank, r) in ranking.iter().enumerate() {
            if r.file.replace('\\', "/").to_lowercase() != exp_file_norm {
                continue;
            }
            let i = iou((exp_start, exp_end), (r.line_start, r.line_end));
            if i > best_iou {
                best_iou = i;
            }
            let overlaps = r.line_start <= exp_end && r.line_end >= exp_start;
            if overlaps && hit_rank.is_none() {
                hit_rank = Some(rank);
            }
        }
        if let Some(rank) = hit_rank {
            if rank < 1 {
                self.r1 += 1;
            }
            if rank < 5 {
                self.r5 += 1;
            }
            if rank < 10 {
                self.r10 += 1;
            }
        }
        self.iou_sum += best_iou;
        self.n += 1;
    }

    /// Number of cases observed — the denominator behind every rate below.
    pub fn observed(&self) -> u64 {
        self.n
    }

    pub fn finish(&self, label: &str) -> RankingScore {
        let denom = self.n.max(1) as f64;
        RankingScore {
            label: label.to_owned(),
            recall_at_1: self.r1 as f64 / denom,
            recall_at_5: self.r5 as f64 / denom,
            recall_at_10: self.r10 as f64 / denom,
            mean_iou: self.iou_sum / denom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(file: &str, line_start: u32, line_end: u32) -> QueryResultRow {
        QueryResultRow {
            file: file.to_owned(),
            line_start,
            line_end,
        }
    }

    #[test]
    fn rank_zero_hit_counts_at_every_k() {
        let mut t = RecallTally::default();
        t.observe("/r/a.rs", (10, 20), &[row("/r/a.rs", 12, 18)]);
        let s = t.finish("t");
        assert_eq!((s.recall_at_1, s.recall_at_5, s.recall_at_10), (1.0, 1.0, 1.0));
    }

    #[test]
    fn rank_boundaries_are_exclusive_upper() {
        // Hit at 0-based rank 5 -> counts for @10 only (`rank < 5` is false).
        let mut t = RecallTally::default();
        let mut ranking: Vec<QueryResultRow> =
            (0..5).map(|i| row("/r/a.rs", 100 + i, 100 + i)).collect();
        ranking.push(row("/r/a.rs", 12, 18));
        t.observe("/r/a.rs", (10, 20), &ranking);
        let s = t.finish("t");
        assert_eq!((s.recall_at_1, s.recall_at_5, s.recall_at_10), (0.0, 0.0, 1.0));
    }

    #[test]
    fn a_different_file_is_never_a_hit() {
        let mut t = RecallTally::default();
        t.observe("/r/a.rs", (10, 20), &[row("/r/b.rs", 10, 20)]);
        let s = t.finish("t");
        assert_eq!(s.recall_at_10, 0.0);
        assert_eq!(s.mean_iou, 0.0);
    }

    #[test]
    fn same_file_without_overlap_scores_no_hit() {
        let mut t = RecallTally::default();
        t.observe("/r/a.rs", (10, 20), &[row("/r/a.rs", 30, 40)]);
        assert_eq!(t.finish("t").recall_at_10, 0.0);
    }

    #[test]
    fn a_single_shared_line_is_an_overlap() {
        let mut t = RecallTally::default();
        t.observe("/r/a.rs", (10, 20), &[row("/r/a.rs", 20, 40)]);
        assert_eq!(t.finish("t").recall_at_1, 1.0);
    }

    #[test]
    fn file_matching_normalizes_separators_and_case() {
        let mut t = RecallTally::default();
        t.observe("/r/a.rs", (10, 20), &[row("\\R\\A.rs", 12, 18)]);
        assert_eq!(t.finish("t").recall_at_1, 1.0);
    }

    #[test]
    fn best_iou_wins_across_same_file_results() {
        let mut t = RecallTally::default();
        t.observe(
            "/r/a.rs",
            (10, 20),
            &[row("/r/a.rs", 10, 40), row("/r/a.rs", 10, 20)],
        );
        assert!((t.finish("t").mean_iou - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn denominator_counts_observed_cases_not_hits() {
        let mut t = RecallTally::default();
        t.observe("/r/a.rs", (10, 20), &[row("/r/a.rs", 12, 18)]); // hit
        t.observe("/r/a.rs", (10, 20), &[row("/r/b.rs", 12, 18)]); // miss
        assert_eq!(t.finish("t").recall_at_1, 0.5);
        assert_eq!(t.observed(), 2);
    }

    #[test]
    fn iou_of_disjoint_ranges_is_zero() {
        assert_eq!(iou((1, 5), (6, 10)), 0.0);
    }

    #[test]
    fn iou_of_identical_ranges_is_one() {
        assert!((iou((10, 20), (10, 20)) - 1.0).abs() < f64::EPSILON);
    }
}

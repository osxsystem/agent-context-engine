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

/// Whether `r` is the expected answer: same file (compared lowercased with
/// forward slashes, so a Windows-indexed path still matches) and at least one
/// line of overlap with the expected range.
pub fn is_hit(exp_file_norm: &str, exp: (u32, u32), r: &QueryResultRow) -> bool {
    is_expected_file(exp_file_norm, r) && overlaps(exp, r)
}

fn is_expected_file(exp_file_norm: &str, r: &QueryResultRow) -> bool {
    r.file.replace('\\', "/").to_lowercase() == exp_file_norm
}

fn overlaps(exp: (u32, u32), r: &QueryResultRow) -> bool {
    r.line_start <= exp.1 && r.line_end >= exp.0
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

const RANK_CUTOFF: usize = 10;

#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CaseRank {
    Hit(u32),
    Miss,
    Unscored,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct RankingDetail {
    pub mrr: f64,
    pub lines_at_5: f64,
    pub ranks: Vec<CaseRank>,
    pub case_hashes: Vec<Option<String>>,
    pub result_hash: String,
}

pub fn lists_differing(a: &[Option<String>], b: &[Option<String>]) -> usize {
    a.iter()
        .zip(b)
        .filter(|(x, y)| matches!((x, y), (Some(x), Some(y)) if x != y))
        .count()
}

#[derive(serde::Serialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Paired {
    pub wins: u32,
    pub losses: u32,
    pub ties: u32,
}

pub fn paired_at(a: &[CaseRank], b: &[CaseRank], k: u32) -> Paired {
    let hit = |r: &CaseRank| matches!(r, CaseRank::Hit(rank) if *rank <= k);
    let mut out = Paired::default();
    for (ra, rb) in a.iter().zip(b) {
        if matches!(ra, CaseRank::Unscored) || matches!(rb, CaseRank::Unscored) {
            continue;
        }
        match (hit(ra), hit(rb)) {
            (false, true) => out.wins += 1,
            (true, false) => out.losses += 1,
            _ => out.ties += 1,
        }
    }
    out
}

/// FNV-1a. The hash only has to be stable across runs and builds, which
/// `DefaultHasher` does not promise.
struct Fnv(u64);

impl Fnv {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

impl Default for Fnv {
    fn default() -> Self {
        Self(Self::OFFSET)
    }
}

/// Running recall/IoU tally across eval cases.
#[derive(Default)]
pub struct RecallTally {
    r1: u64,
    r5: u64,
    r10: u64,
    iou_sum: f64,
    rr_sum: f64,
    lines5_sum: u64,
    n: u64,
    ranks: Vec<CaseRank>,
    case_hashes: Vec<Option<u64>>,
}

impl RecallTally {
    /// Score one ranking for one eval case.
    ///
    /// Hit rule: see [`is_hit`]. The hit rank is the first such result. IoU
    /// takes the best overlap among all same-file results, which is why a poor
    /// result ranking above a good one costs rank but not IoU.
    ///
    /// Every observed case lands in the denominator whether or not it hit, so a
    /// case dropped before this point (unresolved symbol, failed request)
    /// silently shrinks the sample rather than counting as a miss.
    pub fn observe(&mut self, exp_file_norm: &str, exp: (u32, u32), ranking: &[QueryResultRow]) {
        let (exp_start, exp_end) = exp;
        let mut best_iou = 0.0f64;
        let mut hit_rank: Option<usize> = None;
        for (rank, r) in ranking.iter().enumerate() {
            if !is_expected_file(exp_file_norm, r) {
                continue;
            }
            let i = iou((exp_start, exp_end), (r.line_start, r.line_end));
            if i > best_iou {
                best_iou = i;
            }
            if hit_rank.is_none() && overlaps(exp, r) {
                hit_rank = Some(rank);
            }
        }
        self.lines5_sum += ranking
            .iter()
            .take(5)
            .map(|r| u64::from(r.line_end.saturating_sub(r.line_start)) + 1)
            .sum::<u64>();
        let mut list_hash = Fnv::default();
        for r in ranking {
            list_hash.write(r.file.as_bytes());
            list_hash.write(format!(":{}-{};", r.line_start, r.line_end).as_bytes());
        }
        self.case_hashes.push(Some(list_hash.0));
        let scored_rank = hit_rank.filter(|rank| *rank < RANK_CUTOFF);
        self.ranks.push(match scored_rank {
            Some(rank) => {
                self.rr_sum += 1.0 / (rank + 1) as f64;
                CaseRank::Hit(rank as u32 + 1)
            }
            None => CaseRank::Miss,
        });
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

    pub fn observe_unscored(&mut self) {
        self.ranks.push(CaseRank::Unscored);
        self.case_hashes.push(None);
    }

    /// Number of cases observed — the denominator behind every rate below.
    pub fn observed(&self) -> u64 {
        self.n
    }

    pub fn detail(&self) -> RankingDetail {
        let denom = self.n.max(1) as f64;
        let mut all = Fnv::default();
        for h in &self.case_hashes {
            all.write(&h.unwrap_or(0).to_le_bytes());
        }
        RankingDetail {
            mrr: self.rr_sum / denom,
            lines_at_5: self.lines5_sum as f64 / denom,
            ranks: self.ranks.clone(),
            result_hash: format!("{:016x}", all.0),
            case_hashes: self
                .case_hashes
                .iter()
                .map(|h| h.map(|h| format!("{h:016x}")))
                .collect(),
        }
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
            relevance: None,
        }
    }

    #[test]
    fn rank_zero_hit_counts_at_every_k() {
        let mut t = RecallTally::default();
        t.observe("/r/a.rs", (10, 20), &[row("/r/a.rs", 12, 18)]);
        let s = t.finish("t");
        assert_eq!(
            (s.recall_at_1, s.recall_at_5, s.recall_at_10),
            (1.0, 1.0, 1.0)
        );
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
        assert_eq!(
            (s.recall_at_1, s.recall_at_5, s.recall_at_10),
            (0.0, 0.0, 1.0)
        );
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

    fn tally_of(rankings: &[Vec<QueryResultRow>]) -> RecallTally {
        let mut t = RecallTally::default();
        for ranking in rankings {
            t.observe("/r/a.rs", (10, 20), ranking);
        }
        t
    }

    fn miss_row(i: u32) -> QueryResultRow {
        row("/r/other.rs", i, i)
    }

    #[test]
    fn mrr_is_the_mean_reciprocal_of_the_first_hit_rank() {
        let t = tally_of(&[
            vec![row("/r/a.rs", 12, 18)],
            vec![miss_row(1), row("/r/a.rs", 12, 18)],
            vec![miss_row(1), miss_row(2), miss_row(3)],
        ]);
        assert!((t.detail().mrr - (1.0 + 0.5 + 0.0) / 3.0).abs() < 1e-12);
    }

    #[test]
    fn mrr_ignores_hits_past_the_tenth_result() {
        let mut ranking: Vec<QueryResultRow> = (1..=10).map(miss_row).collect();
        ranking.push(row("/r/a.rs", 12, 18));
        let detail = tally_of(&[ranking]).detail();
        assert_eq!(detail.mrr, 0.0);
        assert_eq!(detail.ranks, [CaseRank::Miss]);
    }

    #[test]
    fn lines_at_5_sums_the_first_five_results_and_averages_over_cases() {
        let five_by_ten: Vec<QueryResultRow> = (0..7)
            .map(|i| row("/r/b.rs", i * 100 + 1, i * 100 + 10))
            .collect();
        let t = tally_of(&[five_by_ten, vec![row("/r/b.rs", 1, 4)]]);
        assert!((t.detail().lines_at_5 - (50.0 + 4.0) / 2.0).abs() < 1e-12);
    }

    #[test]
    fn per_case_ranks_are_one_based_and_in_case_order() {
        let detail = tally_of(&[
            vec![miss_row(1), miss_row(2), row("/r/a.rs", 12, 18)],
            vec![miss_row(1)],
        ])
        .detail();
        assert_eq!(detail.ranks, [CaseRank::Hit(3), CaseRank::Miss]);
    }

    #[test]
    fn an_unscored_case_stays_out_of_the_denominator() {
        let mut t = tally_of(&[vec![row("/r/a.rs", 12, 18)]]);
        t.observe_unscored();
        assert_eq!(t.observed(), 1);
        assert_eq!(t.finish("t").recall_at_1, 1.0);
        assert_eq!(t.detail().ranks, [CaseRank::Hit(1), CaseRank::Unscored]);
    }

    #[test]
    fn paired_counts_wins_losses_and_ties_at_a_cutoff() {
        use CaseRank::{Hit, Miss};
        let a = [Miss, Hit(1), Hit(3), Miss, Hit(2)];
        let b = [Hit(1), Miss, Hit(2), Miss, Hit(4)];
        assert_eq!(
            paired_at(&a, &b, 5),
            Paired {
                wins: 1,
                losses: 1,
                ties: 3
            }
        );
        assert_eq!(
            paired_at(&a, &b, 1),
            Paired {
                wins: 1,
                losses: 1,
                ties: 3
            }
        );
    }

    #[test]
    fn paired_tie_counts_neither_a_win_nor_a_loss() {
        use CaseRank::Hit;
        let same = [Hit(1), Hit(4)];
        assert_eq!(
            paired_at(&same, &same, 5),
            Paired {
                wins: 0,
                losses: 0,
                ties: 2
            }
        );
    }

    #[test]
    fn paired_cutoff_decides_what_counts_as_a_hit() {
        use CaseRank::Hit;
        assert_eq!(
            paired_at(&[Hit(1)], &[Hit(3)], 1),
            Paired {
                wins: 0,
                losses: 1,
                ties: 0
            }
        );
        assert_eq!(
            paired_at(&[Hit(1)], &[Hit(3)], 5),
            Paired {
                wins: 0,
                losses: 0,
                ties: 1
            }
        );
    }

    #[test]
    fn paired_skips_a_case_either_ranking_left_unscored() {
        use CaseRank::{Hit, Miss, Unscored};
        assert_eq!(
            paired_at(&[Hit(1), Unscored], &[Unscored, Miss], 5),
            Paired::default()
        );
    }

    #[test]
    fn identical_rankings_hash_alike_and_different_ones_do_not() {
        let a = tally_of(&[vec![row("/r/a.rs", 12, 18), miss_row(1)]]).detail();
        let same = tally_of(&[vec![row("/r/a.rs", 12, 18), miss_row(1)]]).detail();
        let reordered = tally_of(&[vec![miss_row(1), row("/r/a.rs", 12, 18)]]).detail();
        assert_eq!(a.result_hash, same.result_hash);
        assert_ne!(a.result_hash, reordered.result_hash);
    }

    #[test]
    fn lists_differing_counts_cases_whose_lists_disagree() {
        let a = tally_of(&[
            vec![row("/r/a.rs", 1, 9)],
            vec![row("/r/a.rs", 1, 9), miss_row(1)],
            vec![row("/r/a.rs", 1, 9)],
        ]);
        let mut b = tally_of(&[
            vec![row("/r/a.rs", 1, 9)],
            vec![miss_row(1), row("/r/a.rs", 1, 9)],
        ]);
        b.observe_unscored();
        assert_eq!(
            lists_differing(&a.detail().case_hashes, &b.detail().case_hashes),
            1,
            "one case differs, one matches, one is unscored"
        );
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

use std::collections::HashMap;

use crate::parsing::generated::is_generated_file;

/// A code chunk that participates in the merge pipeline.
#[derive(Debug, Clone)]
pub struct MergeChunk {
    pub file: String,
    pub line_start: u32,
    pub line_end: u32,
    pub score: f32,
    /// Pre-fetched content (may be empty string; will be re-read after merge).
    pub content: String,
    pub symbol: Option<String>,
    /// Full qualified name from the symbol table (e.g. "src/foo.rs::Mod::func").
    pub symbol_fqn: Option<String>,
    /// Symbol kind (e.g. "function", "class", "struct") — populated from the
    /// symbol table during fetch_chunk_content. Used for kind: filter application.
    pub symbol_kind: Option<String>,
    pub duplicates: Vec<String>,
}

impl MergeChunk {
    /// Whether the chunk is short enough to always show whole: rerankers never
    /// narrow it to line ranges.
    pub fn is_under_prune_floor(&self, min_prune_lines: u32) -> bool {
        self.line_end.saturating_sub(self.line_start) < min_prune_lines
    }
}

struct Ranked {
    chunk: MergeChunk,
    rank: f32,
}

impl Ranked {
    fn new(chunk: MergeChunk, weight: f32) -> Self {
        Self {
            rank: chunk.score * weight,
            chunk,
        }
    }
}

fn by_rank_desc(a: f32, b: f32) -> std::cmp::Ordering {
    b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
}

/// Dedup + merge adjacent chunks, then cap to `top_k`. Every chunk ranks by its
/// raw score.
///
/// Steps (as specified):
/// 1. Dedup by (file, line_start, line_end) — keep highest score.
/// 2. Collapse chunks whose content is identical across files — keep the best
///    one (score, then hand-written over generated, then smallest path) and
///    record the other files on it.
/// 3. Group by file.
/// 4. Sort within each group by line_start ASC.
/// 5. Merge pass: merge when next.line_start <= current.line_end + 2 (gap ≤ 1),
///    capping merged range at 60 lines. Chunks with different recorded copies
///    never merge.
/// 6. Drop contained: within each file group, sort by width DESC, skip any chunk
///    fully inside a wider one, unless it carries recorded copies.
/// 7. Collect all groups, sort by score DESC, cap to top_k.
pub fn merge_chunks(chunks: Vec<MergeChunk>, top_k: usize) -> Vec<MergeChunk> {
    merge_weighted(chunks.into_iter().map(|c| (c, 1.0)).collect(), top_k)
}

/// [`merge_chunks`] where each chunk carries a weight and every "score" in the
/// steps above reads `score * weight` instead. A merged chunk keeps the best
/// rank of its parts. The weight orders the pool and picks which slots survive
/// the cap; it never rewrites `MergeChunk::score`.
pub fn merge_weighted(chunks: Vec<(MergeChunk, f32)>, top_k: usize) -> Vec<MergeChunk> {
    if chunks.is_empty() {
        return vec![];
    }

    // Step 1: dedup — (file, line_start, line_end) → best rank entry.
    let mut dedup_map: HashMap<(String, u32, u32), Ranked> = HashMap::new();
    for (chunk, weight) in chunks {
        let key = (chunk.file.clone(), chunk.line_start, chunk.line_end);
        let incoming = Ranked::new(chunk, weight);
        dedup_map
            .entry(key)
            .and_modify(|existing| {
                if incoming.rank > existing.rank {
                    *existing = Ranked {
                        chunk: incoming.chunk.clone(),
                        rank: incoming.rank,
                    };
                }
            })
            .or_insert(incoming);
    }

    // Step 2: collapse identical content across files, before step 5 joins neighbours.
    let collapsed = {
        let mut deduped: Vec<Ranked> = dedup_map.into_values().collect();
        deduped.sort_unstable_by(|a, b| {
            a.chunk
                .file
                .cmp(&b.chunk.file)
                .then(a.chunk.line_start.cmp(&b.chunk.line_start))
                .then(a.chunk.line_end.cmp(&b.chunk.line_end))
        });
        collapse_identical_content(deduped)
    };

    // Step 3: group by file.
    let mut by_file: HashMap<String, Vec<Ranked>> = HashMap::new();
    for ranked in collapsed {
        by_file
            .entry(ranked.chunk.file.clone())
            .or_default()
            .push(ranked);
    }

    let mut result: Vec<Ranked> = Vec::new();

    for (_file, mut file_chunks) in by_file {
        // Step 4: sort by line_start ASC.
        file_chunks.sort_unstable_by_key(|r| r.chunk.line_start);

        // Step 5: merge pass.
        let mut merged: Vec<Ranked> = Vec::new();
        for next in file_chunks {
            if let Some(current) = merged.last_mut() {
                // Merge when next starts within one line of current and both carry the same copy list.
                if next.chunk.line_start <= current.chunk.line_end + 2
                    && next.chunk.duplicates == current.chunk.duplicates
                {
                    let new_end = current.chunk.line_end.max(next.chunk.line_end);
                    let new_start = current.chunk.line_start;
                    // Cap at 60 lines.
                    if new_end - new_start + 1 > 60 {
                        merged.push(next);
                    } else {
                        let Ranked { chunk: next, rank } = next;
                        // Compute overlap BEFORE updating line_end.
                        let old_end = current.chunk.line_end;
                        let overlap_lines = if next.line_start <= old_end {
                            (old_end - next.line_start + 1) as usize
                        } else {
                            0
                        };

                        let current_chunk = &mut current.chunk;
                        current_chunk.line_end = new_end;
                        current_chunk.score = current_chunk.score.max(next.score);
                        current.rank = current.rank.max(rank);

                        // Append only the non-overlapping tail of next's content as fallback.
                        if !next.content.is_empty() {
                            let tail: String = next
                                .content
                                .lines()
                                .skip(overlap_lines)
                                .collect::<Vec<_>>()
                                .join("\n");
                            if !tail.is_empty() {
                                current_chunk.content.push('\n');
                                current_chunk.content.push_str(&tail);
                            }
                        }
                        if current_chunk.symbol.is_none() && next.symbol.is_some() {
                            current_chunk.symbol = next.symbol;
                        }
                        if current_chunk.symbol_fqn.is_none() && next.symbol_fqn.is_some() {
                            current_chunk.symbol_fqn = next.symbol_fqn;
                        }
                        if current_chunk.symbol_kind.is_none() && next.symbol_kind.is_some() {
                            current_chunk.symbol_kind = next.symbol_kind;
                        }
                    }
                } else {
                    merged.push(next);
                }
            } else {
                merged.push(next);
            }
        }

        // Step 6: drop contained chunks.
        // Sort by width DESC (wider chunks survive).
        merged.sort_unstable_by(|a, b| {
            let wa = a.chunk.line_end.saturating_sub(a.chunk.line_start);
            let wb = b.chunk.line_end.saturating_sub(b.chunk.line_start);
            wb.cmp(&wa)
        });
        let mut survivors: Vec<Ranked> = Vec::new();
        for candidate in merged {
            let contained = candidate.chunk.duplicates.is_empty()
                && survivors.iter().any(|s| {
                    s.chunk.file == candidate.chunk.file
                        && s.chunk.line_start <= candidate.chunk.line_start
                        && s.chunk.line_end >= candidate.chunk.line_end
                });
            if !contained {
                survivors.push(candidate);
            }
        }

        result.extend(survivors);
    }

    // Step 7: sort by rank DESC and cap to top_k.
    result.sort_unstable_by(|a, b| {
        by_rank_desc(a.rank, b.rank)
            .then_with(|| a.chunk.file.cmp(&b.chunk.file))
            .then_with(|| a.chunk.line_start.cmp(&b.chunk.line_start))
    });
    result.truncate(top_k);
    result.into_iter().map(|r| r.chunk).collect()
}

fn content_key(content: &str) -> Option<String> {
    let normalized: String = content
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    if normalized.trim().is_empty() {
        return None;
    }
    Some(normalized)
}

fn collapse_identical_content(chunks: Vec<Ranked>) -> Vec<Ranked> {
    let mut by_content: HashMap<String, Vec<Ranked>> = HashMap::new();
    let mut result: Vec<Ranked> = Vec::new();
    for ranked in chunks {
        match content_key(&ranked.chunk.content) {
            Some(key) => by_content.entry(key).or_default().push(ranked),
            None => result.push(ranked),
        }
    }

    for (_, mut group) in by_content {
        group.sort_unstable_by(|a, b| {
            by_rank_desc(a.rank, b.rank)
                .then_with(|| {
                    is_generated_file(&a.chunk.file).cmp(&is_generated_file(&b.chunk.file))
                })
                .then_with(|| a.chunk.file.cmp(&b.chunk.file))
                .then_with(|| a.chunk.line_start.cmp(&b.chunk.line_start))
        });
        let mut group = group.into_iter();
        let Some(mut kept) = group.next() else {
            continue;
        };
        let mut others: Vec<String> = Vec::new();
        for ranked in group {
            // A repeat in the kept chunk's own file stays: same-file overlap belongs to the merge pass.
            if ranked.chunk.file == kept.chunk.file {
                result.push(ranked);
            } else {
                others.push(ranked.chunk.file);
            }
        }
        others.sort();
        others.dedup();
        kept.chunk.duplicates = others;
        result.push(kept);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = "export function SaveAsUI() {\n  return <Modal open />;\n}";

    fn chunk(file: &str, content: &str, score: f32) -> MergeChunk {
        MergeChunk {
            file: file.to_owned(),
            line_start: 269,
            line_end: 310,
            score,
            content: content.to_owned(),
            symbol: None,
            symbol_fqn: None,
            symbol_kind: None,
            duplicates: Vec::new(),
        }
    }

    fn chunk_at(file: &str, line_start: u32, content: &str) -> MergeChunk {
        MergeChunk {
            line_start,
            line_end: line_start + 2,
            ..chunk(file, content, 0.5)
        }
    }

    fn listed(chunks: &[MergeChunk]) -> Vec<(&str, f32)> {
        chunks.iter().map(|c| (c.file.as_str(), c.score)).collect()
    }

    #[test]
    fn weight_orders_the_cap_but_leaves_the_score_alone() {
        let merged = merge_weighted(
            vec![
                (chunk("a.rs", "fn a() {}", 0.9), 0.5),
                (chunk("b.rs", "fn b() {}", 0.6), 1.0),
                (chunk("c.rs", "fn c() {}", 0.5), 1.0),
            ],
            2,
        );

        assert_eq!(listed(&merged), vec![("b.rs", 0.6), ("c.rs", 0.5)]);
    }

    #[test]
    fn merged_neighbours_keep_the_best_rank_of_the_pair() {
        let merged = merge_weighted(
            vec![
                (spanning("a.rs", 1, 3, "header", 0.9), 0.5),
                (spanning("a.rs", 4, 6, "body", 0.6), 1.0),
                (chunk("b.rs", "other", 0.55), 1.0),
            ],
            2,
        );

        assert_eq!(listed(&merged), vec![("a.rs", 0.9), ("b.rs", 0.55)]);
        assert_eq!(merged[0].line_start, 1);
        assert_eq!(merged[0].line_end, 6);
    }

    #[test]
    fn identical_copies_pick_the_heavier_weight_on_a_score_tie() {
        let merged = merge_weighted(
            vec![
                (chunk("a/mock.js", BLOCK, 0.8), 0.9),
                (chunk("z/real.js", BLOCK, 0.8), 1.0),
            ],
            5,
        );

        assert_eq!(listed(&merged), vec![("z/real.js", 0.8)]);
        assert_eq!(merged[0].duplicates, vec!["a/mock.js".to_owned()]);
    }

    #[test]
    fn identical_content_across_files_collapses_to_one_representative() {
        let merged = merge_chunks(
            vec![
                chunk("b/forms/SaveAsUI.jsx", BLOCK, 0.9),
                chunk("a/forms/SaveAsUI.jsx", BLOCK, 0.9),
                chunk("c/forms/SaveAsUI.jsx", BLOCK, 0.8),
                chunk("src/Modal.jsx", "export function Modal() {}", 0.7),
                chunk("src/useSaveAs.js", "export function useSaveAs() {}", 0.6),
            ],
            3,
        );

        assert_eq!(
            listed(&merged),
            vec![
                ("a/forms/SaveAsUI.jsx", 0.9),
                ("src/Modal.jsx", 0.7),
                ("src/useSaveAs.js", 0.6),
            ],
            "the copy with the best score and, on a tie, the smallest path stands for all three"
        );
        assert_eq!(
            merged[0].duplicates,
            vec![
                "b/forms/SaveAsUI.jsx".to_owned(),
                "c/forms/SaveAsUI.jsx".to_owned()
            ]
        );
    }

    #[test]
    fn copies_differing_only_by_line_endings_and_trailing_space_collapse() {
        let merged = merge_chunks(
            vec![
                chunk("win/x.js", "fn a() {\r\n  b();  \r\n}\r\n", 0.9),
                chunk("unix/x.js", "fn a() {\n  b();\n}\n", 0.8),
            ],
            10,
        );

        assert_eq!(merged.len(), 1, "the two copies are the same code");
        assert_eq!(merged[0].file, "win/x.js");
        assert_eq!(merged[0].duplicates, vec!["unix/x.js".to_owned()]);
    }

    #[test]
    fn blank_content_never_collapses() {
        let merged = merge_chunks(vec![chunk("a.rs", "", 0.9), chunk("b.rs", "", 0.8)], 10);

        assert_eq!(merged.len(), 2, "two empty chunks are not the same code");
    }

    #[test]
    fn equal_scores_are_ordered_by_file_then_start_line() {
        let mut chunks = Vec::new();
        for (file, line) in [
            ("e.rs", 269u32),
            ("a.rs", 900),
            ("c.rs", 500),
            ("b.rs", 269),
            ("d.rs", 269),
            ("a.rs", 269),
        ] {
            chunks.push(chunk_at(file, line, &format!("fn {file}_{line}() {{}}")));
        }

        let merged = merge_chunks(chunks, 10);
        let order: Vec<(String, u32)> = merged
            .iter()
            .map(|c| (c.file.clone(), c.line_start))
            .collect();

        assert_eq!(
            order,
            vec![
                ("a.rs".to_owned(), 269),
                ("a.rs".to_owned(), 900),
                ("b.rs".to_owned(), 269),
                ("c.rs".to_owned(), 500),
                ("d.rs".to_owned(), 269),
                ("e.rs".to_owned(), 269),
            ]
        );
    }

    fn spanning(
        file: &str,
        line_start: u32,
        line_end: u32,
        content: &str,
        score: f32,
    ) -> MergeChunk {
        MergeChunk {
            line_start,
            line_end,
            ..chunk(file, content, score)
        }
    }

    fn located(chunks: &[MergeChunk]) -> Vec<(&str, u32, u32, Vec<&str>)> {
        chunks
            .iter()
            .map(|c| {
                (
                    c.file.as_str(),
                    c.line_start,
                    c.line_end,
                    c.duplicates.iter().map(String::as_str).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn adjacent_chunks_with_different_copies_stay_separate() {
        let merged = merge_chunks(
            vec![
                spanning("a.rs", 1, 3, "fn one() {}", 0.9),
                spanning("a.rs", 4, 6, "fn two() {}", 0.7),
                spanning("z.rs", 1, 3, "fn one() {}", 0.8),
                spanning("y.rs", 4, 6, "fn two() {}", 0.6),
            ],
            10,
        );

        assert_eq!(
            located(&merged),
            vec![("a.rs", 1, 3, vec!["z.rs"]), ("a.rs", 4, 6, vec!["y.rs"]),],
            "each note names copies of its own lines only"
        );
    }

    #[test]
    fn chunk_with_copies_stays_apart_from_an_adjacent_chunk_without() {
        let merged = merge_chunks(
            vec![
                spanning("a.jsx", 1, 20, "import shared", 0.9),
                spanning("a.jsx", 21, 40, "function Unique() {}", 0.8),
                spanning("b.jsx", 1, 20, "import shared", 0.7),
            ],
            10,
        );

        assert_eq!(
            located(&merged),
            vec![("a.jsx", 1, 20, vec!["b.jsx"]), ("a.jsx", 21, 40, vec![]),]
        );
    }

    #[test]
    fn adjacent_chunks_with_the_same_copies_still_merge() {
        let merged = merge_chunks(
            vec![
                spanning("a.rs", 1, 3, "fn one() {}", 0.9),
                spanning("a.rs", 4, 6, "fn two() {}", 0.7),
                spanning("z.rs", 1, 3, "fn one() {}", 0.8),
                spanning("z.rs", 4, 6, "fn two() {}", 0.6),
            ],
            10,
        );

        assert_eq!(located(&merged), vec![("a.rs", 1, 6, vec!["z.rs"])]);
    }

    #[test]
    fn overlapping_chunk_with_copies_is_not_folded_into_one_without() {
        let merged = merge_chunks(
            vec![
                spanning("a.rs", 1, 50, "fn wide() {}", 0.9),
                spanning("a.rs", 10, 20, "fn inner() {}", 0.8),
                spanning("b.rs", 10, 20, "fn inner() {}", 0.7),
            ],
            10,
        );

        assert_eq!(
            located(&merged),
            vec![("a.rs", 1, 50, vec![]), ("a.rs", 10, 20, vec!["b.rs"]),]
        );
    }

    #[test]
    fn chunk_with_copies_beyond_the_merge_cap_is_not_dropped_as_contained() {
        let merged = merge_chunks(
            vec![
                spanning("a.rs", 1, 80, "fn wide() {}", 0.9),
                spanning("a.rs", 10, 20, "fn inner() {}", 0.8),
                spanning("b.rs", 10, 20, "fn inner() {}", 0.7),
            ],
            10,
        );

        assert_eq!(
            located(&merged),
            vec![("a.rs", 1, 80, vec![]), ("a.rs", 10, 20, vec!["b.rs"]),],
            "the copy in b.rs must stay reachable from the output"
        );
    }

    #[test]
    fn contained_chunk_without_copies_is_still_dropped() {
        let merged = merge_chunks(
            vec![
                spanning("a.rs", 1, 80, "fn wide() {}", 0.9),
                spanning("a.rs", 10, 20, "fn inner() {}", 0.8),
            ],
            10,
        );

        assert_eq!(located(&merged), vec![("a.rs", 1, 80, vec![])]);
    }

    #[test]
    fn a_generated_copy_never_stands_for_a_hand_written_one_on_a_tie() {
        for generated in ["__generated__/app.js", "dist/app.js", ".next/server/app.js"] {
            let merged = merge_chunks(
                vec![
                    chunk(generated, BLOCK, 0.9),
                    chunk("src/app.js", BLOCK, 0.9),
                ],
                10,
            );

            assert_eq!(listed(&merged), vec![("src/app.js", 0.9)], "{generated}");
            assert_eq!(merged[0].duplicates, vec![generated.to_owned()]);
        }
    }

    #[test]
    fn a_generated_copy_with_a_better_score_still_stands_for_the_group() {
        let merged = merge_chunks(
            vec![
                chunk("dist/app.js", BLOCK, 0.95),
                chunk("src/app.js", BLOCK, 0.9),
            ],
            10,
        );

        assert_eq!(listed(&merged), vec![("dist/app.js", 0.95)]);
    }

    #[test]
    fn among_hand_written_copies_a_tie_still_takes_the_smallest_path() {
        let merged = merge_chunks(
            vec![
                chunk("src/z.js", BLOCK, 0.9),
                chunk("dist/a.js", BLOCK, 0.9),
                chunk("src/b.js", BLOCK, 0.9),
            ],
            10,
        );

        assert_eq!(listed(&merged), vec![("src/b.js", 0.9)]);
        assert_eq!(
            merged[0].duplicates,
            vec!["dist/a.js".to_owned(), "src/z.js".to_owned()]
        );
    }
}

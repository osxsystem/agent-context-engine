use std::collections::HashMap;

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
    /// Other files holding this same code. `merge_chunks` fills it on the chunk
    /// it keeps when it collapses identical copies.
    pub duplicates: Vec<String>,
}

impl MergeChunk {
    /// Whether the chunk is short enough to always show whole: rerankers never
    /// narrow it to line ranges.
    pub fn is_under_prune_floor(&self, min_prune_lines: u32) -> bool {
        self.line_end.saturating_sub(self.line_start) < min_prune_lines
    }
}

/// Dedup + merge adjacent chunks, then cap to `top_k`.
///
/// Steps (as specified):
/// 1. Dedup by (file, line_start, line_end) — keep highest score.
/// 2. Collapse chunks whose content is identical across files — keep the best one
///    and record the other files on it. Runs before the merge pass, which
///    concatenates neighbour content, so the key sees one chunk's own text.
/// 3. Group by file.
/// 4. Sort within each group by line_start ASC.
/// 5. Merge pass: merge when next.line_start <= current.line_end + 2 (gap ≤ 1),
///    capping merged range at 60 lines.
/// 6. Drop contained: within each file group, sort by width DESC, skip any chunk
///    fully inside a wider one.
/// 7. Collect all groups, sort by score DESC, cap to top_k.
pub fn merge_chunks(chunks: Vec<MergeChunk>, top_k: usize) -> Vec<MergeChunk> {
    if chunks.is_empty() {
        return vec![];
    }

    // Step 1: dedup — (file, line_start, line_end) → best score entry.
    let mut dedup_map: HashMap<(String, u32, u32), MergeChunk> = HashMap::new();
    for chunk in chunks {
        let key = (chunk.file.clone(), chunk.line_start, chunk.line_end);
        dedup_map
            .entry(key)
            .and_modify(|existing| {
                if chunk.score > existing.score {
                    *existing = chunk.clone();
                }
            })
            .or_insert(chunk);
    }

    // Step 2: one chunk per distinct content.
    let collapsed = {
        let mut deduped: Vec<MergeChunk> = dedup_map.into_values().collect();
        deduped.sort_unstable_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then(a.line_start.cmp(&b.line_start))
                .then(a.line_end.cmp(&b.line_end))
        });
        collapse_identical_content(deduped)
    };

    // Step 3: group by file.
    let mut by_file: HashMap<String, Vec<MergeChunk>> = HashMap::new();
    for chunk in collapsed {
        by_file.entry(chunk.file.clone()).or_default().push(chunk);
    }

    let mut result = Vec::new();

    for (_file, mut file_chunks) in by_file {
        // Step 4: sort by line_start ASC.
        file_chunks.sort_unstable_by_key(|c| c.line_start);

        // Step 5: merge pass.
        let mut merged: Vec<MergeChunk> = Vec::new();
        for next in file_chunks {
            if let Some(current) = merged.last_mut() {
                // Merge condition: next starts within gap of 1 line from current end.
                if next.line_start <= current.line_end + 2 {
                    let new_end = current.line_end.max(next.line_end);
                    let new_start = current.line_start;
                    // Cap at 60 lines.
                    if new_end - new_start + 1 > 60 {
                        merged.push(next);
                    } else {
                        // Compute overlap BEFORE updating line_end.
                        let old_end = current.line_end;
                        let overlap_lines = if next.line_start <= old_end {
                            (old_end - next.line_start + 1) as usize
                        } else {
                            0
                        };

                        current.line_end = new_end;
                        current.score = current.score.max(next.score);

                        // Append only the non-overlapping tail of next's content as fallback.
                        if !next.content.is_empty() {
                            let tail: String = next
                                .content
                                .lines()
                                .skip(overlap_lines)
                                .collect::<Vec<_>>()
                                .join("\n");
                            if !tail.is_empty() {
                                current.content.push('\n');
                                current.content.push_str(&tail);
                            }
                        }
                        if current.symbol.is_none() && next.symbol.is_some() {
                            current.symbol = next.symbol;
                        }
                        if current.symbol_fqn.is_none() && next.symbol_fqn.is_some() {
                            current.symbol_fqn = next.symbol_fqn;
                        }
                        if current.symbol_kind.is_none() && next.symbol_kind.is_some() {
                            current.symbol_kind = next.symbol_kind;
                        }
                        current.duplicates.extend(next.duplicates);
                        current.duplicates.sort();
                        current.duplicates.dedup();
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
            let wa = a.line_end.saturating_sub(a.line_start);
            let wb = b.line_end.saturating_sub(b.line_start);
            wb.cmp(&wa)
        });
        let mut survivors: Vec<MergeChunk> = Vec::new();
        for candidate in merged {
            let contained = survivors.iter().any(|s| {
                s.file == candidate.file
                    && s.line_start <= candidate.line_start
                    && s.line_end >= candidate.line_end
            });
            if !contained {
                survivors.push(candidate);
            }
        }

        result.extend(survivors);
    }

    // Step 7: sort by score DESC and cap to top_k. Equal scores break on the
    // file path then the start line, so the same candidates always rank alike.
    result.sort_unstable_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line_start.cmp(&b.line_start))
    });
    result.truncate(top_k);
    result
}

/// Key under which two chunks count as the same code: the text with CRLF folded
/// to LF and each line's trailing whitespace removed. A reindented copy does not
/// match, so only copies that differ in nothing but line endings collapse.
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

/// Keep one chunk per distinct content and record the other files holding it.
///
/// The kept chunk is the highest scoring one; an equal score goes to the smaller
/// file path, so the choice never depends on input order. A repeat inside the
/// kept chunk's own file stays in place: collapsing it would name the file it
/// already is, and same-file overlap belongs to the merge pass.
fn collapse_identical_content(chunks: Vec<MergeChunk>) -> Vec<MergeChunk> {
    let mut by_content: HashMap<String, Vec<MergeChunk>> = HashMap::new();
    let mut result: Vec<MergeChunk> = Vec::new();
    for chunk in chunks {
        match content_key(&chunk.content) {
            Some(key) => by_content.entry(key).or_default().push(chunk),
            None => result.push(chunk),
        }
    }

    for (_, mut group) in by_content {
        group.sort_unstable_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.file.cmp(&b.file))
                .then_with(|| a.line_start.cmp(&b.line_start))
        });
        let mut group = group.into_iter();
        let Some(mut kept) = group.next() else {
            continue;
        };
        let mut others: Vec<String> = Vec::new();
        for chunk in group {
            if chunk.file == kept.file {
                result.push(chunk);
            } else {
                others.push(chunk.file);
            }
        }
        others.sort();
        others.dedup();
        kept.duplicates = others;
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

    #[test]
    fn adjacent_chunks_merged_in_one_file_keep_both_duplicate_lists() {
        let mut first = chunk_at("a.rs", 1, "fn one() {}");
        first.score = 0.9;
        let mut second = chunk_at("a.rs", 4, "fn two() {}");
        second.score = 0.7;
        let mut first_copy = chunk_at("z.rs", 1, "fn one() {}");
        first_copy.score = 0.8;
        let mut second_copy = chunk_at("y.rs", 4, "fn two() {}");
        second_copy.score = 0.6;

        let merged = merge_chunks(vec![first, second, first_copy, second_copy], 10);

        assert_eq!(merged.len(), 1);
        assert_eq!((merged[0].line_start, merged[0].line_end), (1, 6));
        assert_eq!(
            merged[0].duplicates,
            vec!["y.rs".to_owned(), "z.rs".to_owned()]
        );
    }
}

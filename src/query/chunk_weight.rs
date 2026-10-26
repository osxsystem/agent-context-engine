//! One ranking weight per candidate chunk, applied before the merge cap.

use crate::query::filters::QueryFilters;
use crate::query::merger::{MergeChunk, merge_chunks};

pub fn weighted_merge(
    chunks: Vec<MergeChunk>,
    _query_text: &str,
    _filters: &QueryFilters,
    _repo_roots: &[&str],
    top_k: usize,
) -> Vec<MergeChunk> {
    merge_chunks(chunks, top_k)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "/repo";
    const DEFINITION: &str = "/repo/src/components/Modal.jsx";
    const IMPORT_HEADER: &str = "/repo/src/pages/Page.jsx";
    const MOCK: &str = "/repo/src/pages/Foo.test.jsx";
    const STYLESHEET: &str = "/repo/src/styles/_modal.scss";

    fn chunk(file: &str, content: &str, score: f32) -> MergeChunk {
        MergeChunk {
            file: file.to_owned(),
            line_start: 1,
            line_end: 4,
            score,
            content: content.to_owned(),
            symbol: None,
            symbol_fqn: None,
            symbol_kind: None,
            duplicates: Vec::new(),
        }
    }

    fn pool() -> Vec<MergeChunk> {
        vec![
            chunk(
                DEFINITION,
                "export function Modal({ open }) {\n  return open ? <dialog /> : null;\n}",
                0.70,
            ),
            chunk(
                IMPORT_HEADER,
                "import React from 'react';\nimport Modal from '@components/modal';\n",
                0.72,
            ),
            chunk(
                MOCK,
                "jest.mock('@components/modal', () => () => <div>modal</div>);",
                0.74,
            ),
            chunk(STYLESHEET, ".modal {\n  display: flex;\n}", 0.73),
        ]
    }

    fn ranked(query: &str, top_k: usize) -> Vec<(String, f32)> {
        weighted_merge(pool(), query, &QueryFilters::default(), &[REPO], top_k)
            .into_iter()
            .map(|c| (c.file, c.score))
            .collect()
    }

    fn files(ranked: &[(String, f32)]) -> Vec<&str> {
        ranked.iter().map(|(f, _)| f.as_str()).collect()
    }

    #[test]
    fn usage_query_puts_the_definition_above_every_noise_chunk() {
        let kept = ranked("Modal component and its usages", 2);

        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].0, DEFINITION);
    }

    #[test]
    fn usage_query_ranks_all_three_noise_chunks_below_the_definition() {
        let all = ranked("Modal component and its usages", 4);

        assert_eq!(all[0].0, DEFINITION);
        let mut noise: Vec<&str> = files(&all[1..]);
        noise.sort_unstable();
        assert_eq!(noise, vec![MOCK, IMPORT_HEADER, STYLESHEET]);
    }

    #[test]
    fn a_penalised_chunk_still_fills_a_slot_when_nothing_better_exists() {
        let only_noise: Vec<MergeChunk> = pool().into_iter().skip(1).collect();

        let kept = weighted_merge(
            only_noise,
            "Modal component and its usages",
            &QueryFilters::default(),
            &[REPO],
            3,
        );

        assert_eq!(kept.len(), 3);
    }

    #[test]
    fn shown_score_stays_the_raw_similarity() {
        let all = ranked("Modal component and its usages", 4);

        let score_of = |file: &str| all.iter().find(|(f, _)| f == file).map(|(_, s)| *s);
        assert_eq!(score_of(MOCK), Some(0.74));
        assert_eq!(score_of(IMPORT_HEADER), Some(0.72));
        assert_eq!(score_of(STYLESHEET), Some(0.73));
        assert_eq!(score_of(DEFINITION), Some(0.70));
    }

    #[test]
    fn test_query_keeps_the_mock_on_top() {
        let all = ranked("tests for Modal", 4);

        assert_eq!(all[0].0, MOCK);
    }

    #[test]
    fn style_query_keeps_the_stylesheet_above_the_definition() {
        let all = ranked("modal styles", 4);

        let position = |file: &str| all.iter().position(|(f, _)| f == file).unwrap();
        assert!(position(STYLESHEET) < position(DEFINITION));
    }

    #[test]
    fn src_copy_represents_an_identical_mock_copy_on_a_score_tie() {
        let body = "export const modalProps = { open: true };";
        let mock_copy = "/repo/src/__mocks__/modalProps.js";
        let src_copy = "/repo/src/components/modalProps.js";
        let pool = vec![chunk(mock_copy, body, 0.70), chunk(src_copy, body, 0.70)];

        let kept = weighted_merge(pool, "modal props", &QueryFilters::default(), &[REPO], 5);

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].file, src_copy);
        assert_eq!(kept[0].duplicates, vec![mock_copy.to_owned()]);
    }
}

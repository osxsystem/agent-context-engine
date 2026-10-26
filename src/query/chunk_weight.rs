use std::path::Path;

use crate::parsing::test_paths::{is_test_path, repo_relative_in};
use crate::query::filters::QueryFilters;
use crate::query::merger::{MergeChunk, merge_weighted};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkClass {
    Mock,
    Stylesheet,
    ImportOnly,
}

impl ChunkClass {
    const ALL: [ChunkClass; 3] = [Self::Mock, Self::Stylesheet, Self::ImportOnly];

    const fn weight(self) -> f32 {
        match self {
            Self::Mock => 0.90,
            Self::Stylesheet => 0.90,
            Self::ImportOnly => 0.85,
        }
    }

    fn matches(self, rel_path: &str, content: &str) -> bool {
        match self {
            Self::Mock => is_mock(rel_path, content),
            Self::Stylesheet => {
                extension(rel_path).is_some_and(|e| STYLE_EXTENSIONS.contains(&e.as_str()))
            }
            Self::ImportOnly => {
                extension(rel_path).is_some_and(|e| SCRIPT_EXTENSIONS.contains(&e.as_str()))
                    && is_import_only(content)
            }
        }
    }
}

const STYLE_EXTENSIONS: &[&str] = &["css", "scss", "sass", "less"];
const MOCK_CALLS: &[&str] = &["jest.mock(", "vi.mock(", "jest.doMock("];
const SCRIPT_EXTENSIONS: &[&str] = &["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"];
const TEST_WORDS: &[&str] = &[
    "test", "tests", "spec", "specs", "mock", "mocks", "jest", "fixture", "fixtures",
];
const STYLE_WORDS: &[&str] = &[
    "style",
    "styles",
    "css",
    "scss",
    "sass",
    "less",
    "stylesheet",
    "theme",
];

fn is_mock(rel_path: &str, content: &str) -> bool {
    let in_mocks_dir = rel_path
        .split('/')
        .any(|dir| dir.eq_ignore_ascii_case("__mocks__"));
    in_mocks_dir || (is_test_path(rel_path) && MOCK_CALLS.iter().any(|c| content.contains(c)))
}

fn extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

#[derive(Debug, Clone, Copy)]
struct QueryIntent {
    mocks: bool,
    styles: bool,
}

impl QueryIntent {
    fn of(query_texts: &[&str], filters: &QueryFilters) -> Self {
        let texts = || {
            query_texts
                .iter()
                .copied()
                .chain(filters.path_filters.iter().map(String::as_str))
        };
        let style_language = filters
            .languages
            .iter()
            .any(|l| STYLE_EXTENSIONS.contains(&l.to_ascii_lowercase().as_str()));
        Self {
            mocks: texts().any(|t| mentions(t, TEST_WORDS)),
            styles: style_language || texts().any(|t| mentions(t, STYLE_WORDS)),
        }
    }

    fn asks_for(self, class: ChunkClass) -> bool {
        match class {
            ChunkClass::Mock => self.mocks,
            ChunkClass::Stylesheet => self.styles,
            ChunkClass::ImportOnly => false,
        }
    }
}

fn mentions(text: &str, words: &[&str]) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|token| words.iter().any(|w| token.eq_ignore_ascii_case(w)))
}

fn rank_weight(chunk: &MergeChunk, repo_roots: &[&str], intent: QueryIntent) -> f32 {
    let rel_path = repo_relative_in(&chunk.file, repo_roots);
    ChunkClass::ALL
        .into_iter()
        .filter(|class| !intent.asks_for(*class) && class.matches(&rel_path, &chunk.content))
        .map(ChunkClass::weight)
        .fold(1.0, f32::min)
}

/// [`merge_weighted`] over a pool whose chunks are weighted by class.
/// `repo_roots` are the checkouts the chunks live in, to make their paths
/// repo-relative.
pub fn weighted_merge(
    chunks: Vec<MergeChunk>,
    query_texts: &[&str],
    filters: &QueryFilters,
    repo_roots: &[&str],
    top_k: usize,
) -> Vec<MergeChunk> {
    let intent = QueryIntent::of(query_texts, filters);
    let weighted = chunks
        .into_iter()
        .map(|chunk| {
            let weight = rank_weight(&chunk, repo_roots, intent);
            (chunk, weight)
        })
        .collect();
    merge_weighted(weighted, top_k)
}

fn is_import_only(content: &str) -> bool {
    let mut cursor = Cursor {
        tokens: tokenize(content),
        pos: 0,
    };
    let mut seen = false;
    while let Some(token) = cursor.next() {
        let is_directive = matches!(token.kind, Tok::Str(s) if DIRECTIVES.contains(&s));
        let ok = match token.kind {
            Tok::Punct(';') => continue,
            Tok::Str(_) if is_directive && !seen => cursor.ends_statement(),
            Tok::Word("import") => cursor.module_clause(),
            Tok::Word("export") => {
                cursor.take(Tok::Word("type"));
                matches!(cursor.peek(), Some(Tok::Punct('{' | '*'))) && cursor.module_clause()
            }
            Tok::Word("const" | "let" | "var") => cursor.binding_then_require(),
            Tok::Word("require") => cursor.require_call(),
            _ => false,
        };
        if !ok {
            return false;
        }
        seen |= !is_directive;
    }
    seen
}

const DIRECTIVES: &[&str] = &["use client", "use server", "use strict"];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok<'a> {
    Word(&'a str),
    Str(&'a str),
    Punct(char),
}

struct Token<'a> {
    kind: Tok<'a>,
    after_newline: bool,
}

fn tokenize(src: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut after_newline = false;
    let mut rest = src;
    while let Some(c) = rest.chars().next() {
        let (len, kind) = if rest.starts_with("//") {
            (rest.find('\n').unwrap_or(rest.len()), None)
        } else if let Some(comment) = rest.strip_prefix("/*") {
            let len = comment.find("*/").map_or(rest.len(), |i| i + 4);
            after_newline |= rest[..len].contains('\n');
            (len, None)
        } else if matches!(c, '\'' | '"' | '`') {
            let mut escaped = false;
            let close = rest.char_indices().skip(1).find(|&(_, ch)| {
                let closes = !escaped && ch == c;
                escaped = !escaped && ch == '\\';
                closes
            });
            let len = close.map_or(rest.len(), |(i, _)| i + 1);
            let body = &rest[1..len];
            (len, Some(Tok::Str(body.strip_suffix(c).unwrap_or(body))))
        } else if c.is_alphanumeric() || c == '_' || c == '$' {
            let len = rest
                .find(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '$'))
                .unwrap_or(rest.len());
            (len, Some(Tok::Word(&rest[..len])))
        } else if c.is_whitespace() {
            after_newline |= c == '\n';
            (c.len_utf8(), None)
        } else {
            (c.len_utf8(), Some(Tok::Punct(c)))
        };
        if let Some(kind) = kind {
            tokens.push(Token {
                kind,
                after_newline: std::mem::take(&mut after_newline),
            });
        }
        rest = &rest[len..];
    }
    tokens
}

struct Cursor<'a> {
    tokens: Vec<Token<'a>>,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<Tok<'a>> {
        self.tokens.get(self.pos).map(|t| t.kind)
    }

    fn next(&mut self) -> Option<&Token<'a>> {
        self.pos += 1;
        self.tokens.get(self.pos - 1)
    }

    fn take(&mut self, want: Tok) -> bool {
        let found = self.peek() == Some(want);
        self.pos += usize::from(found);
        found
    }

    fn ends_statement(&mut self) -> bool {
        match self.tokens.get(self.pos) {
            Some(t) if t.kind == Tok::Punct(';') => {
                self.pos += 1;
                true
            }
            Some(t) => t.after_newline,
            None => true,
        }
    }

    fn module_clause(&mut self) -> bool {
        let mut in_braces = false;
        while let Some(kind) = self.next().map(|t| t.kind) {
            match (kind, in_braces) {
                (Tok::Str(_), false) => return self.ends_statement(),
                (Tok::Punct('{'), false) => in_braces = true,
                (Tok::Punct('}'), true) => {
                    in_braces = false;
                    if !matches!(self.peek(), None | Some(Tok::Word("from"))) {
                        return false;
                    }
                }
                (Tok::Word(_) | Tok::Str(_) | Tok::Punct(',' | '*'), _) => {}
                _ => return false,
            }
        }
        true
    }

    fn binding_then_require(&mut self) -> bool {
        loop {
            match self.next().map(|t| t.kind) {
                Some(Tok::Punct('=')) => break,
                Some(Tok::Word(_) | Tok::Punct(',' | ':' | '{' | '}' | '[' | ']')) => {}
                _ => return false,
            }
        }
        self.take(Tok::Word("require")) && self.require_call()
    }

    fn require_call(&mut self) -> bool {
        self.take(Tok::Punct('('))
            && matches!(self.next().map(|t| t.kind), Some(Tok::Str(_)))
            && self.take(Tok::Punct(')'))
            && self.ends_statement()
    }
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
        weighted_merge(pool(), &[query], &QueryFilters::default(), &[REPO], top_k)
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
            &["Modal component and its usages"],
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

        let kept = weighted_merge(pool, &["modal props"], &QueryFilters::default(), &[REPO], 5);

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].file, src_copy);
        assert_eq!(kept[0].duplicates, vec![mock_copy.to_owned()]);
    }

    #[test]
    fn path_filter_for_tests_or_styles_asks_for_that_class() {
        let by_path = |path: &str| QueryFilters {
            path_filters: vec![path.to_owned()],
            ..QueryFilters::default()
        };
        let weighted = |filters: &QueryFilters| {
            weighted_merge(pool(), &["Modal usages"], filters, &[REPO], 4)
                .into_iter()
                .map(|c| c.file)
                .collect::<Vec<_>>()
        };

        let tests_filter = weighted(&by_path("__tests__/"));
        let styles_filter = weighted(&by_path("styles/"));

        assert_eq!(tests_filter[0], MOCK);
        assert_eq!(styles_filter[0], STYLESHEET);
    }

    #[test]
    fn query_words_match_whole_words_only() {
        let intent = |q: &str| {
            let i = QueryIntent::of(&[q], &QueryFilters::default());
            (i.mocks, i.styles)
        };

        assert_eq!(intent("Modal component and its usages"), (false, false));
        assert_eq!(intent("tests for Modal"), (true, false));
        assert_eq!(intent("Jest mock of the modal"), (true, false));
        assert_eq!(intent("modal styles"), (false, true));
        assert_eq!(intent("SCSS theme variables"), (false, true));
        assert_eq!(intent("contest latest attest prestyle"), (false, false));
        assert_eq!(intent("test_utils helper"), (true, false));
    }

    #[test]
    fn paths_are_judged_relative_to_their_repo() {
        let roots = ["/home/me/tests/app", "/other"];

        assert_eq!(
            repo_relative_in("/home/me/tests/app/src/a.rs", &roots),
            "src/a.rs"
        );
        assert_eq!(repo_relative_in("/elsewhere/tests/x.rs", &roots), "x.rs");
        assert_eq!(
            repo_relative_in(r"/other\src\a.rs", &roots),
            "src/a.rs",
            "backslash separators are normalised"
        );
    }

    fn weight_of(file: &str, content: &str) -> f32 {
        let intent = QueryIntent::of(&["Modal usages"], &QueryFilters::default());
        rank_weight(&chunk(file, content, 0.5), &[REPO], intent)
    }

    #[test]
    fn each_class_gets_its_own_weight_and_the_strongest_wins() {
        let imports = "import a from 'a';";

        assert_eq!(weight_of("/repo/src/a.js", "export const a = 1;"), 1.0);
        assert_eq!(weight_of("/repo/src/a.test.js", "it('x', () => {});"), 1.0);
        assert_eq!(weight_of("/repo/src/a.test.js", "jest.mock('a');"), 0.90);
        assert_eq!(weight_of("/repo/src/a.less", ".a {}"), 0.90);
        assert_eq!(weight_of("/repo/src/a.js", imports), 0.85);
        assert_eq!(weight_of("/repo/src/a.test.js", imports), 0.85);
        assert_eq!(weight_of("/repo/src/a.rs", imports), 1.0);
    }

    #[test]
    fn mocks_directory_counts_as_mocks() {
        assert_eq!(
            weight_of("/repo/src/__mocks__/api.js", "export default {};"),
            0.90
        );
    }

    fn import_only(src: &str) -> bool {
        is_import_only(src)
    }

    #[test]
    fn import_headers_are_import_only() {
        assert!(import_only(
            "import React from 'react';\nimport { a } from './a';"
        ));
        assert!(import_only("import './polyfill';"));
        assert!(import_only(
            "import type { A } from \"./a\"\nimport * as b from './b'"
        ));
        assert!(import_only(
            "import {\n  one,\n  two, // keep\n} from '@lib/x';\n\nimport y from './y';"
        ));
        assert!(import_only(
            "// @flow\n/* license\n * text\n */\nimport a from 'a';"
        ));
        assert!(import_only(
            "export * from './a';\nexport { b } from './b';"
        ));
        assert!(import_only("export type { T } from './t';"));
        assert!(import_only(
            "const fs = require('fs');\nconst { join } = require(\"path\");\nrequire('./side');"
        ));
        assert!(import_only("import page from './[id]/page';"));
        assert!(import_only("import a from 'a'; import b from 'b';"));
        assert!(import_only("import {\n  one,\n  two,"), "cut mid-statement");
    }

    #[test]
    fn anything_else_in_the_chunk_is_not_import_only() {
        assert!(!import_only(""));
        assert!(!import_only("// only a comment"));
        assert!(!import_only("import a from 'a';\nexport const x = 1;"));
        assert!(!import_only("import a from 'a';\nfoo();"));
        assert!(!import_only("import a from 'a'; foo('x');"));
        assert!(!import_only("export { a };\nexport function f() {}"));
        assert!(!import_only("const a = load('x');"));
        assert!(!import_only("const lazy = import('./x');"));
        assert!(!import_only("importantValue = 1;"));
        assert!(!import_only("} from 'a';"));
        assert!(!import_only(
            "jest.mock('@components/modal', () => () => null);"
        ));
    }

    #[test]
    fn quotes_and_comments_do_not_confuse_the_scan() {
        assert!(import_only(r"import a from 'it\'s';"));
        assert!(import_only("/* don't */ import a from 'a'; // it's"));
        assert!(!import_only("import a from 'a';\n/* x */ run('b');"));
    }

    #[test]
    fn real_code_around_a_require_is_not_import_only() {
        assert!(!import_only(
            "const server = require('http').createServer((req, res) => {\n  res.end('ok');\n});"
        ));
        assert!(!import_only(
            "require('dotenv').config({\n  path: env,\n});"
        ));
        assert!(!import_only("import Foo = Bar.Baz;\nfoo(bar);"));
        assert!(!import_only("const x = require('a'); foo('y');"));
        assert!(!import_only("const x = require('a').default;"));
        assert!(!import_only("export default 'x';"));
    }

    #[test]
    fn directive_lines_do_not_stop_a_header_being_import_only() {
        assert!(import_only("'use client';\nimport React from 'react';"));
        assert!(import_only("\"use client\";\nimport React from 'react';"));
        assert!(import_only("'use strict';\nconst fs = require('fs');"));
        assert!(import_only("\"use strict\";\nimport a from 'a';"));
        assert!(!import_only("'use client';"));
        assert!(!import_only("'use client';\nrender();"));
    }

    #[test]
    fn a_sub_query_keeps_the_exemptions_the_user_query_asked_for() {
        let kept = weighted_merge(
            pool(),
            &["tests for Modal", "Modal render assertions"],
            &QueryFilters::default(),
            &[REPO],
            4,
        );

        assert_eq!(kept[0].file, MOCK);
    }

    #[test]
    fn a_sub_query_can_ask_for_an_exemption_the_user_query_did_not() {
        let kept = weighted_merge(
            pool(),
            &["Modal usages", "modal styles"],
            &QueryFilters::default(),
            &[REPO],
            4,
        );

        let position = |file: &str| kept.iter().position(|c| c.file == file).unwrap();
        assert!(position(STYLESHEET) < position(DEFINITION));
    }

    #[test]
    fn a_stylesheet_language_filter_lifts_the_stylesheet_penalty() {
        for lang in ["css", "scss", "sass", "less"] {
            let parsed =
                crate::query::filters::parse_query(&format!("Modal usages lang:{lang}"), None);

            let kept = weighted_merge(pool(), &[&parsed.text], &parsed.filters, &[REPO], 4);

            assert_eq!(kept[0].file, STYLESHEET, "lang:{lang}");
        }
    }

    #[test]
    fn a_code_language_filter_keeps_the_stylesheet_penalty() {
        let parsed = crate::query::filters::parse_query("Modal usages lang:javascript", None);

        let kept = weighted_merge(pool(), &[&parsed.text], &parsed.filters, &[REPO], 4);

        assert_eq!(kept[0].file, DEFINITION);
    }

    #[test]
    fn repo_root_matching_ignores_case() {
        let weight = |file: &str| {
            let intent = QueryIntent::of(&["Modal usages"], &QueryFilters::default());
            rank_weight(&chunk(file, "x", 0.5), &["/HOME/ME/APP"], intent)
        };

        assert_eq!(weight("/home/me/app/__MOCKS__/a.js"), 0.90);
        assert_eq!(weight("/home/me/app/src/a.js"), 1.0);
    }

    #[test]
    fn a_repo_nested_under_another_repos_tests_dir_is_judged_against_its_own_root() {
        let file = "/p/tests/app/src/a.js";
        let intent = QueryIntent::of(&["Modal usages"], &QueryFilters::default());

        for roots in [["/p", "/p/tests/app"], ["/p/tests/app", "/p"]] {
            let weight = rank_weight(&chunk(file, "x", 0.5), &roots, intent);

            assert_eq!(weight, 1.0, "{roots:?}");
        }
    }

    #[test]
    fn plain_rust_test_chunk_keeps_its_rank_above_plain_code() {
        let pool = vec![
            chunk(
                "/repo/src/indexing/load_repos_tests.rs",
                "#[test]\nfn loads_every_repo() {\n    assert_eq!(load(), 2);\n}",
                0.747,
            ),
            chunk(
                "/repo/src/indexing/load_repos.rs",
                "pub fn load() -> usize {\n    2\n}",
                0.738,
            ),
        ];

        let kept = weighted_merge(
            pool,
            &["load repos from settings"],
            &QueryFilters::default(),
            &[REPO],
            2,
        );

        assert_eq!(kept[0].file, "/repo/src/indexing/load_repos_tests.rs");
    }

    #[test]
    fn test_chunk_without_a_mock_call_takes_no_penalty() {
        assert_eq!(
            weight_of(
                "/repo/src/pages/Foo.test.jsx",
                "it('renders', () => {\n  render(<Foo />);\n});"
            ),
            1.0
        );
        assert_eq!(
            weight_of("/repo/tests/jev_wire.rs", "#[test]\nfn wire() {}"),
            1.0
        );
    }

    #[test]
    fn mock_calls_in_a_test_path_are_mocks() {
        for call in [
            "jest.mock('./a');",
            "vi.mock('./a');",
            "jest.doMock('./a');",
        ] {
            assert_eq!(weight_of("/repo/src/Foo.test.jsx", call), 0.90, "{call}");
        }
    }

    #[test]
    fn mock_call_outside_a_test_path_is_not_a_mock() {
        assert_eq!(weight_of("/repo/src/setup.js", "jest.mock('./a');"), 1.0);
    }
}

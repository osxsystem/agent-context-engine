use std::path::Path;

use crate::parsing::test_paths::is_test_path;
use crate::path_in_repo;
use crate::query::filters::QueryFilters;
use crate::query::merger::{MergeChunk, merge_weighted};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkClass {
    Test,
    Stylesheet,
    ImportOnly,
}

impl ChunkClass {
    const ALL: [ChunkClass; 3] = [Self::Test, Self::Stylesheet, Self::ImportOnly];

    const fn weight(self) -> f32 {
        match self {
            Self::Test => 0.90,
            Self::Stylesheet => 0.90,
            Self::ImportOnly => 0.85,
        }
    }

    fn matches(self, rel_path: &str, content: &str) -> bool {
        match self {
            Self::Test => is_test_path(rel_path),
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

fn extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

#[derive(Debug, Clone, Copy)]
struct QueryIntent {
    tests: bool,
    styles: bool,
}

impl QueryIntent {
    fn of(query_text: &str, filters: &QueryFilters) -> Self {
        let texts =
            || std::iter::once(query_text).chain(filters.path_filters.iter().map(String::as_str));
        Self {
            tests: texts().any(|t| mentions(t, TEST_WORDS)),
            styles: texts().any(|t| mentions(t, STYLE_WORDS)),
        }
    }

    fn asks_for(self, class: ChunkClass) -> bool {
        match class {
            ChunkClass::Test => self.tests,
            ChunkClass::Stylesheet => self.styles,
            ChunkClass::ImportOnly => false,
        }
    }
}

fn mentions(text: &str, words: &[&str]) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|token| words.iter().any(|w| token.eq_ignore_ascii_case(w)))
}

fn repo_relative_or_file_name(file: &str, repo_roots: &[&str]) -> String {
    let owner = repo_roots.iter().find(|root| path_in_repo(file, root));
    let rel = match owner {
        Some(root) => {
            let root = root.trim_end_matches(['/', '\\']);
            &file[root.len()..]
        }
        None => file.rsplit(['/', '\\']).next().unwrap_or(""),
    };
    rel.trim_start_matches(['/', '\\']).replace('\\', "/")
}

fn rank_weight(chunk: &MergeChunk, repo_roots: &[&str], intent: QueryIntent) -> f32 {
    let rel_path = repo_relative_or_file_name(&chunk.file, repo_roots);
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
    query_text: &str,
    filters: &QueryFilters,
    repo_roots: &[&str],
    top_k: usize,
) -> Vec<MergeChunk> {
    let intent = QueryIntent::of(query_text, filters);
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
    let mut statement: Option<Statement> = None;
    let mut seen = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if statement.is_none() {
            if line.starts_with("/*") || line.starts_with('*') {
                continue;
            }
            let Some(kind) = StatementKind::starting(line) else {
                return false;
            };
            statement = Some(Statement {
                kind,
                depth: 0,
                quoted: false,
            });
        }
        let open = statement.as_mut().expect("set above");
        let scanned = scan(line);
        open.depth += scanned.bracket_delta;
        let last_quote_end = scanned.last_quote_end;
        open.quoted |= last_quote_end.is_some();
        if open.depth < 0
            || (open.depth == 0 && !open.quoted && open.kind == StatementKind::ReExport)
        {
            return false;
        }
        if open.depth == 0 && open.quoted {
            if let Some(end) = last_quote_end {
                let closer = if open.kind == StatementKind::Require {
                    ')'
                } else {
                    ';'
                };
                let rest = line[end..]
                    .trim_start_matches(|c: char| c == closer || c == ';' || c.is_whitespace());
                if !(rest.is_empty() || rest.starts_with("//") || rest.starts_with("/*")) {
                    return false;
                }
            }
            statement = None;
            seen = true;
        }
    }
    seen || statement.is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatementKind {
    Import,
    ReExport,
    Require,
}

struct Statement {
    kind: StatementKind,
    depth: i32,
    quoted: bool,
}

impl StatementKind {
    fn starting(line: &str) -> Option<Self> {
        if let Some(rest) = line.strip_prefix("import") {
            let starts_clause =
                rest.starts_with(|c: char| c.is_whitespace() || "{*'\"".contains(c));
            return starts_clause.then_some(Self::Import);
        }
        if let Some(rest) = line.strip_prefix("export") {
            let rest = rest.trim_start();
            let rest = rest.strip_prefix("type ").map_or(rest, str::trim_start);
            return (rest.starts_with('{') || rest.starts_with('*')).then_some(Self::ReExport);
        }
        let after_binding = ["const ", "let ", "var "]
            .iter()
            .find_map(|kw| line.strip_prefix(kw))
            .map_or(line, |rest| {
                rest.split_once('=').map_or("", |(_, rhs)| rhs.trim_start())
            });
        after_binding
            .starts_with("require(")
            .then_some(Self::Require)
    }
}

struct Scanned {
    bracket_delta: i32,
    last_quote_end: Option<usize>,
}

fn scan(line: &str) -> Scanned {
    let mut quote: Option<char> = None;
    let mut scanned = Scanned {
        bracket_delta: 0,
        last_quote_end: None,
    };
    let mut prev = '\0';
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => {
                quote = None;
                scanned.last_quote_end = Some(i + c.len_utf8());
            }
            Some(_) => {}
            None => match c {
                '\'' | '"' | '`' => quote = Some(c),
                '(' | '[' | '{' => scanned.bracket_delta += 1,
                ')' | ']' | '}' => scanned.bracket_delta -= 1,
                '/' if prev == '/' => break,
                _ => {}
            },
        }
        prev = c;
    }
    scanned
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

    #[test]
    fn path_filter_for_tests_or_styles_asks_for_that_class() {
        let by_path = |path: &str| QueryFilters {
            path_filters: vec![path.to_owned()],
            ..QueryFilters::default()
        };
        let weighted = |filters: &QueryFilters| {
            weighted_merge(pool(), "Modal usages", filters, &[REPO], 4)
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
            let i = QueryIntent::of(q, &QueryFilters::default());
            (i.tests, i.styles)
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
            repo_relative_or_file_name("/home/me/tests/app/src/a.rs", &roots),
            "src/a.rs"
        );
        assert_eq!(
            repo_relative_or_file_name("/elsewhere/tests/x.rs", &roots),
            "x.rs"
        );
        assert_eq!(
            repo_relative_or_file_name(r"/other\src\a.rs", &roots),
            "src/a.rs",
            "backslash separators are normalised"
        );
    }

    fn weight_of(file: &str, content: &str) -> f32 {
        let intent = QueryIntent::of("Modal usages", &QueryFilters::default());
        rank_weight(&chunk(file, content, 0.5), &[REPO], intent)
    }

    #[test]
    fn each_class_gets_its_own_weight_and_the_strongest_wins() {
        let imports = "import a from 'a';";

        assert_eq!(weight_of("/repo/src/a.js", "export const a = 1;"), 1.0);
        assert_eq!(weight_of("/repo/src/a.test.js", "it('x', () => {});"), 0.90);
        assert_eq!(weight_of("/repo/src/a.less", ".a {}"), 0.90);
        assert_eq!(weight_of("/repo/src/a.js", imports), 0.85);
        assert_eq!(weight_of("/repo/src/a.test.js", imports), 0.85);
        assert_eq!(weight_of("/repo/src/a.rs", imports), 1.0);
    }

    #[test]
    fn mocks_directory_counts_as_tests() {
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
}

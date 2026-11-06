//! Retrieval ground truth, derived from the repo under test.
//!
//! This used to be ~40 hand-written `(query, file, symbol)` triples anchored to
//! one specific C++ codebase. That made the benchmark unrunnable for anyone
//! without that checkout, and every unresolved symbol dropped silently out of
//! the denominator, so a missing corpus produced confident-looking zeros rather
//! than an error.
//!
//! Cases are now derived from whatever repo you point at. The idea: **a doc
//! comment is a natural-language description of a symbol, written by a developer
//! who was not thinking about retrieval.** Use the comment as the query and the
//! symbol it documents as the expected answer. That is the retrieval task
//! exactly, the ground-truth span is exact, and nobody hand-picked the queries
//! to flatter the engine.
//!
//! Spans come from the same frozen `parse_file` extraction the old fixture used,
//! so ground truth remains independent of any chunk boundary and cannot favour
//! one chunker or reranker over another.

use std::collections::HashMap;
use std::path::Path;

use context_engine_rs::indexing::walker::walk_repo;
use context_engine_rs::parsing::parse_file;
use context_engine_rs::parsing::symbols::SymbolKind;

/// One retrieval case: ask `query`, expect `symbol` at `file:line_start-line_end`.
#[derive(Debug, Clone)]
pub struct EvalCase {
    pub query: String,
    pub file: String,
    pub symbol: String,
    pub line_start: u32,
    pub line_end: u32,
}

/// Minimum body size for a symbol to be worth retrieving. One-line accessors
/// carry no meaning and their doc comments are boilerplate.
const MIN_SYMBOL_LINES: u32 = 5;
/// A query shorter than this is not a description, it is a label.
const MIN_QUERY_WORDS: usize = 8;
/// Past this the comment is a design essay; the tail stops describing the symbol.
const MAX_QUERY_WORDS: usize = 60;
/// How far above a symbol to look for its doc block.
const MAX_DOC_SCAN_LINES: usize = 12;
/// A name shorter than this is too common to be a unique identifier query.
const MIN_IDENT_CHARS: usize = 6;
/// An item shorter than this has no body worth retrieving.
const MIN_IDENT_LINES: u32 = 3;
/// Cases per identifier set. The mixed set reuses these, so each template has
/// the same count.
pub const IDENT_LIMIT: usize = 60;

/// The mixed set: each identifier case wrapped in one of these templates, scored
/// as its own set so a retrieval change fitted to one phrasing shows up.
const MIXED_TEMPLATES: [(&str, &str); 3] = [
    ("where", "where is {} defined and what does it do"),
    ("how", "how does {} work"),
    ("show", "show me the implementation of {}"),
];

const PARA_FIXTURE: &str = include_str!("fixtures/para_cases.json");

/// A named group of cases that is scored and reported on its own.
#[derive(Debug, Clone)]
pub struct CaseSet {
    pub name: String,
    pub cases: Vec<EvalCase>,
    /// Cases whose gold range no longer matches the corpus. They are reported
    /// but never scored, so a moved item is not mistaken for a retrieval miss.
    pub stale: Vec<EvalCase>,
}

impl CaseSet {
    fn new(name: &str, cases: Vec<EvalCase>) -> Self {
        Self {
            name: name.to_owned(),
            cases,
            stale: Vec::new(),
        }
    }
}

/// The doc-comment set, as a [`CaseSet`].
pub fn derive_doc_set(repo: &str, limit: usize) -> CaseSet {
    CaseSet::new("doc", derive_eval_set(repo, limit))
}

/// A symbol as the identifier set sees it: just enough to judge uniqueness.
struct SymbolRef {
    file: String,
    name: String,
    kind: SymbolKind,
    line_start: u32,
    line_end: u32,
}

/// Derive up to `limit` identifier cases from `repo`: the bare name of an item,
/// expecting that item's own line range.
pub fn derive_ident_set(repo: &str, limit: usize) -> CaseSet {
    let mut symbols: Vec<SymbolRef> = Vec::new();
    for path in walk_repo(repo) {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let parsed = parse_file(&path, &source);
        let file = path.replace('\\', "/");
        symbols.extend(parsed.symbols.into_iter().map(|s| SymbolRef {
            file: file.clone(),
            name: s.qualified.name,
            kind: s.kind,
            line_start: s.line_start,
            line_end: s.line_end,
        }));
    }
    CaseSet::new("ident", sample_evenly(unique_ident_cases(&symbols), limit))
}

/// Keep the items whose name occurs exactly once across every symbol of any
/// kind, compared case-insensitively. A name that is also a method on another
/// type, or a function in another file, has no single right answer.
fn unique_ident_cases(symbols: &[SymbolRef]) -> Vec<EvalCase> {
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    for s in symbols {
        *occurrences.entry(s.name.to_lowercase()).or_default() += 1;
    }
    let mut cases: Vec<EvalCase> = symbols
        .iter()
        .filter(|s| {
            matches!(
                s.kind,
                SymbolKind::Function
                    | SymbolKind::Method
                    | SymbolKind::Struct
                    | SymbolKind::Enum
                    | SymbolKind::Trait
            )
        })
        .filter(|s| occurrences[&s.name.to_lowercase()] == 1)
        .filter(|s| s.name.chars().count() >= MIN_IDENT_CHARS)
        .filter(|s| s.line_end.saturating_sub(s.line_start) + 1 >= MIN_IDENT_LINES)
        .map(|s| EvalCase {
            query: s.name.clone(),
            file: s.file.clone(),
            symbol: s.name.clone(),
            line_start: s.line_start,
            line_end: s.line_end,
        })
        .collect();
    cases.sort_by(|a, b| a.file.cmp(&b.file).then(a.line_start.cmp(&b.line_start)));
    cases
}

/// Wrap each identifier case in every mixed template. One set per template, in
/// template order, each holding the same items as `ident`.
pub fn derive_mixed_set(ident: &CaseSet) -> Vec<CaseSet> {
    MIXED_TEMPLATES
        .iter()
        .map(|(label, template)| {
            let cases = ident
                .cases
                .iter()
                .map(|c| EvalCase {
                    query: template.replace("{}", &c.symbol),
                    ..c.clone()
                })
                .collect();
            CaseSet::new(&format!("mixed:{label}"), cases)
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct ParaFixture {
    model: String,
    prompt: String,
    corpus_commit: String,
    cases: Vec<ParaCase>,
}

#[derive(serde::Deserialize)]
struct ParaCase {
    query: String,
    /// Relative to the repo root.
    file: String,
    name: String,
    start: u32,
    end: u32,
}

/// Who wrote the paraphrases and against which commit, for the artifact.
#[derive(serde::Serialize, Debug, Clone)]
pub struct ParaProvenance {
    pub model: String,
    pub prompt: String,
    pub corpus_commit: String,
}

pub fn para_provenance() -> ParaProvenance {
    let f = parse_para_fixture();
    ParaProvenance {
        model: f.model,
        prompt: f.prompt,
        corpus_commit: f.corpus_commit,
    }
}

fn parse_para_fixture() -> ParaFixture {
    serde_json::from_str(PARA_FIXTURE).expect("para_cases.json is embedded and must parse")
}

/// Load the paraphrase fixture against `repo`. A case whose named item no longer
/// starts at its recorded line goes to `stale`.
pub fn derive_para_set(repo: &str) -> CaseSet {
    let root = repo.trim_end_matches('/');
    split_stale(parse_para_fixture().cases, root, |path| {
        std::fs::read_to_string(path).ok()
    })
}

fn split_stale(
    cases: Vec<ParaCase>,
    root: &str,
    read: impl Fn(&str) -> Option<String>,
) -> CaseSet {
    let mut set = CaseSet::new("para", Vec::new());
    for c in cases {
        let path = Path::new(root)
            .join(&c.file)
            .to_string_lossy()
            .replace('\\', "/");
        let fresh = read(&path).is_some_and(|src| item_starts_at(&src, &c.name, c.start));
        let case = EvalCase {
            query: c.query,
            file: path,
            symbol: c.name,
            line_start: c.start,
            line_end: c.end,
        };
        if fresh {
            set.cases.push(case);
        } else {
            set.stale.push(case);
        }
    }
    set
}

/// Whether line `start` (1-based) of `source` names `name` as a whole word.
fn item_starts_at(source: &str, name: &str, start: u32) -> bool {
    let Some(line) = (start as usize)
        .checked_sub(1)
        .and_then(|i| source.lines().nth(i))
    else {
        return false;
    };
    line.match_indices(name).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + name.len()..].chars().next();
        !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char)
    })
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Derive up to `limit` retrieval cases from `repo`.
///
/// Deterministic for a given repo state: cases are sorted by (file, line) and
/// sampled at an even stride, so two runs over an unchanged tree produce the
/// same set and their numbers are comparable.
pub fn derive_eval_set(repo: &str, limit: usize) -> Vec<EvalCase> {
    let mut cases: Vec<EvalCase> = Vec::new();

    for path in walk_repo(repo) {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = source.lines().collect();
        let parsed = parse_file(&path, &source);

        for sym in &parsed.symbols {
            if !matches!(sym.kind, SymbolKind::Function | SymbolKind::Method) {
                continue;
            }
            if sym.line_end.saturating_sub(sym.line_start) < MIN_SYMBOL_LINES {
                continue;
            }
            let Some(doc) = doc_block_above(&lines, sym.line_start) else {
                continue;
            };
            let name = &sym.qualified.name;
            if leaks_symbol_name(&doc, name) {
                continue;
            }
            let words = doc.split_whitespace().count();
            if !(MIN_QUERY_WORDS..=MAX_QUERY_WORDS).contains(&words) {
                continue;
            }
            cases.push(EvalCase {
                query: doc,
                file: path.replace('\\', "/"),
                symbol: name.clone(),
                line_start: sym.line_start,
                line_end: sym.line_end,
            });
        }
    }

    cases.sort_by(|a, b| a.file.cmp(&b.file).then(a.line_start.cmp(&b.line_start)));
    sample_evenly(cases, limit)
}

/// Take `limit` items spread across the whole set rather than the first `limit`,
/// so one heavily-documented directory cannot become the entire benchmark.
fn sample_evenly(cases: Vec<EvalCase>, limit: usize) -> Vec<EvalCase> {
    if limit == 0 || cases.len() <= limit {
        return cases;
    }
    let stride = cases.len() as f64 / limit as f64;
    (0..limit)
        .map(|i| cases[((i as f64) * stride) as usize].clone())
        .collect()
}

/// Collect the contiguous comment block immediately above `line_start`, strip
/// comment markers, and join it into one line of prose.
///
/// `line_start` is 1-based, so the line above it is `lines[line_start - 2]`.
/// Scanning stops at the first line that is not a comment, which is what keeps
/// an unrelated comment further up the file out of the query.
fn doc_block_above(lines: &[&str], line_start: u32) -> Option<String> {
    let mut idx = (line_start as usize).checked_sub(2)?;
    let mut collected: Vec<String> = Vec::new();

    for _ in 0..MAX_DOC_SCAN_LINES {
        let raw = lines.get(idx)?.trim();
        // Allow decorators/attributes between the doc and the symbol.
        if collected.is_empty() && (raw.starts_with('#') && raw.contains('[') || raw.is_empty()) {
            if idx == 0 {
                break;
            }
            idx -= 1;
            continue;
        }
        match strip_comment_marker(raw) {
            Some(text) => {
                if !text.is_empty() {
                    collected.push(text);
                }
            }
            None => break,
        }
        if idx == 0 {
            break;
        }
        idx -= 1;
    }

    if collected.is_empty() {
        return None;
    }
    collected.reverse();
    Some(collected.join(" "))
}

/// Strip a line's comment marker. `None` means the line is not a comment, which
/// terminates the upward scan.
fn strip_comment_marker(line: &str) -> Option<String> {
    for marker in ["///", "//!", "//", "--", "*/", "/**", "/*", "*", "#"] {
        if let Some(rest) = line.strip_prefix(marker) {
            // `#` starts a comment in Python/Ruby/shell but an attribute in Rust
            // and a directive in C; those are filtered before we get here.
            return Some(rest.trim().to_owned());
        }
    }
    None
}

/// True when the query gives the answer away by naming the symbol outright.
///
/// Checked against the raw name, the underscore-stripped form, and the
/// space-separated form, so `run_retrieval` is caught by "run_retrieval",
/// "runretrieval" and "run retrieval" alike. Incidental overlap of a single word
/// is left alone — that is ordinary vocabulary, not a giveaway.
fn leaks_symbol_name(query: &str, symbol: &str) -> bool {
    if symbol.len() < 4 {
        return false;
    }
    let q = query.to_lowercase();
    let s = symbol.to_lowercase();
    q.contains(&s) || q.contains(&s.replace('_', "")) || q.contains(&s.replace('_', " "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_common_comment_markers() {
        assert_eq!(strip_comment_marker("/// doc").as_deref(), Some("doc"));
        assert_eq!(strip_comment_marker("// doc").as_deref(), Some("doc"));
        assert_eq!(strip_comment_marker("# doc").as_deref(), Some("doc"));
        assert_eq!(strip_comment_marker("* doc").as_deref(), Some("doc"));
        assert_eq!(strip_comment_marker("let x = 1;"), None);
    }

    #[test]
    fn doc_block_stops_at_the_first_non_comment_line() {
        let lines = vec![
            "// unrelated comment far above",
            "let x = 1;",
            "/// describes the thing",
            "/// across two lines",
            "fn thing() {}",
        ];
        // `fn thing` is on 1-based line 5.
        let doc = doc_block_above(&lines, 5).expect("doc");
        assert_eq!(doc, "describes the thing across two lines");
        assert!(!doc.contains("unrelated"));
    }

    #[test]
    fn doc_block_skips_attributes_between_doc_and_symbol() {
        let lines = vec!["/// the description", "#[inline]", "fn thing() {}"];
        assert_eq!(doc_block_above(&lines, 3).as_deref(), Some("the description"));
    }

    #[test]
    fn no_doc_block_yields_none() {
        let lines = vec!["let x = 1;", "fn thing() {}"];
        assert!(doc_block_above(&lines, 2).is_none());
    }

    #[test]
    fn name_leaks_are_caught_in_every_spelling() {
        assert!(leaks_symbol_name("run_retrieval scores the ranking", "run_retrieval"));
        assert!(leaks_symbol_name("the runretrieval helper", "run_retrieval"));
        assert!(leaks_symbol_name("we run retrieval over the set", "run_retrieval"));
    }

    #[test]
    fn incidental_word_overlap_is_not_a_leak() {
        assert!(!leaks_symbol_name(
            "score every candidate against the ground truth",
            "run_retrieval"
        ));
    }

    #[test]
    fn very_short_names_are_never_treated_as_leaks() {
        // "new"/"run" appear in ordinary prose constantly.
        assert!(!leaks_symbol_name("create a new session for the user", "new"));
    }

    #[test]
    fn even_sampling_spans_the_whole_set_and_is_deterministic() {
        let cases: Vec<EvalCase> = (0..100)
            .map(|i| EvalCase {
                query: format!("q{i}"),
                file: format!("f{i}"),
                symbol: format!("s{i}"),
                line_start: i,
                line_end: i + 10,
            })
            .collect();
        let a = sample_evenly(cases.clone(), 10);
        let b = sample_evenly(cases, 10);
        assert_eq!(a.len(), 10);
        assert_eq!(a[0].symbol, "s0");
        assert_eq!(a[9].symbol, "s90");
        let a_syms: Vec<_> = a.iter().map(|c| c.symbol.clone()).collect();
        let b_syms: Vec<_> = b.iter().map(|c| c.symbol.clone()).collect();
        assert_eq!(a_syms, b_syms, "sampling must be deterministic");
    }

    fn sym(file: &str, name: &str, kind: SymbolKind, start: u32, end: u32) -> SymbolRef {
        SymbolRef {
            file: file.to_owned(),
            name: name.to_owned(),
            kind,
            line_start: start,
            line_end: end,
        }
    }

    fn names(cases: &[EvalCase]) -> Vec<&str> {
        cases.iter().map(|c| c.symbol.as_str()).collect()
    }

    #[test]
    fn a_name_defined_twice_is_never_an_ident_case() {
        let symbols = vec![
            sym("a.rs", "build_index", SymbolKind::Function, 1, 9),
            sym("b.rs", "build_index", SymbolKind::Function, 1, 9),
            sym("a.rs", "only_here", SymbolKind::Function, 20, 29),
        ];
        assert_eq!(names(&unique_ident_cases(&symbols)), ["only_here"]);
    }

    #[test]
    fn uniqueness_counts_every_symbol_kind_and_ignores_case() {
        let symbols = vec![
            sym("a.rs", "Pipeline", SymbolKind::Struct, 1, 9),
            sym("b.rs", "pipeline", SymbolKind::Module, 1, 9),
            sym("c.rs", "WalkerConfig", SymbolKind::Struct, 1, 9),
        ];
        assert_eq!(names(&unique_ident_cases(&symbols)), ["WalkerConfig"]);
    }

    #[test]
    fn ident_name_floor_is_six_characters() {
        let symbols = vec![
            sym("a.rs", "abcde", SymbolKind::Function, 1, 9),
            sym("a.rs", "abcdef", SymbolKind::Function, 20, 29),
        ];
        assert_eq!(names(&unique_ident_cases(&symbols)), ["abcdef"]);
    }

    #[test]
    fn ident_span_floor_is_three_lines() {
        let symbols = vec![
            sym("a.rs", "two_lines", SymbolKind::Function, 10, 11),
            sym("a.rs", "three_lines", SymbolKind::Function, 20, 22),
        ];
        assert_eq!(names(&unique_ident_cases(&symbols)), ["three_lines"]);
    }

    #[test]
    fn ident_covers_only_the_five_named_kinds() {
        let symbols = vec![
            sym("a.rs", "a_function", SymbolKind::Function, 1, 9),
            sym("a.rs", "a_method", SymbolKind::Method, 11, 19),
            sym("a.rs", "AStruct", SymbolKind::Struct, 21, 29),
            sym("a.rs", "AnEnum", SymbolKind::Enum, 31, 39),
            sym("a.rs", "ATrait", SymbolKind::Trait, 41, 49),
            sym("a.rs", "AModule", SymbolKind::Module, 51, 59),
            sym("a.rs", "AnImpl", SymbolKind::Impl, 61, 69),
            sym("a.rs", "AClass", SymbolKind::Class, 71, 79),
        ];
        assert_eq!(
            names(&unique_ident_cases(&symbols)),
            ["a_function", "a_method", "AStruct", "AnEnum", "ATrait"]
        );
    }

    #[test]
    fn ident_gold_is_the_symbols_own_range() {
        let symbols = vec![sym("src/a.rs", "only_here", SymbolKind::Function, 20, 29)];
        let case = &unique_ident_cases(&symbols)[0];
        assert_eq!(case.query, "only_here");
        assert_eq!((case.file.as_str(), case.line_start, case.line_end), ("src/a.rs", 20, 29));
    }

    #[test]
    fn mixed_wraps_each_ident_in_three_separate_templates() {
        let ident = CaseSet::new(
            "ident",
            vec![EvalCase {
                query: "merge_weighted".into(),
                file: "src/query/merger.rs".into(),
                symbol: "merge_weighted".into(),
                line_start: 73,
                line_end: 125,
            }],
        );
        let sets = derive_mixed_set(&ident);
        let queries: Vec<_> = sets.iter().map(|s| (s.name.as_str(), s.cases[0].query.as_str())).collect();
        assert_eq!(
            queries,
            [
                ("mixed:where", "where is merge_weighted defined and what does it do"),
                ("mixed:how", "how does merge_weighted work"),
                ("mixed:show", "show me the implementation of merge_weighted"),
            ]
        );
        for set in &sets {
            assert_eq!((set.cases[0].line_start, set.cases[0].line_end), (73, 125));
        }
    }

    #[test]
    fn item_start_matches_the_name_as_a_whole_word() {
        let src = "// header\npub fn resolve_alias(x: u8) {\n}\n";
        assert!(item_starts_at(src, "resolve_alias", 2));
        assert!(!item_starts_at(src, "resolve", 2), "a prefix is not the item");
        assert!(!item_starts_at(src, "resolve_alias", 1));
        assert!(!item_starts_at(src, "resolve_alias", 99));
        assert!(!item_starts_at(src, "resolve_alias", 0));
    }

    fn para(name: &str, file: &str, start: u32) -> ParaCase {
        ParaCase {
            query: format!("do {name}"),
            file: file.to_owned(),
            name: name.to_owned(),
            start,
            end: start + 9,
        }
    }

    #[test]
    fn a_shifted_paraphrase_target_is_stale_not_a_miss() {
        let moved = "\n\nfn target_one() {}\n";
        let intact = "fn target_two() {}\n";
        let cases = vec![para("target_one", "a.rs", 1), para("target_two", "b.rs", 1)];
        let set = split_stale(cases, "/repo", |path| match path {
            "/repo/a.rs" => Some(moved.to_owned()),
            "/repo/b.rs" => Some(intact.to_owned()),
            _ => None,
        });
        assert_eq!(names(&set.stale), ["target_one"]);
        assert_eq!(names(&set.cases), ["target_two"]);
        assert_eq!(set.cases[0].file, "/repo/b.rs");
    }

    #[test]
    fn a_missing_paraphrase_file_is_stale() {
        let set = split_stale(vec![para("gone", "gone.rs", 1)], "/repo", |_| None);
        assert!(set.cases.is_empty());
        assert_eq!(set.stale.len(), 1);
    }

    #[test]
    fn the_embedded_fixture_parses_and_uses_relative_paths() {
        let fixture = parse_para_fixture();
        assert_eq!(fixture.cases.len(), 60);
        assert!(fixture.cases.iter().all(|c| !c.file.starts_with('/')));
        assert!(!fixture.model.is_empty() && !fixture.prompt.is_empty());
    }

    #[test]
    fn sampling_returns_everything_when_under_the_limit() {
        let cases: Vec<EvalCase> = (0..3)
            .map(|i| EvalCase {
                query: "q".into(),
                file: "f".into(),
                symbol: format!("s{i}"),
                line_start: i,
                line_end: i,
            })
            .collect();
        assert_eq!(sample_evenly(cases, 10).len(), 3);
    }
}

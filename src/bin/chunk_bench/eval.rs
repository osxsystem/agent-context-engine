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

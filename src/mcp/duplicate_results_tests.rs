//! Identical code in several files must reach the output as one block with a
//! note naming the other copies. Each test indexes a real repo through the
//! engine against a local embeddings endpoint, so no quota is spent, and reads
//! the answer a client would get.

use super::availability_tests::Harness;
use super::*;

fn function(name: &str, salt: &str, lines: u32) -> String {
    let mut text = format!("pub fn {name}() -> u32 {{\n");
    for i in 0..lines {
        text.push_str(&format!("    let value_{i} = {i} + {salt}_{i}_marker;\n"));
    }
    text.push_str("    0\n}\n");
    text
}

/// The shared chunk is two short functions, so a span-narrowing reranker can
/// pick one range per function. The unique function is long enough that the
/// chunker gives it a chunk of its own.
const FUNCTION_LINES: u32 = 43;

fn fixture() -> Vec<(&'static str, String)> {
    let shared = format!(
        "{}\n{}",
        function("shared_setup", "shared", 18),
        function("shared_teardown", "shared", 18)
    );
    ["a", "b", "c"]
        .into_iter()
        .map(|name| {
            (
                match name {
                    "a" => "copies/a.rs",
                    "b" => "copies/b.rs",
                    _ => "copies/c.rs",
                },
                format!(
                    "{shared}\n{}",
                    function(&format!("unique_{name}"), name, 40)
                ),
            )
        })
        .collect()
}

async fn indexed_copies() -> Harness {
    let files = fixture();
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(n, t)| (*n, t.as_str())).collect();
    Harness::indexed_with(&borrowed).await
}

fn path(h: &Harness, relative: &str) -> String {
    std::path::Path::new(&h.repo)
        .join(relative)
        .to_string_lossy()
        .into_owned()
}

async fn answer(h: &Harness) -> String {
    run_codebase_retrieval(
        &h.home,
        &h.home,
        &h.engine,
        &h.repo_dbs,
        &h.settings,
        "set up the shared module",
        None,
        &h.repo,
        crate::vector::Scope::Code,
    )
    .await
}

fn headers(answer: &str) -> Vec<&str> {
    answer
        .lines()
        .filter(|line| line.contains("#L") && !line.starts_with(' '))
        .collect()
}

fn shared_header(h: &Harness) -> String {
    format!(
        "{}#L1-{FUNCTION_LINES} (+2 identical: {}, {})",
        path(h, "copies/a.rs"),
        path(h, "copies/b.rs"),
        path(h, "copies/c.rs"),
    )
}

fn assert_shared_block_is_one_and_unique_blocks_are_kept(h: &Harness, answer: &str) {
    let headers = headers(answer);
    assert_eq!(
        headers
            .iter()
            .filter(|header| header.contains("identical"))
            .copied()
            .collect::<Vec<_>>(),
        vec![shared_header(h).as_str()],
        "{answer}"
    );
    for name in ["a", "b", "c"] {
        let own = path(h, &format!("copies/{name}.rs"));
        let unique_block = headers.iter().any(|header| {
            header.starts_with(&format!("{own}#L"))
                && !header.starts_with(&format!("{own}#L1-"))
                && !header.contains("identical")
        });
        assert!(
            unique_block,
            "unique body of {name}.rs is a block: {answer}"
        );
        let from_the_top = headers
            .iter()
            .filter(|header| header.starts_with(&format!("{own}#L1-")))
            .count();
        assert_eq!(
            from_the_top,
            usize::from(name == "a"),
            "{name}.rs: {answer}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn answer_collapses_copies_and_keeps_each_files_unique_chunk() {
    let h = indexed_copies().await;

    let answer = answer(&h).await;

    assert_shared_block_is_one_and_unique_blocks_are_kept(&h, &answer);
}

#[tokio::test(flavor = "multi_thread")]
async fn engine_result_and_pre_rerank_result_carry_the_copies() {
    let h = indexed_copies().await;
    let client = VoyageClient::shared_for_provider(
        crate::embedding::voyage::Provider::parse(&h.settings.embedding.provider),
        h.settings.embedding.model.clone(),
        h.settings.embedding.api_keys.clone(),
        h.settings.embedding.voyage_base_url.as_deref(),
        h.settings.embedding.dimensions,
    )
    .unwrap();

    let result = crate::query::engine::run_query_with_filters(
        "set up the shared module",
        30,
        Some(&h.repo),
        &client,
        &h.engine,
        &h.repo_dbs,
        h.settings.llm.rerank_min_prune_lines,
        &RerankProvider::from_settings(&h.settings.llm),
        Duration::from_secs(5),
        false,
        0,
        0,
        false,
        None,
    )
    .await
    .unwrap();

    let expected = vec![path(&h, "copies/b.rs"), path(&h, "copies/c.rs")];
    for (stage, chunks) in [
        ("results", &result.results),
        ("pre_rerank_results", &result.pre_rerank_results),
    ] {
        let shared: Vec<_> = chunks.iter().filter(|c| !c.duplicates.is_empty()).collect();
        assert_eq!(shared.len(), 1, "{stage}");
        assert_eq!(shared[0].file, path(&h, "copies/a.rs"), "{stage}");
        assert_eq!(shared[0].duplicates, expected, "{stage}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn note_survives_when_the_file_is_gone_and_content_comes_from_the_index() {
    let h = indexed_copies().await;
    std::fs::remove_file(std::path::Path::new(&h.repo).join("copies/a.rs")).unwrap();

    let answer = answer(&h).await;

    assert!(answer.contains(&shared_header(&h)), "{answer}");
}

#[tokio::test(flavor = "multi_thread")]
async fn note_survives_when_the_reranker_selects_line_ranges() {
    use axum::{Json, Router, routing::post};
    let mut h = indexed_copies().await;
    let app = Router::new().route(
        "/v1/systemone",
        post(|Json(body): Json<serde_json::Value>| async move {
            let answers: serde_json::Map<String, serde_json::Value> = body["questions"]
                .as_object()
                .unwrap()
                .keys()
                .map(|key| {
                    (
                        key.clone(),
                        serde_json::json!({"type": "noul", "noul": 0.9}),
                    )
                })
                .collect();
            Json(serde_json::json!({"answers": answers}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    h.settings.llm.provider = "custom".to_owned();
    h.settings.llm.openai_base_url = Some("http://127.0.0.1:9/v1".to_owned());
    h.settings.llm.rerank_provider = Some("jev".to_owned());
    h.settings.llm.jev_api_keys = vec!["duplicate-test-key".to_owned()];
    h.settings.llm.jev_base_url = Some(format!("http://{addr}"));

    let answer = answer(&h).await;

    let note = format!(
        " (+2 identical: {}, {})",
        path(&h, "copies/b.rs"),
        path(&h, "copies/c.rs")
    );
    let noted: Vec<&str> = headers(&answer)
        .into_iter()
        .filter(|header| header.starts_with(&path(&h, "copies/a.rs")) && header.contains(&note))
        .collect();
    assert_eq!(noted.len(), 2, "one note per selected range: {answer}");
}

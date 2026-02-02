use anyhow::Result;
use ce_core::model::{FragKind, Fragment, Span};
use ce_store::Db;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn sample_fragment(path: &str, id: &str) -> Fragment {
    Fragment {
        id: id.to_string(),
        ast_hash: format!("ast-{id}"),
        file: PathBuf::from(path),
        kind: FragKind::Function,
        symbol: Some("Widget".to_string()),
        span: Span {
            start_byte: 0,
            end_byte: 42,
            start_line: 0,
            start_col: 0,
            end_line: 2,
            end_col: 0,
        },
        signature: "fn widget()".to_string(),
        body: "fn widget() { println!(\"ok\"); }".to_string(),
        doc: String::new(),
        retrieval_text: "widget helper".to_string(),
        refs: Vec::new(),
    }
}

fn send_json(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    msg: &Value,
) -> Result<Value> {
    let line = serde_json::to_string(msg)?;
    writeln!(stdin, "{line}")?;
    stdin.flush()?;

    let mut resp = String::new();
    stdout.read_line(&mut resp)?;
    let resp = resp.trim();
    Ok(serde_json::from_str(resp)?)
}

#[test]
fn mcp_initialize_and_pack() -> Result<()> {
    let dir = TempDir::new()?;
    let db_path = dir.path().join("index.sqlite");
    let hnsw_dir = dir.path().join("hnsw");
    std::fs::create_dir_all(&hnsw_dir)?;

    let db = Db::open(&db_path)?;
    let file_id = db.upsert_file("src/foo.rs", "rust", 42, 0, "hash-foo")?;
    let frag = sample_fragment("src/foo.rs", "frag-widget");
    let rowid = db.upsert_fragment(file_id, &frag)?;
    db.replace_symbols_for_fragment(rowid, &frag)?;

    let bin = env!("CARGO_BIN_EXE_ce-mcp");
    let cache_dir = repo_root().join(".fastembed_cache");
    let mut child = Command::new(bin)
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "--hnsw-dir",
            hnsw_dir.to_str().expect("hnsw dir"),
            "--repo",
            dir.path().to_str().expect("repo path"),
        ])
        .env("FASTEMBED_CACHE_DIR", cache_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let init_resp = send_json(
        &mut stdin,
        &mut stdout,
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    )?;
    assert_eq!(init_resp["result"]["protocolVersion"], "2025-03-26");

    let list_resp = send_json(
        &mut stdin,
        &mut stdout,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )?;
    let tools = list_resp["result"]["tools"].as_array().expect("tools list");
    assert!(tools.iter().any(|tool| tool["name"] == "context.pack"));
    assert!(tools.iter().any(|tool| tool["name"] == "memory.recall"));

    let pack_resp = send_json(
        &mut stdin,
        &mut stdout,
        &json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"tools/call",
            "params":{
                "name":"context.pack",
                "arguments":{
                    "task":"widget helper",
                    "format":"json",
                    "strategy_overrides":{
                        "semantic_k":0,
                        "lexical_k":8,
                        "candidate_pool_limit":8,
                        "graph_expand":false,
                        "signals_enabled":false,
                        "include_api_summaries":false
                    }
                }
            }
        }),
    )?;
    let text = pack_resp["result"]["content"][0]["text"]
        .as_str()
        .expect("pack text");
    let pack: Value = serde_json::from_str(text)?;
    let items = pack["items"].as_array().expect("pack items");
    assert!(!items.is_empty());
    assert_eq!(items[0]["path"], "src/foo.rs");

    let remember_resp = send_json(
        &mut stdin,
        &mut stdout,
        &json!({
            "jsonrpc":"2.0",
            "id":4,
            "method":"tools/call",
            "params":{
                "name":"memory.remember",
                "arguments":{
                    "content":"Always run tests before committing",
                    "tags":["process","testing"]
                }
            }
        }),
    )?;
    let remember_text = remember_resp["result"]["content"][0]["text"]
        .as_str()
        .expect("remember text");
    let remember_json: Value = serde_json::from_str(remember_text)?;
    assert!(remember_json["items"].as_array().is_some());

    let recall_resp = send_json(
        &mut stdin,
        &mut stdout,
        &json!({
            "jsonrpc":"2.0",
            "id":5,
            "method":"tools/call",
            "params":{
                "name":"memory.recall",
                "arguments":{
                    "query":"tests before committing",
                    "k":5,
                    "token_budget":400
                }
            }
        }),
    )?;
    let recall_text = recall_resp["result"]["content"][0]["text"]
        .as_str()
        .expect("recall text");
    let recall_json: Value = serde_json::from_str(recall_text)?;
    let recall_items = recall_json["items"].as_array().expect("recall items");
    assert!(!recall_items.is_empty());

    let _ = child.kill();
    let _ = child.wait();

    Ok(())
}

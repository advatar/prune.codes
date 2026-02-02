use std::fs;
use tempfile::TempDir;

// We test the integration generator by invoking the same helpers used by the CLI.
// This keeps it repo-local and avoids needing any external agent installed.

#[test]
fn integrate_opencode_writes_valid_json() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path();
    // minimal repo layout
    fs::create_dir_all(repo.join(".ce")).unwrap();
    fs::write(repo.join(".ce/index.sqlite"), "").unwrap();
    fs::create_dir_all(repo.join(".ce/hnsw")).unwrap();

    ce_cli::integrations::cmd_integrate(
        repo.to_str().unwrap(),
        ce_cli::integrations::Agent::Opencode,
        false,
        false,
        false,
        None,
        false,
    )
    .unwrap();

    let opencode_path = repo.join("opencode.json");
    let contents = fs::read_to_string(opencode_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert!(v.get("mcp").is_some());
    assert!(v["mcp"].get("prune").is_some());
}

#[test]
fn integrate_claude_writes_mcp_and_skill() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".ce")).unwrap();
    fs::write(repo.join(".ce/index.sqlite"), "").unwrap();
    fs::create_dir_all(repo.join(".ce/hnsw")).unwrap();

    ce_cli::integrations::cmd_integrate(
        repo.to_str().unwrap(),
        ce_cli::integrations::Agent::Claude,
        false,
        false,
        false,
        None,
        false,
    )
    .unwrap();

    assert!(repo.join("CLAUDE.md").exists());
    assert!(repo.join(".mcp.json").exists());
    assert!(repo.join(".claude/skills/prune-context/SKILL.md").exists());
}

#[test]
fn integrate_codex_writes_agents_md() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".ce")).unwrap();
    fs::write(repo.join(".ce/index.sqlite"), "").unwrap();
    fs::create_dir_all(repo.join(".ce/hnsw")).unwrap();

    ce_cli::integrations::cmd_integrate(
        repo.to_str().unwrap(),
        ce_cli::integrations::Agent::Codex,
        false,
        false,
        false,
        None,
        false,
    )
    .unwrap();
    assert!(repo.join("AGENTS.md").exists());
}

#[test]
fn integrate_opencode_with_context7_adds_mcp() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".ce")).unwrap();
    fs::write(repo.join(".ce/index.sqlite"), "").unwrap();
    fs::create_dir_all(repo.join(".ce/hnsw")).unwrap();

    ce_cli::integrations::cmd_integrate(
        repo.to_str().unwrap(),
        ce_cli::integrations::Agent::Opencode,
        false,
        true,
        false,
        None,
        false,
    )
    .unwrap();

    let opencode_path = repo.join("opencode.json");
    let contents = fs::read_to_string(opencode_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert!(v["mcp"].get("context7").is_some());
}

#[test]
fn integrate_opencode_with_cortex_adds_mcp() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".ce")).unwrap();
    fs::write(repo.join(".ce/index.sqlite"), "").unwrap();
    fs::create_dir_all(repo.join(".ce/hnsw")).unwrap();

    let dist = repo.join(".prune/vendors/cortex/dist");
    fs::create_dir_all(&dist).unwrap();
    fs::write(dist.join("mcp-server.js"), "").unwrap();

    ce_cli::integrations::cmd_integrate(
        repo.to_str().unwrap(),
        ce_cli::integrations::Agent::Opencode,
        false,
        false,
        true,
        None,
        false,
    )
    .unwrap();

    let opencode_path = repo.join("opencode.json");
    let contents = fs::read_to_string(opencode_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert!(v["mcp"].get("cortex").is_some());
    assert_eq!(
        v["mcp"]["cortex"]["command"][0],
        serde_json::Value::String(".prune/bin/cortex-mcp".into())
    );
    assert!(repo.join(".prune/bin/cortex-mcp").exists());
}

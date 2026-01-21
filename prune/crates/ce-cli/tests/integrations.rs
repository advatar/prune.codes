use std::fs;
use std::path::Path;

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

    ce_cli::integrations::cmd_integrate(repo.to_str().unwrap(), ce_cli::integrations::Agent::Opencode, false, false).unwrap();

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

    ce_cli::integrations::cmd_integrate(repo.to_str().unwrap(), ce_cli::integrations::Agent::Claude, false, false).unwrap();

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

    ce_cli::integrations::cmd_integrate(repo.to_str().unwrap(), ce_cli::integrations::Agent::Codex, false, false).unwrap();
    assert!(repo.join("AGENTS.md").exists());
}

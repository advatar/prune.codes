use std::fs;
use std::path::Path;

use ce_cli::integrations::{cmd_doctor, cmd_integrate, Agent};
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
    )
    .unwrap();
    assert!(repo.join("AGENTS.md").exists());
}

fn install(repo: &Path, agent: Agent, dry_run: bool) -> anyhow::Result<()> {
    cmd_integrate(repo.to_str().unwrap(), agent, false, dry_run)
}

#[test]
fn codex_installs_the_bundled_portable_skill() {
    let tmp = TempDir::new().unwrap();
    install(tmp.path(), Agent::Codex, false).unwrap();
    assert_eq!(
        fs::read_to_string(tmp.path().join(".agents/skills/prune-context/SKILL.md")).unwrap(),
        include_str!("../../../integrations/skills/prune-context/SKILL.md")
    );
}

#[test]
fn all_preserves_existing_instructions_and_customized_skills() {
    let tmp = TempDir::new().unwrap();
    let files = [
        "AGENTS.md",
        "CLAUDE.md",
        ".agents/skills/prune-context/SKILL.md",
        ".claude/skills/prune-context/SKILL.md",
    ];
    for name in files {
        let path = tmp.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("custom content for {name}\n")).unwrap();
    }
    install(tmp.path(), Agent::All, false).unwrap();
    for name in files {
        assert_eq!(
            fs::read_to_string(tmp.path().join(name)).unwrap(),
            format!("custom content for {name}\n")
        );
    }
}

#[test]
fn mcp_configuration_preserves_other_servers_and_user_settings() {
    for (agent, file, key) in [
        (Agent::Claude, ".mcp.json", "mcpServers"),
        (Agent::Opencode, "opencode.json", "mcp"),
    ] {
        let tmp = TempDir::new().unwrap();
        let original = serde_json::json!({key: {"other": {"command": "user-tool"}}, "user-setting": {"nested": true}});
        fs::write(tmp.path().join(file), original.to_string()).unwrap();
        install(tmp.path(), agent, false).unwrap();
        let actual: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(tmp.path().join(file)).unwrap()).unwrap();
        assert_eq!(actual[key]["other"], original[key]["other"]);
        assert_eq!(actual["user-setting"], original["user-setting"]);
        assert!(actual[key].get("prune").is_some());
    }
}

#[test]
fn existing_prune_configuration_is_preserved() {
    for (agent, file, key) in [
        (Agent::Claude, ".mcp.json", "mcpServers"),
        (Agent::Opencode, "opencode.json", "mcp"),
    ] {
        let tmp = TempDir::new().unwrap();
        let original =
            serde_json::json!({key: {"prune": {"command": "custom-ce", "disabled": true}}});
        fs::write(tmp.path().join(file), original.to_string()).unwrap();
        install(tmp.path(), agent, false).unwrap();
        let actual: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(tmp.path().join(file)).unwrap()).unwrap();
        assert_eq!(actual[key]["prune"], original[key]["prune"]);
    }
}

#[test]
fn malformed_or_non_object_configuration_is_rejected_before_writes() {
    for (file, invalid_values) in [
        (".mcp.json", vec!["{", "[]", "null", "{\"mcpServers\": []}"]),
        ("opencode.json", vec!["{", "[]", "null", "{\"mcp\": false}"]),
    ] {
        for invalid in invalid_values {
            let tmp = TempDir::new().unwrap();
            fs::write(tmp.path().join(file), invalid).unwrap();
            assert!(
                install(tmp.path(), Agent::All, false).is_err(),
                "accepted {file}: {invalid}"
            );
            assert_eq!(fs::read_to_string(tmp.path().join(file)).unwrap(), invalid);
            assert_eq!(
                fs::read_dir(tmp.path()).unwrap().count(),
                1,
                "partial installation for {file}"
            );
        }
    }
}

#[test]
fn dry_run_does_not_create_files_or_directories() {
    let tmp = TempDir::new().unwrap();
    install(tmp.path(), Agent::All, true).unwrap();
    assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
}

#[test]
fn repeated_installation_is_byte_for_byte_idempotent() {
    let tmp = TempDir::new().unwrap();
    install(tmp.path(), Agent::All, false).unwrap();
    let files = [
        "AGENTS.md",
        "CLAUDE.md",
        ".mcp.json",
        "opencode.json",
        ".agents/skills/prune-context/SKILL.md",
        ".claude/skills/prune-context/SKILL.md",
    ];
    let before: Vec<_> = files
        .iter()
        .map(|name| fs::read(tmp.path().join(name)).unwrap())
        .collect();
    install(tmp.path(), Agent::All, false).unwrap();
    for (name, expected) in files.iter().zip(before) {
        assert_eq!(fs::read(tmp.path().join(name)).unwrap(), expected);
    }
}

#[test]
fn codex_doctor_requires_a_skill_as_well_as_instructions() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".ce/hnsw")).unwrap();
    fs::write(tmp.path().join(".ce/index.sqlite"), "").unwrap();
    fs::write(tmp.path().join("AGENTS.md"), "existing instructions").unwrap();
    assert!(cmd_doctor(tmp.path().to_str().unwrap(), Agent::Codex).is_err());
    install(tmp.path(), Agent::Codex, false).unwrap();
    cmd_doctor(tmp.path().to_str().unwrap(), Agent::Codex).unwrap();
}

#[cfg(unix)]
#[test]
fn installer_rejects_symlinks_that_would_write_outside_the_repo() {
    use std::os::unix::fs::symlink;
    let tmp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    symlink(outside.path(), tmp.path().join(".agents")).unwrap();
    assert!(install(tmp.path(), Agent::Codex, false).is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert!(!tmp.path().join("AGENTS.md").exists());
}

use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use serde_json::json;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// Supported agent clients.
///
/// We keep integration logic in Prune (this repo) and avoid forking any agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Agent {
    Codex,
    Opencode,
    Claude,
    /// Integrate or check all supported clients.
    All,
}

const TEMPLATE_AGENTS: &str = include_str!("../../../integrations/templates/AGENTS.md");
const TEMPLATE_CLAUDE: &str = include_str!("../../../integrations/templates/CLAUDE.md");
const TEMPLATE_SKILL: &str = include_str!("../../../integrations/skills/prune-context/SKILL.md");

pub fn cmd_integrate(repo: &str, agent: Agent, write_global: bool, dry_run: bool) -> Result<()> {
    let repo_path = fs::canonicalize(repo).with_context(|| format!("repo not found: {repo}"))?;
    if !repo_path.is_dir() {
        return Err(anyhow!("repo is not a directory: {repo}"));
    }

    // Validate every selected destination/config before creating any files.
    let mut writes = Vec::new();
    match agent {
        Agent::Codex => plan_codex(&repo_path, write_global, &mut writes)?,
        Agent::Opencode => plan_opencode(&repo_path, &mut writes)?,
        Agent::Claude => plan_claude(&repo_path, &mut writes)?,
        Agent::All => {
            plan_codex(&repo_path, write_global, &mut writes)?;
            plan_opencode(&repo_path, &mut writes)?;
            plan_claude(&repo_path, &mut writes)?;
        }
    }
    for write in writes {
        write.apply(dry_run)?;
    }
    Ok(())
}

pub fn cmd_doctor(repo: &str, agent: Agent) -> Result<()> {
    let repo_path = PathBuf::from(repo);
    if !repo_path.exists() {
        return Err(anyhow!("repo not found: {repo}"));
    }

    let mut problems: Vec<String> = Vec::new();
    let ce_dir = repo_path.join(".ce");
    let db = ce_dir.join("index.sqlite");
    let hnsw = ce_dir.join("hnsw");
    if !db.exists() {
        problems.push("Missing .ce/index.sqlite (run: ce index --repo . --db .ce/index.sqlite --hnsw-dir .ce/hnsw)".into());
    }
    if !hnsw.exists() {
        problems.push(
            "Missing .ce/hnsw (run: ce index --repo . --db .ce/index.sqlite --hnsw-dir .ce/hnsw)"
                .into(),
        );
    }

    let check_codex = agent == Agent::Codex || agent == Agent::All;
    let check_opencode = agent == Agent::Opencode || agent == Agent::All;
    let check_claude = agent == Agent::Claude || agent == Agent::All;

    if check_codex {
        if !repo_path.join("AGENTS.md").exists() {
            problems.push("Missing AGENTS.md (run: ce integrate --agent codex --repo .)".into());
        }
        if !repo_path
            .join(".agents/skills/prune-context/SKILL.md")
            .is_file()
        {
            problems.push(
                "Missing .agents/skills/prune-context/SKILL.md (run: ce integrate --agent codex --repo .)"
                    .into(),
            );
        }
        // Global config is best-effort; we do not error if absent.
    }

    if check_opencode {
        if !repo_path.join("AGENTS.md").exists() {
            problems.push("Missing AGENTS.md (run: ce integrate --agent opencode --repo .)".into());
        }
        if !repo_path.join("opencode.json").exists() {
            problems
                .push("Missing opencode.json (run: ce integrate --agent opencode --repo .)".into());
        }
        if !repo_path.join("CLAUDE.md").exists() {
            problems.push("Missing CLAUDE.md (recommended for OpenCode compatibility; run: ce integrate --agent opencode --repo .)".into());
        }
        let skill = repo_path
            .join(".claude")
            .join("skills")
            .join("prune-context")
            .join("SKILL.md");
        if !skill.exists() {
            problems.push("Missing .claude/skills/prune-context/SKILL.md (recommended; run: ce integrate --agent opencode --repo .)".into());
        }
    }

    if check_claude {
        if !repo_path.join("CLAUDE.md").exists() {
            problems.push("Missing CLAUDE.md (run: ce integrate --agent claude --repo .)".into());
        }
        if !repo_path.join(".mcp.json").exists() {
            problems.push("Missing .mcp.json (run: ce integrate --agent claude --repo .)".into());
        }
        let skill = repo_path
            .join(".claude")
            .join("skills")
            .join("prune-context")
            .join("SKILL.md");
        if !skill.exists() {
            problems.push(
                "Missing .claude/skills/prune-context/SKILL.md (run: ce integrate --agent claude --repo .)"
                    .into(),
            );
        }
    }

    if problems.is_empty() {
        println!("doctor: OK (repo looks ready)");
        return Ok(());
    }

    println!("doctor: found {} issue(s):", problems.len());
    for p in problems {
        println!("- {p}");
    }
    Err(anyhow!("repo is not ready"))
}

// Instructions and skills are create-only; existing customizations require manual merging.
// MCP configs are merged only after successful parsing, preserving existing server entries.
struct PlannedWrite {
    root: PathBuf,
    relative: PathBuf,
    contents: String,
    before: Option<String>,
}

impl PlannedWrite {
    fn apply(self, dry_run: bool) -> Result<()> {
        let path = safe_destination(&self.root, &self.relative)?;
        if dry_run {
            println!("[dry-run] would write {}", path.display());
            return Ok(());
        }
        // Refuse to overwrite edits made between planning and writing.
        if read_optional(&path)? != self.before {
            return Err(anyhow!(
                "{} changed during integration; retry after reviewing it",
                path.display()
            ));
        }
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| anyhow!("missing parent directory"))?,
        )?;
        safe_destination(&self.root, &self.relative)?;
        write_atomically(&path, self.before.as_deref(), |file| {
            file.write_all(self.contents.as_bytes())
        })?;
        println!("Wrote {}", path.display());
        Ok(())
    }
}

// Keep the previous config intact if writing or syncing the new contents fails.
// Each replacement is atomic; an entire multi-client installation is not.
fn write_atomically(
    path: &Path,
    expected: Option<&str>,
    write_contents: impl FnOnce(&mut fs::File) -> std::io::Result<()>,
) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("missing parent directory"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    if expected.is_some() {
        let permissions = fs::metadata(path)?.permissions();
        if permissions.readonly() {
            return Err(anyhow!(
                "refusing to replace read-only config: {}",
                path.display()
            ));
        }
        temporary.as_file().set_permissions(permissions)?;
    }
    write_contents(temporary.as_file_mut())?;
    temporary.as_file().sync_all()?;
    if read_optional(path)?.as_deref() != expected {
        return Err(anyhow!(
            "{} changed during integration; retry after reviewing it",
            path.display()
        ));
    }
    let result = if expected.is_some() {
        temporary.persist(path)
    } else {
        temporary.persist_noclobber(path)
    };
    result
        .map_err(|error| error.error)
        .with_context(|| format!("cannot install {}", path.display()))?;
    Ok(())
}

// A project-relative symlink must not redirect installer writes outside the repo.
// Canonicalizing only the caller-selected root also permits ordinary /tmp aliases.
fn safe_destination(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(anyhow!(
                "integration destination must be relative to its root"
            ));
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(anyhow!(
                    "refusing symlinked integration destination: {} (merge manually)",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    if current.exists() && !current.is_file() {
        return Err(anyhow!(
            "integration destination is not a file: {}",
            current.display()
        ));
    }
    Ok(current)
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn plan_missing(
    root: &Path,
    relative: &str,
    contents: &str,
    writes: &mut Vec<PlannedWrite>,
) -> Result<()> {
    let relative = PathBuf::from(relative);
    let path = safe_destination(root, &relative)?;
    if path.exists() {
        println!(
            "Preserved {} (merge updates manually if needed)",
            path.display()
        );
    } else if !writes
        .iter()
        .any(|write| write.root == root && write.relative == relative)
    {
        writes.push(PlannedWrite {
            root: root.into(),
            relative,
            contents: contents.into(),
            before: None,
        });
    }
    Ok(())
}

fn plan_json_config(
    root: &Path,
    relative: &str,
    map_key: &str,
    server: serde_json::Value,
    schema: Option<&str>,
    writes: &mut Vec<PlannedWrite>,
) -> Result<()> {
    let relative = PathBuf::from(relative);
    let path = safe_destination(root, &relative)?;
    let before = read_optional(&path)?;
    let mut config: serde_json::Value = serde_json::from_str(before.as_deref().unwrap_or("{}"))
        .with_context(|| format!("invalid JSON in {}; file left unchanged", path.display()))?;
    let original = config.clone();
    let object = config
        .as_object_mut()
        .ok_or_else(|| anyhow!("{} must contain a JSON object", path.display()))?;
    let servers = object
        .entry(map_key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow!("{}: {map_key} must be an object", path.display()))?;
    servers.entry("prune").or_insert(server);
    if let Some(schema) = schema {
        object.entry("$schema").or_insert_with(|| json!(schema));
    }
    if config != original {
        writes.push(PlannedWrite {
            root: root.into(),
            relative,
            contents: format!("{}\n", serde_json::to_string_pretty(&config)?),
            before,
        });
    } else {
        println!("Preserved {} (Prune already configured)", path.display());
    }
    Ok(())
}

fn plan_codex(repo: &Path, write_global: bool, writes: &mut Vec<PlannedWrite>) -> Result<()> {
    plan_missing(repo, "AGENTS.md", TEMPLATE_AGENTS, writes)?;
    plan_missing(
        repo,
        ".agents/skills/prune-context/SKILL.md",
        TEMPLATE_SKILL,
        writes,
    )?;
    if write_global {
        let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME not set"))?;
        let root = fs::canonicalize(home).context("cannot locate the Codex config home")?;
        plan_codex_config(&root, repo, writes)?;
    }
    Ok(())
}

fn plan_codex_config(root: &Path, repo: &Path, writes: &mut Vec<PlannedWrite>) -> Result<()> {
    let relative = PathBuf::from(".codex/config.toml");
    let path = safe_destination(root, &relative)?;
    let before = read_optional(&path)?;
    let existing = before.as_deref().unwrap_or("");
    let parsed: toml::Value = toml::from_str(existing)
        .with_context(|| format!("invalid TOML in {}; file left unchanged", path.display()))?;
    if let Some(servers) = parsed.get("mcp_servers") {
        let servers = servers
            .as_table()
            .ok_or_else(|| anyhow!("{}: mcp_servers must be a table", path.display()))?;
        if servers.contains_key("prune") {
            println!("Preserved {} (Prune already configured)", path.display());
            return Ok(());
        }
    }
    let repo_string = repo
        .to_str()
        .ok_or_else(|| anyhow!("Codex MCP repo path must be valid UTF-8"))?;
    let repo_literal = toml::Value::String(repo_string.into());
    let contents = format!("{existing}\n[mcp_servers.prune]\ncommand = \"ce\"\nargs = [\"mcp\", \"serve\", \"--repo\", {repo_literal}]\n");
    // Inline TOML tables can prohibit appending a child; refuse instead of rewriting user config.
    let _: toml::Value = toml::from_str(&contents).with_context(|| {
        format!(
            "cannot append Prune to {}; merge the server entry manually",
            path.display()
        )
    })?;
    writes.push(PlannedWrite {
        root: root.into(),
        relative,
        contents,
        before,
    });
    Ok(())
}

fn plan_opencode(repo: &Path, writes: &mut Vec<PlannedWrite>) -> Result<()> {
    plan_missing(repo, "AGENTS.md", TEMPLATE_AGENTS, writes)?;
    plan_missing(repo, "CLAUDE.md", TEMPLATE_CLAUDE, writes)?;
    plan_missing(
        repo,
        ".claude/skills/prune-context/SKILL.md",
        TEMPLATE_SKILL,
        writes,
    )?;
    plan_json_config(
        repo,
        "opencode.json",
        "mcp",
        json!({
            "type": "local",
            "command": ["ce", "mcp", "serve", "--repo", "."],
            "enabled": true
        }),
        Some("https://opencode.ai/config.json"),
        writes,
    )
}

fn plan_claude(repo: &Path, writes: &mut Vec<PlannedWrite>) -> Result<()> {
    plan_missing(repo, "CLAUDE.md", TEMPLATE_CLAUDE, writes)?;
    plan_missing(
        repo,
        ".claude/skills/prune-context/SKILL.md",
        TEMPLATE_SKILL,
        writes,
    )?;
    plan_json_config(
        repo,
        ".mcp.json",
        "mcpServers",
        json!({
            "command": "ce",
            "args": ["mcp", "serve", "--repo", "."]
        }),
        None,
        writes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn interrupted_write_preserves_original_and_removes_temporary_file() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("config.json");
        let original = "{\"mcpServers\":{\"other\":{\"command\":\"user-tool\"}}}";
        fs::write(&path, original).unwrap();
        let result = write_atomically(&path, Some(original), |file| {
            file.write_all(b"partial replacement")?;
            Err(std::io::Error::other("injected write failure"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replacement_retains_private_permissions_and_respects_readonly() {
        use std::os::unix::fs::PermissionsExt;
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomically(&path, Some("old"), |file| file.write_all(b"new")).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        assert!(write_atomically(&path, Some("new"), |file| file.write_all(b"forbidden")).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn global_toml_preserves_comments_and_escapes_repo_paths() {
        let home = TempDir::new().unwrap();
        let config = home.path().join(".codex/config.toml");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        let original = "# [mcp_servers.prune] is only a comment\nmodel = \"custom\"\n[mcp_servers.other]\ncommand = \"other\"\n";
        fs::write(&config, original).unwrap();
        let repo = Path::new("/repo/quote\"and\\slash");
        let mut writes = Vec::new();
        plan_codex_config(home.path(), repo, &mut writes).unwrap();
        assert_eq!(writes.len(), 1);
        writes.pop().unwrap().apply(false).unwrap();
        let actual = fs::read_to_string(&config).unwrap();
        assert!(actual.starts_with(original));
        let parsed: toml::Value = toml::from_str(&actual).unwrap();
        assert_eq!(
            parsed["mcp_servers"]["prune"]["args"][3].as_str(),
            repo.to_str()
        );
        plan_codex_config(home.path(), repo, &mut writes).unwrap();
        assert!(writes.is_empty());
        assert_eq!(fs::read_to_string(config).unwrap(), actual);
    }

    #[test]
    fn global_toml_rejects_invalid_or_sealed_tables_without_writes() {
        for contents in [
            "[",
            "mcp_servers = false",
            "mcp_servers = { other = { command = 'custom' } }",
        ] {
            let home = TempDir::new().unwrap();
            let config = home.path().join(".codex/config.toml");
            fs::create_dir_all(config.parent().unwrap()).unwrap();
            fs::write(&config, contents).unwrap();
            let mut writes = Vec::new();
            assert!(plan_codex_config(home.path(), Path::new("/repo"), &mut writes).is_err());
            assert!(writes.is_empty());
            assert_eq!(fs::read_to_string(config).unwrap(), contents);
        }
    }
}

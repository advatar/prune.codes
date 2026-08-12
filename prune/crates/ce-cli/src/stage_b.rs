use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct StageBOptions {
    pub input: PathBuf,
    pub workspace: PathBuf,
    pub out: PathBuf,
    pub instance_id: Option<String>,
    pub agent_command: Option<String>,
    pub harness_command: String,
    pub run_id: String,
    pub dry_run: bool,
}

struct Instance {
    id: String,
    repo: String,
    base_commit: String,
    task: String,
}

pub fn run(options: StageBOptions) -> Result<()> {
    let instances = load_instances(&options.input, options.instance_id.as_deref())?;
    if instances.is_empty() {
        return Err(anyhow!("no matching SWE-bench instances"));
    }
    fs::create_dir_all(&options.workspace)?;
    fs::create_dir_all(&options.out)?;
    let predictions = options.out.join("predictions.jsonl");
    let mut prediction_file = (!options.dry_run)
        .then(|| fs::File::create(&predictions))
        .transpose()?;
    let ce = std::env::current_exe()?;
    for instance in &instances {
        let checkout = options.workspace.join(safe_id(&instance.id));
        let state = checkout.join(".ce");
        let context = options
            .out
            .join(format!("{}.context.json", safe_id(&instance.id)));
        let patch = options.out.join(format!("{}.patch", safe_id(&instance.id)));
        let clone_url = if instance.repo.contains("://") {
            instance.repo.clone()
        } else {
            format!("https://github.com/{}.git", instance.repo)
        };
        if !checkout.join(".git").exists() {
            command(
                options.dry_run,
                "git",
                &[
                    "clone",
                    "--filter=blob:none",
                    &clone_url,
                    checkout.to_string_lossy().as_ref(),
                ],
                None,
                &[],
                false,
            )?;
        }
        command(
            options.dry_run,
            "git",
            &["checkout", "--detach", &instance.base_commit],
            Some(&checkout),
            &[],
            false,
        )?;
        command(
            options.dry_run,
            ce.to_string_lossy().as_ref(),
            &[
                "index",
                "--repo",
                checkout.to_string_lossy().as_ref(),
                "--db",
                state.join("index.sqlite").to_string_lossy().as_ref(),
                "--hnsw-dir",
                state.join("hnsw").to_string_lossy().as_ref(),
            ],
            None,
            &[],
            false,
        )?;
        let packed = command(
            options.dry_run,
            ce.to_string_lossy().as_ref(),
            &[
                "pack",
                "--db",
                state.join("index.sqlite").to_string_lossy().as_ref(),
                "--hnsw-dir",
                state.join("hnsw").to_string_lossy().as_ref(),
                "--task",
                &instance.task,
                "--format",
                "json",
            ],
            None,
            &[],
            true,
        )?;
        if !options.dry_run {
            fs::write(&context, packed)?;
        }
        if let Some(agent) = &options.agent_command {
            command(
                options.dry_run,
                "/bin/sh",
                &["-c", agent],
                Some(&checkout),
                &[
                    ("PRUNE_INSTANCE_ID", &instance.id),
                    ("PRUNE_CONTEXT_PATH", context.to_string_lossy().as_ref()),
                    ("PRUNE_PATCH_PATH", patch.to_string_lossy().as_ref()),
                ],
                false,
            )?;
        }
        if let Some(file) = prediction_file.as_mut() {
            writeln!(
                file,
                "{}",
                json!({"instance_id": instance.id, "model_name_or_path":"prune-stage-b", "model_patch":fs::read_to_string(&patch).unwrap_or_default()})
            )?;
        }
    }
    let harness = options
        .harness_command
        .replace("{dataset}", options.input.to_string_lossy().as_ref())
        .replace("{predictions}", predictions.to_string_lossy().as_ref())
        .replace("{run_id}", &options.run_id);
    command(
        options.dry_run,
        "/bin/sh",
        &["-c", &harness],
        None,
        &[],
        false,
    )?;
    Ok(())
}

fn load_instances(path: &Path, selected: Option<&str>) -> Result<Vec<Instance>> {
    let raw = fs::read_to_string(path)?;
    let values: Vec<Value> = if raw.trim_start().starts_with('[') {
        serde_json::from_str(&raw)?
    } else {
        raw.lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?
    };
    values
        .into_iter()
        .filter_map(|value| {
            let id = value
                .get("instance_id")
                .or_else(|| value.get("id"))?
                .as_str()?
                .to_string();
            if selected.is_some_and(|wanted| wanted != id) {
                return None;
            }
            Some(Ok(Instance {
                id,
                repo: value.get("repo")?.as_str()?.to_string(),
                base_commit: value.get("base_commit")?.as_str()?.to_string(),
                task: value
                    .get("problem_statement")
                    .or_else(|| value.get("task"))?
                    .as_str()?
                    .to_string(),
            }))
        })
        .collect()
}

fn safe_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn command(
    dry: bool,
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    envs: &[(&str, &str)],
    capture: bool,
) -> Result<Vec<u8>> {
    eprintln!("stage-b: {} {}", program, args.join(" "));
    if dry {
        return Ok(Vec::new());
    }
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.envs(envs.iter().copied());
    if capture {
        let output = cmd
            .output()
            .with_context(|| format!("failed to run {program}"))?;
        if !output.status.success() {
            return Err(anyhow!(
                "command failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(output.stdout)
    } else {
        let status = cmd
            .status()
            .with_context(|| format!("failed to run {program}"))?;
        if !status.success() {
            return Err(anyhow!("command failed ({status}): {program}"));
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dry_run_covers_complete_pipeline() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let input = dir.path().join("tasks.jsonl");
        fs::write(
            &input,
            r#"{"instance_id":"org__repo-1","repo":"org/repo","base_commit":"abc123","problem_statement":"Fix crash"}"#,
        )?;
        run(StageBOptions { input, workspace: dir.path().join("work"), out: dir.path().join("out"), instance_id: None, agent_command: Some("agent --context $PRUNE_CONTEXT_PATH".into()), harness_command: "python -m swebench.harness.run_evaluation --dataset_name {dataset} --predictions_path {predictions} --run_id {run_id}".into(), run_id: "test".into(), dry_run: true })
    }
}

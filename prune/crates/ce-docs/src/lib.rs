use anyhow::{anyhow, Result};
use ce_core::tokenizer::TokenCounter;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub mod context7;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocsSnippet {
    pub provider: String,
    pub library: String,
    pub title: String,
    pub text: String,
    pub approx_tokens: usize,
    pub source_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocsQuery {
    pub intent: String,
    pub libraries: Vec<String>,
    pub query: String,
    pub k: usize,
    pub max_tokens: usize,
}

pub trait DocsProvider: Send + Sync {
    fn name(&self) -> &str;
    fn enabled(&self) -> bool;
    fn fetch(&self, q: &DocsQuery) -> Result<Vec<DocsSnippet>>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DocsConfig {
    pub version: u32,
    pub providers: DocsProviders,
}

impl Default for DocsConfig {
    fn default() -> Self {
        Self {
            version: 1,
            providers: DocsProviders::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DocsProviders {
    pub context7: Option<Context7Config>,
}

impl Default for DocsProviders {
    fn default() -> Self {
        Self {
            context7: Some(Context7Config::default()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Context7Config {
    pub enabled: bool,
    pub api_base: String,
    pub api_key_env: String,
    pub max_tokens: usize,
    pub k: usize,
    pub cache_dir: String,
    pub cache_ttl_days: u64,
    pub only_for_intents: Vec<String>,
    pub deny_send_code: bool,
}

impl Default for Context7Config {
    fn default() -> Self {
        Self {
            enabled: false,
            api_base: "https://context7.com".to_string(),
            api_key_env: "CONTEXT7_API_KEY".to_string(),
            max_tokens: 800,
            k: 4,
            cache_dir: ".ce/docs-cache/context7".to_string(),
            cache_ttl_days: 7,
            only_for_intents: vec![
                "inception".to_string(),
                "integration".to_string(),
                "dependency_error".to_string(),
            ],
            deny_send_code: true,
        }
    }
}

impl Context7Config {
    pub fn cache_dir_path(&self, repo_root: &Path) -> PathBuf {
        let path = PathBuf::from(&self.cache_dir);
        if path.is_absolute() {
            path
        } else {
            repo_root.join(path)
        }
    }

    pub fn api_key(&self) -> Option<String> {
        if self.api_key_env.trim().is_empty() {
            return None;
        }
        std::env::var(&self.api_key_env).ok()
    }

    pub fn intent_allowed(&self, intent: &str) -> bool {
        if self.only_for_intents.is_empty() {
            return true;
        }
        let needle = intent.trim().to_ascii_lowercase();
        self.only_for_intents
            .iter()
            .any(|i| i.trim().eq_ignore_ascii_case(&needle))
    }
}

pub fn docs_config_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".prune").join("docs.json")
}

pub fn default_docs_json() -> Result<String> {
    serde_json::to_string_pretty(&DocsConfig::default()).map_err(|e| anyhow!(e))
}

pub fn load_docs_config(repo_root: &Path) -> Result<Option<DocsConfig>> {
    let path = docs_config_path(repo_root);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    let cfg: DocsConfig = serde_json::from_str(&raw)?;
    Ok(Some(cfg))
}

pub fn infer_intent(task: &str) -> String {
    let text = task.to_ascii_lowercase();
    let dep_error = [
        "dependency",
        "module not found",
        "cannot find module",
        "crate not found",
        "no such package",
        "missing package",
        "failed to resolve",
    ];
    if dep_error.iter().any(|k| text.contains(k)) {
        return "dependency_error".to_string();
    }

    let integration = [
        "integrate",
        "integration",
        "configure",
        "hook up",
        "wire up",
        "connect",
        "setup",
        "install",
    ];
    if integration.iter().any(|k| text.contains(k)) {
        return "integration".to_string();
    }

    let inception = [
        "inception",
        "bootstrap",
        "onboard",
        "greenfield",
        "new project",
        "project setup",
    ];
    if inception.iter().any(|k| text.contains(k)) {
        return "inception".to_string();
    }

    "general".to_string()
}

pub fn load_repo_dependencies(repo_root: &Path) -> Result<Vec<String>> {
    let mut libs: HashSet<String> = HashSet::new();
    load_package_json(repo_root, &mut libs);
    load_cargo_toml(repo_root, &mut libs);
    load_package_swift(repo_root, &mut libs);

    let mut out: Vec<String> = libs.into_iter().collect();
    out.sort();
    Ok(out)
}

pub fn select_libraries(
    task: &str,
    deps: &[String],
    refs: &[String],
    max: usize,
) -> Vec<String> {
    if max == 0 {
        return vec![];
    }

    let mut scores: HashMap<String, f32> = HashMap::new();
    let mut originals: HashMap<String, String> = HashMap::new();
    let mut dep_set: HashSet<String> = HashSet::new();

    for dep in deps {
        let key = normalize_lib(dep);
        if key.is_empty() {
            continue;
        }
        dep_set.insert(key.clone());
        originals.entry(key.clone()).or_insert_with(|| dep.clone());
        scores.entry(key).or_insert(0.2);
    }

    let task_tokens = ce_core::util::extract_ident_tokens(task);
    for tok in &task_tokens {
        if dep_set.contains(tok) {
            let entry = scores.entry(tok.clone()).or_insert(0.0);
            *entry += 2.0;
        } else if looks_like_external(tok, &dep_set) {
            let entry = scores.entry(tok.clone()).or_insert(0.0);
            *entry += 0.6;
            originals.entry(tok.clone()).or_insert_with(|| tok.clone());
        }
    }

    for r in refs {
        let key = normalize_lib(r);
        if key.is_empty() {
            continue;
        }
        if looks_like_external(r, &dep_set) {
            let entry = scores.entry(key.clone()).or_insert(0.0);
            *entry += 1.0;
            originals.entry(key.clone()).or_insert_with(|| r.clone());
        }
    }

    if scores.is_empty() {
        return deps.iter().take(max).cloned().collect();
    }

    let mut ranked: Vec<(String, f32)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut out: Vec<String> = Vec::new();
    for (key, _) in ranked.into_iter() {
        if out.len() >= max {
            break;
        }
        if let Some(orig) = originals.get(&key) {
            out.push(orig.clone());
        } else {
            out.push(key.clone());
        }
    }
    out
}

pub fn approx_tokens(text: &str) -> usize {
    let counter = TokenCounter::new("o200k_base");
    counter.count(text)
}

fn normalize_lib(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn looks_like_external(name: &str, deps: &HashSet<String>) -> bool {
    if name.trim().is_empty() {
        return false;
    }
    let norm = normalize_lib(name);
    if deps.contains(&norm) {
        return true;
    }
    norm.starts_with('@') || norm.contains('/')
}

fn load_package_json(repo_root: &Path, libs: &mut HashSet<String>) {
    let path = repo_root.join("package.json");
    let raw = match fs::read_to_string(&path) {
        Ok(v) => v,
        Err(_) => return,
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return,
    };
    for key in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(obj) = value.get(key).and_then(|v| v.as_object()) {
            for (dep, _) in obj {
                libs.insert(dep.to_string());
            }
        }
    }
}

fn load_cargo_toml(repo_root: &Path, libs: &mut HashSet<String>) {
    let path = repo_root.join("Cargo.toml");
    let raw = match fs::read_to_string(&path) {
        Ok(v) => v,
        Err(_) => return,
    };
    let value: toml::Value = match raw.parse() {
        Ok(v) => v,
        Err(_) => return,
    };
    if let Some(table) = value.as_table() {
        add_toml_deps(table.get("dependencies"), libs);
        add_toml_deps(table.get("dev-dependencies"), libs);
        add_toml_deps(table.get("build-dependencies"), libs);
        if let Some(workspace) = table.get("workspace").and_then(|v| v.as_table()) {
            add_toml_deps(workspace.get("dependencies"), libs);
        }
    }
}

fn add_toml_deps(value: Option<&toml::Value>, libs: &mut HashSet<String>) {
    if let Some(table) = value.and_then(|v| v.as_table()) {
        for (dep, _) in table {
            libs.insert(dep.to_string());
        }
    }
}

fn load_package_swift(repo_root: &Path, libs: &mut HashSet<String>) {
    let path = repo_root.join("Package.swift");
    let raw = match fs::read_to_string(&path) {
        Ok(v) => v,
        Err(_) => return,
    };
    for line in raw.lines() {
        let trimmed = line.trim();
        if !trimmed.contains(".package") {
            continue;
        }
        if let Some(name) = extract_swift_field(trimmed, "name:") {
            libs.insert(name);
            continue;
        }
        if let Some(url) = extract_swift_field(trimmed, "url:") {
            let mut parts = url.split('/').filter(|p| !p.is_empty()).collect::<Vec<_>>();
            if let Some(last) = parts.pop() {
                let name = last.trim_end_matches(".git");
                if !name.is_empty() {
                    libs.insert(name.to_string());
                }
            }
        }
    }
}

fn extract_swift_field(line: &str, key: &str) -> Option<String> {
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            break;
        }
    }
    let mut out = String::new();
    for ch in chars {
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

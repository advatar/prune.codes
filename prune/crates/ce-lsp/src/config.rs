use globset::GlobBuilder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspFeatureFlags {
    pub definition: bool,
    pub type_definition: bool,
    pub implementation: bool,
    pub references: bool,
}

impl Default for LspFeatureFlags {
    fn default() -> Self {
        Self {
            definition: true,
            type_definition: true,
            implementation: false,
            references: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub command: Vec<String>,
    #[serde(default)]
    pub file_globs: Vec<String>,
    #[serde(default)]
    pub root_markers: Vec<String>,
    #[serde(default)]
    pub language_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspConfig {
    pub version: u32,
    pub enabled: bool,
    pub default_timeout_ms: u64,
    pub initialize_timeout_ms: u64,
    pub max_requests_per_pack: usize,
    #[serde(default)]
    pub servers: HashMap<String, ServerConfig>,
    #[serde(default)]
    pub features: LspFeatureFlags,
}

#[derive(Debug, Clone, Copy)]
pub enum LspTemplate {
    Web,
    Mobile,
    Rust,
}

impl LspTemplate {
    pub fn default_config(&self) -> LspConfig {
        let mut servers = HashMap::new();
        servers.insert(
            "ts".to_string(),
            ServerConfig {
                command: vec![
                    "typescript-language-server".to_string(),
                    "--stdio".to_string(),
                ],
                file_globs: vec![
                    "**/*.ts".to_string(),
                    "**/*.tsx".to_string(),
                    "**/*.js".to_string(),
                    "**/*.jsx".to_string(),
                ],
                root_markers: vec!["package.json".to_string(), "tsconfig.json".to_string()],
                language_id: Some("typescript".to_string()),
            },
        );
        servers.insert(
            "swift".to_string(),
            ServerConfig {
                command: vec!["xcrun".to_string(), "sourcekit-lsp".to_string()],
                file_globs: vec!["**/*.swift".to_string()],
                root_markers: vec![
                    "*.xcodeproj".to_string(),
                    "*.xcworkspace".to_string(),
                    "Package.swift".to_string(),
                ],
                language_id: Some("swift".to_string()),
            },
        );
        servers.insert(
            "rust".to_string(),
            ServerConfig {
                command: vec!["rust-analyzer".to_string()],
                file_globs: vec!["**/*.rs".to_string()],
                root_markers: vec!["Cargo.toml".to_string()],
                language_id: Some("rust".to_string()),
            },
        );

        let mut config = LspConfig {
            version: 1,
            enabled: true,
            default_timeout_ms: 8000,
            initialize_timeout_ms: 30000,
            max_requests_per_pack: 24,
            servers,
            features: LspFeatureFlags::default(),
        };

        match self {
            Self::Web => config,
            Self::Mobile => config,
            Self::Rust => {
                config.servers.retain(|k, _| k == "rust");
                config
            }
        }
    }
}

impl LspConfig {
    pub fn path_for_repo(repo_root: &Path) -> PathBuf {
        repo_root.join(".prune").join("lsp.json")
    }

    pub fn load(repo_root: &Path) -> anyhow::Result<Option<Self>> {
        let path = Self::path_for_repo(repo_root);
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path)?;
        let config: LspConfig = serde_json::from_str(&text)?;
        Ok(Some(config))
    }

    pub fn find_server_for_path(
        &self,
        repo_root: &Path,
        file: &Path,
    ) -> Option<(&str, &ServerConfig)> {
        for (key, server) in &self.servers {
            if server.matches_path(repo_root, file) {
                return Some((key.as_str(), server));
            }
        }
        None
    }

    pub fn find_root_marker(&self, repo_root: &Path, server: &ServerConfig) -> Option<PathBuf> {
        server.find_root_marker(repo_root)
    }
}

impl ServerConfig {
    pub fn matches_path(&self, repo_root: &Path, target: &Path) -> bool {
        if self.file_globs.is_empty() {
            return false;
        }
        let rel = match target.strip_prefix(repo_root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => target.to_string_lossy().replace('\\', "/"),
        };
        for pattern in &self.file_globs {
            let mut builder = GlobBuilder::new(pattern);
            builder.literal_separator(true);
            if let Ok(glob) = builder.build() {
                if glob.compile_matcher().is_match(&rel) {
                    return true;
                }
            }
        }
        false
    }

    pub fn find_root_marker(&self, repo_root: &Path) -> Option<PathBuf> {
        for marker in &self.root_markers {
            if has_glob(marker) {
                if let Ok(entries) = fs::read_dir(repo_root) {
                    let mut builder = GlobBuilder::new(marker);
                    builder.literal_separator(true);
                    if let Ok(glob) = builder.build() {
                        let matcher = glob.compile_matcher();
                        for entry in entries.flatten() {
                            let name_os = entry.file_name();
                            let name = name_os.to_string_lossy();
                            if matcher.is_match(name.as_ref()) {
                                return Some(entry.path());
                            }
                        }
                    }
                }
            } else {
                let candidate = repo_root.join(marker);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

fn has_glob(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}

pub fn merge_value_with_default(mut existing: Value, defaults: &Value) -> Value {
    match (&mut existing, defaults) {
        (Value::Object(map), Value::Object(def_map)) => {
            for (key, def_value) in def_map {
                match map.get_mut(key) {
                    Some(existing_value) => {
                        let merged = merge_value_with_default(existing_value.clone(), def_value);
                        *existing_value = merged;
                    }
                    None => {
                        map.insert(key.clone(), def_value.clone());
                    }
                }
            }
            Value::Object(map.clone())
        }
        _ => existing,
    }
}

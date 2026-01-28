use crate::client::LspClient;
use crate::config::{LspFeatureFlags, ServerConfig};
use crate::transport::LspTransport;
use anyhow::{anyhow, Context, Result};
use lsp_types::{Location, LocationLink};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use url::Url as StdUrl;

#[derive(Debug)]
pub enum ResolveMode {
    Definition,
    Type,
}

pub struct LspSession {
    client: LspClient,
    open_files: Mutex<HashMap<PathBuf, i64>>,
    request_timeout: Duration,
    max_requests: usize,
    requests: AtomicUsize,
    language_id: String,
    #[allow(dead_code)]
    reader_handle: JoinHandle<()>,
    #[allow(dead_code)]
    writer_handle: JoinHandle<()>,
    child: Child,
    capabilities: Value,
}

impl LspSession {
    pub async fn start(
        server_name: &str,
        server: &ServerConfig,
        repo_root: &Path,
        features: &LspFeatureFlags,
        default_timeout: Duration,
        init_timeout: Duration,
        max_requests: usize,
    ) -> Result<Self> {
        if server.command.is_empty() {
            return Err(anyhow!("server command not configured"));
        }

        let mut cmd = Command::new(&server.command[0]);
        if server.command.len() > 1 {
            cmd.args(&server.command[1..]);
        }
        cmd.current_dir(repo_root);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit());

        let mut child = cmd.spawn().context("failed to spawn LSP server")?;
        let stdin = child.stdin.take().context("LSP server missing stdin")?;
        let stdout = child.stdout.take().context("LSP server missing stdout")?;

        let transport = LspTransport::new(stdin, stdout);
        let (writer, reader) = transport.split();

        let (tx, mut rx) = mpsc::channel::<String>(256);
        let client = LspClient::new(tx.clone());

        let writer_handle = tokio::spawn(async move {
            let mut writer = writer;
            while let Some(msg) = rx.recv().await {
                if writer.write_message(&msg).await.is_err() {
                    break;
                }
            }
        });

        let client_clone = client.clone();
        let reader_handle = tokio::spawn(async move {
            let mut reader = reader;
            loop {
                match reader.read_message().await {
                    Ok(text) => match serde_json::from_str::<Value>(&text) {
                        Ok(msg) => {
                            if msg.get("method").is_some() {
                                continue;
                            }
                            let _ = client_clone.on_response(msg).await;
                        }
                        Err(_) => break,
                    },
                    Err(_) => break,
                }
            }
        });

        let capabilities =
            Self::initialize(client.clone(), repo_root, features, init_timeout).await?;

        let language_id = server
            .language_id
            .clone()
            .unwrap_or_else(|| Self::guess_language(server_name));

        Ok(Self {
            client,
            open_files: Mutex::new(HashMap::new()),
            request_timeout: default_timeout,
            max_requests,
            requests: AtomicUsize::new(0),
            language_id,
            reader_handle,
            writer_handle,
            child,
            capabilities,
        })
    }

    pub fn capabilities(&self) -> &Value {
        &self.capabilities
    }

    pub async fn resolve_definition(
        &self,
        path: &Path,
        line: u32,
        col: u32,
    ) -> Result<Vec<Location>> {
        self.resolve(path, line, col, ResolveMode::Definition).await
    }

    pub async fn resolve_type_definition(
        &self,
        path: &Path,
        line: u32,
        col: u32,
    ) -> Result<Vec<Location>> {
        self.resolve(path, line, col, ResolveMode::Type).await
    }

    async fn resolve(
        &self,
        path: &Path,
        line: u32,
        col: u32,
        mode: ResolveMode,
    ) -> Result<Vec<Location>> {
        self.ensure_open(path).await?;
        let method = match mode {
            ResolveMode::Definition => "textDocument/definition",
            ResolveMode::Type => "textDocument/typeDefinition",
        };
        let uri = path_to_uri(path)?;
        let params = json!({
            "textDocument": {
                "uri": uri,
            },
            "position": {
                "line": line,
                "character": col,
            }
        });

        let result = self.send_request(method, params).await?;
        Self::extract_locations(result)
    }

    fn extract_locations(value: Value) -> Result<Vec<Location>> {
        if value.is_null() {
            return Ok(Vec::new());
        }
        if let Ok(loc) = serde_json::from_value::<Location>(value.clone()) {
            return Ok(vec![loc]);
        }
        if let Ok(locations) = serde_json::from_value::<Vec<Location>>(value.clone()) {
            return Ok(locations);
        }
        if let Ok(links) = serde_json::from_value::<Vec<LocationLink>>(value.clone()) {
            return Ok(links
                .into_iter()
                .map(|link| Location {
                    uri: link.target_uri,
                    range: link.target_selection_range,
                })
                .collect());
        }
        Err(anyhow!("unexpected LSP location payload"))
    }

    async fn send_request(&self, method: &str, params: Value) -> Result<Value> {
        let current = self.requests.fetch_add(1, Ordering::Relaxed);
        if current >= self.max_requests {
            return Err(anyhow!("LSP request budget exhausted"));
        }
        let fut = self.client.request(method, params);
        match timeout(self.request_timeout, fut).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(anyhow!("LSP request timed out")),
        }
    }

    async fn ensure_open(&self, path: &Path) -> Result<()> {
        let open = self.open_files.lock().await;
        if open.contains_key(path) {
            return Ok(());
        }
        let text = tokio::fs::read_to_string(path).await?;
        let uri = path_to_uri(path)?;
        let params = json!({
            "textDocument": {
                "uri": uri,
                "languageId": &self.language_id,
                "version": 1,
                "text": text,
            }
        });
        drop(open);
        self.client.notify("textDocument/didOpen", params).await?;
        let mut open = self.open_files.lock().await;
        open.insert(path.to_path_buf(), 1);
        Ok(())
    }

    fn guess_language(server_name: &str) -> String {
        match server_name {
            "ts" => "typescript",
            "swift" => "swift",
            "rust" => "rust",
            _ => "plaintext",
        }
        .to_string()
    }

    async fn initialize(
        client: LspClient,
        repo_root: &Path,
        features: &LspFeatureFlags,
        timeout_dur: Duration,
    ) -> Result<Value> {
        let root_uri = StdUrl::from_directory_path(repo_root)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| format!("file://{}", repo_root.display()));

        let mut text_doc_caps = Map::new();
        if features.definition {
            text_doc_caps.insert(
                "definition".to_string(),
                json!({ "dynamicRegistration": false }),
            );
        }
        if features.type_definition {
            text_doc_caps.insert(
                "typeDefinition".to_string(),
                json!({ "dynamicRegistration": false }),
            );
        }
        if features.implementation {
            text_doc_caps.insert(
                "implementation".to_string(),
                json!({ "dynamicRegistration": false }),
            );
        }
        if features.references {
            text_doc_caps.insert(
                "references".to_string(),
                json!({ "dynamicRegistration": false }),
            );
        }

        let capabilities = json!({
            "textDocument": Value::Object(text_doc_caps)
        });

        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": capabilities,
        });

        let init = timeout(timeout_dur, client.request("initialize", params)).await;
        let result = match init {
            Ok(Ok(value)) => value,
            Ok(Err(err)) => return Err(err),
            Err(_) => return Err(anyhow!("LSP initialize timed out")),
        };
        client.notify("initialized", json!({})).await?;
        Ok(result)
    }
}

fn path_to_uri(path: &Path) -> Result<String> {
    StdUrl::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| anyhow!("invalid file path: {}", path.display()))
}

impl Drop for LspSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        self.reader_handle.abort();
        self.writer_handle.abort();
    }
}

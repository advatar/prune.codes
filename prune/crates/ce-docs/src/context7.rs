use crate::{DocsProvider, DocsQuery, DocsSnippet, Context7Config};
use anyhow::{anyhow, Result};
use ce_core::tokenizer::TokenCounter;
use reqwest::blocking::Client;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct Context7Provider {
    cfg: Context7Config,
    client: Client,
    cache: DiskCache,
    token_counter: TokenCounter,
}

impl Context7Provider {
    pub fn new(cfg: Context7Config, repo_root: &Path) -> Result<Self> {
        let cache_dir = cfg.cache_dir_path(repo_root);
        let cache = DiskCache::new(cache_dir, cfg.cache_ttl_days)?;
        let client = Client::builder().build()?;
        let token_counter = TokenCounter::new("o200k_base");
        Ok(Self {
            cfg,
            client,
            cache,
            token_counter,
        })
    }

    fn build_url(&self, path: &str, params: &[(&str, &str)]) -> Result<Url> {
        let base = self.cfg.api_base.trim_end_matches('/');
        let raw = format!("{base}{path}");
        let mut url = Url::parse(&raw)?;
        {
            let mut qp = url.query_pairs_mut();
            for (k, v) in params {
                qp.append_pair(k, v);
            }
        }
        Ok(url)
    }

    fn request_json(&self, url: Url) -> Result<Value> {
        let mut req = self.client.get(url);
        if let Some(key) = self.cfg.api_key() {
            req = req.bearer_auth(key);
        } else if !self.cfg.api_key_env.trim().is_empty() {
            return Err(anyhow!(
                "missing API key (env {})",
                self.cfg.api_key_env
            ));
        }
        let resp = req.send()?;
        let status = resp.status();
        let body = resp.text()?;
        if !status.is_success() {
            return Err(anyhow!("context7 request failed ({status})"));
        }
        match serde_json::from_str::<Value>(&body) {
            Ok(v) => Ok(v),
            Err(_) => Ok(Value::String(body)),
        }
    }

    fn search_libs(&self, library: &str, query: &str) -> Result<Vec<Context7Library>> {
        let key = cache_key(&["context7", "libs_search", library, query]);
        if let Some(cached) = self.cache.get::<Vec<Context7Library>>(&key)? {
            return Ok(cached);
        }
        let url = self.build_url(
            "/api/v2/libs/search",
            &[("libraryName", library), ("query", query)],
        )?;
        let value = self.request_json(url)?;
        let libs = extract_libraries(value);
        self.cache.set(&key, &libs)?;
        Ok(libs)
    }

    fn fetch_context(&self, library_id: &str, query: &str) -> Result<Vec<Context7Doc>> {
        let key = cache_key(&["context7", "context", library_id, query]);
        if let Some(cached) = self.cache.get::<Vec<Context7Doc>>(&key)? {
            return Ok(cached);
        }
        let url = self.build_url(
            "/api/v2/context",
            &[("libraryId", library_id), ("query", query)],
        )?;
        let value = self.request_json(url)?;
        let docs = extract_docs(value);
        self.cache.set(&key, &docs)?;
        Ok(docs)
    }
}

impl DocsProvider for Context7Provider {
    fn name(&self) -> &str {
        "context7"
    }

    fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    fn fetch(&self, q: &DocsQuery) -> Result<Vec<DocsSnippet>> {
        if !self.enabled() {
            return Ok(vec![]);
        }
        if q.libraries.is_empty() {
            return Ok(vec![]);
        }
        if !self.cfg.intent_allowed(&q.intent) {
            return Ok(vec![]);
        }

        let query = sanitize_query(&q.query, self.cfg.deny_send_code);
        if query.is_empty() {
            return Ok(vec![]);
        }

        let mut out: Vec<DocsSnippet> = Vec::new();
        let mut used_tokens = 0usize;
        let mut remaining_k = q.k.max(1);

        for lib in q.libraries.iter().take(2) {
            if remaining_k == 0 || used_tokens >= q.max_tokens {
                break;
            }
            let libs = self.search_libs(lib, &query)?;
            let Some(best) = pick_best_library(lib, &libs) else {
                continue;
            };
            let docs = self.fetch_context(&best.id, &query)?;
            for doc in docs {
                if remaining_k == 0 || used_tokens >= q.max_tokens {
                    break;
                }
                let text = doc.text.trim();
                if text.is_empty() {
                    continue;
                }
                let mut snippet_text = text.to_string();
                let mut snippet_tokens = self.token_counter.count(&snippet_text);
                let remaining_tokens = q.max_tokens.saturating_sub(used_tokens);
                if remaining_tokens == 0 {
                    break;
                }
                if snippet_tokens > remaining_tokens {
                    snippet_text = truncate_to_tokens(&snippet_text, remaining_tokens, &self.token_counter);
                    snippet_tokens = self.token_counter.count(&snippet_text);
                    if snippet_tokens == 0 {
                        continue;
                    }
                }
                out.push(DocsSnippet {
                    provider: "context7".to_string(),
                    library: best.name.clone(),
                    title: doc
                        .title
                        .unwrap_or_else(|| format!("{} reference", best.name)),
                    text: snippet_text,
                    approx_tokens: snippet_tokens,
                    source_url: doc.source_url,
                });
                used_tokens += snippet_tokens;
                remaining_k = remaining_k.saturating_sub(1);
            }
        }

        Ok(out)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Context7Library {
    id: String,
    name: String,
    description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Context7Doc {
    title: Option<String>,
    text: String,
    source_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry<T> {
    stored_at: u64,
    value: T,
}

struct DiskCache {
    root: PathBuf,
    ttl: Duration,
}

impl DiskCache {
    fn new(root: PathBuf, ttl_days: u64) -> Result<Self> {
        if ttl_days == 0 {
            fs::create_dir_all(&root)?;
        } else {
            fs::create_dir_all(&root)?;
        }
        Ok(Self {
            root,
            ttl: Duration::from_secs(ttl_days.saturating_mul(86_400)),
        })
    }

    fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        if self.ttl.is_zero() {
            return Ok(None);
        }
        let path = self.root.join(format!("{key}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path)?;
        let entry: CacheEntry<T> = serde_json::from_str(&raw)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now.saturating_sub(entry.stored_at) > self.ttl.as_secs() {
            return Ok(None);
        }
        Ok(Some(entry.value))
    }

    fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        if self.ttl.is_zero() {
            return Ok(());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let entry = CacheEntry {
            stored_at: now,
            value,
        };
        let raw = serde_json::to_string(&entry)?;
        let path = self.root.join(format!("{key}.json"));
        fs::write(path, raw)?;
        Ok(())
    }
}

fn cache_key(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.trim().as_bytes());
        hasher.update([0u8]);
    }
    hex::encode(hasher.finalize())
}

fn extract_libraries(value: Value) -> Vec<Context7Library> {
    let mut out = Vec::new();
    let arr = match value {
        Value::Array(v) => Some(v),
        Value::Object(map) => map
            .get("results")
            .or_else(|| map.get("libraries"))
            .or_else(|| map.get("libs"))
            .or_else(|| map.get("data"))
            .and_then(|v| v.as_array().cloned()),
        _ => None,
    };

    if let Some(items) = arr {
        for item in items {
            if let Value::Object(map) = item {
                let id = map
                    .get("id")
                    .or_else(|| map.get("libraryId"))
                    .or_else(|| map.get("library_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let name = map
                    .get("name")
                    .or_else(|| map.get("libraryName"))
                    .or_else(|| map.get("slug"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let description = map
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if let (Some(id), Some(name)) = (id, name) {
                    out.push(Context7Library {
                        id,
                        name,
                        description,
                    });
                }
            }
        }
    }
    out
}

fn extract_docs(value: Value) -> Vec<Context7Doc> {
    let mut out = Vec::new();
    match value {
        Value::String(text) => {
            out.push(Context7Doc {
                title: None,
                text,
                source_url: None,
            });
        }
        Value::Array(arr) => {
            for item in arr {
                if let Some(doc) = parse_doc_value(&item) {
                    out.push(doc);
                }
            }
        }
        Value::Object(map) => {
            if let Some(context) = map.get("context").and_then(|v| v.as_str()) {
                out.push(Context7Doc {
                    title: map
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    text: context.to_string(),
                    source_url: map
                        .get("source_url")
                        .or_else(|| map.get("url"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                });
            }
            let arrays = ["snippets", "results", "data", "items"];
            for key in arrays {
                if let Some(arr) = map.get(key).and_then(|v| v.as_array()) {
                    for item in arr {
                        if let Some(doc) = parse_doc_value(item) {
                            out.push(doc);
                        }
                    }
                    if !out.is_empty() {
                        break;
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn parse_doc_value(value: &Value) -> Option<Context7Doc> {
    let map = value.as_object()?;
    let text = map
        .get("text")
        .or_else(|| map.get("content"))
        .or_else(|| map.get("snippet"))
        .or_else(|| map.get("context"))
        .and_then(|v| v.as_str())?
        .to_string();
    let title = map
        .get("title")
        .or_else(|| map.get("heading"))
        .or_else(|| map.get("section"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let source_url = map
        .get("source_url")
        .or_else(|| map.get("url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Some(Context7Doc {
        title,
        text,
        source_url,
    })
}

fn pick_best_library(query: &str, libs: &[Context7Library]) -> Option<Context7Library> {
    if libs.is_empty() {
        return None;
    }
    let needle = query.trim().to_ascii_lowercase();
    for lib in libs {
        if lib.name.to_ascii_lowercase() == needle {
            return Some(lib.clone());
        }
    }
    Some(libs[0].clone())
}

fn sanitize_query(query: &str, deny_send_code: bool) -> String {
    let mut out = String::new();
    let mut in_block = false;
    for line in query.lines() {
        let trimmed = line.trim_start();
        if deny_send_code && trimmed.starts_with("```") {
            in_block = !in_block;
            continue;
        }
        if deny_send_code && in_block {
            continue;
        }
        out.push_str(line);
        out.push(' ');
    }
    if deny_send_code {
        out = strip_inline_code(&out);
    }
    let cleaned = out.split_whitespace().collect::<Vec<_>>().join(" ");
    cleaned.trim().to_string()
}

fn strip_inline_code(text: &str) -> String {
    let mut out = String::new();
    let mut in_tick = false;
    for ch in text.chars() {
        if ch == '`' {
            in_tick = !in_tick;
            continue;
        }
        if !in_tick {
            out.push(ch);
        }
    }
    out
}

fn truncate_to_tokens(text: &str, max_tokens: usize, counter: &TokenCounter) -> String {
    if max_tokens == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for line in text.lines() {
        let candidate = if out.is_empty() {
            line.to_string()
        } else {
            format!("{out}\n{line}")
        };
        let next = counter.count(&candidate);
        if next > max_tokens {
            break;
        }
        out = candidate;
        used = next;
    }
    if used == 0 {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut out_words = Vec::new();
        for w in words {
            out_words.push(w);
            let candidate = out_words.join(" ");
            if counter.count(&candidate) > max_tokens {
                out_words.pop();
                break;
            }
        }
        return out_words.join(" ");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocsConfig, DocsQuery};
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use std::env;
    use tempfile::TempDir;

    #[test]
    fn cache_key_stable() {
        let a = cache_key(&["context7", "libs_search", "react", "useEffect"]);
        let b = cache_key(&["context7", "libs_search", "react", "useEffect"]);
        assert_eq!(a, b);
    }

    #[test]
    fn docs_config_roundtrip() {
        let cfg = DocsConfig::default();
        let raw = serde_json::to_string(&cfg).unwrap();
        let parsed: DocsConfig = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.version, 1);
        assert!(parsed.providers.context7.is_some());
    }

    #[test]
    fn truncate_respects_token_budget() {
        let counter = TokenCounter::new("o200k_base");
        let text = "one two three four five six seven eight nine ten";
        let truncated = truncate_to_tokens(text, 3, &counter);
        assert!(counter.count(&truncated) <= 3);
    }

    #[test]
    fn context7_fetch_uses_cache() -> Result<()> {
        let server = MockServer::start();
        let libs_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/libs/search")
                .query_param("libraryName", "supabase")
                .query_param("query", "auth");
            then.status(200).json_body(serde_json::json!([
                {"id": "lib-1", "name": "supabase"}
            ]));
        });

        let ctx_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/context")
                .query_param("libraryId", "lib-1")
                .query_param("query", "auth");
            then.status(200).json_body(serde_json::json!({
                "snippets": [
                    {"title": "Auth", "text": "Use auth()", "source_url": "https://example.com"}
                ]
            }));
        });

        env::set_var("CONTEXT7_API_KEY", "test");
        let tmp = TempDir::new()?;
        let mut cfg = Context7Config::default();
        cfg.enabled = true;
        cfg.api_base = server.base_url();
        cfg.cache_dir = tmp.path().join("cache").to_string_lossy().to_string();
        let provider = Context7Provider::new(cfg, tmp.path())?;
        let query = DocsQuery {
            intent: "integration".to_string(),
            libraries: vec!["supabase".to_string()],
            query: "auth".to_string(),
            k: 1,
            max_tokens: 50,
        };

        let first = provider.fetch(&query)?;
        let second = provider.fetch(&query)?;
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        libs_mock.assert_hits(1);
        ctx_mock.assert_hits(1);
        Ok(())
    }
}

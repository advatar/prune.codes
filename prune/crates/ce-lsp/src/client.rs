use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

#[derive(Clone)]
pub struct LspClient {
    next_id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    tx: mpsc::Sender<String>,
}

impl LspClient {
    pub fn new(tx: mpsc::Sender<String>) -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(1)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            tx,
        }
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, sx);

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        self.tx
            .send(msg.to_string())
            .await
            .map_err(|_| anyhow!("LSP tx closed"))?;

        let resp = rx.await.map_err(|_| anyhow!("LSP response dropped"))?;
        if let Some(err) = resp.get("error") {
            return Err(anyhow!("LSP error: {}", err));
        }
        resp.get("result")
            .cloned()
            .ok_or_else(|| anyhow!("LSP response missing result"))
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.tx
            .send(msg.to_string())
            .await
            .map_err(|_| anyhow!("LSP tx closed"))
    }

    pub async fn on_response(&self, raw: Value) -> Result<()> {
        let id = raw
            .get("id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("missing id"))?;
        if let Some(tx) = self.pending.lock().await.remove(&id) {
            let _ = tx.send(raw);
        }
        Ok(())
    }
}

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use super::progress::Progress;
use super::protocol::{
    retryable, CallHierarchyItem, Location, LspError, LspPosition, OutgoingCall,
};
use super::transport::{self, PendingMap};

pub struct LspClient {
    tx: mpsc::UnboundedSender<Value>,
    pending: PendingMap,
    next_id: AtomicI64,
    child: AsyncMutex<Option<Child>>,
    stderr: Arc<Mutex<String>>,
    progress: Progress,
}

pub async fn start(
    root: PathBuf,
    command: String,
    args: Vec<String>,
    verbose: bool,
) -> Result<(LspClient, Value)> {
    let root_uri = url::Url::from_directory_path(&root)
        .map_err(|_| anyhow!("cannot convert root to file URL"))?
        .to_string();
    let child = Command::new(&command)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| anyhow!("failed to start LSP server \"{command}\": {error}"))?;

    let progress = Progress::default();
    let io = transport::spawn(child, &root_uri, progress.clone(), verbose)?;
    let client = LspClient {
        tx: io.tx,
        pending: io.pending,
        next_id: AtomicI64::new(1),
        child: AsyncMutex::new(Some(io.child)),
        stderr: io.stderr,
        progress,
    };

    let initialize = json!({
        "processId": std::process::id(),
        "clientInfo": { "name": "code-graph", "version": "0.1.0" },
        "rootUri": root_uri,
        "workspaceFolders": [{ "uri": root_uri, "name": "root" }],
        "capabilities": {
            "general": { "positionEncodings": ["utf-16"] },
            "workspace": {
                "workspaceFolders": true,
                "configuration": true,
                "symbol": { "symbolKind": { "valueSet": [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26] } }
            },
            "textDocument": {
                "synchronization": { "dynamicRegistration": false },
                "documentSymbol": {
                    "hierarchicalDocumentSymbolSupport": true,
                    "symbolKind": { "valueSet": [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26] }
                },
                "callHierarchy": { "dynamicRegistration": false },
                "references": { "dynamicRegistration": false },
                "hover": { "dynamicRegistration": false }
            },
            "window": { "workDoneProgress": true, "showMessage": {} }
        },
        "initializationOptions": {}
    });

    let result = match client.request("initialize", initialize, 1).await {
        Ok(result) => result,
        Err(error) => {
            let tail = client.stderr_tail();
            client.stop().await;
            return Err(anyhow!(
                "initialize failed: {error}{}{tail}",
                if tail.is_empty() { "" } else { "\n" }
            ));
        }
    };
    client.notify("initialized", json!({})).await?;
    Ok((client, result))
}

impl LspClient {
    pub(crate) fn progress(&self) -> &Progress {
        &self.progress
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.tx
            .send(message)
            .map_err(|_| anyhow!("LSP server connection is closed"))
    }

    pub async fn request(&self, method: &str, params: Value, max_attempts: u32) -> Result<Value> {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let id = self.next_id.fetch_add(1, Ordering::SeqCst);
            let (sender, receiver) = tokio::sync::oneshot::channel();
            self.pending.lock().unwrap().insert(id, sender);
            let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
            if self.tx.send(message).is_err() {
                self.pending.lock().unwrap().remove(&id);
                return Err(anyhow!("LSP server connection is closed"));
            }
            let result = tokio::time::timeout(Duration::from_secs(120), receiver).await;
            let error = match result {
                Ok(Ok(Ok(value))) => return Ok(value),
                Ok(Ok(Err(error))) => error,
                Ok(Err(_)) => LspError {
                    code: None,
                    message: "LSP response channel closed".to_string(),
                },
                Err(_) => {
                    self.pending.lock().unwrap().remove(&id);
                    LspError {
                        code: None,
                        message: format!("{method} timed out after 120s"),
                    }
                }
            };
            if retryable(&error) && attempt < max_attempts {
                let delay = std::cmp::min(150 * attempt as u64, 1000);
                tokio::time::sleep(Duration::from_millis(delay)).await;
                continue;
            }
            return Err(anyhow!("{method} failed: {error}"));
        }
    }

    pub async fn open_document(&self, uri: &str, language_id: &str, text: &str) -> Result<()> {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": { "uri": uri, "languageId": language_id, "version": 1, "text": text }
            }),
        )
        .await
    }

    pub async fn document_symbols(&self, uri: &str) -> Result<Value> {
        self.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
            60,
        )
        .await
    }

    pub async fn prepare_call_hierarchy(
        &self,
        uri: &str,
        position: LspPosition,
    ) -> Result<Vec<CallHierarchyItem>> {
        self.prepare_call_hierarchy_attempts(uri, position, 60)
            .await
    }

    pub async fn prepare_call_hierarchy_quick(
        &self,
        uri: &str,
        position: LspPosition,
    ) -> Result<Vec<CallHierarchyItem>> {
        self.prepare_call_hierarchy_attempts(uri, position, 1).await
    }

    async fn prepare_call_hierarchy_attempts(
        &self,
        uri: &str,
        position: LspPosition,
        attempts: u32,
    ) -> Result<Vec<CallHierarchyItem>> {
        let value = self
            .request(
                "textDocument/prepareCallHierarchy",
                json!({ "textDocument": { "uri": uri }, "position": position }),
                attempts,
            )
            .await?;
        Ok(serde_json::from_value(value).unwrap_or_default())
    }

    pub async fn outgoing_calls(&self, item: &CallHierarchyItem) -> Result<Vec<OutgoingCall>> {
        let value = self
            .request("callHierarchy/outgoingCalls", json!({ "item": item }), 60)
            .await?;
        Ok(serde_json::from_value(value).unwrap_or_default())
    }

    pub async fn references(
        &self,
        uri: &str,
        position: LspPosition,
        include_declaration: bool,
    ) -> Result<Vec<Location>> {
        let value = self
            .request(
                "textDocument/references",
                json!({
                    "textDocument": { "uri": uri },
                    "position": position,
                    "context": { "includeDeclaration": include_declaration }
                }),
                60,
            )
            .await?;
        Ok(serde_json::from_value(value).unwrap_or_default())
    }

    pub async fn workspace_symbol_count(&self, query: &str) -> Result<usize> {
        let value = self
            .request("workspace/symbol", json!({ "query": query }), 1)
            .await?;
        Ok(value.as_array().map(Vec::len).unwrap_or(0))
    }

    pub fn stderr_tail(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }

    pub async fn stop(&self) {
        let _ = tokio::time::timeout(
            Duration::from_millis(1500),
            self.request("shutdown", Value::Null, 1),
        )
        .await;
        let _ = self.notify("exit", Value::Null).await;
        let mut guard = self.child.lock().await;
        if let Some(child) = guard.as_mut() {
            let wait = tokio::time::timeout(Duration::from_millis(1000), child.wait()).await;
            if wait.is_err() {
                let _ = child.kill().await;
            }
        }
        *guard = None;
    }
}

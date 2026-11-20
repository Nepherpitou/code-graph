use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot};

use super::progress::Progress;
use super::protocol::LspError;

pub(super) type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, LspError>>>>>;

pub(super) struct Io {
    pub tx: mpsc::UnboundedSender<Value>,
    pub pending: PendingMap,
    pub stderr: Arc<Mutex<String>>,
    pub child: Child,
}

/// Takes the child's pipes and starts the writer, stderr, and reader tasks.
pub(super) fn spawn(
    mut child: Child,
    root_uri: &str,
    progress: Progress,
    verbose: bool,
) -> Result<Io> {
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("LSP server has no stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("LSP server has no stdout"))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("LSP server has no stderr"))?;

    let (tx, rx) = mpsc::unbounded_channel::<Value>();
    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
    let stderr: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

    tokio::spawn(async move {
        let mut writer = stdin;
        let mut rx = rx;
        while let Some(message) = rx.recv().await {
            let body = match serde_json::to_vec(&message) {
                Ok(body) => body,
                Err(_) => continue,
            };
            let header = format!("Content-Length: {}\r\n\r\n", body.len());
            if writer.write_all(header.as_bytes()).await.is_err() {
                break;
            }
            if writer.write_all(&body).await.is_err() {
                break;
            }
            let _ = writer.flush().await;
        }
    });

    {
        let stderr = stderr.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr_pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if verbose {
                    eprintln!("[lsp] {line}");
                }
                let mut buffer = stderr.lock().unwrap();
                buffer.push_str(&line);
                buffer.push('\n');
                if buffer.len() > 8000 {
                    let cutoff = buffer.len() - 8000;
                    *buffer = buffer[cutoff..].to_string();
                }
            }
        });
    }

    {
        let pending = pending.clone();
        let tx = tx.clone();
        let root_uri = root_uri.to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader).await {
                    Ok(Some(message)) => {
                        dispatch(message, &pending, &tx, &root_uri, &progress, verbose)
                    }
                    Ok(None) => break,
                    Err(error) => {
                        if verbose {
                            eprintln!("[lsp] read error: {error}");
                        }
                        break;
                    }
                }
            }
            let mut map = pending.lock().unwrap();
            for (_, sender) in map.drain() {
                let _ = sender.send(Err(LspError {
                    code: None,
                    message: "LSP server closed the stream".to_string(),
                }));
            }
        });
    }

    Ok(Io {
        tx,
        pending,
        stderr,
        child,
    })
}

async fn read_message<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            if let Some(length) = content_length {
                let mut body = vec![0u8; length];
                reader.read_exact(&mut body).await?;
                return Ok(Some(serde_json::from_slice(&body)?));
            }
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse()?);
        } else if let Some(value) = trimmed.strip_prefix("Content-Type:") {
            let _ = value;
        }
    }
}

fn dispatch(
    message: Value,
    pending: &PendingMap,
    tx: &mpsc::UnboundedSender<Value>,
    root_uri: &str,
    progress: &Progress,
    verbose: bool,
) {
    let id = message.get("id").cloned();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string);

    match (id, method) {
        (Some(id), Some(method)) => {
            let result = server_request_result(&method, message.get("params"), root_uri);
            let response = json!({ "jsonrpc": "2.0", "id": id, "result": result });
            let _ = tx.send(response);
        }
        (Some(id), None) => {
            let Some(id) = id.as_i64() else { return };
            let sender = pending.lock().unwrap().remove(&id);
            let Some(sender) = sender else { return };
            let result = if let Some(error) = message.get("error") {
                Err(LspError {
                    code: error.get("code").and_then(Value::as_i64),
                    message: error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown LSP error")
                        .to_string(),
                })
            } else {
                Ok(message.get("result").cloned().unwrap_or(Value::Null))
            };
            let _ = sender.send(result);
        }
        (None, Some(method)) => {
            let params = message.get("params");
            progress.observe(&method, params);
            if verbose {
                if let Some(params) = params {
                    if method == "window/logMessage" {
                        if let Some(text) = params.get("message").and_then(Value::as_str) {
                            eprintln!("[server] {text}");
                        }
                    } else if method == "window/showMessage" {
                        let message_type = params.get("type").and_then(Value::as_i64).unwrap_or(4);
                        if message_type <= 2 {
                            if let Some(text) = params.get("message").and_then(Value::as_str) {
                                eprintln!("[server] {text}");
                            }
                        }
                    }
                }
            }
        }
        (None, None) => {}
    }
}

fn server_request_result(method: &str, params: Option<&Value>, root_uri: &str) -> Value {
    match method {
        "workspace/configuration" => {
            let count = params
                .and_then(|params| params.get("items"))
                .and_then(Value::as_array)
                .map(|items| items.len())
                .unwrap_or(0);
            Value::Array(vec![Value::Null; count])
        }
        "workspace/workspaceFolders" => json!([{ "uri": root_uri, "name": "root" }]),
        "workspace/applyEdit" => json!({ "applied": false }),
        _ => Value::Null,
    }
}

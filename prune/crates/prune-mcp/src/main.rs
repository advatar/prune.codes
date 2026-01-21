use anyhow::{anyhow, Context, Result};
use clap::Parser;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

#[derive(Parser, Debug, Clone)]
#[command(name = "prune-mcp")]
#[command(about = "HTTP MCP gateway for Prune")]
struct Args {
    /// Host:port to bind (HTTP).
    #[arg(long, default_value = "127.0.0.1:47800")]
    bind: String,
    /// Path to sqlite db.
    #[arg(long)]
    db: String,
    /// Directory for HNSW dumps (shared with `ce index`).
    #[arg(long)]
    hnsw_dir: String,
    /// Path to ce-mcp binary.
    #[arg(long, default_value = "ce-mcp")]
    ce_mcp_path: String,
    /// Base URL for prune-sync (webhook service).
    #[arg(long, default_value = "http://127.0.0.1:47801")]
    sync_url: String,
    /// Optional bearer token for MCP access.
    #[arg(long)]
    auth_token: Option<String>,
}

struct McpProcess {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let auth_token = args
        .auth_token
        .clone()
        .or_else(|| std::env::var("PRUNE_MCP_TOKEN").ok())
        .or_else(|| std::env::var("MCP_BEARER_TOKEN").ok());

    let mut child = Command::new(&args.ce_mcp_path)
        .arg("--db")
        .arg(&args.db)
        .arg("--hnsw-dir")
        .arg(&args.hnsw_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to spawn {}", args.ce_mcp_path))?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("missing ce-mcp stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("missing ce-mcp stdout"))?;
    thread::spawn(move || {
        let _ = child.wait();
    });

    let mcp = Arc::new(Mutex::new(McpProcess {
        stdin,
        stdout: BufReader::new(stdout),
    }));

    let server = Server::http(&args.bind)
        .map_err(|err| anyhow!("bind server failed: {err}"))?;
    println!("prune-mcp listening on {}", args.bind);

    for request in server.incoming_requests() {
        if let Err(err) = handle_request(request, &args, &auth_token, &mcp) {
            eprintln!("request error: {err:#}");
        }
    }
    Ok(())
}

fn handle_request(
    request: Request,
    args: &Args,
    auth_token: &Option<String>,
    mcp: &Arc<Mutex<McpProcess>>,
) -> Result<()> {
    let method = request.method().clone();
    let url = request.url().to_string();
    match (method, url.as_str()) {
        (Method::Get, "/health") => {
            respond_json(request, 200, "{\"status\":\"ok\"}");
        }
        (Method::Get, "/mcp") => {
            respond_json(request, 200, "{\"status\":\"ok\"}");
        }
        (Method::Get, "/github/webhook") => {
            respond_json(request, 200, "{\"status\":\"ok\"}");
        }
        (Method::Post, "/mcp") => {
            handle_mcp_request(request, auth_token, mcp)?;
        }
        (Method::Post, "/github/webhook") | (Method::Post, "/sync") => {
            forward_to_sync(request, args)?;
        }
        (Method::Get, "/") => {
            respond_json(request, 200, "{\"service\":\"prune-mcp\"}");
        }
        _ => {
            respond_json(request, 404, "{\"error\":\"not_found\"}");
        }
    }
    Ok(())
}

fn handle_mcp_request(
    request: Request,
    auth_token: &Option<String>,
    mcp: &Arc<Mutex<McpProcess>>,
) -> Result<()> {
    if let Some(token) = auth_token {
        let expected = format!("Bearer {token}");
        match header_value(&request, "Authorization") {
            Some(value) if value == expected => {}
            _ => {
                respond_json(request, 401, "{\"error\":\"unauthorized\"}");
                return Ok(());
            }
        }
    }

    let mut request = request;
    let mut body = Vec::new();
    request
        .as_reader()
        .read_to_end(&mut body)
        .context("read mcp body")?;
    if body.is_empty() {
        respond_json(request, 400, "{\"error\":\"empty_body\"}");
        return Ok(());
    }

    let value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            respond_json(request, 400, "{\"error\":\"invalid_json\"}");
            return Ok(());
        }
    };
    let expects_response = has_json_id(&value);
    let payload = serde_json::to_string(&value)?;

    let response_line = {
        let mut guard = mcp.lock().expect("mcp mutex poisoned");
        send_to_mcp(&mut guard, payload.as_bytes(), expects_response)?
    };

    if let Some(line) = response_line {
        respond_json(request, 200, &line);
    } else {
        respond_json(request, 202, "{\"status\":\"accepted\"}");
    }
    Ok(())
}

fn forward_to_sync(request: Request, args: &Args) -> Result<()> {
    let mut request = request;
    let mut body = Vec::new();
    request
        .as_reader()
        .read_to_end(&mut body)
        .context("read proxy body")?;

    let base = args.sync_url.trim_end_matches('/');
    let url = format!("{base}{}", request.url());
    let method = method_string(request.method());
    let mut proxy = ureq::request(&method, &url);

    for header in request.headers() {
        let name = header.field.as_str();
        let name_str = name.as_str();
        if name_str.eq_ignore_ascii_case("host") || name_str.eq_ignore_ascii_case("content-length") {
            continue;
        }
        proxy = proxy.set(name_str, header.value.as_str());
    }

    let response = match proxy.send_bytes(&body) {
        Ok(resp) => resp,
        Err(ureq::Error::Status(_, resp)) => resp,
        Err(err) => {
            respond_json(
                request,
                502,
                &format!("{{\"error\":\"proxy_failed\",\"detail\":\"{err}\"}}"),
            );
            return Ok(());
        }
    };

    let status = response.status();
    let content_type = response.header("Content-Type").map(|value| value.to_string());
    let mut response_body = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut response_body)
        .context("read proxy response")?;

    let mut reply = Response::from_data(response_body).with_status_code(StatusCode(status));
    if let Some(content_type) = content_type {
        if let Ok(header) = Header::from_bytes("Content-Type", content_type.as_bytes()) {
            reply = reply.with_header(header);
        }
    }
    request.respond(reply)?;
    Ok(())
}

fn send_to_mcp(
    process: &mut McpProcess,
    payload: &[u8],
    expects_response: bool,
) -> Result<Option<String>> {
    process
        .stdin
        .write_all(payload)
        .context("write to ce-mcp")?;
    process.stdin.write_all(b"\n")?;
    process.stdin.flush()?;

    if expects_response {
        let mut line = String::new();
        let bytes = process.stdout.read_line(&mut line)?;
        if bytes == 0 {
            return Err(anyhow!("ce-mcp closed stdout"));
        }
        Ok(Some(line.trim_end().to_string()))
    } else {
        Ok(None)
    }
}

fn has_json_id(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.get("id").is_some(),
        Value::Array(values) => values.iter().any(has_json_id),
        _ => false,
    }
}

fn header_value(request: &Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().to_string())
}

fn respond_json(request: Request, status: u16, body: &str) {
    let response = Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
    let _ = request.respond(response);
}

fn method_string(method: &Method) -> String {
    match method {
        Method::Get => "GET".to_string(),
        Method::Head => "HEAD".to_string(),
        Method::Post => "POST".to_string(),
        Method::Put => "PUT".to_string(),
        Method::Delete => "DELETE".to_string(),
        Method::Connect => "CONNECT".to_string(),
        Method::Options => "OPTIONS".to_string(),
        Method::Trace => "TRACE".to_string(),
        Method::Patch => "PATCH".to_string(),
        Method::NonStandard(value) => value.to_string(),
    }
}

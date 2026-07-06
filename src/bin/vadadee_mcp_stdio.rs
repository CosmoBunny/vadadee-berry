//! MCP stdio ↔ Vadadee Berry TCP JSON-RPC bridge (replaces the Python script).
//!
//! Handshake and tool discovery are answered locally; tool calls forward to the
//! editor TCP bridge on 127.0.0.1:17345 (override with `VADADEE_MCP_PORT`).
//!
//! Configure in Grok / Cursor:
//!   command: /path/to/vadadee-mcp-stdio
//!   (no args)

#![cfg(not(target_os = "android"))]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

fn main() {
    let port = std::env::var("VADADEE_MCP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(vadadee_berry::mcp::DEFAULT_MCP_PORT);
    let host = std::env::var("VADADEE_MCP_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let addr = format!("{host}:{port}");
    let stdin = BufReader::new(std::io::stdin());
    let mut stdout = std::io::stdout();

    for line in stdin.lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match vadadee_berry::mcp::try_answer_stdio_line(line) {
            Some(None) => continue,
            Some(Some(response)) => {
                if writeln!(stdout, "{response}").is_err() {
                    break;
                }
                let _ = stdout.flush();
            }
            None => {
                let response = match forward_line(&addr, line) {
                    Ok(r) => r,
                    Err(e) => {
                        let mut err = serde_json::json!({
                            "jsonrpc": "2.0",
                            "error": {
                                "code": -32000,
                                "message": format!(
                                    "Cannot reach Vadadee Berry MCP at {addr} — is the app running? ({e})"
                                )
                            },
                            "id": null
                        });
                        if let Ok(req) = serde_json::from_str::<serde_json::Value>(line) {
                            if let Some(id) = req.get("id") {
                                err["id"] = id.clone();
                            }
                        }
                        err.to_string()
                    }
                };
                if writeln!(stdout, "{response}").is_err() {
                    break;
                }
                let _ = stdout.flush();
            }
        }
    }
}

fn forward_line(addr: &str, line: &str) -> Result<String, std::io::Error> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;
    writeln!(stream, "{line}")?;
    let mut reader = BufReader::new(stream);
    let mut out = String::new();
    reader.read_line(&mut out)?;
    Ok(out.trim_end().to_string())
}
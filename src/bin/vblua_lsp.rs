//! `vblua-lsp` — LSP adapter over the VBLua API model (stdio JSON-RPC).
//!
//! Transport: `Content-Length` frames on stdin/stdout. **Never log to
//! stdout** (it carries protocol); diagnostics go to stderr.
//! Capabilities: full text sync, diagnostics, completion, hover.
//! NOT implemented: definition, references, rename, formatting (the server
//! does not advertise them).

use std::io::{BufRead, Read, Write};

use vadadee_berry::vblua::lsp::LspServer;

fn log(msg: &str) {
    eprintln!("[vblua-lsp] {msg}");
}

fn read_message(input: &mut impl BufRead) -> Option<serde_json::Value> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim().to_string();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok();
        }
    }
    let len = len?;
    let mut buf = vec![0u8; len];
    input.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

fn write_message(output: &mut impl Write, payload: &serde_json::Value) {
    let body = payload.to_string();
    // Stdout carries protocol ONLY — no logs here, ever.
    let _ = write!(output, "Content-Length: {}\r\n\r\n{body}", body.len());
    let _ = output.flush();
}

fn respond(output: &mut impl Write, id: &serde_json::Value, result: serde_json::Value) {
    write_message(
        output,
        &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    );
}

fn respond_error(output: &mut impl Write, id: &serde_json::Value, code: i32, message: &str) {
    write_message(
        output,
        &serde_json::json!({
            "jsonrpc": "2.0", "id": id,
            "error": { "code": code, "message": message },
        }),
    );
}

fn notify_diagnostics(
    output: &mut impl Write,
    uri: &str,
    diags: Vec<lsp_types::Diagnostic>,
) {
    write_message(
        output,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": { "uri": uri, "diagnostics": diags },
        }),
    );
}

fn position_of(params: &serde_json::Value) -> (String, u32, u32) {
    let uri = params
        .pointer("/textDocument/uri")
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();
    let line = params
        .pointer("/position/line")
        .and_then(|n| n.as_u64())
        .unwrap_or(0) as u32;
    let character = params
        .pointer("/position/character")
        .and_then(|n| n.as_u64())
        .unwrap_or(0) as u32;
    (uri, line, character)
}

fn main() {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut output = std::io::stdout();
    let mut server = LspServer::new();

    log("listening on stdio");
    while let Some(msg) = read_message(&mut input) {
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = msg.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
        match method {
            "initialize" => {
                respond(
                    &mut output,
                    &id,
                    serde_json::json!({
                        "capabilities": LspServer::capabilities(),
                        "serverInfo": { "name": "vblua-lsp", "version": vadadee_berry::vblua::VBLUA_VERSION },
                    }),
                );
            }
            "initialized" | "$/cancelRequest" => {}
            "shutdown" => {
                respond(&mut output, &id, serde_json::Value::Null);
            }
            "exit" => break,
            "textDocument/didOpen" => {
                let uri = params
                    .pointer("/textDocument/uri")
                    .and_then(|u| u.as_str())
                    .unwrap_or("");
                let text = params
                    .pointer("/textDocument/text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                notify_diagnostics(&mut output, uri, server.open(uri.to_string(), text.to_string()));
            }
            "textDocument/didChange" => {
                let uri = params
                    .pointer("/textDocument/uri")
                    .and_then(|u| u.as_str())
                    .unwrap_or("");
                let mut diags = Vec::new();
                if let Some(changes) = params.pointer("/contentChanges").and_then(|c| c.as_array())
                    && let Some(last) = changes.last()
                    && let Some(text) = last.get("text").and_then(|t| t.as_str())
                {
                    diags = server.change(uri, text.to_string());
                }
                notify_diagnostics(&mut output, uri, diags);
            }
            "textDocument/didClose" => {
                if let Some(uri) = params.pointer("/textDocument/uri").and_then(|u| u.as_str()) {
                    server.close(uri);
                }
            }
            "textDocument/completion" => {
                let (uri, line, character) = position_of(&params);
                let prefix = server.word_at(&uri, line, character);
                let items = server.complete(&prefix);
                respond(
                    &mut output,
                    &id,
                    serde_json::json!({ "isIncomplete": false, "items": items }),
                );
            }
            "textDocument/hover" => {
                let (uri, line, character) = position_of(&params);
                let word = server.word_at(&uri, line, character);
                match server.hover(&word) {
                    Some(contents) => respond(
                        &mut output,
                        &id,
                        serde_json::json!({
                            "contents": { "kind": "markdown", "value": contents },
                        }),
                    ),
                    None => respond(&mut output, &id, serde_json::Value::Null),
                }
            }
            _ => {
                if !id.is_null() {
                    respond_error(&mut output, &id, -32601, "method not implemented");
                }
            }
        }
    }
    log("bye");
}

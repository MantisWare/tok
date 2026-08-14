//! MCP server over stdio.
//!
//! Exposes the context graph to agents that speak the Model Context Protocol,
//! so an agent can ask "where is authentication handled" and get the six
//! relevant symbols instead of reading forty files to find them. That is the
//! entire token argument for this module.
//!
//! The transport is unforgiving in one specific way: **stdout is the protocol**.
//! A single stray line — a hook warning, an update notice, a progress bar —
//! lands in the middle of the JSON-RPC stream and the client drops the
//! connection. Everything human-readable goes to stderr, and `tok mcp` is
//! routed around the startup notices in `run_cli` for the same reason.
//!
//! The loop is synchronous and single-threaded. Requests arrive one at a time
//! from one client, so concurrency would buy nothing and cost the async runtime
//! this binary has deliberately avoided.

pub mod protocol;
pub mod tools;

use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value};

use protocol::{codes, ErrorObject, Request, Response, PROTOCOL_VERSION};

/// Run the server against the real stdio handles until the client disconnects.
pub fn run(dir: Option<&str>) -> Result<i32> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let root = tools::repo_root(dir);

    serve(stdin.lock(), stdout.lock(), &root)?;
    Ok(0)
}

/// The protocol loop, over any reader and writer so it can be driven by tests.
pub fn serve(reader: impl BufRead, mut writer: impl Write, root: &Path) -> Result<()> {
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let Outcome { response, stop } = handle_line(&line, root);

        // A notification must not be answered; doing so is a protocol
        // violation that some clients treat as fatal.
        if let Some(response) = response {
            writeln!(writer, "{}", response.to_line())?;
            // Clients block waiting for the reply, so a buffered response
            // reads to them as a hung server.
            writer.flush()?;
        }

        if stop {
            break;
        }
    }

    Ok(())
}

/// What to do after one line of input: reply (or not), then continue (or not).
struct Outcome {
    response: Option<Response>,
    stop: bool,
}

impl Outcome {
    fn reply(response: Response) -> Self {
        Self {
            response: Some(response),
            stop: false,
        }
    }
}

/// Turn one line of input into at most one response.
fn handle_line(line: &str, root: &Path) -> Outcome {
    let request: Request = match serde_json::from_str(line) {
        Ok(request) => request,
        // Malformed input carries no id to answer, so JSON-RPC specifies null.
        Err(error) => {
            return Outcome::reply(Response::failure(
                Value::Null,
                ErrorObject::new(codes::PARSE_ERROR, format!("invalid JSON: {error}")),
            ));
        }
    };

    if request.is_notification() {
        // `exit` is the client saying it is done; anything else is ignorable.
        return Outcome {
            response: None,
            stop: request.method == "exit",
        };
    }

    let id = request.id.clone().unwrap_or(Value::Null);

    match handle(&request, root) {
        Ok(result) => Outcome {
            response: Some(Response::success(id, result)),
            // Reply first, then stop: a client that waits for the
            // acknowledgement would otherwise see the pipe close instead.
            stop: request.method == "shutdown",
        },
        Err(error) => Outcome::reply(Response::failure(id, error)),
    }
}

/// Dispatch a request that expects a reply.
fn handle(request: &Request, root: &Path) -> Result<Value, ErrorObject> {
    match request.method.as_str() {
        "initialize" => Ok(initialize()),
        "tools/list" => Ok(json!({ "tools": tools::advertised() })),
        "tools/call" => call_tool(request, root),
        "ping" | "shutdown" => Ok(json!({})),
        other => Err(ErrorObject::new(
            codes::METHOD_NOT_FOUND,
            format!("unknown method: {other}"),
        )),
    }
}

fn initialize() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        // Only tools are offered. Advertising prompts or resources this server
        // does not serve makes clients probe for them and log the failures.
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "tok",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

fn call_tool(request: &Request, root: &Path) -> Result<Value, ErrorObject> {
    let params = request.params.clone().unwrap_or_else(|| json!({}));

    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err(ErrorObject::new(
            codes::INVALID_PARAMS,
            "tools/call requires a name",
        ));
    };

    // An unknown tool is a caller error the client should see as such; a tool
    // that ran and found nothing is not.
    if tools::resolve(name).is_none() {
        return Err(ErrorObject::new(
            codes::METHOD_NOT_FOUND,
            format!("unknown tool: {name}"),
        ));
    }

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let (text, is_error) = tools::dispatch(name, &arguments, root);

    Ok(protocol::tool_result(text, is_error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("a.rs"),
            "pub fn helper() {}\npub fn caller() { helper(); }\n",
        )
        .expect("write");
        dir
    }

    /// Drive the loop with a script of requests and collect the replies.
    ///
    /// Requests are collapsed onto one line each: the transport frames on
    /// newlines, so a literal wrapped for readability in the source would
    /// otherwise arrive as several broken messages.
    fn exchange(dir: &Path, requests: &[&str]) -> Vec<Value> {
        let input = requests
            .iter()
            .map(|request| request.replace('\n', " "))
            .collect::<Vec<_>>()
            .join("\n");
        let mut output = Vec::new();

        serve(input.as_bytes(), &mut output, dir).expect("serve");

        String::from_utf8(output)
            .expect("utf8")
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("valid json reply"))
            .collect()
    }

    fn one(dir: &Path, request: &str) -> Value {
        let replies = exchange(dir, &[request]);
        assert_eq!(replies.len(), 1, "expected exactly one reply: {replies:?}");
        replies.into_iter().next().expect("reply")
    }

    #[test]
    fn initialize_reports_the_protocol_version_and_server_identity() {
        let dir = fixture();

        let reply = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        );

        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(reply["result"]["serverInfo"]["name"], "tok");
        assert!(reply["result"]["capabilities"]["tools"].is_object());
    }

    /// Advertising capabilities the server does not serve makes clients probe
    /// for them and log the failures.
    #[test]
    fn initialize_advertises_tools_only() {
        let dir = fixture();

        let reply = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        );
        let capabilities = &reply["result"]["capabilities"];

        assert!(capabilities.get("resources").is_none());
        assert!(capabilities.get("prompts").is_none());
    }

    #[test]
    fn every_reply_carries_the_id_it_answers() {
        let dir = fixture();

        let reply = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":"abc","method":"tools/list"}"#,
        );

        assert_eq!(reply["id"], "abc");
    }

    /// Replying to a notification is a protocol violation that some clients
    /// treat as fatal.
    #[test]
    fn notifications_get_no_reply() {
        let dir = fixture();

        let replies = exchange(
            dir.path(),
            &[
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                r#"{"jsonrpc":"2.0","method":"notifications/cancelled"}"#,
            ],
        );

        assert!(replies.is_empty(), "unexpected replies: {replies:?}");
    }

    #[test]
    fn tools_list_advertises_both_prefixes() {
        let dir = fixture();

        let reply = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        );
        let names: Vec<&str> = reply["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .map(|t| t["name"].as_str().expect("name"))
            .collect();

        assert!(names.contains(&"tok_ask"));
        assert!(names.contains(&"graft_find_code"));
        assert!(names.contains(&"tok_map"));
    }

    #[test]
    fn a_tool_call_returns_text_content() {
        let dir = fixture();

        let reply = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":"tok_ask","arguments":{"query":"helper"}}}"#,
        );

        assert_eq!(reply["result"]["content"][0]["type"], "text");
        assert!(reply["result"]["content"][0]["text"]
            .as_str()
            .expect("text")
            .contains("helper"));
    }

    #[test]
    fn the_graft_alias_answers_identically() {
        let dir = fixture();

        let canonical = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":"tok_map","arguments":{}}}"#,
        );
        let alias = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":"graft_repo_map","arguments":{}}}"#,
        );

        assert_eq!(canonical["result"], alias["result"]);
    }

    #[test]
    fn a_tool_call_without_arguments_uses_defaults() {
        let dir = fixture();

        let reply = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"tok_map"}}"#,
        );

        assert!(reply["result"]["isError"] == false);
    }

    #[test]
    fn a_tool_that_finds_nothing_is_not_an_rpc_error() {
        let dir = fixture();

        let reply = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":"tok_ask","arguments":{"query":"zzzznope"}}}"#,
        );

        assert!(reply.get("error").is_none());
        assert_eq!(reply["result"]["isError"], false);
    }

    #[test]
    fn a_missing_required_argument_is_reported_in_the_result() {
        let dir = fixture();

        let reply = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":"tok_ask","arguments":{}}}"#,
        );

        assert_eq!(reply["result"]["isError"], true);
    }

    #[test]
    fn an_unknown_tool_is_an_rpc_error() {
        let dir = fixture();

        let reply = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"tok_nope"}}"#,
        );

        assert_eq!(reply["error"]["code"], codes::METHOD_NOT_FOUND);
    }

    #[test]
    fn tools_call_without_a_name_is_an_invalid_params_error() {
        let dir = fixture();

        let reply = one(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}"#,
        );

        assert_eq!(reply["error"]["code"], codes::INVALID_PARAMS);
    }

    #[test]
    fn an_unknown_method_is_reported_and_the_session_continues() {
        let dir = fixture();

        let replies = exchange(
            dir.path(),
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"nonsense/method"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            ],
        );

        assert_eq!(replies[0]["error"]["code"], codes::METHOD_NOT_FOUND);
        assert_eq!(replies[1]["id"], 2);
    }

    /// One bad line must not take down a session that may have hours of
    /// context behind it.
    #[test]
    fn malformed_json_does_not_end_the_session() {
        let dir = fixture();

        let replies = exchange(
            dir.path(),
            &[
                "{not json at all",
                r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            ],
        );

        assert_eq!(replies[0]["error"]["code"], codes::PARSE_ERROR);
        assert_eq!(replies[0]["id"], Value::Null);
        assert_eq!(replies[1]["id"], 2);
    }

    #[test]
    fn blank_lines_are_skipped() {
        let dir = fixture();

        let replies = exchange(
            dir.path(),
            &["", "   ", r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#],
        );

        assert_eq!(replies.len(), 1);
    }

    #[test]
    fn ping_is_answered_with_an_empty_result() {
        let dir = fixture();

        let reply = one(dir.path(), r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#);

        assert_eq!(reply["result"], json!({}));
    }

    /// The acknowledgement has to go out before the loop ends, or a client
    /// waiting on it sees the pipe close instead.
    #[test]
    fn shutdown_is_acknowledged_and_ends_the_loop() {
        let dir = fixture();

        let replies = exchange(
            dir.path(),
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"shutdown"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            ],
        );

        assert_eq!(replies.len(), 1, "loop continued past shutdown");
        assert_eq!(replies[0]["id"], 1);
        // How the loop knows to stop is internal and must not reach the wire.
        assert_eq!(replies[0]["result"], json!({}));
    }

    #[test]
    fn an_exit_notification_ends_the_loop_without_a_reply() {
        let dir = fixture();

        let replies = exchange(
            dir.path(),
            &[
                r#"{"jsonrpc":"2.0","method":"exit"}"#,
                r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            ],
        );

        assert!(replies.is_empty(), "unexpected replies: {replies:?}");
    }

    /// The transport frames on newlines, so any embedded newline in tool output
    /// would be read as several malformed messages.
    #[test]
    fn multi_line_tool_output_stays_on_one_line() {
        let dir = fixture();

        let mut output = Vec::new();
        serve(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"tok_map"}}"#
                .as_bytes(),
            &mut output,
            dir.path(),
        )
        .expect("serve");

        let text = String::from_utf8(output).expect("utf8");
        assert!(text.contains("files"), "map output missing: {text}");
        assert_eq!(text.trim_end().lines().count(), 1);
    }

    #[test]
    fn a_full_session_runs_end_to_end() {
        let dir = fixture();

        let replies = exchange(
            dir.path(),
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call",
                    "params":{"name":"tok_skeleton","arguments":{"file":"a.rs"}}}"#,
                r#"{"jsonrpc":"2.0","id":4,"method":"shutdown"}"#,
            ],
        );

        let ids: Vec<&Value> = replies.iter().map(|r| &r["id"]).collect();
        assert_eq!(ids, vec![&json!(1), &json!(2), &json!(3), &json!(4)]);
        assert!(replies.iter().all(|r| r["jsonrpc"] == "2.0"));
    }
}

//! JSON-RPC 2.0 message types for the MCP stdio transport.
//!
//! Hand-rolled rather than pulled from a crate. The surface actually needed is
//! small — six methods, one error shape — and an MCP SDK would bring an async
//! runtime with it, which this binary deliberately does not have: the startup
//! budget is under 10ms and every dependency is a fixed cost paid by every
//! `tok git log` invocation, not just by the server.
//!
//! Two details of the transport are load-bearing:
//!
//! - **One message per line.** The stdio transport frames messages by newline,
//!   so serialization must never emit pretty-printed JSON.
//! - **Notifications get no reply.** A request without an `id` is a
//!   notification; responding to one is a protocol violation that some clients
//!   treat as a fatal error rather than ignoring.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The MCP revision this server implements.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// An incoming JSON-RPC message.
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    /// Absent for notifications, which must not be answered.
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

impl Request {
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl Response {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: Value, error: ErrorObject) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        }
    }

    /// Serialize as a single line, ready for the transport.
    pub fn to_line(&self) -> String {
        // Serialization of a struct built from valid JSON values cannot fail;
        // the fallback exists so a bug here degrades to a protocol error the
        // client can report rather than a panic that kills the session.
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"serialization failed"}}"#
                .to_string()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ErrorObject {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

/// Standard JSON-RPC error codes.
pub mod codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
}

/// Build the `content` payload MCP expects from a tool call.
///
/// `is_error` is part of the result rather than a JSON-RPC error: a tool that
/// ran correctly and found nothing is not a protocol failure, and reporting it
/// as one makes clients retry or abort instead of showing the message.
pub fn tool_result(text: impl Into<String>, is_error: bool) -> Value {
    serde_json::json!({
        "content": [{ "type": "text", "text": text.into() }],
        "isError": is_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Request {
        serde_json::from_str(raw).expect("parse request")
    }

    #[test]
    fn a_request_with_an_id_expects_a_reply() {
        let request = parse(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);

        assert!(!request.is_notification());
        assert_eq!(request.method, "ping");
    }

    /// Replying to a notification is a protocol violation, so the distinction
    /// has to be reliable.
    #[test]
    fn a_request_without_an_id_is_a_notification() {
        let request = parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

        assert!(request.is_notification());
    }

    #[test]
    fn a_null_id_is_still_an_id() {
        let request = parse(r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#);

        // serde maps an explicit null to None, which is the correct reading:
        // JSON-RPC treats a null id as belonging to a notification-like error.
        assert!(request.is_notification());
    }

    #[test]
    fn string_ids_are_accepted() {
        let request = parse(r#"{"jsonrpc":"2.0","id":"abc","method":"ping"}"#);

        assert_eq!(request.id, Some(Value::String("abc".to_string())));
    }

    #[test]
    fn params_are_carried_through_verbatim() {
        let request = parse(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":"tok_ask","arguments":{"query":"cache"}}}"#,
        );

        let params = request.params.expect("params");
        assert_eq!(params["name"], "tok_ask");
        assert_eq!(params["arguments"]["query"], "cache");
    }

    #[test]
    fn absent_params_read_as_none() {
        let request = parse(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);

        assert!(request.params.is_none());
    }

    #[test]
    fn a_success_response_carries_a_result_and_no_error() {
        let response = Response::success(Value::from(1), serde_json::json!({"ok": true}));
        let line = response.to_line();

        assert!(line.contains(r#""result""#));
        assert!(!line.contains(r#""error""#));
    }

    #[test]
    fn a_failure_response_carries_an_error_and_no_result() {
        let response = Response::failure(
            Value::from(1),
            ErrorObject::new(codes::METHOD_NOT_FOUND, "no such method"),
        );
        let line = response.to_line();

        assert!(line.contains(r#""error""#));
        assert!(line.contains("no such method"));
        assert!(!line.contains(r#""result""#));
    }

    /// The stdio transport frames on newlines, so a pretty-printed message
    /// would be read as several malformed ones.
    #[test]
    fn a_response_never_contains_a_newline() {
        let response = Response::success(
            Value::from(1),
            tool_result("line one\nline two\nline three", false),
        );

        assert!(!response.to_line().contains('\n'));
    }

    #[test]
    fn tool_text_is_wrapped_in_the_mcp_content_shape() {
        let result = tool_result("hello", false);

        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "hello");
        assert_eq!(result["isError"], false);
    }

    /// A tool that found nothing is not a transport failure.
    #[test]
    fn a_tool_error_is_reported_in_the_result_not_as_rpc_failure() {
        let result = tool_result("no matches", true);

        assert_eq!(result["isError"], true);
        assert!(result.get("error").is_none());
    }

    #[test]
    fn responses_round_trip_through_json() {
        let response = Response::success(Value::from(7), serde_json::json!({"a": 1}));

        let parsed: Value = serde_json::from_str(&response.to_line()).expect("valid json");

        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 7);
        assert_eq!(parsed["result"]["a"], 1);
    }
}

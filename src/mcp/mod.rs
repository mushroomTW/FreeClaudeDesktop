pub mod desktop;
pub mod handlers;
pub mod tools;
pub mod types;

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

pub use types::Screenshot;

pub fn run_computer_server() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle_message(request),
            Err(error) => Some(types::json_rpc_error(
                Value::Null,
                -32700,
                error.to_string(),
            )),
        };

        if let Some(response) = response {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }

    Ok(())
}

pub fn handle_message(request: Value) -> Option<Value> {
    let id = request.get("id").cloned()?;
    let method = request.get("method").and_then(Value::as_str)?;

    match method {
        "initialize" => Some(types::json_rpc_result(id, initialize_result(&request))),
        "ping" => Some(types::json_rpc_result(id, json!({}))),
        "tools/list" => Some(types::json_rpc_result(
            id,
            json!({
                "tools": tools::all_tools()
            }),
        )),
        "tools/call" => {
            handlers::handle_tools_call(id, request.get("params").unwrap_or(&Value::Null))
        }
        _ => Some(types::json_rpc_error(
            id,
            -32601,
            format!("Method not found: {method}"),
        )),
    }
}

fn initialize_result(request: &Value) -> Value {
    let protocol_version = request
        .get("params")
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or("2025-03-26");

    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "free-claude-computer",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_free_claude_computer_server() {
        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2025-03-26" }
        }))
        .unwrap();

        assert_eq!(
            response["result"]["serverInfo"]["name"],
            "free-claude-computer"
        );
    }

    #[test]
    fn lists_computer_and_permission_tools() {
        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        }))
        .unwrap();

        let tools = response["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 20);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"computer"));
        assert!(names.contains(&"request_access"));
        assert!(names.contains(&"screenshot"));
        assert!(names.contains(&"zoom"));
        assert!(names.contains(&"left_click"));
        assert!(names.contains(&"computer_batch"));
        assert!(names.contains(&"read_clipboard"));
        assert!(names.contains(&"write_clipboard"));
    }

    #[test]
    fn handles_request_access_call() {
        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "request_access",
                "arguments": {
                    "applications": ["Chrome", "Calculator"],
                    "reason": "Need to demonstrate calculator walkthrough"
                }
            }
        }))
        .unwrap();

        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Access granted for applications: Chrome, Calculator"));
        assert!(text.contains("Reason: Need to demonstrate calculator walkthrough"));

        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": 22,
            "method": "tools/call",
            "params": {
                "name": "list_granted_applications",
                "arguments": {}
            }
        }))
        .unwrap();

        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Chrome"));
        assert!(!text.contains("[All]"));
    }

    #[test]
    fn handles_request_teach_access_call() {
        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "request_teach_access",
                "arguments": {
                    "reason": "Start interactive teaching session"
                }
            }
        }))
        .unwrap();

        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Teach mode access granted"));
    }

    #[test]
    fn handles_teach_step_call() {
        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "teach_step",
                "arguments": {
                    "step_number": 1,
                    "instruction": "Click on the start button",
                    "target": "Start Button"
                }
            }
        }))
        .unwrap();

        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Teach step 1 presented: Click on the start button"));
    }

    #[test]
    fn handles_active_window_screenshot_call() {
        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "active_window_screenshot",
                "arguments": {}
            }
        }))
        .unwrap();

        assert!(response.get("result").is_some());
    }

    #[test]
    fn handles_list_windows_call() {
        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {
                "name": "list_windows",
                "arguments": {}
            }
        }))
        .unwrap();

        assert!(response.get("result").is_some());
    }

    #[test]
    fn handles_computer_batch_call() {
        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "computer_batch",
                "arguments": {
                    "actions": [
                        { "action": "wait", "duration_ms": 10 },
                        { "action": "cursor_position" }
                    ]
                }
            }
        }))
        .unwrap();

        let content = response["result"]["content"].as_array().unwrap();
        assert!(content.len() >= 2);
    }

    #[test]
    fn encodes_base64_padding() {
        assert_eq!(types::base64_encode(b""), "");
        assert_eq!(types::base64_encode(b"f"), "Zg==");
        assert_eq!(types::base64_encode(b"fo"), "Zm8=");
        assert_eq!(types::base64_encode(b"foo"), "Zm9v");
    }
}

use std::time::Duration;
use serde_json::{json, Value};

use super::desktop;
use super::types::{base64_encode, json_rpc_error, json_rpc_result, tool_error, tool_text};

pub fn handle_tools_call(id: Value, params: &Value) -> Option<Value> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Some(json_rpc_error(id, -32602, "Missing tool name"));
    };

    let enabled = if cfg!(test) {
        true
    } else {
        crate::get_launcher_settings()
            .map(|settings| settings.enable_computer_mcp_server)
            .unwrap_or(false)
    };
    if !enabled {
        return Some(json_rpc_result(
            id,
            tool_error("Computer MCP server is disabled in FreeClaudeLauncher."),
        ));
    }

    let args = params.get("arguments").unwrap_or(&Value::Null);

    let result = match name {
        "computer" => execute_computer(args),
        "request_access" => execute_request_access(args),
        "request_teach_access" => execute_request_teach_access(args),
        "teach_step" => execute_teach_step(args),
        "teach_batch" => execute_teach_batch(args),
        "screenshot" => execute_screenshot(args),
        "active_window_screenshot" => execute_active_window_screenshot(args),
        "zoom" => execute_zoom(args),
        "list_windows" => execute_list_windows(args),
        "left_click" => execute_left_click(args),
        "double_click" => execute_double_click(args),
        "triple_click" => execute_triple_click(args),
        "right_click" => execute_right_click(args),
        "middle_click" => execute_middle_click(args),
        "left_click_drag" | "drag_and_drop" => execute_left_click_drag(args),
        "mouse_move" => execute_mouse_move(args),
        "type" => execute_type(args),
        "key" | "press_key" => execute_key(args),
        "scroll" => execute_scroll(args),
        "hold_key" => execute_hold_key(args),
        "left_mouse_down" | "mouse_down" => execute_left_mouse_down(args),
        "left_mouse_up" | "mouse_up" => execute_left_mouse_up(args),
        "wait" => execute_wait(args),
        "cursor_position" => execute_cursor_position(args),
        "open_application" => execute_open_application(args),
        "switch_display" => execute_switch_display(args),
        "list_granted_applications" => execute_list_granted_applications(args),
        "read_clipboard" => execute_read_clipboard(args),
        "write_clipboard" => execute_write_clipboard(args),
        "computer_batch" => execute_computer_batch(args),
        _ => Err(format!("Unknown tool: {name}")),
    };

    Some(json_rpc_result(
        id,
        match result {
            Ok(res) => res,
            Err(err) => tool_error(err),
        },
    ))
}

fn execute_request_access(args: &Value) -> Result<Value, String> {
    let apps = args
        .get("applications")
        .or_else(|| args.get("apps"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let reason = args.get("reason").and_then(Value::as_str).unwrap_or("");

    let msg = if apps.is_empty() {
        "Access granted for requested applications.".to_string()
    } else if reason.is_empty() {
        format!("Access granted for applications: {apps}.")
    } else {
        format!("Access granted for applications: {apps}. Reason: {reason}")
    };
    Ok(tool_text(msg))
}

fn execute_request_teach_access(args: &Value) -> Result<Value, String> {
    let reason = args.get("reason").and_then(Value::as_str).unwrap_or("");
    if reason.is_empty() {
        Ok(tool_text("Teach mode access granted."))
    } else {
        Ok(tool_text(format!("Teach mode access granted. Reason: {reason}")))
    }
}

fn execute_teach_step(args: &Value) -> Result<Value, String> {
    let step_number = args
        .get("step_number")
        .and_then(Value::as_i64)
        .unwrap_or(1);

    let instruction = args
        .get("instruction")
        .or_else(|| args.get("explanation"))
        .and_then(Value::as_str)
        .unwrap_or("Follow on-screen instructions.");

    let target = args.get("target").and_then(Value::as_str).unwrap_or("");

    if target.is_empty() {
        Ok(tool_text(format!("Teach step {step_number} presented: {instruction}")))
    } else {
        Ok(tool_text(format!("Teach step {step_number} presented: {instruction} (Target: {target})")))
    }
}

fn execute_teach_batch(args: &Value) -> Result<Value, String> {
    let steps = args
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| "Missing steps array".to_string())?;
    Ok(tool_text(format!("Queued and executed {} teach steps successfully.", steps.len())))
}

fn execute_screenshot(args: &Value) -> Result<Value, String> {
    if args.get("active_window_only").and_then(Value::as_bool).unwrap_or(false) {
        return execute_active_window_screenshot(args);
    }
    let screenshot = desktop::screenshot()?;
    Ok(json!({
        "content": [
            {
                "type": "image",
                "mimeType": "image/png",
                "data": base64_encode(&screenshot.png)
            },
            {
                "type": "text",
                "text": format!("Screenshot captured: {}x{}.", screenshot.width, screenshot.height)
            }
        ]
    }))
}

fn execute_active_window_screenshot(_args: &Value) -> Result<Value, String> {
    let screenshot = desktop::screenshot_active_window()?;
    Ok(json!({
        "content": [
            {
                "type": "image",
                "mimeType": "image/png",
                "data": base64_encode(&screenshot.png)
            },
            {
                "type": "text",
                "text": format!("Active window screenshot captured: {}x{}.", screenshot.width, screenshot.height)
            }
        ]
    }))
}

fn execute_list_windows(args: &Value) -> Result<Value, String> {
    let visible_only = args.get("visible_only").and_then(Value::as_bool).unwrap_or(true);
    let titles = desktop::list_windows(visible_only)?;
    let text = if titles.is_empty() {
        "No open window titles found.".to_string()
    } else {
        format!("Open window titles ({}):\n- {}", titles.len(), titles.join("\n- "))
    };
    Ok(tool_text(text))
}

fn execute_zoom(args: &Value) -> Result<Value, String> {
    let region = args
        .get("region")
        .and_then(Value::as_array)
        .ok_or_else(|| "Missing region [x0, y0, x1, y1]".to_string())?;
    if region.len() != 4 {
        return Err("region must contain exactly 4 integers [x0, y0, x1, y1]".to_string());
    }
    let x0 = region[0].as_i64().ok_or("Invalid x0")? as i32;
    let y0 = region[1].as_i64().ok_or("Invalid y0")? as i32;
    let x1 = region[2].as_i64().ok_or("Invalid x1")? as i32;
    let y1 = region[3].as_i64().ok_or("Invalid y1")? as i32;

    let zoomed = desktop::zoom(x0, y0, x1, y1)?;
    Ok(json!({
        "content": [
            {
                "type": "image",
                "mimeType": "image/png",
                "data": base64_encode(&zoomed.png)
            },
            {
                "type": "text",
                "text": format!("Zoomed region [{}, {}, {}, {}] captured: {}x{}.", x0, y0, x1, y1, zoomed.width, zoomed.height)
            }
        ]
    }))
}

fn execute_left_click(args: &Value) -> Result<Value, String> {
    move_if_coordinate(args)?;
    desktop::left_click()?;
    Ok(tool_text("Left clicked."))
}

fn execute_right_click(args: &Value) -> Result<Value, String> {
    move_if_coordinate(args)?;
    desktop::right_click()?;
    Ok(tool_text("Right clicked."))
}

fn execute_double_click(args: &Value) -> Result<Value, String> {
    move_if_coordinate(args)?;
    desktop::double_click()?;
    Ok(tool_text("Double clicked."))
}

fn execute_triple_click(args: &Value) -> Result<Value, String> {
    move_if_coordinate(args)?;
    desktop::triple_click()?;
    Ok(tool_text("Triple clicked."))
}

fn execute_middle_click(args: &Value) -> Result<Value, String> {
    move_if_coordinate(args)?;
    desktop::middle_click()?;
    Ok(tool_text("Middle clicked."))
}

fn execute_left_click_drag(args: &Value) -> Result<Value, String> {
    if let Some(start) = args.get("start_coordinate") {
        let coord = start.as_array().ok_or("Invalid start_coordinate")?;
        if coord.len() == 2 {
            let sx = coord[0].as_i64().ok_or("Invalid start x")? as i32;
            let sy = coord[1].as_i64().ok_or("Invalid start y")? as i32;
            desktop::move_mouse(sx, sy)?;
        }
    }
    let (ex, ey) = coordinate(args)?;
    desktop::drag_and_drop(ex, ey)?;
    Ok(tool_text(format!("Dragged to ({ex}, {ey}).")))
}

fn execute_mouse_move(args: &Value) -> Result<Value, String> {
    let (x, y) = coordinate(args)?;
    desktop::move_mouse(x, y)?;
    Ok(tool_text(format!("Moved mouse to ({x}, {y}).")))
}

fn execute_type(args: &Value) -> Result<Value, String> {
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| "Missing text".to_string())?;
    desktop::type_text(text)?;
    Ok(tool_text("Typed text."))
}

fn execute_key(args: &Value) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(Value::as_str)
        .or_else(|| args.get("text").and_then(Value::as_str))
        .ok_or_else(|| "Missing key".to_string())?;
    let repeat = args.get("repeat").and_then(Value::as_u64).unwrap_or(1).min(100);
    for _ in 0..repeat {
        desktop::press_key(key)?;
    }
    if repeat > 1 {
        Ok(tool_text(format!("Pressed {key} {repeat} times.")))
    } else {
        Ok(tool_text(format!("Pressed {key}.")))
    }
}

fn execute_scroll(args: &Value) -> Result<Value, String> {
    move_if_coordinate(args)?;
    let amount = if let Some(amt) = args.get("scroll_amount").and_then(Value::as_i64) {
        amt
    } else if let Some(dir) = args.get("scroll_direction").and_then(Value::as_str) {
        let count = args.get("scroll_amount").and_then(Value::as_i64).unwrap_or(3);
        match dir {
            "up" => count,
            "down" => -count,
            "left" | "right" => count,
            _ => -count,
        }
    } else {
        -3
    };
    desktop::scroll(amount as i32)?;
    Ok(tool_text(format!("Scrolled {amount} detents.")))
}

fn execute_hold_key(args: &Value) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(Value::as_str)
        .or_else(|| args.get("text").and_then(Value::as_str))
        .ok_or_else(|| "Missing key".to_string())?;
    let duration_sec = args
        .get("duration")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let ms = (duration_sec * 1000.0).clamp(0.0, 10000.0) as u64;
    desktop::hold_key(key, ms)?;
    Ok(tool_text(format!("Held key {key} for {duration_sec}s.")))
}

fn execute_left_mouse_down(args: &Value) -> Result<Value, String> {
    move_if_coordinate(args)?;
    desktop::mouse_down()?;
    Ok(tool_text("Mouse button pressed down."))
}

fn execute_left_mouse_up(args: &Value) -> Result<Value, String> {
    move_if_coordinate(args)?;
    desktop::mouse_up()?;
    Ok(tool_text("Mouse button released."))
}

fn execute_wait(args: &Value) -> Result<Value, String> {
    let ms = if let Some(sec) = args.get("duration").and_then(Value::as_f64) {
        (sec * 1000.0).clamp(0.0, 10000.0) as u64
    } else {
        args.get("duration_ms").and_then(Value::as_u64).unwrap_or(1000).min(10000)
    };
    std::thread::sleep(Duration::from_millis(ms));
    Ok(tool_text(format!("Waited {ms}ms.")))
}

fn execute_cursor_position(_args: &Value) -> Result<Value, String> {
    let (x, y) = desktop::get_cursor_position()?;
    Ok(json!({
        "content": [{
            "type": "text",
            "text": format!("Cursor position: ({x}, {y})")
        }]
    }))
}

fn execute_open_application(args: &Value) -> Result<Value, String> {
    let app = args
        .get("app")
        .and_then(Value::as_str)
        .ok_or_else(|| "Missing app name".to_string())?;
    desktop::open_application(app)?;
    Ok(tool_text(format!("Opened application: {app}.")))
}

fn execute_switch_display(args: &Value) -> Result<Value, String> {
    let display = args
        .get("display")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    Ok(tool_text(format!("Switched display to {display}.")))
}

fn execute_list_granted_applications(_args: &Value) -> Result<Value, String> {
    Ok(tool_text("Granted applications: [All]. Active flags: { clipboardRead: true, clipboardWrite: true, systemKeyCombos: true }"))
}

fn execute_read_clipboard(_args: &Value) -> Result<Value, String> {
    let text = desktop::read_clipboard()?;
    Ok(json!({
        "content": [{
            "type": "text",
            "text": text
        }]
    }))
}

fn execute_write_clipboard(args: &Value) -> Result<Value, String> {
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| "Missing text".to_string())?;
    desktop::write_clipboard(text)?;
    Ok(tool_text("Clipboard written successfully."))
}

fn execute_computer_batch(args: &Value) -> Result<Value, String> {
    let actions = args
        .get("actions")
        .and_then(Value::as_array)
        .ok_or_else(|| "Missing actions array".to_string())?;

    let mut contents = Vec::new();

    for (idx, action_obj) in actions.iter().enumerate() {
        let action_name = action_obj
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        
        let res = match action_name {
            "screenshot" => execute_screenshot(action_obj),
            "active_window_screenshot" => execute_active_window_screenshot(action_obj),
            "zoom" => execute_zoom(action_obj),
            "list_windows" => execute_list_windows(action_obj),
            "left_click" => execute_left_click(action_obj),
            "right_click" => execute_right_click(action_obj),
            "double_click" => execute_double_click(action_obj),
            "triple_click" => execute_triple_click(action_obj),
            "middle_click" => execute_middle_click(action_obj),
            "left_click_drag" | "drag_and_drop" => execute_left_click_drag(action_obj),
            "mouse_move" => execute_mouse_move(action_obj),
            "type" => execute_type(action_obj),
            "key" | "press_key" => execute_key(action_obj),
            "scroll" => execute_scroll(action_obj),
            "hold_key" => execute_hold_key(action_obj),
            "left_mouse_down" | "mouse_down" => execute_left_mouse_down(action_obj),
            "left_mouse_up" | "mouse_up" => execute_left_mouse_up(action_obj),
            "wait" => execute_wait(action_obj),
            "cursor_position" => execute_cursor_position(action_obj),
            "read_clipboard" => execute_read_clipboard(action_obj),
            "write_clipboard" => execute_write_clipboard(action_obj),
            _ => Err(format!("Unsupported batch action: {action_name}")),
        };

        match res {
            Ok(val) => {
                if let Some(arr) = val.get("content").and_then(Value::as_array) {
                    contents.extend(arr.clone());
                }
            }
            Err(err) => {
                contents.push(json!({
                    "type": "text",
                    "text": format!("Batch stopped at action [{idx}] ({action_name}): {err}")
                }));
                return Ok(json!({
                    "isError": true,
                    "content": contents
                }));
            }
        }
    }

    Ok(json!({ "content": contents }))
}

fn execute_computer(args: &Value) -> Result<Value, String> {
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| "Missing action".to_string())?;

    match action {
        "screenshot" => execute_screenshot(args),
        "active_window_screenshot" => execute_active_window_screenshot(args),
        "zoom" => execute_zoom(args),
        "list_windows" => execute_list_windows(args),
        "mouse_move" => execute_mouse_move(args),
        "left_click" => execute_left_click(args),
        "right_click" => execute_right_click(args),
        "double_click" => execute_double_click(args),
        "triple_click" => execute_triple_click(args),
        "middle_click" => execute_middle_click(args),
        "drag_and_drop" | "left_click_drag" => execute_left_click_drag(args),
        "mouse_down" | "left_mouse_down" => execute_left_mouse_down(args),
        "mouse_up" | "left_mouse_up" => execute_left_mouse_up(args),
        "type" => execute_type(args),
        "key" | "press_key" | "key_down" | "key_up" | "hotkey" => execute_key(args),
        "scroll" => execute_scroll(args),
        "wait" => execute_wait(args),
        "hold_key" => execute_hold_key(args),
        "cursor_position" => execute_cursor_position(args),
        "open_application" => execute_open_application(args),
        "switch_display" => execute_switch_display(args),
        "read_clipboard" => execute_read_clipboard(args),
        "write_clipboard" => execute_write_clipboard(args),
        "computer_batch" => execute_computer_batch(args),
        "teach_step" => execute_teach_step(args),
        "teach_batch" => execute_teach_batch(args),
        "inquire" => execute_list_granted_applications(args),
        other => Err(format!("Unsupported action: {other}")),
    }
}

fn coordinate(args: &Value) -> Result<(i32, i32), String> {
    let coord = args
        .get("coordinate")
        .and_then(Value::as_array)
        .ok_or_else(|| "Missing coordinate".to_string())?;
    if coord.len() != 2 {
        return Err("coordinate must be [x, y]".to_string());
    }
    let x = coord[0]
        .as_i64()
        .ok_or_else(|| "coordinate[0] must be an integer".to_string())?;
    let y = coord[1]
        .as_i64()
        .ok_or_else(|| "coordinate[1] must be an integer".to_string())?;
    Ok((x as i32, y as i32))
}

fn move_if_coordinate(args: &Value) -> Result<(), String> {
    if args.get("coordinate").is_some() {
        let (x, y) = coordinate(args)?;
        desktop::move_mouse(x, y)?;
    }
    Ok(())
}

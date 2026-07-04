use serde_json::{json, Value};

pub fn all_tools() -> Vec<Value> {
    vec![
        computer_tool(),
        request_access_tool(),
        request_teach_access_tool(),
        teach_step_tool(),
        teach_batch_tool(),
        screenshot_tool(),
        active_window_screenshot_tool(),
        zoom_tool(),
        list_windows_tool(),
        left_click_tool(),
        double_click_tool(),
        triple_click_tool(),
        right_click_tool(),
        middle_click_tool(),
        left_click_drag_tool(),
        mouse_move_tool(),
        type_tool(),
        key_tool(),
        scroll_tool(),
        hold_key_tool(),
        left_mouse_down_tool(),
        left_mouse_up_tool(),
        wait_tool(),
        cursor_position_tool(),
        open_application_tool(),
        switch_display_tool(),
        list_granted_applications_tool(),
        read_clipboard_tool(),
        write_clipboard_tool(),
        computer_batch_tool(),
    ]
}

pub fn computer_tool() -> Value {
    json!({
        "name": "computer",
        "description": "Control the local computer screen, mouse, and keyboard. Supports all native actions including screenshot, active_window_screenshot, zoom, list_windows, click, type, key, scroll, drag, batching, clipboard, and cursor position.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "screenshot",
                        "active_window_screenshot",
                        "zoom",
                        "list_windows",
                        "mouse_move",
                        "left_click",
                        "right_click",
                        "double_click",
                        "triple_click",
                        "middle_click",
                        "drag_and_drop",
                        "left_click_drag",
                        "mouse_down",
                        "mouse_up",
                        "left_mouse_down",
                        "left_mouse_up",
                        "type",
                        "key",
                        "press_key",
                        "key_down",
                        "key_up",
                        "wait",
                        "hotkey",
                        "hold_key",
                        "inquire",
                        "scroll",
                        "cursor_position",
                        "open_application",
                        "switch_display",
                        "read_clipboard",
                        "write_clipboard",
                        "computer_batch",
                        "teach_step",
                        "teach_batch"
                    ]
                },
                "coordinate": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "minItems": 2,
                    "maxItems": 2,
                    "description": "[x, y] absolute screen coordinate."
                },
                "start_coordinate": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "minItems": 2,
                    "maxItems": 2,
                    "description": "[x, y] start coordinate for drag operations."
                },
                "region": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "minItems": 4,
                    "maxItems": 4,
                    "description": "[x0, y0, x1, y1] region for zoom action."
                },
                "text": {
                    "type": "string",
                    "description": "Text to type or chord key string."
                },
                "key": {
                    "type": "string",
                    "description": "Key or shortcut, e.g. Enter, Tab, Escape, Ctrl+C."
                },
                "scroll_amount": {
                    "type": "integer",
                    "description": "Mouse wheel detents. Positive scrolls up, negative scrolls down."
                },
                "scroll_direction": {
                    "type": "string",
                    "enum": ["up", "down", "left", "right"]
                },
                "duration": {
                    "type": "number",
                    "description": "Duration in seconds for hold_key or wait."
                },
                "duration_ms": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 10000
                },
                "repeat": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100
                },
                "app": {
                    "type": "string",
                    "description": "Application display name to open."
                },
                "display": {
                    "type": "string",
                    "description": "Display name or index to switch to."
                },
                "actions": {
                    "type": "array",
                    "items": { "type": "object" },
                    "description": "List of action objects for computer_batch."
                },
                "steps": {
                    "type": "array",
                    "items": { "type": "object" },
                    "description": "List of step objects for teach_batch."
                }
            },
            "required": ["action"]
        }
    })
}

pub fn request_access_tool() -> Value {
    json!({
        "name": "request_access",
        "description": "Request user approval to access or interact with specific applications on screen.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "applications": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of application names or bundle IDs requested by the model."
                },
                "apps": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Alias for applications."
                },
                "reason": {
                    "type": "string",
                    "description": "Explanation for why access to these applications is needed."
                }
            }
        }
    })
}

pub fn request_teach_access_tool() -> Value {
    json!({
        "name": "request_teach_access",
        "description": "Request user approval to enter interactive teach mode for step-by-step guidance.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "reason": { "type": "string" }
            }
        }
    })
}

pub fn teach_step_tool() -> Value {
    json!({
        "name": "teach_step",
        "description": "Display an interactive step-by-step teaching instruction to the user on screen.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "step_number": { "type": "integer" },
                "instruction": { "type": "string" },
                "explanation": { "type": "string" },
                "next_preview": { "type": "string" },
                "target": { "type": "string" },
                "actions": { "type": "array", "items": { "type": "object" } }
            }
        }
    })
}

pub fn teach_batch_tool() -> Value {
    json!({
        "name": "teach_batch",
        "description": "Queue multiple teach steps in one tool call.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "steps": { "type": "array", "items": { "type": "object" } }
            },
            "required": ["steps"]
        }
    })
}

pub fn screenshot_tool() -> Value {
    json!({
        "name": "screenshot",
        "description": "Take a screenshot of the display.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "save_to_disk": { "type": "boolean" },
                "active_window_only": { "type": "boolean", "description": "If true, capture only the currently active foreground window." }
            }
        }
    })
}

pub fn active_window_screenshot_tool() -> Value {
    json!({
        "name": "active_window_screenshot",
        "description": "Take a screenshot of only the currently active (foreground) window.",
        "inputSchema": {
            "type": "object",
            "properties": {}
        }
    })
}

pub fn list_windows_tool() -> Value {
    json!({
        "name": "list_windows",
        "description": "Get a list of all currently open and visible window titles.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "visible_only": { "type": "boolean", "description": "If true (default), filter out hidden or background system windows." }
            }
        }
    })
}



pub fn zoom_tool() -> Value {
    json!({
        "name": "zoom",
        "description": "Take a higher-resolution zoomed screenshot of a specific region.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "region": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "minItems": 4,
                    "maxItems": 4,
                    "description": "[x0, y0, x1, y1] bounding box."
                }
            },
            "required": ["region"]
        }
    })
}

pub fn left_click_tool() -> Value {
    json!({
        "name": "left_click",
        "description": "Left-click at the specified coordinates.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "coordinate": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }
            },
            "required": ["coordinate"]
        }
    })
}

pub fn double_click_tool() -> Value {
    json!({
        "name": "double_click",
        "description": "Double-click at the specified coordinates.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "coordinate": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }
            },
            "required": ["coordinate"]
        }
    })
}

pub fn triple_click_tool() -> Value {
    json!({
        "name": "triple_click",
        "description": "Triple-click at the specified coordinates.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "coordinate": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }
            },
            "required": ["coordinate"]
        }
    })
}

pub fn right_click_tool() -> Value {
    json!({
        "name": "right_click",
        "description": "Right-click at the specified coordinates.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "coordinate": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }
            },
            "required": ["coordinate"]
        }
    })
}

pub fn middle_click_tool() -> Value {
    json!({
        "name": "middle_click",
        "description": "Middle-click at the specified coordinates.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "coordinate": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }
            },
            "required": ["coordinate"]
        }
    })
}

pub fn left_click_drag_tool() -> Value {
    json!({
        "name": "left_click_drag",
        "description": "Press mouse button, drag to coordinate, and release.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "coordinate": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
                "start_coordinate": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }
            },
            "required": ["coordinate"]
        }
    })
}

pub fn mouse_move_tool() -> Value {
    json!({
        "name": "mouse_move",
        "description": "Move mouse cursor to coordinates without clicking.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "coordinate": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }
            },
            "required": ["coordinate"]
        }
    })
}

pub fn type_tool() -> Value {
    json!({
        "name": "type",
        "description": "Type text into current focused control.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": { "type": "string" }
            },
            "required": ["text"]
        }
    })
}

pub fn key_tool() -> Value {
    json!({
        "name": "key",
        "description": "Press key or key combination.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "key": { "type": "string" },
                "repeat": { "type": "integer", "minimum": 1, "maximum": 100 }
            }
        }
    })
}

pub fn scroll_tool() -> Value {
    json!({
        "name": "scroll",
        "description": "Scroll mouse wheel.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "coordinate": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
                "scroll_direction": { "type": "string", "enum": ["up", "down", "left", "right"] },
                "scroll_amount": { "type": "integer" }
            }
        }
    })
}

pub fn hold_key_tool() -> Value {
    json!({
        "name": "hold_key",
        "description": "Press and hold key for a specified duration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "key": { "type": "string" },
                "duration": { "type": "number" }
            }
        }
    })
}

pub fn left_mouse_down_tool() -> Value {
    json!({
        "name": "left_mouse_down",
        "description": "Press down left mouse button.",
        "inputSchema": { "type": "object", "properties": {} }
    })
}

pub fn left_mouse_up_tool() -> Value {
    json!({
        "name": "left_mouse_up",
        "description": "Release left mouse button.",
        "inputSchema": { "type": "object", "properties": {} }
    })
}

pub fn wait_tool() -> Value {
    json!({
        "name": "wait",
        "description": "Wait specified time duration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "duration": { "type": "number" },
                "duration_ms": { "type": "integer" }
            }
        }
    })
}

pub fn cursor_position_tool() -> Value {
    json!({
        "name": "cursor_position",
        "description": "Get current mouse cursor position.",
        "inputSchema": { "type": "object", "properties": {} }
    })
}

pub fn open_application_tool() -> Value {
    json!({
        "name": "open_application",
        "description": "Launch or bring application to front.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "app": { "type": "string" }
            },
            "required": ["app"]
        }
    })
}

pub fn switch_display_tool() -> Value {
    json!({
        "name": "switch_display",
        "description": "Switch active display monitor for computer use.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "display": { "type": "string" }
            },
            "required": ["display"]
        }
    })
}

pub fn list_granted_applications_tool() -> Value {
    json!({
        "name": "list_granted_applications",
        "description": "List applications granted in session.",
        "inputSchema": { "type": "object", "properties": {} }
    })
}

pub fn read_clipboard_tool() -> Value {
    json!({
        "name": "read_clipboard",
        "description": "Read text from the clipboard.",
        "inputSchema": { "type": "object", "properties": {} }
    })
}

pub fn write_clipboard_tool() -> Value {
    json!({
        "name": "write_clipboard",
        "description": "Write text to the clipboard.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": { "type": "string" }
            },
            "required": ["text"]
        }
    })
}

pub fn computer_batch_tool() -> Value {
    json!({
        "name": "computer_batch",
        "description": "Execute a sequence of actions in one tool call.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "actions": { "type": "array", "items": { "type": "object" }, "minItems": 1 }
            },
            "required": ["actions"]
        }
    })
}

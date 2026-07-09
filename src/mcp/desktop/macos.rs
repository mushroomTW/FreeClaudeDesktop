use super::super::types::Screenshot;
use std::process::Command;

fn run_command(command: &mut Command, label: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("{label} failed: {e}"))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err(format!("{label} exited with {}", output.status))
    } else {
        Err(format!("{label} failed: {stderr}"))
    }
}

pub fn screenshot() -> Result<Screenshot, String> {
    let tmp_file = std::env::temp_dir().join(format!("mcp_shot_{}.png", std::process::id()));
    let output = Command::new("/usr/sbin/screencapture")
        .args(["-x", "-t", "png", tmp_file.to_str().unwrap()])
        .output()
        .map_err(|e| format!("Failed to run screencapture: {e}"))?;

    if !output.status.success() {
        return Err("screencapture failed".to_string());
    }

    let png = std::fs::read(&tmp_file).map_err(|e| format!("Failed to read screenshot: {e}"))?;
    let _ = std::fs::remove_file(tmp_file);

    let img =
        image::load_from_memory(&png).map_err(|e| format!("Failed to decode screenshot: {e}"))?;

    Ok(Screenshot {
        width: img.width(),
        height: img.height(),
        png,
    })
}

pub fn screenshot_active_window() -> Result<Screenshot, String> {
    let tmp_file = std::env::temp_dir().join(format!("mcp_shot_active_{}.png", std::process::id()));
    let output = Command::new("/usr/sbin/screencapture")
        .args(["-x", "-t", "png", "-c"])
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let _ = Command::new("/usr/bin/osascript")
                .arg("-e")
                .arg(format!("tell app \"System Events\" to do shell script \"/usr/sbin/screencapture -x -t png -l $(osascript -e 'tell app \\\"System Events\\\" to get id of window 1 of (first process whose frontmost is true)') {}\"", tmp_file.to_str().unwrap()))
                .output();
            if let Ok(png) = std::fs::read(&tmp_file) {
                let _ = std::fs::remove_file(&tmp_file);
                if let Ok(img) = image::load_from_memory(&png) {
                    return Ok(Screenshot {
                        width: img.width(),
                        height: img.height(),
                        png,
                    });
                }
            }
        }
    }
    screenshot()
}

pub fn zoom(x0: i32, y0: i32, x1: i32, y1: i32) -> Result<Screenshot, String> {
    let full = screenshot()?;
    let w = full.width as i32;
    let h = full.height as i32;

    let left = x0.clamp(0, w);
    let top = y0.clamp(0, h);
    let right = x1.clamp(left + 1, w);
    let bottom = y1.clamp(top + 1, h);

    let crop_w = (right - left) as u32;
    let crop_h = (bottom - top) as u32;

    let img = image::load_from_memory(&full.png)
        .map_err(|e| format!("Failed to decode image for zoom: {e}"))?;
    let cropped = img.crop_imm(left as u32, top as u32, crop_w, crop_h);

    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            cropped.to_rgba8().as_raw(),
            crop_w,
            crop_h,
            image::ColorType::Rgba8.into(),
        )
        .map_err(|e| e.to_string())?;

    Ok(Screenshot {
        width: crop_w,
        height: crop_h,
        png,
    })
}

pub fn get_cursor_position() -> Result<(i32, i32), String> {
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to get the position of the mouse cursor")
        .output()
        .map_err(|e| e.to_string())?;

    let text = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = text.trim().split(',').map(|s| s.trim()).collect();
    if parts.len() == 2 {
        let x = parts[0].parse::<i32>().unwrap_or(0);
        let y = parts[1].parse::<i32>().unwrap_or(0);
        Ok((x, y))
    } else {
        Ok((0, 0))
    }
}

pub fn move_mouse(x: i32, y: i32) -> Result<(), String> {
    let script = format!(
        "tell application \"System Events\" to set position of mouse cursor to {{{x}, {y}}}"
    );
    run_command(
        Command::new("/usr/bin/osascript").args(["-e", &script]),
        "osascript",
    )
}

pub fn left_click() -> Result<(), String> {
    let script = "tell application \"System Events\" to click";
    run_command(
        Command::new("/usr/bin/osascript").args(["-e", script]),
        "osascript",
    )
}

pub fn right_click() -> Result<(), String> {
    let script = "tell application \"System Events\" to key code 87 using control down";
    run_command(
        Command::new("/usr/bin/osascript").args(["-e", script]),
        "osascript",
    )
}

pub fn double_click() -> Result<(), String> {
    left_click()?;
    left_click()
}

pub fn triple_click() -> Result<(), String> {
    left_click()?;
    left_click()?;
    left_click()
}

pub fn middle_click() -> Result<(), String> {
    let script = "tell application \"System Events\" to key code 87";
    run_command(
        Command::new("/usr/bin/osascript").args(["-e", script]),
        "osascript",
    )
}

pub fn mouse_down() -> Result<(), String> {
    let script = "tell application \"System Events\" to do shell script \"osascript -e 'tell app \\\"System Events\\\" to click'\"";
    run_command(
        Command::new("/usr/bin/osascript").args(["-e", script]),
        "osascript",
    )
}

pub fn mouse_up() -> Result<(), String> {
    Ok(())
}

pub fn drag_and_drop(end_x: i32, end_y: i32) -> Result<(), String> {
    mouse_down()?;
    std::thread::sleep(std::time::Duration::from_millis(50));
    move_mouse(end_x, end_y)?;
    std::thread::sleep(std::time::Duration::from_millis(50));
    mouse_up()
}

pub fn scroll(amount: i32) -> Result<(), String> {
    let dir = if amount > 0 { "up" } else { "down" };
    let script = format!("tell application \"System Events\" to scroll {dir}");
    run_command(
        Command::new("/usr/bin/osascript").args(["-e", &script]),
        "osascript",
    )
}

pub fn read_clipboard() -> Result<String, String> {
    let output = Command::new("/usr/bin/pbpaste")
        .output()
        .map_err(|e| format!("pbpaste failed: {e}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn write_clipboard(text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut child = Command::new("/usr/bin/pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("pbcopy failed: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
    Ok(())
}

pub fn type_text(text: &str) -> Result<(), String> {
    if text.len() > 5 || text.contains('\n') {
        let old_clip = read_clipboard().unwrap_or_default();
        if write_clipboard(text).is_ok() {
            let paste_result = press_key("cmd+v");
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = write_clipboard(&old_clip);
            paste_result?;
            return Ok(());
        }
    }
    let safe_text = text.replace('"', "\\\"");
    let script = format!("tell application \"System Events\" to keystroke \"{safe_text}\"");
    run_command(
        Command::new("/usr/bin/osascript").args(["-e", &script]),
        "osascript",
    )
}

pub fn press_key(key: &str) -> Result<(), String> {
    let lower = key.to_lowercase();
    let script = match lower.as_str() {
        "enter" | "return" => "tell application \"System Events\" to key code 36".to_string(),
        "tab" => "tell application \"System Events\" to key code 48".to_string(),
        "escape" | "esc" => "tell application \"System Events\" to key code 53".to_string(),
        "space" => "tell application \"System Events\" to key code 49".to_string(),
        "backspace" => "tell application \"System Events\" to key code 51".to_string(),
        "cmd+v" | "command+v" => {
            "tell application \"System Events\" to keystroke \"v\" using command down".to_string()
        }
        "cmd+c" | "command+c" => {
            "tell application \"System Events\" to keystroke \"c\" using command down".to_string()
        }
        "cmd+a" | "command+a" => {
            "tell application \"System Events\" to keystroke \"a\" using command down".to_string()
        }
        other => format!("tell application \"System Events\" to keystroke \"{other}\""),
    };
    run_command(
        Command::new("/usr/bin/osascript").args(["-e", &script]),
        "osascript",
    )
}

pub fn hold_key(key: &str, duration_ms: u64) -> Result<(), String> {
    press_key(key)?;
    std::thread::sleep(std::time::Duration::from_millis(duration_ms));
    Ok(())
}

pub fn open_application(app: &str) -> Result<(), String> {
    let _ = Command::new("/usr/bin/open").args(["-a", app]).spawn();
    Ok(())
}

pub fn list_windows(_visible_only: bool) -> Result<Vec<String>, String> {
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", "tell application \"System Events\" to get name of every window of (every process whose background only is false)"])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout);
    let titles: Vec<String> = text
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "missing value")
        .collect();
    Ok(titles)
}

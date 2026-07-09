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
    let tmp_file = std::env::temp_dir().join(format!("mcp_shot_unix_{}.png", std::process::id()));

    let status = Command::new("gnome-screenshot")
        .args(["-f", tmp_file.to_str().unwrap()])
        .status()
        .or_else(|_| {
            Command::new("scrot")
                .arg(tmp_file.to_str().unwrap())
                .status()
        })
        .or_else(|_| {
            Command::new("grim")
                .arg(tmp_file.to_str().unwrap())
                .status()
        })
        .map_err(|e| format!("Failed to capture Linux screenshot: {e}"))?;

    if !status.success() {
        return Err("Screenshot tool returned error".to_string());
    }

    let png =
        std::fs::read(&tmp_file).map_err(|e| format!("Failed to read screenshot file: {e}"))?;
    let _ = std::fs::remove_file(tmp_file);

    let img = image::load_from_memory(&png).map_err(|e| format!("Failed to decode image: {e}"))?;

    Ok(Screenshot {
        width: img.width(),
        height: img.height(),
        png,
    })
}

pub fn screenshot_active_window() -> Result<Screenshot, String> {
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
    let output = Command::new("xdotool").arg("getmouselocation").output();
    if let Ok(out) = output {
        let text = String::from_utf8_lossy(&out.stdout);
        let mut x = 0;
        let mut y = 0;
        for part in text.split_whitespace() {
            if let Some(val) = part.strip_prefix("x:") {
                x = val.parse().unwrap_or(0);
            } else if let Some(val) = part.strip_prefix("y:") {
                y = val.parse().unwrap_or(0);
            }
        }
        return Ok((x, y));
    }
    Ok((0, 0))
}

pub fn move_mouse(x: i32, y: i32) -> Result<(), String> {
    run_command(
        Command::new("xdotool").args(["mousemove", &x.to_string(), &y.to_string()]),
        "xdotool",
    )
}

pub fn left_click() -> Result<(), String> {
    run_command(Command::new("xdotool").args(["click", "1"]), "xdotool")
}

pub fn right_click() -> Result<(), String> {
    run_command(Command::new("xdotool").args(["click", "3"]), "xdotool")
}

pub fn double_click() -> Result<(), String> {
    run_command(
        Command::new("xdotool").args(["click", "--repeat", "2", "1"]),
        "xdotool",
    )
}

pub fn triple_click() -> Result<(), String> {
    run_command(
        Command::new("xdotool").args(["click", "--repeat", "3", "1"]),
        "xdotool",
    )
}

pub fn middle_click() -> Result<(), String> {
    run_command(Command::new("xdotool").args(["click", "2"]), "xdotool")
}

pub fn mouse_down() -> Result<(), String> {
    run_command(Command::new("xdotool").args(["mousedown", "1"]), "xdotool")
}

pub fn mouse_up() -> Result<(), String> {
    run_command(Command::new("xdotool").args(["mouseup", "1"]), "xdotool")
}

pub fn drag_and_drop(end_x: i32, end_y: i32) -> Result<(), String> {
    mouse_down()?;
    move_mouse(end_x, end_y)?;
    mouse_up()
}

pub fn scroll(amount: i32) -> Result<(), String> {
    let btn = if amount > 0 { "4" } else { "5" };
    run_command(Command::new("xdotool").args(["click", btn]), "xdotool")
}

pub fn read_clipboard() -> Result<String, String> {
    let output = Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
        .or_else(|_| Command::new("xsel").args(["-b", "-o"]).output())
        .or_else(|_| Command::new("wl-paste").output())
        .map_err(|e| format!("Failed to read Linux clipboard: {e}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn write_clipboard(text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut child = Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .or_else(|_| {
            Command::new("xsel")
                .args(["-b", "-i"])
                .stdin(std::process::Stdio::piped())
                .spawn()
        })
        .or_else(|_| {
            Command::new("wl-copy")
                .stdin(std::process::Stdio::piped())
                .spawn()
        })
        .map_err(|e| format!("Failed to write Linux clipboard: {e}"))?;

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
            let paste_result = press_key("ctrl+v");
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = write_clipboard(&old_clip);
            paste_result?;
            return Ok(());
        }
    }
    run_command(Command::new("xdotool").args(["type", text]), "xdotool")
}

pub fn press_key(key: &str) -> Result<(), String> {
    run_command(Command::new("xdotool").args(["key", key]), "xdotool")
}

pub fn hold_key(key: &str, duration_ms: u64) -> Result<(), String> {
    run_command(Command::new("xdotool").args(["keydown", key]), "xdotool")?;
    std::thread::sleep(std::time::Duration::from_millis(duration_ms));
    run_command(Command::new("xdotool").args(["keyup", key]), "xdotool")
}

pub fn open_application(app: &str) -> Result<(), String> {
    let _ = Command::new("xdg-open")
        .arg(app)
        .spawn()
        .or_else(|_| Command::new("gtk-launch").arg(app).spawn());
    Ok(())
}

pub fn list_windows(_visible_only: bool) -> Result<Vec<String>, String> {
    let output = Command::new("xdotool")
        .args(["search", "--onlyvisible", "--name", "."])
        .output();
    if let Ok(out) = output {
        let ids = String::from_utf8_lossy(&out.stdout);
        let mut titles = Vec::new();
        for id in ids.lines().take(50) {
            let name_out = Command::new("xdotool")
                .args(["getwindowname", id.trim()])
                .output();
            if let Ok(n_out) = name_out {
                let name = String::from_utf8_lossy(&n_out.stdout).trim().to_string();
                if !name.is_empty() && !titles.contains(&name) {
                    titles.push(name);
                }
            }
        }
        return Ok(titles);
    }
    Ok(Vec::new())
}

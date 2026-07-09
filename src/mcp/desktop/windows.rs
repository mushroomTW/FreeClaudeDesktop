use super::super::types::Screenshot;
use image::ImageEncoder;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use winapi::shared::minwindef::{DWORD, UINT, WORD};
use winapi::shared::windef::{HWND, POINT};
use winapi::um::wingdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
    SelectObject, BITMAPINFO, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
};
use winapi::um::winuser::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetCursorPos, GetDC, GetSystemMetrics,
    OpenClipboard, ReleaseDC, SendInput, SetClipboardData, SetCursorPos, CF_UNICODETEXT, INPUT,
    INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END,
    VK_ESCAPE, VK_F1, VK_HOME, VK_INSERT, VK_LEFT, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT,
    VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};

pub fn screenshot() -> Result<Screenshot, String> {
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if width <= 0 || height <= 0 {
            return Err("No display found.".to_string());
        }

        let hwnd: HWND = null_mut();
        let screen_dc = GetDC(hwnd);
        if screen_dc.is_null() {
            return Err(last_error());
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            ReleaseDC(hwnd, screen_dc);
            return Err(last_error());
        }
        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        if bitmap.is_null() {
            DeleteDC(mem_dc);
            ReleaseDC(hwnd, screen_dc);
            return Err(last_error());
        }

        let old = SelectObject(mem_dc, bitmap as _);
        if BitBlt(
            mem_dc,
            0,
            0,
            width,
            height,
            screen_dc,
            x,
            y,
            SRCCOPY | CAPTUREBLT,
        ) == 0
        {
            SelectObject(mem_dc, old);
            DeleteObject(bitmap as _);
            DeleteDC(mem_dc);
            ReleaseDC(hwnd, screen_dc);
            return Err(last_error());
        }

        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<winapi::um::wingdi::BITMAPINFOHEADER>() as DWORD;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;

        let mut bgra = vec![0u8; (width as usize) * (height as usize) * 4];
        let copied = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height as UINT,
            bgra.as_mut_ptr() as *mut _,
            &mut info,
            DIB_RGB_COLORS,
        );

        SelectObject(mem_dc, old);
        DeleteObject(bitmap as _);
        DeleteDC(mem_dc);
        ReleaseDC(hwnd, screen_dc);

        if copied == 0 {
            return Err(last_error());
        }

        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }

        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &bgra,
                width as u32,
                height as u32,
                image::ColorType::Rgba8.into(),
            )
            .map_err(|error| error.to_string())?;

        Ok(Screenshot {
            width: width as u32,
            height: height as u32,
            png,
        })
    }
}

pub fn screenshot_active_window() -> Result<Screenshot, String> {
    unsafe {
        let hwnd = winapi::um::winuser::GetForegroundWindow();
        if !hwnd.is_null() {
            let mut rect: winapi::shared::windef::RECT = zeroed();
            if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) != 0 {
                let left = rect.left;
                let top = rect.top;
                let right = rect.right;
                let bottom = rect.bottom;
                if right > left && bottom > top {
                    return zoom(left, top, right, bottom);
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
    unsafe {
        let mut pt: POINT = zeroed();
        if GetCursorPos(&mut pt) != 0 {
            Ok((pt.x, pt.y))
        } else {
            Err(last_error())
        }
    }
}

pub fn move_mouse(x: i32, y: i32) -> Result<(), String> {
    unsafe {
        if SetCursorPos(x, y) == 0 {
            return Err(last_error());
        }
    }
    Ok(())
}

pub fn left_click() -> Result<(), String> {
    mouse(MOUSEEVENTF_LEFTDOWN, 0);
    mouse(MOUSEEVENTF_LEFTUP, 0);
    Ok(())
}

pub fn right_click() -> Result<(), String> {
    mouse(MOUSEEVENTF_RIGHTDOWN, 0);
    mouse(MOUSEEVENTF_RIGHTUP, 0);
    Ok(())
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
    mouse(MOUSEEVENTF_MIDDLEDOWN, 0);
    mouse(MOUSEEVENTF_MIDDLEUP, 0);
    Ok(())
}

pub fn mouse_down() -> Result<(), String> {
    mouse(MOUSEEVENTF_LEFTDOWN, 0);
    Ok(())
}

pub fn mouse_up() -> Result<(), String> {
    mouse(MOUSEEVENTF_LEFTUP, 0);
    Ok(())
}

pub fn drag_and_drop(end_x: i32, end_y: i32) -> Result<(), String> {
    mouse_down()?;
    let steps = 10;
    let (start_x, start_y) = get_cursor_position().unwrap_or((end_x, end_y));
    for i in 1..=steps {
        let cx = start_x + (end_x - start_x) * i / steps;
        let cy = start_y + (end_y - start_y) * i / steps;
        let _ = move_mouse(cx, cy);
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    mouse_up()?;
    Ok(())
}

pub fn scroll(amount: i32) -> Result<(), String> {
    mouse(MOUSEEVENTF_WHEEL, amount.saturating_mul(120) as DWORD);
    Ok(())
}

pub fn read_clipboard() -> Result<String, String> {
    unsafe {
        if OpenClipboard(null_mut()) == 0 {
            return Err("Failed to open clipboard".to_string());
        }
        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle.is_null() {
            CloseClipboard();
            return Ok(String::new());
        }
        let ptr = winapi::um::winbase::GlobalLock(handle) as *const u16;
        if ptr.is_null() {
            CloseClipboard();
            return Ok(String::new());
        }
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(ptr, len);
        let result = String::from_utf16_lossy(slice);
        winapi::um::winbase::GlobalUnlock(handle);
        CloseClipboard();
        Ok(result)
    }
}

pub fn write_clipboard(text: &str) -> Result<(), String> {
    unsafe {
        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let size = utf16.len() * size_of::<u16>();
        let handle = winapi::um::winbase::GlobalAlloc(winapi::um::winbase::GMEM_MOVEABLE, size);
        if handle.is_null() {
            return Err("GlobalAlloc failed".to_string());
        }
        let ptr = winapi::um::winbase::GlobalLock(handle) as *mut u16;
        if ptr.is_null() {
            winapi::um::winbase::GlobalFree(handle);
            return Err("GlobalLock failed".to_string());
        }
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr, utf16.len());
        winapi::um::winbase::GlobalUnlock(handle);

        if OpenClipboard(null_mut()) == 0 {
            winapi::um::winbase::GlobalFree(handle);
            return Err("Failed to open clipboard".to_string());
        }
        EmptyClipboard();
        if SetClipboardData(CF_UNICODETEXT, handle).is_null() {
            CloseClipboard();
            winapi::um::winbase::GlobalFree(handle);
            return Err("SetClipboardData failed".to_string());
        }
        CloseClipboard();
        Ok(())
    }
}

pub fn type_text(text: &str) -> Result<(), String> {
    if text.len() > 5 || text.contains('\n') {
        let old_clip = read_clipboard().unwrap_or_default();
        if write_clipboard(text).is_ok() {
            let _ = press_key("ctrl+v");
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = write_clipboard(&old_clip);
            return Ok(());
        }
    }
    for unit in text.encode_utf16() {
        key_input(0, unit, KEYEVENTF_UNICODE)?;
        key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)?;
    }
    Ok(())
}

pub fn press_key(key: &str) -> Result<(), String> {
    let parts: Vec<String> = key
        .split('+')
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    let Some(final_key) = parts.last() else {
        return Err("Missing key".to_string());
    };

    let modifiers: Vec<WORD> = parts[..parts.len().saturating_sub(1)]
        .iter()
        .map(|part| modifier_vk(part))
        .collect::<Result<Vec<_>, _>>()?;
    let final_vk = key_vk(final_key)?;

    for vk in &modifiers {
        key_input(*vk, 0, 0)?;
    }
    key_input(final_vk, 0, 0)?;
    key_input(final_vk, 0, KEYEVENTF_KEYUP)?;
    for vk in modifiers.iter().rev() {
        key_input(*vk, 0, KEYEVENTF_KEYUP)?;
    }

    Ok(())
}

pub fn hold_key(key: &str, duration_ms: u64) -> Result<(), String> {
    let parts: Vec<String> = key
        .split('+')
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    let Some(final_key) = parts.last() else {
        return Err("Missing key".to_string());
    };

    let modifiers: Vec<WORD> = parts[..parts.len().saturating_sub(1)]
        .iter()
        .map(|part| modifier_vk(part))
        .collect::<Result<Vec<_>, _>>()?;
    let final_vk = key_vk(final_key)?;

    for vk in &modifiers {
        key_input(*vk, 0, 0)?;
    }
    key_input(final_vk, 0, 0)?;

    std::thread::sleep(std::time::Duration::from_millis(duration_ms));

    key_input(final_vk, 0, KEYEVENTF_KEYUP)?;
    for vk in modifiers.iter().rev() {
        key_input(*vk, 0, KEYEVENTF_KEYUP)?;
    }

    Ok(())
}

pub fn open_application(app: &str) -> Result<(), String> {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", app])
        .spawn();
    Ok(())
}

fn mouse(flags: DWORD, data: DWORD) {
    unsafe {
        winapi::um::winuser::mouse_event(flags, 0, 0, data, 0);
    }
}

fn key_input(vk: WORD, scan: WORD, flags: DWORD) -> Result<(), String> {
    unsafe {
        let mut input: INPUT = zeroed();
        input.type_ = INPUT_KEYBOARD;
        *input.u.ki_mut() = KEYBDINPUT {
            wVk: vk,
            wScan: scan,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: 0,
        };
        if SendInput(1, &mut input, size_of::<INPUT>() as i32) == 0 {
            return Err(last_error());
        }
    }
    Ok(())
}

fn modifier_vk(key: &str) -> Result<WORD, String> {
    match key {
        "ctrl" | "control" => Ok(VK_CONTROL as WORD),
        "alt" => Ok(VK_MENU as WORD),
        "shift" => Ok(VK_SHIFT as WORD),
        other => Err(format!("Unsupported modifier: {other}")),
    }
}

fn key_vk(key: &str) -> Result<WORD, String> {
    match key {
        "enter" | "return" => Ok(VK_RETURN as WORD),
        "tab" => Ok(VK_TAB as WORD),
        "escape" | "esc" => Ok(VK_ESCAPE as WORD),
        "backspace" => Ok(VK_BACK as WORD),
        "delete" | "del" => Ok(VK_DELETE as WORD),
        "insert" | "ins" => Ok(VK_INSERT as WORD),
        "home" => Ok(VK_HOME as WORD),
        "end" => Ok(VK_END as WORD),
        "pageup" | "page_up" => Ok(VK_PRIOR as WORD),
        "pagedown" | "page_down" => Ok(VK_NEXT as WORD),
        "arrowup" | "up" => Ok(VK_UP as WORD),
        "arrowdown" | "down" => Ok(VK_DOWN as WORD),
        "arrowleft" | "left" => Ok(VK_LEFT as WORD),
        "arrowright" | "right" => Ok(VK_RIGHT as WORD),
        "space" => Ok(VK_SPACE as WORD),
        _ => {
            if let Some(num) = key
                .strip_prefix('f')
                .and_then(|value| value.parse::<u16>().ok())
            {
                if (1..=24).contains(&num) {
                    return Ok((VK_F1 as u16 + num - 1) as WORD);
                }
            }
            let mut chars = key.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) if ch.is_ascii_alphanumeric() => {
                    Ok(ch.to_ascii_uppercase() as WORD)
                }
                _ => Err(format!("Unsupported key: {key}")),
            }
        }
    }
}

pub fn list_windows(_visible_only: bool) -> Result<Vec<String>, String> {
    unsafe extern "system" fn enum_proc(
        hwnd: winapi::shared::windef::HWND,
        lparam: winapi::shared::minwindef::LPARAM,
    ) -> winapi::shared::minwindef::BOOL {
        let titles = &mut *(lparam as *mut Vec<String>);
        if winapi::um::winuser::IsWindowVisible(hwnd) != 0 {
            let mut buf: [u16; 512] = [0; 512];
            let len = winapi::um::winuser::GetWindowTextW(hwnd, buf.as_mut_ptr(), 512);
            if len > 0 {
                let text = String::from_utf16_lossy(&buf[..len as usize])
                    .trim()
                    .to_string();
                if !text.is_empty() && !titles.contains(&text) {
                    titles.push(text);
                }
            }
        }
        1
    }

    let mut titles: Vec<String> = Vec::new();
    unsafe {
        winapi::um::winuser::EnumWindows(
            Some(enum_proc),
            &mut titles as *mut _ as winapi::shared::minwindef::LPARAM,
        );
    }
    Ok(titles)
}

fn last_error() -> String {
    std::io::Error::last_os_error().to_string()
}

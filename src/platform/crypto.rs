use crate::error::{AppError, AppResult};
#[cfg(target_os = "windows")]
use std::ptr;

const KEYRING_PREFIX: &str = "keyring:";
const KEYRING_SERVICE: &str = "FreeClaudeDesktop";
const KEYRING_USER: &str = "real_api_key";
const DPAPI_PREFIX: &str = "dpapi:";

fn keyring_entry() -> AppResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|error| AppError::Crypto(error.to_string()))
}

/// 將 API key 存進作業系統原生金鑰庫，設定檔只保留參照標記。
pub fn protect_secret(secret: &str) -> AppResult<String> {
    if secret.is_empty() {
        return Ok(String::new());
    }

    keyring_entry()?
        .set_password(secret)
        .map_err(|error| AppError::Crypto(error.to_string()))?;
    Ok(format!("{KEYRING_PREFIX}{KEYRING_USER}"))
}

/// 從作業系統原生金鑰庫還原 API key，並相容舊版明文值。
pub fn unprotect_secret(stored: &str) -> AppResult<String> {
    if stored.is_empty() {
        return Ok(String::new());
    }

    if stored.starts_with(KEYRING_PREFIX) {
        return keyring_entry()?
            .get_password()
            .map_err(|error| AppError::Crypto(error.to_string()));
    }

    if stored.starts_with(DPAPI_PREFIX) {
        return unprotect_dpapi_secret(stored);
    }

    Ok(stored.to_string())
}

#[cfg(not(target_os = "windows"))]
fn unprotect_dpapi_secret(_stored: &str) -> AppResult<String> {
    Ok(String::new())
}

#[cfg(target_os = "windows")]
fn unprotect_dpapi_secret(stored: &str) -> AppResult<String> {
    use winapi::um::dpapi::{CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData};
    use winapi::um::winbase::LocalFree;
    use winapi::um::wincrypt::DATA_BLOB;

    let encoded = stored
        .strip_prefix(DPAPI_PREFIX)
        .ok_or_else(|| AppError::Crypto("Invalid encrypted API key".to_string()))?;
    let mut bytes = hex_decode(encoded)?;
    let mut input = DATA_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = DATA_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(AppError::Crypto(
            std::io::Error::last_os_error().to_string(),
        ));
    }

    let decrypted =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    String::from_utf8(decrypted).map_err(|error| AppError::Crypto(error.to_string()))
}

#[cfg(target_os = "windows")]
fn hex_decode(text: &str) -> AppResult<Vec<u8>> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(AppError::Crypto("Invalid encrypted API key".to_string()));
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = value(pair[0])
            .ok_or_else(|| AppError::Crypto("Invalid encrypted API key".to_string()))?;
        let low = value(pair[1])
            .ok_or_else(|| AppError::Crypto("Invalid encrypted API key".to_string()))?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

use crate::error::{AppError, AppResult};
use std::ptr;

const DPAPI_PREFIX: &str = "dpapi:";

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

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

#[cfg(windows)]
pub fn protect_secret(secret: &str) -> AppResult<String> {
    use winapi::um::dpapi::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN};
    use winapi::um::winbase::LocalFree;
    use winapi::um::wincrypt::DATA_BLOB;

    let mut bytes_mut = secret.as_bytes().to_vec();
    let mut input = DATA_BLOB {
        cbData: bytes_mut.len() as u32,
        pbData: bytes_mut.as_mut_ptr(),
    };
    let mut output = DATA_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptProtectData(
            &mut input,
            ptr::null(),
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

    let protected =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(format!("{DPAPI_PREFIX}{}", hex_encode(&protected)))
}

#[cfg(not(windows))]
pub fn protect_secret(secret: &str) -> AppResult<String> {
    Ok(secret.to_string())
}

#[cfg(windows)]
pub fn unprotect_secret(stored: &str) -> AppResult<String> {
    use winapi::um::dpapi::{CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN};
    use winapi::um::winbase::LocalFree;
    use winapi::um::wincrypt::DATA_BLOB;

    let Some(encoded) = stored.strip_prefix(DPAPI_PREFIX) else {
        return Ok(stored.to_string());
    };
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

#[cfg(not(windows))]
pub fn unprotect_secret(stored: &str) -> AppResult<String> {
    Ok(stored.to_string())
}

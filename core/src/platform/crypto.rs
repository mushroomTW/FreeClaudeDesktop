use crate::error::{AppError, AppResult};
const KEYRING_PREFIX: &str = "keyring:";
const KEYRING_SERVICE: &str = "FreeClaudeDesktop";
const KEYRING_USER: &str = "real_api_key";
const FALLBACK_PREFIX: &str = "fallback:";

/// 執行 `keyring_entry` 對應的處理流程。
fn keyring_entry() -> AppResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|error| AppError::Crypto(error.to_string()))
}

/// 移除 FreeClaudeDesktop 寫入作業系統金鑰庫的 API key。
///
/// 找不到既有項目時視為已完成，讓解除安裝可安全重複執行。
pub fn delete_stored_secret() -> AppResult<()> {
    let entry = match keyring_entry() {
        Ok(entry) => entry,
        Err(_) => return Ok(()),
    };
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(AppError::Crypto(error.to_string())),
    }
}

/// 將 API key 存進作業系統原生金鑰庫，設定檔只保留參照標記。
pub fn protect_secret(secret: &str) -> AppResult<String> {
    if secret.is_empty() {
        return Ok(String::new());
    }

    let entry = match keyring_entry() {
        Ok(e) => e,
        Err(_) => return Ok(format!("{FALLBACK_PREFIX}{secret}")),
    };

    match entry.set_password(secret) {
        Ok(_) => Ok(format!("{KEYRING_PREFIX}{KEYRING_USER}")),
        Err(_) => Ok(format!("{FALLBACK_PREFIX}{secret}")),
    }
}

/// 從作業系統原生金鑰庫還原 API key，並相容舊版明文值。
pub fn unprotect_secret(stored: &str) -> AppResult<String> {
    if stored.is_empty() {
        return Ok(String::new());
    }

    if stored.starts_with(FALLBACK_PREFIX) {
        return Ok(stored
            .strip_prefix(FALLBACK_PREFIX)
            .unwrap_or(stored)
            .to_string());
    }

    if stored.starts_with(KEYRING_PREFIX) {
        return keyring_entry()?
            .get_password()
            .map_err(|error| AppError::Crypto(error.to_string()));
    }

    Err(AppError::Crypto("不支援的 API key 儲存格式".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 驗證 `test_fallback_crypto` 的行為符合預期。
    fn test_fallback_crypto() {
        let secret = "sk-ant-test-key-123";
        let fallback_stored = format!("{FALLBACK_PREFIX}{secret}");
        let raw = unprotect_secret(&fallback_stored).unwrap();
        assert_eq!(raw, secret);

        let protected = protect_secret(secret).unwrap();
        let restored = unprotect_secret(&protected).unwrap();
        assert_eq!(restored, secret);
    }
}

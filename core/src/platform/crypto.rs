use crate::error::{AppError, AppResult};
const KEYRING_PREFIX: &str = "keyring:";
const KEYRING_SERVICE: &str = "FreeClaudeDesktop";
const KEYRING_USER: &str = "real_api_key";

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

    let entry = keyring_entry()?;
    entry
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

    Err(AppError::Crypto("不支援的 API key 儲存格式".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 舊版明文 fallback 格式不得再被當成 API key 使用。
    fn test_fallback_crypto() {
        let secret = "sk-ant-test-key-123";
        assert!(unprotect_secret(&format!("fallback:{secret}")).is_err());
    }
}

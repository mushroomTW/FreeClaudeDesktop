/// 代理伺服器監聽的預設連接埠
pub const DEFAULT_PORT: u16 = 3000;
/// 代理伺服器內部驗證用的 Token
pub const PROXY_AUTH_TOKEN: &str = "local-proxy-token";
/// 代理伺服器接收的請求最大位元組數限制
pub const MAX_PROXY_BODY_BYTES: usize = 16 * 1024 * 1024;
/// HTTP 上遊請求逾時時間（秒）
pub const HTTP_TIMEOUT_SECS: u64 = 60;
/// 應用程式在 Claude Desktop 配置檔案中的唯一識別碼
pub const CONFIG_ID: &str = "ec29f0cd-700e-4d28-beb3-f4b1b3831fb6";

/// 可選的 API 供應商清單
pub const PROVIDERS: &[&str] = &["OpenRouter", "NVIDIA", "自訂"];
/// 可選的驗證方案標頭清單
pub const AUTH_SCHEMES: &[&str] = &["bearer", "x-api-key"];
/// Windows API 建立處理序時不顯示視窗的旗標值
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

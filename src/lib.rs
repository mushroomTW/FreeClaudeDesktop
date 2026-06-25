use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use url::Url;

const PORT: u16 = 3000;
const MAX_PROXY_BODY_BYTES: usize = 16 * 1024 * 1024;
const CONFIG_ID: &str = "ec29f0cd-700e-4d28-beb3-f4b1b3831fb6";
const PROXY_AUTH_TOKEN: &str = "local-proxy-token";
const HTTP_TIMEOUT_SECS: u64 = 60;
const DPAPI_PREFIX: &str = "dpapi:";
const CLAUDE_MODEL_ALIASES: [&str; 13] = [
    "anthropic/claude-sonnet-4-5",
    "anthropic/claude-haiku-4-5",
    "anthropic/claude-opus-4-5",
    "anthropic/claude-sonnet-4",
    "anthropic/claude-haiku-4",
    "anthropic/claude-opus-4",
    "anthropic/claude-3-5-sonnet",
    "anthropic/claude-3-5-haiku",
    "anthropic/claude-3-opus",
    "anthropic/claude-3-sonnet",
    "anthropic/claude-3-haiku",
    "anthropic/claude-2.1",
    "anthropic/claude-2.0",
];
const PREFERRED_FREE_MODEL_IDS: [&str; 5] = [
    "openai/gpt-oss-20b:free",
    "openai/gpt-oss-120b:free",
    "qwen/qwen3-next-80b-a3b-instruct:free",
    "meta-llama/llama-3.3-70b-instruct:free",
    "openrouter/free",
];

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub real_base_url: String,
    #[serde(default)]
    pub real_api_key: String,
    #[serde(default = "default_auth_scheme")]
    pub real_auth_scheme: String,
    #[serde(default)]
    pub real_model: Option<String>,
    #[serde(default)]
    pub real_model_routes: HashMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicConfig {
    base_url: String,
    auth_scheme: String,
    has_api_key: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NormalizedModels {
    pub data: Vec<NormalizedModel>,
    pub has_more: bool,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
    pub routes: HashMap<String, String>,
}

fn default_capabilities() -> Value {
    json!({
        "thinking": {
            "supported": true,
            "types": {
                "adaptive": {
                    "supported": true
                },
                "enabled": {
                    "supported": true
                }
            }
        }
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NormalizedModel {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub display_name: String,
    pub created_at: String,
    pub provider_model_id: String,
    #[serde(default = "default_capabilities")]
    pub capabilities: Value,
}

#[derive(Deserialize)]
struct ProviderModelsResponse {
    #[serde(default)]
    data: Vec<ProviderModel>,
}

#[derive(Clone, Deserialize)]
struct ProviderModel {
    id: String,
    name: Option<String>,
    #[serde(default)]
    pricing: Option<Pricing>,
}

#[derive(Clone, Deserialize)]
struct Pricing {
    prompt: Option<String>,
    completion: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InferenceModel {
    name: String,
    label_override: String,
    provider_model_id: String,
    display_name: String,
}

fn default_auth_scheme() -> String {
    "bearer".to_string()
}

fn http_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

pub fn proxy_auth_token() -> &'static str {
    PROXY_AUTH_TOKEN
}

pub fn is_valid_proxy_authorization(header: Option<&str>) -> bool {
    header
        .and_then(|value| value.trim().strip_prefix("Bearer "))
        .map(str::trim)
        == Some(PROXY_AUTH_TOKEN)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
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
        return Err("Invalid encrypted API key".to_string());
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = value(pair[0]).ok_or_else(|| "Invalid encrypted API key".to_string())?;
        let low = value(pair[1]).ok_or_else(|| "Invalid encrypted API key".to_string())?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

#[cfg(windows)]
fn protect_secret(secret: &str) -> Result<String, String> {
    use winapi::um::dpapi::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN};
    use winapi::um::winbase::LocalFree;
    use winapi::um::wincrypt::DATA_BLOB;

    let bytes = secret.as_bytes();
    let mut input = DATA_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
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
        return Err(std::io::Error::last_os_error().to_string());
    }

    let protected =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(format!("{DPAPI_PREFIX}{}", hex_encode(&protected)))
}

#[cfg(not(windows))]
fn protect_secret(secret: &str) -> Result<String, String> {
    Ok(secret.to_string())
}

#[cfg(windows)]
fn unprotect_secret(stored: &str) -> Result<String, String> {
    use winapi::um::dpapi::{CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN};
    use winapi::um::winbase::LocalFree;
    use winapi::um::wincrypt::DATA_BLOB;

    let Some(encoded) = stored.strip_prefix(DPAPI_PREFIX) else {
        return Ok(stored.to_string());
    };
    let bytes = hex_decode(encoded)?;
    let mut input = DATA_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
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
        return Err(std::io::Error::last_os_error().to_string());
    }

    let decrypted =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    String::from_utf8(decrypted).map_err(|error| error.to_string())
}

#[cfg(not(windows))]
fn unprotect_secret(stored: &str) -> Result<String, String> {
    Ok(stored.to_string())
}

fn is_local_hostname(hostname: &str) -> bool {
    matches!(hostname, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

pub fn is_allowed_origin(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    Url::parse(origin)
        .map(|url| {
            url.scheme() == "http"
                && is_local_hostname(url.host_str().unwrap_or(""))
                && url.port_or_known_default() == Some(PORT)
        })
        .unwrap_or(false)
}

fn normalize_gateway_url(base_url: &str, endpoint: &str) -> Result<String, String> {
    let mut target_url =
        Url::parse(base_url.trim()).map_err(|_| "Invalid Gateway Base URL".to_string())?;
    if target_url.scheme() != "https" && target_url.scheme() != "http" {
        return Err("Gateway Base URL must use HTTP or HTTPS".to_string());
    }
    if target_url.scheme() == "http" && !is_local_hostname(target_url.host_str().unwrap_or("")) {
        return Err("Remote Gateway Base URL must use HTTPS".to_string());
    }

    let base_path = target_url.path().trim_end_matches('/');
    let path = if base_path.ends_with("/v1") {
        format!("{base_path}/{endpoint}")
    } else {
        format!("{base_path}/v1/{endpoint}")
    };
    target_url.set_path(&path);
    target_url.set_query(None);
    target_url.set_fragment(None);
    Ok(target_url.to_string())
}

pub fn normalize_messages_url(base_url: &str) -> Result<String, String> {
    normalize_gateway_url(base_url, "messages")
}

pub fn normalize_models_url(base_url: &str) -> Result<String, String> {
    normalize_gateway_url(base_url, "models")
}

pub fn prepare_proxy_body(body: &str, settings: &Settings) -> String {
    let mut data: Value = match serde_json::from_str(body) {
        Ok(data) => data,
        Err(_) => return body.to_string(),
    };

    if let Some(model) = data.get("model").and_then(Value::as_str) {
        if let Some(mapped) = settings.real_model_routes.get(model) {
            data["model"] = Value::String(mapped.clone());
        } else if let Some(model) = &settings.real_model {
            data["model"] = Value::String(model.clone());
        }
    } else if let Some(model) = &settings.real_model {
        data["model"] = Value::String(model.clone());
    }

    serde_json::to_string(&data).unwrap_or_else(|_| body.to_string())
}

pub fn parse_json_text(text: &str) -> serde_json::Result<Value> {
    serde_json::from_str(text.trim_start_matches('\u{feff}'))
}

pub fn to_public_config(settings: &Settings) -> Value {
    json!(PublicConfig {
        base_url: settings.real_base_url.clone(),
        auth_scheme: if settings.real_auth_scheme.is_empty() {
            default_auth_scheme()
        } else {
            settings.real_auth_scheme.clone()
        },
        has_api_key: unprotect_secret(&settings.real_api_key)
            .map(|key| !key.is_empty())
            .unwrap_or(!settings.real_api_key.is_empty()),
    })
}

pub fn validate_launch_path(target_path: &str) -> Result<String, String> {
    let trimmed = target_path.trim();
    if trimmed.is_empty() {
        return Err("Claude executable path is required".to_string());
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err("Claude executable path must be absolute".to_string());
    }
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("exe"))
        != Some(true)
    {
        return Err("Claude executable path must end with .exe".to_string());
    }
    Ok(trimmed.to_string())
}

fn local_app_data() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Local"))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn settings_file() -> PathBuf {
    local_app_data()
        .join("FreeClaudeLauncher")
        .join("launcher_settings.json")
}

fn legacy_settings_file() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("launcher_settings.json")
}

fn config_lib_dir() -> PathBuf {
    local_app_data().join("Claude-3p").join("configLibrary")
}

fn meta_file() -> PathBuf {
    config_lib_dir().join("_meta.json")
}

pub fn known_claude_paths() -> Vec<PathBuf> {
    let local = local_app_data();
    vec![
        local
            .join("Programs")
            .join("claude-desktop")
            .join("Claude.exe"),
        local.join("Programs").join("Claude").join("Claude.exe"),
        PathBuf::from(env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string()))
            .join("Claude")
            .join("Claude.exe"),
        PathBuf::from(
            env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string()),
        )
        .join("Claude")
        .join("Claude.exe"),
    ]
}

fn migrate_legacy_settings() {
    let legacy = legacy_settings_file();
    let current = settings_file();
    if !legacy.exists() || legacy == current {
        return;
    }
    if let Some(parent) = current.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if !current.exists() {
        let _ = fs::copy(&legacy, &current);
    }
    let _ = fs::remove_file(legacy);
}

pub fn get_launcher_settings() -> Option<Settings> {
    migrate_legacy_settings();
    let text = fs::read_to_string(settings_file()).ok()?;
    serde_json::from_value(parse_json_text(&text).ok()?).ok()
}

fn save_launcher_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_file();
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(
        &path,
        serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let legacy = legacy_settings_file();
    if legacy.exists() && legacy != path {
        let _ = fs::remove_file(legacy);
    }
    Ok(())
}

fn write_config_to_all_paths(file_name: &str, content: &str) -> Result<(), String> {
    let standard_dir = config_lib_dir();
    fs::create_dir_all(&standard_dir).map_err(|e| e.to_string())?;
    fs::write(standard_dir.join(file_name), content).map_err(|e| e.to_string())?;

    let packages_dir = env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("AppData")
        .join("Local")
        .join("Packages");
    if let Ok(entries) = fs::read_dir(packages_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if !name.contains("claude") {
                continue;
            }
            let dir = entry
                .path()
                .join("LocalCache")
                .join("Local")
                .join("Claude-3p")
                .join("configLibrary");
            let _ = fs::create_dir_all(&dir);
            let _ = fs::write(dir.join(file_name), content);
        }
    }
    Ok(())
}

fn fetch_models_list(base_url: &str, api_key: &str, auth_scheme: &str) -> Result<Value, String> {
    let url = normalize_models_url(base_url)?;
    let mut req = http_client().get(url);
    if auth_scheme == "x-api-key" {
        req = req.header("x-api-key", api_key);
    } else {
        req = req.bearer_auth(api_key);
    }
    let res = req.send().map_err(|e| format!("Request failed: {e}"))?;
    let status = res.status();
    let text = res.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("API responded with status {status}: {text}"));
    }
    parse_json_text(&text).map_err(|e| format!("Failed to parse models response: {e}"))
}

fn is_free_model(model: &ProviderModel) -> bool {
    model.id.ends_with(":free")
        || model
            .pricing
            .as_ref()
            .map(|pricing| {
                pricing.prompt.as_deref() == Some("0") && pricing.completion.as_deref() == Some("0")
            })
            .unwrap_or(false)
}

fn model_priority(model: &ProviderModel) -> usize {
    PREFERRED_FREE_MODEL_IDS
        .iter()
        .position(|id| *id == model.id)
        .unwrap_or(PREFERRED_FREE_MODEL_IDS.len() + if is_free_model(model) { 0 } else { 1000 })
}

pub fn normalize_models_response(provider_response: Value) -> Result<NormalizedModels, String> {
    let parsed: ProviderModelsResponse =
        serde_json::from_value(provider_response).map_err(|e| e.to_string())?;
    let mut models: Vec<_> = parsed
        .data
        .into_iter()
        .filter(|model| !model.id.is_empty())
        .collect();
    models.sort_by(|a, b| {
        model_priority(a).cmp(&model_priority(b)).then_with(|| {
            a.name
                .as_deref()
                .unwrap_or(&a.id)
                .cmp(b.name.as_deref().unwrap_or(&b.id))
        })
    });

    let data: Vec<_> = models
        .into_iter()
        .take(CLAUDE_MODEL_ALIASES.len())
        .enumerate()
        .map(|(index, model)| NormalizedModel {
            kind: "model".to_string(),
            id: CLAUDE_MODEL_ALIASES[index].to_string(),
            display_name: model.name.unwrap_or_else(|| model.id.clone()),
            created_at: "1970-01-01T00:00:00.000Z".to_string(),
            provider_model_id: model.id,
            capabilities: default_capabilities(),
        })
        .collect();
    let routes = data
        .iter()
        .map(|model| (model.id.clone(), model.provider_model_id.clone()))
        .collect();
    Ok(NormalizedModels {
        first_id: data.first().map(|model| model.id.clone()),
        last_id: data.last().map(|model| model.id.clone()),
        data,
        has_more: false,
        routes,
    })
}

fn build_inference_models(models: &[NormalizedModel]) -> Vec<InferenceModel> {
    models
        .iter()
        .map(|model| InferenceModel {
            name: model.id.clone(),
            label_override: model.display_name.clone(),
            provider_model_id: model.provider_model_id.clone(),
            display_name: model.display_name.clone(),
        })
        .collect()
}

fn claude_config(inference_models: &[InferenceModel]) -> Value {
    let mut config = json!({
        "inferenceProvider": "gateway",
        "inferenceGatewayBaseUrl": format!("http://127.0.0.1:{PORT}"),
        "inferenceGatewayApiKey": PROXY_AUTH_TOKEN,
        "inferenceGatewayAuthScheme": "bearer",
        "modelDiscoveryEnabled": if inference_models.is_empty() { "true" } else { "false" }
    });
    if !inference_models.is_empty() {
        config["inferenceModels"] = serde_json::to_value(inference_models).unwrap_or(Value::Null);
    }
    config
}

fn update_applied_claude_config(inference_models: &[InferenceModel]) {
    let Ok(text) = fs::read_to_string(meta_file()) else {
        return;
    };
    let Ok(meta) = parse_json_text(&text) else {
        return;
    };
    let Some(applied_id) = meta.get("appliedId").and_then(Value::as_str) else {
        return;
    };
    let content = serde_json::to_string_pretty(&claude_config(inference_models)).unwrap();
    let _ = write_config_to_all_paths(&format!("{applied_id}.json"), &content);
}

pub fn save_config(base_url: &str, api_key: &str, auth_scheme: &str) -> Value {
    let existing = get_launcher_settings();
    let real_api_key = if api_key.trim().is_empty() {
        existing
            .as_ref()
            .and_then(|s| unprotect_secret(&s.real_api_key).ok())
            .unwrap_or_default()
    } else {
        api_key.trim().to_string()
    };
    if base_url.trim().is_empty() || real_api_key.is_empty() {
        return json!({ "success": false, "error": "缺少 Gateway Base URL 或 API Key" });
    }
    if auth_scheme != "bearer" && auth_scheme != "x-api-key" {
        return json!({ "success": false, "error": "不支援的 Auth Scheme" });
    }
    if let Err(error) = normalize_messages_url(base_url) {
        return json!({ "success": false, "error": error });
    }

    let mut inference_models = Vec::new();
    let mut routes = HashMap::new();
    if let Ok(raw_models) = fetch_models_list(base_url, &real_api_key, auth_scheme) {
        if let Ok(normalized) = normalize_models_response(raw_models) {
            routes = normalized.routes.clone();
            inference_models = build_inference_models(&normalized.data);
        }
    }
    let stored_api_key = match protect_secret(&real_api_key) {
        Ok(secret) => secret,
        Err(error) => return json!({ "success": false, "error": error }),
    };

    let settings = Settings {
        real_base_url: base_url.trim().to_string(),
        real_api_key: stored_api_key,
        real_auth_scheme: auth_scheme.to_string(),
        real_model: existing.as_ref().and_then(|s| s.real_model.clone()),
        real_model_routes: if routes.is_empty() {
            existing
                .as_ref()
                .map(|s| s.real_model_routes.clone())
                .unwrap_or_default()
        } else {
            routes
        },
    };
    if let Err(error) = save_launcher_settings(&settings) {
        return json!({ "success": false, "error": error });
    }

    let content = serde_json::to_string_pretty(&claude_config(&inference_models)).unwrap();
    if let Err(error) = write_config_to_all_paths(&format!("{CONFIG_ID}.json"), &content) {
        return json!({ "success": false, "error": error });
    }
    let meta = json!({
        "appliedId": CONFIG_ID,
        "entries": [{ "id": CONFIG_ID, "name": "FreeClaudeLauncher" }]
    });
    if let Err(error) =
        write_config_to_all_paths("_meta.json", &serde_json::to_string_pretty(&meta).unwrap())
    {
        return json!({ "success": false, "error": error });
    }
    json!({ "success": true, "id": CONFIG_ID })
}

pub fn restore_official_config() -> Value {
    let _ = fs::remove_dir_all(config_lib_dir());
    if let Ok(entries) = fs::read_dir(
        env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("AppData")
            .join("Local")
            .join("Packages"),
    ) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .contains("claude")
            {
                let _ = fs::remove_dir_all(
                    entry
                        .path()
                        .join("LocalCache")
                        .join("Local")
                        .join("Claude-3p")
                        .join("configLibrary"),
                );
            }
        }
    }
    let _ = fs::remove_file(settings_file());
    let legacy = legacy_settings_file();
    if legacy != settings_file() {
        let _ = fs::remove_file(legacy);
    }
    json!({ "success": true })
}

fn powershell_output(script: &str) -> Option<String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn get_claude_appx_package_family_name() -> Option<String> {
    powershell_output(
        "Get-AppxPackage -Name *Claude* | Select-Object -ExpandProperty PackageFamilyName",
    )
}

fn get_claude_appx_application_id() -> String {
    powershell_output("$app = Get-AppxPackage -Name *Claude*; if ($app) { $manifestPath = Join-Path $app.InstallLocation 'AppxManifest.xml'; if (Test-Path $manifestPath) { [xml]$xml = Get-Content $manifestPath; $xml.Package.Applications.Application.Id } }")
        .unwrap_or_else(|| "Claude".to_string())
}

pub fn detect_claude_path() -> Option<PathBuf> {
    for path in known_claude_paths() {
        if path.exists() {
            return Some(path);
        }
    }
    if let Some(install_location) = powershell_output(
        "Get-AppxPackage -Name *Claude* | Select-Object -ExpandProperty InstallLocation",
    ) {
        for suffix in ["app\\Claude.exe", "Claude.exe"] {
            let path = PathBuf::from(&install_location).join(suffix);
            if path.exists() {
                return Some(path);
            }
        }
    }
    powershell_output("Get-Process -Name claude -ErrorAction SilentlyContinue | Where-Object { $_.Path } | Select-Object -First 1 -ExpandProperty Path")
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

pub fn launch_claude(custom_path: Option<&str>) -> Result<String, String> {
    let target = custom_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .or_else(|| detect_claude_path().map(|p| p.to_string_lossy().to_string()))
        .ok_or_else(|| "找不到 Claude.exe".to_string())?;

    let target = validate_launch_path(&target)?;
    if !Path::new(&target).exists() {
        return Err("找不到 Claude.exe".to_string());
    }

    let launched = if let Some(family) = get_claude_appx_package_family_name() {
        if target.contains("WindowsApps") || target.contains(&family) {
            let aumid = format!(
                "shell:AppsFolder\\{}!{}",
                family,
                get_claude_appx_application_id()
            );
            Command::new("explorer.exe").arg(aumid).spawn()
        } else {
            Command::new(&target).spawn()
        }
    } else {
        Command::new(&target).spawn()
    };

    launched.map(|_| target).map_err(|error| error.to_string())
}

fn read_body(req: &mut Request, max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    req.as_reader()
        .take((max_bytes + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() > max_bytes {
        return Err("Request body too large".to_string());
    }
    Ok(body)
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).unwrap()
}

fn cors_headers(origin: Option<&str>) -> Vec<Header> {
    let mut headers = vec![
        header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
        header("Access-Control-Allow-Headers", "Content-Type"),
    ];
    if let Some(origin) = origin {
        if is_allowed_origin(Some(origin)) {
            headers.push(header("Access-Control-Allow-Origin", origin));
            headers.push(header("Vary", "Origin"));
        }
    }
    headers
}

fn send_json(req: Request, status: u16, data: Value, origin: Option<String>) {
    let mut response = Response::from_string(serde_json::to_string(&data).unwrap())
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", "application/json; charset=utf-8"));
    for header in cors_headers(origin.as_deref()) {
        response.add_header(header);
    }
    let _ = req.respond(response);
}

fn get_origin(req: &Request) -> Option<String> {
    get_header(req, "Origin")
}

fn get_header(req: &Request, name: &str) -> Option<String> {
    req.headers()
        .iter()
        .find(|header| header.field.to_string().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().to_string())
}

struct ChannelReader {
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    offset: usize,
}

impl std::io::Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.offset >= self.buffer.len() {
            match self.rx.recv() {
                Ok(data) => {
                    self.buffer = data;
                    self.offset = 0;
                }
                Err(_) => {
                    return Ok(0);
                }
            }
        }
        
        let available = self.buffer.len() - self.offset;
        let to_copy = std::cmp::min(available, buf.len());
        buf[..to_copy].copy_from_slice(&self.buffer[self.offset..self.offset + to_copy]);
        self.offset += to_copy;
        Ok(to_copy)
    }
}

fn anthropic_to_openai_request(body: &str, settings: &Settings) -> Result<(String, bool), String> {
    let mut data: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    
    // 獲取原始 model 名稱
    let raw_model = data.get("model").and_then(Value::as_str).unwrap_or("").to_string();
    
    // 映射 model
    if let Some(mapped) = settings.real_model_routes.get(&raw_model) {
        data["model"] = Value::String(mapped.clone());
    } else if let Some(model) = &settings.real_model {
        data["model"] = Value::String(model.clone());
    }
    
    // 處理 thinking 欄位以相容 OpenAI (例如 o1/o3-mini 使用 reasoning_effort)
    let thinking_value = data.get("thinking").cloned();
    if let Some(thinking) = thinking_value {
        if thinking.get("type").and_then(Value::as_str) == Some("enabled") {
            let budget = thinking.get("budget_tokens").and_then(Value::as_u64).unwrap_or(1024);
            let effort = if budget > 2048 {
                "high"
            } else if budget > 1024 {
                "medium"
            } else {
                "low"
            };
            let target_model = data.get("model").and_then(Value::as_str).unwrap_or("");
            if target_model.contains("o1") || target_model.contains("o3") {
                data["reasoning_effort"] = Value::String(effort.to_string());
            }
        }
    }
    if let Some(obj) = data.as_object_mut() {
        obj.remove("thinking");
    }
    
    // 轉換 system prompt
    let system_content = data.get("system").and_then(Value::as_str).map(|s| s.to_string());
    if let Some(content) = system_content {
        if let Some(messages) = data.get_mut("messages").and_then(Value::as_array_mut) {
            messages.insert(0, json!({
                "role": "system",
                "content": content
            }));
        }
    }
    
    if let Some(obj) = data.as_object_mut() {
        obj.remove("system");
    }
    
    // 轉換 tools
    if let Some(tools) = data.get_mut("tools").and_then(Value::as_array_mut) {
        let mut openai_tools = Vec::new();
        for t in tools.iter() {
            let name = t.get("name").cloned().unwrap_or(Value::Null);
            let description = t.get("description").cloned().unwrap_or(Value::Null);
            let parameters = t.get("input_schema").cloned().unwrap_or(Value::Null);
            
            openai_tools.push(json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": parameters
                }
            }));
        }
        data["tools"] = Value::Array(openai_tools);
    }
    
    // 轉換 tool_choice
    if let Some(tool_choice) = data.get("tool_choice") {
        if let Some(choice_obj) = tool_choice.as_object() {
            let choice_type = choice_obj.get("type").and_then(Value::as_str).unwrap_or("");
            let new_choice = match choice_type {
                "auto" => Value::String("auto".to_string()),
                "any" => Value::String("required".to_string()),
                "tool" => {
                    let name = choice_obj.get("name").cloned().unwrap_or(Value::Null);
                    json!({
                        "type": "function",
                        "function": {
                            "name": name
                        }
                    })
                }
                _ => Value::Null,
            };
            if new_choice != Value::Null {
                data["tool_choice"] = new_choice;
            } else {
                if let Some(obj) = data.as_object_mut() {
                    obj.remove("tool_choice");
                }
            }
        }
    }
    
    // 轉換 messages 中的 tool_use 與 tool_result
    if let Some(messages) = data.get("messages").and_then(Value::as_array) {
        let mut openai_messages = Vec::new();
        
        for msg in messages {
            let role = msg.get("role").and_then(Value::as_str).unwrap_or("user");
            let content = msg.get("content");
            
            if role == "assistant" {
                let mut text_content = String::new();
                let mut tool_calls = Vec::new();
                
                if let Some(arr) = content.and_then(Value::as_array) {
                    for block in arr {
                        let kind = block.get("type").and_then(Value::as_str).unwrap_or("");
                        if kind == "text" {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                text_content.push_str(text);
                            }
                        } else if kind == "tool_use" {
                            let id = block.get("id").cloned().unwrap_or(Value::Null);
                            let name = block.get("name").cloned().unwrap_or(Value::Null);
                            let input = block.get("input").cloned().unwrap_or(Value::Null);
                            tool_calls.push(json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
                                }
                            }));
                        }
                    }
                } else if let Some(text) = content.and_then(Value::as_str) {
                    text_content.push_str(text);
                }
                
                let mut openai_msg = json!({
                    "role": "assistant"
                });
                if !text_content.is_empty() {
                    openai_msg["content"] = Value::String(text_content);
                } else {
                    openai_msg["content"] = Value::Null;
                }
                if !tool_calls.is_empty() {
                    openai_msg["tool_calls"] = Value::Array(tool_calls);
                }
                openai_messages.push(openai_msg);
                
            } else {
                // role == "user" or "system"
                let mut user_text = String::new();
                let mut tool_messages = Vec::new();
                
                if let Some(arr) = content.and_then(Value::as_array) {
                    for block in arr {
                        let kind = block.get("type").and_then(Value::as_str).unwrap_or("");
                        if kind == "text" {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                user_text.push_str(text);
                            }
                        } else if kind == "tool_result" {
                            let tool_use_id = block.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
                            
                            let mut result_content = String::new();
                            if let Some(res_content) = block.get("content") {
                                if let Some(res_arr) = res_content.as_array() {
                                    for res_block in res_arr {
                                        if res_block.get("type").and_then(Value::as_str) == Some("text") {
                                            if let Some(text) = res_block.get("text").and_then(Value::as_str) {
                                                result_content.push_str(text);
                                            }
                                        }
                                    }
                                } else if let Some(text) = res_content.as_str() {
                                    result_content.push_str(text);
                                }
                            }
                            
                            tool_messages.push(json!({
                                "role": "tool",
                                "tool_call_id": tool_use_id,
                                "content": result_content
                            }));
                        }
                    }
                } else if let Some(text) = content.and_then(Value::as_str) {
                    user_text.push_str(text);
                }
                
                let role_to_send = if role == "system" { "system" } else { "user" };
                if !user_text.is_empty() || tool_messages.is_empty() {
                    openai_messages.push(json!({
                        "role": role_to_send,
                        "content": user_text
                    }));
                }
                for t_msg in tool_messages {
                    openai_messages.push(t_msg);
                }
            }
        }
        
        data["messages"] = Value::Array(openai_messages);
    }
    
    let is_stream = data.get("stream").and_then(Value::as_bool).unwrap_or(false);
    
    Ok((serde_json::to_string(&data).unwrap(), is_stream))
}

fn openai_to_anthropic_response(openai_body: &str, req_model: &str) -> Result<Value, String> {
    let data: Value = serde_json::from_str(openai_body).map_err(|e| e.to_string())?;
    
    let first_choice = data.get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());
        
    let message = first_choice.and_then(|choice| choice.get("message"));
    
    let content_text = message.and_then(|msg| msg.get("content")).and_then(Value::as_str).unwrap_or("");
    let reasoning_text = message.and_then(|msg| msg.get("reasoning_content")).and_then(Value::as_str).unwrap_or("");
    
    let mut content_blocks = Vec::new();
    if !reasoning_text.is_empty() {
        content_blocks.push(json!({
            "type": "thinking",
            "thinking": reasoning_text
        }));
    }
    if !content_text.is_empty() {
        content_blocks.push(json!({
            "type": "text",
            "text": content_text
        }));
    }
    
    let mut stop_reason = "end_turn";
    
    // 處理 tool_calls
    if let Some(tool_calls) = message.and_then(|msg| msg.get("tool_calls")).and_then(Value::as_array) {
        stop_reason = "tool_use";
        for tc in tool_calls {
            let tc_id = tc.get("id").and_then(Value::as_str).unwrap_or("");
            let tc_name = tc.get("function").and_then(|f| f.get("name")).and_then(Value::as_str).unwrap_or("");
            let tc_args_str = tc.get("function").and_then(|f| f.get("arguments")).and_then(Value::as_str).unwrap_or("{}");
            let tc_args: Value = serde_json::from_str(tc_args_str).unwrap_or(json!({}));
            
            content_blocks.push(json!({
                "type": "tool_use",
                "id": tc_id,
                "name": tc_name,
                "input": tc_args
            }));
        }
    }
    
    let finish_reason = first_choice.and_then(|choice| choice.get("finish_reason")).and_then(Value::as_str).unwrap_or("");
    if finish_reason == "tool_calls" || finish_reason == "function_call" {
        stop_reason = "tool_use";
    }
    
    let input_tokens = data.get("usage").and_then(|u| u.get("prompt_tokens")).and_then(Value::as_u64).unwrap_or(0);
    let output_tokens = data.get("usage").and_then(|u| u.get("completion_tokens")).and_then(Value::as_u64).unwrap_or(0);
    
    let msg_id = format!("msg_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    
    Ok(json!({
        "id": msg_id,
        "type": "message",
        "role": "assistant",
        "content": content_blocks,
        "model": req_model,
        "stop_reason": stop_reason,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens
        }
    }))
}

fn handle_proxy_request(mut req: Request, origin: Option<String>) {
    let Some(settings) = get_launcher_settings() else {
        send_json(
            req,
            500,
            json!({ "error": "Launcher has not been configured yet." }),
            origin,
        );
        return;
    };
    let headers: Vec<_> = req
        .headers()
        .iter()
        .map(|header| {
            (
                header.field.as_str().to_string(),
                header.value.as_str().to_string(),
            )
        })
        .collect();
    let body = match read_body(&mut req, MAX_PROXY_BODY_BYTES) {
        Ok(body) => body,
        Err(error) => {
            let status = if error.contains("too large") {
                413
            } else {
                400
            };
            send_json(req, status, json!({ "error": error }), origin);
            return;
        }
    };
    
    let body_str = String::from_utf8_lossy(&body);
    let is_openai_format = !settings.real_base_url.contains("api.anthropic.com") 
                        && !settings.real_base_url.contains("openrouter.ai");
                        
    let req_model = match serde_json::from_str::<Value>(&body_str) {
        Ok(v) => v.get("model").and_then(Value::as_str).unwrap_or("unknown").to_string(),
        Err(_) => "unknown".to_string(),
    };

    // 攔截並偽造連線探測 (Probe) 請求的回應
    let (max_tokens, is_probe_stream) = match serde_json::from_str::<Value>(&body_str) {
        Ok(v) => (
            v.get("max_tokens").and_then(Value::as_u64).unwrap_or(9999),
            v.get("stream").and_then(Value::as_bool).unwrap_or(false)
        ),
        Err(_) => (9999, false),
    };

    if max_tokens <= 5 {
        println!("-> [探測攔截] 繞過 Claude 檢查，自動回傳成功回應 (model: {})", req_model);
        if is_probe_stream {
            let msg_id = format!("msg_probe_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
            let mut response_headers = vec![
                Header::from_bytes("Content-Type", "text/event-stream; charset=utf-8").unwrap(),
                Header::from_bytes("Cache-Control", "no-cache").unwrap(),
                Header::from_bytes("Connection", "keep-alive").unwrap(),
            ];
            for header in cors_headers(origin.as_deref()) {
                response_headers.push(header);
            }
            
            let mut events = Vec::new();
            events.push(format!("event: message_start\ndata: {}\n\n", json!({
                "type": "message_start",
                "message": {
                    "id": msg_id.clone(),
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": req_model,
                    "stop_reason": null,
                    "usage": { "input_tokens": 1, "output_tokens": 0 }
                }
            })).into_bytes());
            
            events.push(format!("event: content_block_start\ndata: {}\n\n", json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "text", "text": "" }
            })).into_bytes());
            
            events.push(format!("event: content_block_delta\ndata: {}\n\n", json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "text_delta", "text": "." }
            })).into_bytes());
            
            events.push(format!("event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n").into_bytes());
            events.push(format!("event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":1}}}}\n\n").into_bytes());
            events.push(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec());
            
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for event in events {
                    let _ = tx.send(event);
                }
            });
            
            let response_reader = ChannelReader { rx, buffer: Vec::new(), offset: 0 };
            let _ = req.respond(Response::new(
                StatusCode(200),
                response_headers,
                response_reader,
                None,
                None,
            ));
        } else {
            let msg_id = format!("msg_probe_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
            let probe_res = json!({
                "id": msg_id,
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "."
                    }
                ],
                "model": req_model,
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 1,
                    "output_tokens": 1
                }
            });
            send_json(req, 200, probe_res, origin);
        }
        return;
    }

    let (proxy_body, is_stream) = if is_openai_format {
        match anthropic_to_openai_request(&body_str, &settings) {
            Ok(res) => res,
            Err(error) => {
                println!("<- 錯誤: 轉換請求格式失敗: {:?}", error);
                send_json(req, 400, json!({ "error": error }), origin);
                return;
            }
        }
    } else {
        (prepare_proxy_body(&body_str, &settings), false)
    };

    let target_url = if is_openai_format {
        match normalize_gateway_url(&settings.real_base_url, "chat/completions") {
            Ok(url) => url,
            Err(error) => {
                println!("<- 錯誤: 無效的 Gateway URL: {:?}", error);
                send_json(req, 400, json!({ "error": error }), origin);
                return;
            }
        }
    } else {
        match normalize_messages_url(&settings.real_base_url) {
            Ok(url) => url,
            Err(error) => {
                println!("<- 錯誤: 無效的 Gateway URL: {:?}", error);
                send_json(req, 400, json!({ "error": error }), origin);
                return;
            }
        }
    };

    let api_key = match unprotect_secret(&settings.real_api_key) {
        Ok(key) if !key.is_empty() => key,
        Ok(_) => {
            println!("<- 錯誤: API key 為空");
            send_json(req, 500, json!({ "error": "API key is empty" }), origin);
            return;
        }
        Err(error) => {
            println!("<- 錯誤: 解密 API key 失敗: {:?}", error);
            send_json(req, 500, json!({ "error": error }), origin);
            return;
        }
    };

    println!("-> 轉發請求至: {}", target_url);
    println!("-> 轉發 Body: {}", proxy_body);

    let mut upstream = http_client().post(&target_url).body(proxy_body);
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "host" | "content-length" | "authorization" | "x-api-key"
        ) {
            continue;
        }
        upstream = upstream.header(name, value);
    }
    upstream = if settings.real_auth_scheme == "x-api-key" {
        upstream.header("x-api-key", api_key)
    } else {
        upstream.bearer_auth(api_key)
    };

    if is_openai_format && is_stream {
        match upstream.send() {
            Ok(response) => {
                let status = response.status().as_u16();
                println!("<- 上游回應狀態碼(流式): {}", status);
                if status != 200 {
                    let text = response.text().unwrap_or_default();
                    println!("<- 上游流式錯誤狀態碼: {}, Body: {}", status, text);
                    send_json(req, status, json!({ "error": text }), origin);
                    return;
                }
                
                let (tx, rx) = std::sync::mpsc::channel();
                let req_model_clone = req_model.clone();
                
                std::thread::spawn(move || {
                    use std::io::BufRead;
                    use std::io::Write;
                    let reader = std::io::BufReader::new(response);
                    let msg_id = format!("msg_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
                    let mut sent_start = false;
                    let mut sent_stop = false;
                    let mut active_tools: std::collections::HashMap<u64, (String, String, bool)> = std::collections::HashMap::new();
                    
                    let mut has_started_thinking = false;
                    let mut has_stopped_thinking = false;
                    let mut has_started_text = false;
                    let mut has_stopped_text = false;
                    
                    let finish_active_tools = |active_tools: &std::collections::HashMap<u64, (String, String, bool)>, has_started_thinking: bool, tx: &std::sync::mpsc::Sender<Vec<u8>>| {
                        for (&idx, state) in active_tools.iter() {
                            if state.2 {
                                let block_idx = idx + (if has_started_thinking { 2 } else { 1 });
                                let _ = tx.send(format!("event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n", block_idx).into_bytes());
                            }
                        }
                    };

                    let finish_our_blocks = |has_started_thinking: bool, has_stopped_thinking: &mut bool, has_started_text: bool, has_stopped_text: &mut bool, tx: &std::sync::mpsc::Sender<Vec<u8>>| {
                        if has_started_thinking && !*has_stopped_thinking {
                            let _ = tx.send(format!("event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n").into_bytes());
                            *has_stopped_thinking = true;
                        }
                        if has_started_text && !*has_stopped_text {
                            let text_idx = if has_started_thinking { 1 } else { 0 };
                            let _ = tx.send(format!("event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n", text_idx).into_bytes());
                            *has_stopped_text = true;
                        }
                    };

                    let ensure_at_least_one_block = |sent_start: &mut bool, has_started_thinking: bool, has_started_text: &mut bool, has_stopped_text: &mut bool, tx: &std::sync::mpsc::Sender<Vec<u8>>| {
                        if !*sent_start {
                            let start_msg = json!({
                                "type": "message_start",
                                "message": {
                                    "id": msg_id.clone(),
                                    "type": "message",
                                    "role": "assistant",
                                    "content": [],
                                    "model": req_model_clone.clone(),
                                    "stop_reason": null,
                                    "usage": { "input_tokens": 0, "output_tokens": 0 }
                                }
                            });
                            let _ = tx.send(format!("event: message_start\ndata: {}\n\n", start_msg).into_bytes());
                            *sent_start = true;
                        }
                        if !has_started_thinking && !*has_started_text {
                            let block_start = json!({
                                "type": "content_block_start",
                                "index": 0,
                                "content_block": { "type": "text", "text": "" }
                            });
                            let _ = tx.send(format!("event: content_block_start\ndata: {}\n\n", block_start).into_bytes());
                            let _ = tx.send(format!("event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n").into_bytes());
                            *has_started_text = true;
                            *has_stopped_text = true;
                        }
                    };

                    let ensure_sent_start = |sent_start: &mut bool, tx: &std::sync::mpsc::Sender<Vec<u8>>| {
                        if !*sent_start {
                            let start_msg = json!({
                                "type": "message_start",
                                "message": {
                                    "id": msg_id.clone(),
                                    "type": "message",
                                    "role": "assistant",
                                    "content": [],
                                    "model": req_model_clone.clone(),
                                    "stop_reason": null,
                                    "usage": { "input_tokens": 0, "output_tokens": 0 }
                                }
                            });
                            let _ = tx.send(format!("event: message_start\ndata: {}\n\n", start_msg).into_bytes());
                            *sent_start = true;
                        }
                    };
                    
                    for line in reader.lines() {
                        let line = match line {
                            Ok(l) => l,
                            Err(_) => break,
                        };
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        if line.starts_with("data:") {
                            let data_str = line.strip_prefix("data:").unwrap().trim();
                            if data_str == "[DONE]" {
                                if sent_start && !sent_stop {
                                    ensure_at_least_one_block(&mut sent_start, has_started_thinking, &mut has_started_text, &mut has_stopped_text, &tx);
                                    finish_our_blocks(has_started_thinking, &mut has_stopped_thinking, has_started_text, &mut has_stopped_text, &tx);
                                    finish_active_tools(&active_tools, has_started_thinking, &tx);
                                    let has_tools = !active_tools.is_empty();
                                    let stop_rs = if has_tools { "tool_use" } else { "end_turn" };
                                    let _ = tx.send(format!("event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{}\",\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":0}}}}\n\n", stop_rs).into_bytes());
                                    let _ = tx.send(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec());
                                    sent_stop = true;
                                }
                                break;
                            }
                            
                            let chunk: Value = match serde_json::from_str(data_str) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };
                            
                            let delta_obj = chunk.get("choices")
                                .and_then(Value::as_array)
                                .and_then(|choices| choices.first())
                                .and_then(|choice| choice.get("delta"));
                                
                            let delta_content = delta_obj
                                .and_then(|delta| delta.get("content"))
                                .and_then(Value::as_str)
                                .unwrap_or("");
                                
                            let delta_reasoning = delta_obj
                                .and_then(|delta| delta.get("reasoning_content"))
                                .and_then(Value::as_str)
                                .unwrap_or("");
                                
                            let finish_reason = chunk.get("choices")
                                .and_then(Value::as_array)
                                .and_then(|choices| choices.first())
                                .and_then(|choice| choice.get("finish_reason"))
                                .and_then(Value::as_str);
                                
                            if !delta_reasoning.is_empty() {
                                ensure_sent_start(&mut sent_start, &tx);
                                if !has_started_thinking {
                                    let block_start = json!({
                                        "type": "content_block_start",
                                        "index": 0,
                                        "content_block": { "type": "thinking", "thinking": "" }
                                    });
                                    let _ = tx.send(format!("event: content_block_start\ndata: {}\n\n", block_start).into_bytes());
                                    has_started_thinking = true;
                                }
                                
                                print!("{}", delta_reasoning);
                                let _ = std::io::stdout().flush();
                                
                                let block_delta = json!({
                                    "type": "content_block_delta",
                                    "index": 0,
                                    "delta": {
                                        "type": "thinking_delta",
                                        "thinking": delta_reasoning
                                    }
                                });
                                let _ = tx.send(format!("event: content_block_delta\ndata: {}\n\n", block_delta).into_bytes());
                            }
                            
                            if !delta_content.is_empty() {
                                ensure_sent_start(&mut sent_start, &tx);
                                if has_started_thinking && !has_stopped_thinking {
                                    let _ = tx.send(format!("event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n").into_bytes());
                                    has_stopped_thinking = true;
                                }
                                let text_idx = if has_started_thinking { 1 } else { 0 };
                                if !has_started_text {
                                    let block_start = json!({
                                        "type": "content_block_start",
                                        "index": text_idx,
                                        "content_block": { "type": "text", "text": "" }
                                    });
                                    let _ = tx.send(format!("event: content_block_start\ndata: {}\n\n", block_start).into_bytes());
                                    has_started_text = true;
                                }
                                
                                print!("{}", delta_content);
                                let _ = std::io::stdout().flush();
                                
                                let block_delta = json!({
                                    "type": "content_block_delta",
                                    "index": text_idx,
                                    "delta": {
                                        "type": "text_delta",
                                        "text": delta_content
                                    }
                                });
                                let _ = tx.send(format!("event: content_block_delta\ndata: {}\n\n", block_delta).into_bytes());
                            }
                            
                            // 處理串流中的 tool_calls
                            if let Some(tool_calls) = delta_obj.and_then(|d| d.get("tool_calls")).and_then(Value::as_array) {
                                for tc in tool_calls {
                                    let idx = tc.get("index").and_then(Value::as_u64).unwrap_or(0);
                                    let tc_id = tc.get("id").and_then(Value::as_str).map(|s| s.to_string());
                                    let function_obj = tc.get("function");
                                    let tc_name = function_obj.and_then(|f| f.get("name")).and_then(Value::as_str).map(|s| s.to_string());
                                    let tc_args = function_obj.and_then(|f| f.get("arguments")).and_then(Value::as_str).unwrap_or("");
                                    
                                    let state = active_tools.entry(idx).or_insert_with(|| {
                                        (tc_id.clone().unwrap_or_default(), tc_name.clone().unwrap_or_default(), false)
                                    });
                                    
                                    if let Some(ref id) = tc_id {
                                        state.0 = id.clone();
                                    }
                                    if let Some(ref name) = tc_name {
                                        state.1 = name.clone();
                                    }
                                    
                                    let block_idx = idx + (if has_started_thinking { 2 } else { 1 });
                                    
                                    if !state.2 && !state.0.is_empty() && !state.1.is_empty() {
                                        let block_start = json!({
                                            "type": "content_block_start",
                                            "index": block_idx,
                                            "content_block": {
                                                "type": "tool_use",
                                                "id": state.0.clone(),
                                                "name": state.1.clone(),
                                                "input": {}
                                            }
                                        });
                                        let _ = tx.send(format!("event: content_block_start\ndata: {}\n\n", block_start).into_bytes());
                                        state.2 = true;
                                    }
                                    
                                    if !tc_args.is_empty() && state.2 {
                                        print!("[tool call delta: {}]", tc_args);
                                        let _ = std::io::stdout().flush();
                                        let block_delta = json!({
                                            "type": "content_block_delta",
                                            "index": block_idx,
                                            "delta": {
                                                "type": "input_json_delta",
                                                "partial_json": tc_args
                                            }
                                        });
                                        let _ = tx.send(format!("event: content_block_delta\ndata: {}\n\n", block_delta).into_bytes());
                                    }
                                }
                            }
                            
                            if finish_reason.is_some() {
                                let is_tool_finish = finish_reason == Some("tool_calls") || finish_reason == Some("function_call") || !active_tools.is_empty();
                                let stop_rs = if is_tool_finish { "tool_use" } else { "end_turn" };
                                
                                if sent_start && !sent_stop {
                                    ensure_at_least_one_block(&mut sent_start, has_started_thinking, &mut has_started_text, &mut has_stopped_text, &tx);
                                    finish_our_blocks(has_started_thinking, &mut has_stopped_thinking, has_started_text, &mut has_stopped_text, &tx);
                                    finish_active_tools(&active_tools, has_started_thinking, &tx);
                                    let _ = tx.send(format!("event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{}\",\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":0}}}}\n\n", stop_rs).into_bytes());
                                    let _ = tx.send(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec());
                                    sent_stop = true;
                                }
                                break;
                            }
                        }
                    }
                    if sent_start && !sent_stop {
                        ensure_at_least_one_block(&mut sent_start, has_started_thinking, &mut has_started_text, &mut has_stopped_text, &tx);
                        finish_our_blocks(has_started_thinking, &mut has_stopped_thinking, has_started_text, &mut has_stopped_text, &tx);
                        finish_active_tools(&active_tools, has_started_thinking, &tx);
                        let has_tools = !active_tools.is_empty();
                        let stop_rs = if has_tools { "tool_use" } else { "end_turn" };
                        let _ = tx.send(format!("event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{}\",\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":0}}}}\n\n", stop_rs).into_bytes());
                        let _ = tx.send(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec());
                    }
                    println!();
                });
                
                let response_reader = ChannelReader { rx, buffer: Vec::new(), offset: 0 };
                let mut response_headers = vec![
                    Header::from_bytes("Content-Type", "text/event-stream; charset=utf-8").unwrap(),
                    Header::from_bytes("Cache-Control", "no-cache").unwrap(),
                    Header::from_bytes("Connection", "keep-alive").unwrap(),
                ];
                for header in cors_headers(origin.as_deref()) {
                    response_headers.push(header);
                }
                
                let _ = req.respond(Response::new(
                    StatusCode(200),
                    response_headers,
                    response_reader,
                    None,
                    None,
                ));
            }
            Err(error) => {
                println!("<- 轉發錯誤: {:?}", error);
                send_json(
                    req,
                    500,
                    json!({ "error": format!("Proxy forwarding error: {error}") }),
                    origin,
                );
            }
        }
    } else {
        match upstream.send() {
            Ok(response) => {
                let status = response.status().as_u16();
                println!("<- 上游回應狀態碼: {}", status);
                if is_openai_format && status == 200 {
                    let response_text = match response.text() {
                        Ok(t) => t,
                        Err(e) => {
                            send_json(req, 500, json!({ "error": e.to_string() }), origin);
                            return;
                        }
                    };
                    match openai_to_anthropic_response(&response_text, &req_model) {
                        Ok(anthropic_res) => {
                            send_json(req, 200, anthropic_res, origin);
                        }
                        Err(e) => {
                            send_json(req, 500, json!({ "error": e }), origin);
                        }
                    }
                } else if is_openai_format {
                    let text = response.text().unwrap_or_default();
                    println!("<- 上游錯誤 Body: {}", text);
                    send_json(req, status, serde_json::from_str(&text).unwrap_or(json!({ "error": text })), origin);
                } else {
                    let mut response_headers = Vec::new();
                    for (name, value) in response.headers() {
                        if let Ok(header) = Header::from_bytes(name.as_str(), value.as_bytes()) {
                            response_headers.push(header);
                        }
                    }
                    for header in cors_headers(origin.as_deref()) {
                        response_headers.push(header);
                    }
                    let _ = req.respond(Response::new(
                        StatusCode(status),
                        response_headers,
                        response,
                        None,
                        None,
                    ));
                }
            }
            Err(error) => {
                println!("<- 轉發錯誤: {:?}", error);
                send_json(
                    req,
                    500,
                    json!({ "error": format!("Proxy forwarding error: {error}") }),
                    origin,
                );
            }
        }
    }
}

fn handle_models_request(req: Request, origin: Option<String>) {
    let Some(mut settings) = get_launcher_settings() else {
        println!("<- 錯誤: Launcher 尚未配置");
        send_json(
            req,
            500,
            json!({ "error": "Launcher has not been configured yet." }),
            origin,
        );
        return;
    };
    let api_key = match unprotect_secret(&settings.real_api_key) {
        Ok(key) => key,
        Err(error) => {
            println!("<- 錯誤: 解密 API key 失敗: {:?}", error);
            send_json(req, 500, json!({ "error": error }), origin);
            return;
        }
    };
    println!("-> 正在獲取模型列表，Gateway: {}", settings.real_base_url);
    match fetch_models_list(
        &settings.real_base_url,
        &api_key,
        &settings.real_auth_scheme,
    )
    .and_then(normalize_models_response)
    {
        Ok(normalized) => {
            println!("<- 獲取模型列表成功，模型數量: {}", normalized.data.len());
            settings.real_model_routes = normalized.routes.clone();
            let _ = save_launcher_settings(&settings);
            update_applied_claude_config(&build_inference_models(&normalized.data));
            send_json(req, 200, serde_json::to_value(normalized).unwrap(), origin);
        }
        Err(error) => {
            println!("<- 獲取模型列表失敗: {:?}", error);
            send_json(req, 500, json!({ "error": error }), origin);
        }
    }
}

fn handle_request(req: Request) -> bool {
    let origin = get_origin(&req);
    let path = req.url().split('?').next().unwrap_or("/");
    println!("-> 收到請求: {} {}", req.method(), path);
    if origin
        .as_deref()
        .is_some_and(|origin| !is_allowed_origin(Some(origin)))
    {
        println!("<- 拒絕來源 (Forbidden origin): {:?}", origin);
        send_json(req, 403, json!({ "error": "Forbidden origin" }), origin);
        return false;
    }
    if req.method() == &Method::Options {
        let mut response = Response::empty(StatusCode(204));
        for header in cors_headers(origin.as_deref()) {
            response.add_header(header);
        }
        let _ = req.respond(response);
        return false;
    }
    match (req.method(), path) {
        (&Method::Post, "/v1/messages") => {
            if !is_valid_proxy_authorization(get_header(&req, "Authorization").as_deref()) {
                send_json(req, 401, json!({ "error": "Unauthorized" }), origin);
                return false;
            }
            handle_proxy_request(req, origin)
        }
        (&Method::Get, "/v1/models") => {
            if !is_valid_proxy_authorization(get_header(&req, "Authorization").as_deref()) {
                send_json(req, 401, json!({ "error": "Unauthorized" }), origin);
                return false;
            }
            handle_models_request(req, origin)
        }
        (&Method::Get, "/") => {
            let _ = req.respond(
                Response::from_string("FreeClaudeLauncher API proxy is running")
                    .with_header(header("Content-Type", "text/plain; charset=utf-8")),
            );
        }
        _ => {
            let _ = req.respond(
                Response::from_string("404 Page Not Found")
                    .with_status_code(StatusCode(404))
                    .with_header(header("Content-Type", "text/plain; charset=utf-8")),
            );
        }
    }
    false
}

pub fn app_url() -> String {
    format!("http://127.0.0.1:{PORT}")
}

fn run_server_loop(server: Server) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("==================================================");
    println!("FreeClaudeLauncher Rust 已啟動");
    println!("本機服務: http://127.0.0.1:{PORT}");
    println!("API 代理: http://127.0.0.1:{PORT}/v1/messages");
    println!("==================================================");
    for req in server.incoming_requests() {
        if handle_request(req) {
            break;
        }
    }
    Ok(())
}

pub fn run_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_server_loop(Server::http(format!("127.0.0.1:{PORT}"))?)
}

pub fn start_server_background() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = Server::http(format!("127.0.0.1:{PORT}"))?;
    thread::spawn(|| {
        if let Err(error) = run_server_loop(server) {
            eprintln!("server failed: {error}");
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_provider_urls_to_messages_endpoint() {
        assert_eq!(
            normalize_messages_url("https://openrouter.ai/api").unwrap(),
            "https://openrouter.ai/api/v1/messages"
        );
        assert_eq!(
            normalize_messages_url("https://api.anthropic.com/v1/").unwrap(),
            "https://api.anthropic.com/v1/messages"
        );
        assert!(normalize_messages_url("http://evil.example").is_err());
    }

    #[test]
    fn rewrites_model_from_saved_routes() {
        let mut routes = HashMap::new();
        routes.insert(
            "anthropic/claude-sonnet-4-5".to_string(),
            "openai/gpt-oss-20b:free".to_string(),
        );
        let settings = Settings {
            real_model_routes: routes,
            ..Settings::default()
        };

        let body = prepare_proxy_body(
            r#"{"model":"anthropic/claude-sonnet-4-5","messages":[]}"#,
            &settings,
        );

        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["model"],
            "openai/gpt-oss-20b:free"
        );
    }

    #[test]
    fn public_config_hides_api_key() {
        let settings = Settings {
            real_base_url: "https://openrouter.ai/api".to_string(),
            real_api_key: "secret".to_string(),
            real_auth_scheme: "bearer".to_string(),
            ..Settings::default()
        };

        assert_eq!(
            to_public_config(&settings),
            json!({
                "baseUrl": "https://openrouter.ai/api",
                "authScheme": "bearer",
                "hasApiKey": true
            })
        );
    }

    #[test]
    fn validates_proxy_authorization_header() {
        assert!(is_valid_proxy_authorization(Some(
            "Bearer local-proxy-token"
        )));
        assert!(!is_valid_proxy_authorization(None));
        assert!(!is_valid_proxy_authorization(Some("Bearer wrong")));
        assert!(!is_valid_proxy_authorization(Some("local-proxy-token")));
    }

    #[test]
    fn protects_and_restores_api_key() {
        let protected = protect_secret("secret-key").unwrap();
        assert_ne!(protected, "secret-key");
        assert_eq!(unprotect_secret(&protected).unwrap(), "secret-key");
        assert_eq!(unprotect_secret("legacy-key").unwrap(), "legacy-key");
    }

    #[test]
    fn validates_launch_path_shape() {
        assert!(validate_launch_path("C:\\Program Files\\Claude\\Claude.exe").is_ok());
        assert!(validate_launch_path("Claude.exe").is_err());
        assert!(validate_launch_path("C:\\Program Files\\Claude\\Claude.txt").is_err());
    }

    #[test]
    fn test_anthropic_to_openai_request_tools_conversion() {
        let body = json!({
            "model": "anthropic/claude-3-5-sonnet",
            "messages": [
                {
                    "role": "user",
                    "content": "Hello"
                }
            ],
            "tools": [
                {
                    "name": "Agent",
                    "description": "Launch a new agent...",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "description": {
                                "type": "string"
                            }
                        },
                        "required": ["description"]
                    }
                }
            ]
        });
        
        let settings = Settings {
            real_base_url: "https://integrate.api.nvidia.com/v1".to_string(),
            ..Settings::default()
        };
        
        let (converted_body, _) = anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
        let converted: Value = serde_json::from_str(&converted_body).unwrap();
        
        let tools = converted.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools.len(), 1);
        let tool = &tools[0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "Agent");
        assert_eq!(tool["function"]["description"], "Launch a new agent...");
        assert_eq!(tool["function"]["parameters"]["properties"]["description"]["type"], "string");
    }

    #[test]
    fn test_anthropic_to_openai_thinking_conversion() {
        let body = json!({
            "model": "anthropic/claude-3-5-sonnet",
            "messages": [
                {
                    "role": "user",
                    "content": "Hello"
                }
            ],
            "thinking": {
                "type": "enabled",
                "budget_tokens": 1024
            }
        });
        
        let settings = Settings {
            real_base_url: "https://integrate.api.nvidia.com/v1".to_string(),
            ..Settings::default()
        };
        
        let (converted_body, _) = anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
        let converted: Value = serde_json::from_str(&converted_body).unwrap();
        
        // 驗證 thinking 欄位是否已被移除
        assert!(converted.get("thinking").is_none());
    }

    #[test]
    fn test_openai_to_anthropic_thinking_response_conversion() {
        let openai_res = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4",
            "usage": {
                "prompt_tokens": 9,
                "completion_tokens": 12,
                "total_tokens": 21
            },
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "This is the final response.",
                        "reasoning_content": "This is the reasoning process."
                    },
                    "finish_reason": "stop",
                    "index": 0
                }
            ]
        });
        
        let converted = openai_to_anthropic_response(&openai_res.to_string(), "anthropic/claude-3-5-sonnet").unwrap();
        
        let content = converted.get("content").unwrap().as_array().unwrap();
        assert_eq!(content.len(), 2);
        
        let block0 = &content[0];
        assert_eq!(block0["type"], "thinking");
        assert_eq!(block0["thinking"], "This is the reasoning process.");
        
        let block1 = &content[1];
        assert_eq!(block1["type"], "text");
        assert_eq!(block1["text"], "This is the final response.");
    }
}

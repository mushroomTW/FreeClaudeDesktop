use crate::common::local_app_data;
use crate::crypto::unprotect_secret;
use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub real_base_url: String,
    pub real_api_key: String,
    pub real_auth_scheme: String,
    pub real_model: Option<String>,
    pub real_model_routes: HashMap<String, String>,
    pub active_port: Option<u16>,
}

struct PublicConfig {
    base_url: String,
    auth_scheme: String,
    has_api_key: bool,
}

pub fn default_auth_scheme() -> String {
    "bearer".to_string()
}

pub fn parse_json_text(text: &str) -> serde_json::Result<Value> {
    let clean = text.trim_start_matches('\u{feff}');
    serde_json::from_str(clean)
}

pub fn to_public_config(settings: &Settings) -> Value {
    let has_key = unprotect_secret(&settings.real_api_key)
        .map(|key| !key.is_empty())
        .unwrap_or(!settings.real_api_key.is_empty());

    json!(PublicConfig {
        base_url: settings.real_base_url.clone(),
        auth_scheme: if settings.real_auth_scheme.is_empty() {
            default_auth_scheme()
        } else {
            settings.real_auth_scheme.clone()
        },
        has_api_key: has_key,
    })
}

impl Serialize for PublicConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("PublicConfig", 3)?;
        state.serialize_field("baseUrl", &self.base_url)?;
        state.serialize_field("authScheme", &self.auth_scheme)?;
        state.serialize_field("hasApiKey", &self.has_api_key)?;
        state.end()
    }
}

pub fn settings_file() -> PathBuf {
    local_app_data()
        .join("FreeClaudeLauncher")
        .join("launcher_settings.json")
}

fn legacy_settings_file() -> PathBuf {
    local_app_data()
        .join("Claude-3p")
        .join("launcher_settings.json")
}

fn migrate_legacy_settings() {
    let legacy = legacy_settings_file();
    let new_file = settings_file();
    if legacy.exists() && !new_file.exists() {
        if let Some(parent) = new_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::copy(&legacy, &new_file).is_ok() {
            let _ = fs::remove_file(legacy);
        }
    }
}

pub fn get_launcher_settings() -> Option<Settings> {
    migrate_legacy_settings();
    let path = settings_file();
    if !path.exists() {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    let value = parse_json_text(&text).ok()?;
    serde_json::from_value(value).ok()
}

pub fn save_launcher_settings(settings: &Settings) -> AppResult<()> {
    let path = settings_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(settings)?;
    fs::write(&path, serialized)?;

    let legacy = legacy_settings_file();
    if legacy.exists() && legacy != path {
        let _ = fs::remove_file(legacy);
    }
    Ok(())
}

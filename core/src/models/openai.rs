use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

fn default_capabilities() -> Value {
    serde_json::json!({
        "thinking": {
            "supported": false,
            "types": {
                "enabled": {
                    "supported": false
                }
            }
        }
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NormalizedModels {
    pub data: Vec<NormalizedModel>,
    pub has_more: bool,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
    pub routes: HashMap<String, String>,
    #[serde(default)]
    pub reasoning_effort_routes: HashMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NormalizedModel {
    #[serde(rename = "type")]
    pub kind: String, // "model"
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub created_at: String,
    pub provider_model_id: String,
    /// 上游模型的最大輸入 token 數；NVIDIA NIM 給 `max_input_tokens`，
    /// Anthropic 對 /v1/models discovery 期待的欄位名稱。
    /// 為 None 時 Claude Desktop 會 fallback 預設（目前為 200k）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
    /// 上游模型的最大輸出 token 數；對應 Anthropic `max_tokens`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default = "default_capabilities")]
    pub capabilities: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports1m: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InferenceModel {
    pub name: String,
    pub label_override: String,
    pub provider_model_id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    pub capabilities: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports1m: Option<bool>,
    /// 傳輸類型: "openai_chat_completions" 或 "anthropic_messages"
    #[serde(
        default = "default_transport_type",
        skip_serializing_if = "Option::is_none"
    )]
    pub transport_type: Option<String>,
}

pub fn default_transport_type() -> Option<String> {
    Some("openai_chat_completions".to_string())
}

#[derive(Deserialize)]
pub struct ProviderModelsResponse {
    #[serde(default)]
    pub data: Vec<ProviderModel>,
}

#[derive(Clone, Deserialize)]
pub struct ProviderModel {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub model_name: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub model_info: Option<Value>,
    #[serde(default)]
    pub capabilities: Option<Value>,
    #[serde(default)]
    pub pricing: Option<Pricing>,
    /// 總 context window 容量。NVIDIA NIM 不一定會回這個欄位，但部份 OpenAI
    /// 相容 provider 會以這個名稱表示視窗大小。
    #[serde(default)]
    pub context_length: Option<u64>,
    /// NVIDIA NIM 的輸入上限。Anthropic discovery 對應 `max_input_tokens`。
    #[serde(default)]
    pub max_input_tokens: Option<u64>,
    /// NVIDIA NIM 對單次輸出的硬上限。對應 Anthropic `max_tokens`。
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    /// 部份 provider 給「總上限」(context_length) 而把輸出另外拆出
    /// `max_completion_tokens` 欄位。兩者並存時兩者皆保留。
    #[serde(default)]
    pub max_completion_tokens: Option<u64>,
    /// OpenAI 格式的 unix timestamp（秒），對應 Anthropic 的 `created_at`。
    #[serde(default)]
    pub created: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Pricing {
    #[serde(default, deserialize_with = "deserialize_price")]
    pub prompt: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_price")]
    pub completion: Option<f64>,
}

fn deserialize_price<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct PriceVisitor;

    impl<'de> serde::de::Visitor<'de> for PriceVisitor {
        type Value = Option<f64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a number representing a price")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_any(self)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if value.trim().is_empty() {
                return Ok(None);
            }
            value
                .trim()
                .parse::<f64>()
                .map(Some)
                .map_err(|e| serde::de::Error::custom(format!("invalid float string: {e}")))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(value))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(value as f64))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(value as f64))
        }
    }

    deserializer.deserialize_option(PriceVisitor)
}

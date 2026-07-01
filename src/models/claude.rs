use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ClaudeContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "thinking")]
    Thinking { thinking: String },

    #[serde(rename = "image")]
    Image { source: ClaudeImageSource },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Option<ClaudeToolResultContent>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClaudeImageSource {
    #[serde(rename = "type")]
    pub source_type: String, // "base64"
    pub media_type: String,
    pub data: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ClaudeToolResultContent {
    Text(String),
    Blocks(Vec<Value>),
    Object(Value),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClaudeSystemContent {
    #[serde(rename = "type")]
    pub kind: String, // "text"
    pub text: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClaudeRole {
    User,
    Assistant,
    System,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClaudeMessage {
    pub role: ClaudeRole,
    pub content: ClaudeMessageContent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ClaudeMessageContent {
    Text(String),
    Blocks(Vec<ClaudeContentBlock>),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClaudeTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClaudeThinkingConfig {
    pub enabled: Option<bool>,
    pub budget_tokens: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClaudeMessagesRequest {
    pub model: String,
    pub max_tokens: Option<u64>,
    pub messages: Vec<ClaudeMessage>,
    pub system: Option<ClaudeSystem>,
    pub stop_sequences: Option<Vec<String>>,
    pub stream: Option<bool>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u64>,
    pub metadata: Option<Value>,
    pub tools: Option<Vec<ClaudeTool>>,
    pub tool_choice: Option<Value>,
    pub thinking: Option<ClaudeThinkingConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ClaudeSystem {
    Text(String),
    Blocks(Vec<ClaudeSystemContent>),
}

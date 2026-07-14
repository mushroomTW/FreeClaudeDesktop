use async_openai::{Client, config::OpenAIConfig};

use crate::{AppError, AppResult, Settings, unprotect_secret};

pub trait GatewayClientFactory {
    fn gateway_client(&self, settings: &Settings) -> AppResult<Client<OpenAIConfig>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AsyncOpenAiGatewayFactory;

impl GatewayClientFactory for AsyncOpenAiGatewayFactory {
    fn gateway_client(&self, settings: &Settings) -> AppResult<Client<OpenAIConfig>> {
        if settings.transport_type == "anthropic_messages" {
            return Err(AppError::InvalidConfig(
                "anthropic_messages transport 不使用 async-openai client".to_string(),
            ));
        }

        let api_base = openai_api_base(&settings.real_base_url)?;
        let api_key = unprotect_secret(&settings.real_api_key)?;
        let config = OpenAIConfig::new()
            .with_api_base(api_base)
            .with_api_key(api_key);
        Ok(Client::with_config(config))
    }
}

pub fn openai_api_base(base_url: &str) -> AppResult<String> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err(AppError::InvalidConfig("Gateway URL 不可為空".to_string()));
    }
    let parsed = url::Url::parse(base_url)
        .map_err(|_| AppError::InvalidConfig("Gateway URL 格式無效".to_string()))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(AppError::InvalidConfig(
            "Gateway URL 必須使用 HTTP 或 HTTPS".to_string(),
        ));
    }
    Ok(base_url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_openai_compatible_api_base() {
        assert_eq!(
            openai_api_base("https://gateway.example/v1/").unwrap(),
            "https://gateway.example/v1"
        );
        assert!(openai_api_base("file:///tmp/gateway").is_err());
    }
}

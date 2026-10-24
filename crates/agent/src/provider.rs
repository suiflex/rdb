use std::time::Duration;

use serde_json::{json, Value};

use crate::{AgentConfig, Message, Provider};

pub struct ModelRequest<'a> {
    pub config: &'a AgentConfig,
    pub api_key: &'a str,
    pub system: &'a str,
    pub messages: &'a [Message],
}

pub struct ModelResponse {
    pub text: String,
}

pub async fn complete(request: &ModelRequest<'_>) -> Result<ModelResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|error| error.to_string())?;
    let (url, body) = match request.config.provider {
        Provider::OpenAi
        | Provider::DeepSeek
        | Provider::Qwen
        | Provider::MiniMax
        | Provider::OpenAiCompatible
        | Provider::Ollama => {
            let base = request.config.endpoint.trim_end_matches('/');
            let url = format!("{base}/chat/completions");
            let mut messages = vec![json!({"role": "system", "content": request.system})];
            messages.extend(
                request
                    .messages
                    .iter()
                    .map(|message| json!({"role": message.role, "content": message.content})),
            );
            let body = json!({"model": request.config.model, "messages": messages});
            (url, body)
        }
        Provider::Anthropic | Provider::AnthropicCompatible => {
            let base = request.config.endpoint.trim_end_matches('/');
            let url = format!("{base}/messages");
            let messages = request
                .messages
                .iter()
                .map(|message| json!({"role": message.role, "content": message.content}))
                .collect::<Vec<_>>();
            let body = json!({"model": request.config.model, "system": request.system,
                "messages": messages, "max_tokens": 4096});
            (url, body)
        }
        Provider::Gemini => {
            let base = request.config.endpoint.trim_end_matches('/');
            let url = format!("{base}/models/{}:generateContent", request.config.model);
            let contents = request
                .messages
                .iter()
                .map(|message| {
                    json!({"role": if message.role == "assistant" {"model"} else {"user"},
                    "parts": [{"text": message.content}]})
                })
                .collect::<Vec<_>>();
            let body = json!({"systemInstruction": {"parts": [{"text": request.system}]},
                "contents": contents});
            (url, body)
        }
    };
    let mut http = client.post(url).json(&body);
    if !request.api_key.is_empty() {
        http = match request.config.provider {
            Provider::Anthropic | Provider::AnthropicCompatible => http
                .header("x-api-key", request.api_key)
                .header("anthropic-version", "2023-06-01"),
            Provider::Gemini => http.header("x-goog-api-key", request.api_key),
            _ => http.bearer_auth(request.api_key),
        };
    }
    let response = http.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        let message = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Model request failed");
        return Err(format!("{status}: {message}"));
    }
    let text = match request.config.provider {
        Provider::Anthropic | Provider::AnthropicCompatible => value.pointer("/content/0/text"),
        Provider::Gemini => value.pointer("/candidates/0/content/parts/0/text"),
        _ => value.pointer("/choices/0/message/content"),
    }
    .and_then(Value::as_str)
    .unwrap_or_default()
    .to_owned();
    Ok(ModelResponse { text })
}

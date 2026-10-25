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
    complete_with_retries(request, 0).await
}

pub(crate) async fn complete_with_retries(
    request: &ModelRequest<'_>,
    max_retries: u8,
) -> Result<ModelResponse, String> {
    let mut attempt = 0;
    loop {
        match complete_once(request).await {
            Ok(response) => return Ok(response),
            Err(error) if error.retryable && attempt < max_retries => {
                let delay = Duration::from_millis(500 * (1_u64 << attempt.min(5)));
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(error) => return Err(error.message),
        }
    }
}

struct ModelFailure {
    message: String,
    retryable: bool,
}

async fn complete_once(request: &ModelRequest<'_>) -> Result<ModelResponse, ModelFailure> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|error| ModelFailure {
            message: error.to_string(),
            retryable: false,
        })?;
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
    let response = http.send().await.map_err(|error| ModelFailure {
        retryable: error.is_timeout() || error.is_connect(),
        message: error.to_string(),
    })?;
    let status = response.status();
    let body = response.text().await.map_err(|error| ModelFailure {
        message: error.to_string(),
        retryable: false,
    })?;
    let value: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    if !status.is_success() {
        let message = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Model request failed");
        return Err(ModelFailure {
            message: format!("{status}: {message}"),
            retryable: retryable_status(status),
        });
    }
    if value.is_null() {
        return Err(ModelFailure {
            message: "Model returned an invalid response".into(),
            retryable: false,
        });
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

fn retryable_status(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 429 || status.is_server_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    use crate::{AgentConfig, Message};

    #[test]
    fn only_transient_http_statuses_retry() {
        assert!(retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(retryable_status(reqwest::StatusCode::SERVICE_UNAVAILABLE));
        assert!(!retryable_status(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!retryable_status(reqwest::StatusCode::BAD_REQUEST));
    }

    #[tokio::test]
    async fn retries_transient_response_then_returns_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (index, stream) in listener.incoming().take(2).enumerate() {
                let mut stream = stream.unwrap();
                let mut request = [0_u8; 4096];
                assert!(stream.read(&mut request).unwrap() > 0);
                let (status, body) = if index == 0 {
                    ("503 Service Unavailable", r#"{"error":{"message":"busy"}}"#)
                } else {
                    ("200 OK", r#"{"choices":[{"message":{"content":"ready"}}]}"#)
                };
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let config = AgentConfig {
            id: "test".into(),
            name: "test".into(),
            provider: Provider::OpenAiCompatible,
            endpoint,
            model: "test".into(),
        };
        let messages = [Message {
            role: "user".into(),
            content: "hello".into(),
        }];
        let request = ModelRequest {
            config: &config,
            api_key: "",
            system: "test",
            messages: &messages,
        };
        let answer =
            tokio::time::timeout(Duration::from_secs(5), complete_with_retries(&request, 1))
                .await
                .unwrap()
                .unwrap();
        server.join().unwrap();
        assert_eq!(answer.text, "ready");
    }
}

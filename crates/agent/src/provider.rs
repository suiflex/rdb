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

pub async fn discover_models(config: &AgentConfig, api_key: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let url = discovery_url(&config.endpoint)?;
    let mut request = client.get(url);
    if !api_key.is_empty() {
        request = match config.provider {
            Provider::Anthropic | Provider::AnthropicCompatible => request
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01"),
            Provider::Gemini => request.header("x-goog-api-key", api_key),
            _ => request.bearer_auth(api_key),
        };
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Model discovery failed: {status}"));
    }
    let body = response.bytes().await.map_err(|error| error.to_string())?;
    if body.len() > 2_000_000 {
        return Err("Model discovery response is too large".into());
    }
    let value: Value =
        serde_json::from_slice(&body).map_err(|_| "Model discovery returned invalid JSON")?;
    let models = parse_model_list(config.provider, &value);
    if models.is_empty() {
        return Err("Provider returned no usable models; enter a model manually".into());
    }
    Ok(models)
}

fn discovery_url(endpoint: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(&format!("{}/models", endpoint.trim_end_matches('/')))
        .map_err(|_| "Enter a valid HTTP or HTTPS endpoint")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Model endpoint must be an HTTP or HTTPS URL without embedded credentials, query, or fragment".into());
    }
    Ok(url)
}

fn parse_model_list(provider: Provider, value: &Value) -> Vec<String> {
    let entries = if provider == Provider::Gemini {
        value.get("models")
    } else {
        value.get("data").or_else(|| value.get("models"))
    };
    let mut names = entries
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| {
            provider != Provider::Gemini
                || item
                    .get("supportedGenerationMethods")
                    .and_then(Value::as_array)
                    .is_none_or(|methods| {
                        methods
                            .iter()
                            .any(|method| method.as_str() == Some("generateContent"))
                    })
        })
        .filter_map(|item| {
            item.get("id")
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
        })
        .map(|name| name.strip_prefix("models/").unwrap_or(name).trim())
        .filter(|name| !name.is_empty() && name.chars().count() <= 200)
        .map(str::to_string)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names.truncate(200);
    names
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

    #[test]
    fn discovers_openai_and_anthropic_model_lists() {
        let list = json!({"data": [{"id": "model-b"}, {"id": "model-a"}, {"id": "model-a"}]});
        assert_eq!(
            parse_model_list(Provider::OpenAi, &list),
            ["model-a", "model-b"]
        );
        assert_eq!(
            parse_model_list(Provider::Anthropic, &list),
            ["model-a", "model-b"]
        );
    }

    #[test]
    fn gemini_discovery_omits_non_chat_models() {
        let list = json!({"models": [
            {"name": "models/gemini-a", "supportedGenerationMethods": ["generateContent"]},
            {"name": "models/embedding-a", "supportedGenerationMethods": ["embedContent"]}
        ]});
        assert_eq!(parse_model_list(Provider::Gemini, &list), ["gemini-a"]);
    }

    #[test]
    fn discovery_rejects_non_http_or_embedded_credentials() {
        assert!(discovery_url("file:///tmp/models").is_err());
        assert!(discovery_url("https://user:secret@example.test/v1").is_err());
        assert!(discovery_url("https://example.test/v1?token=secret").is_err());
        assert_eq!(
            discovery_url("https://example.test/v1").unwrap().as_str(),
            "https://example.test/v1/models"
        );
    }

    #[tokio::test]
    async fn discovery_reports_provider_failure_without_echoing_key() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let n = stream.read(&mut request).unwrap();
            let raw = String::from_utf8_lossy(&request[..n]);
            assert!(raw.starts_with("GET /v1/models HTTP/1.1"));
            stream
                .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        let config = AgentConfig {
            id: "test".into(),
            name: "test".into(),
            provider: Provider::OpenAi,
            endpoint,
            model: String::new(),
        };
        let error = discover_models(&config, "secret-model-key")
            .await
            .unwrap_err();
        assert!(error.contains("401"));
        assert!(!error.contains("secret-model-key"));
        server.join().unwrap();
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

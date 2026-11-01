use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    OpenAi,
    Anthropic,
    Gemini,
    DeepSeek,
    Qwen,
    MiniMax,
    OpenAiCompatible,
    AnthropicCompatible,
    Ollama,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub provider: Provider,
    pub endpoint: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentPreferences {
    pub global_instructions: String,
    pub max_turns: u16,
    pub max_api_retries: u8,
    /// Send a few rows of the latest result grid as context. Off by default:
    /// the rows leave the machine for the model provider.
    pub share_result_preview: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub id: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub default_engines: Vec<String>,
}

impl Default for AgentPreferences {
    fn default() -> Self {
        Self {
            global_instructions: String::new(),
            max_turns: 30,
            max_api_retries: 2,
            share_result_preview: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Ask,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    /// One line per tool the answer used, for the panel. Never sent to the
    /// model: providers get `role` and `content` only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
}

impl Message {
    pub fn user(content: String) -> Self {
        Self {
            role: "user".into(),
            content,
            steps: Vec::new(),
        }
    }

    pub fn assistant(content: String) -> Self {
        Self {
            role: "assistant".into(),
            content,
            steps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub connection_id: String,
    pub database: String,
    pub schema: Option<String>,
    pub mode: Mode,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub selected_templates: Vec<String>,
}

impl Conversation {
    pub fn new(
        id: String,
        connection_id: String,
        database: String,
        schema: Option<String>,
    ) -> Self {
        Self {
            id,
            title: "New conversation".into(),
            connection_id,
            database,
            schema,
            mode: Mode::Ask,
            messages: Vec::new(),
            selected_templates: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_conversation_without_templates_remains_readable() {
        let old = serde_json::json!({
            "id": "one",
            "title": "Before templates",
            "connection_id": "connection",
            "database": "db",
            "schema": null,
            "mode": "ask",
            "messages": []
        });
        let restored: Conversation = serde_json::from_value(old).unwrap();
        assert!(restored.selected_templates.is_empty());
    }
}

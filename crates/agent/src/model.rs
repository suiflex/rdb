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
}

impl Default for AgentPreferences {
    fn default() -> Self {
        Self {
            global_instructions: String::new(),
            max_turns: 30,
            max_api_retries: 2,
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
        }
    }
}

use std::{fs, path::PathBuf};

use directories::ProjectDirs;
use rdb_connstore::secret::{select_backend, SecretBackend};
use serde::{Deserialize, Serialize};

use crate::{AgentConfig, AgentPreferences, Conversation};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredData {
    configs: Vec<AgentConfig>,
    conversations: Vec<Conversation>,
    preferences: AgentPreferences,
}

pub struct AgentStore {
    path: PathBuf,
    data: StoredData,
    secrets: Box<dyn SecretBackend>,
}

impl AgentStore {
    pub fn open_default() -> Result<Self, String> {
        let dir = match std::env::var("RDB_STORE_DIR") {
            Ok(dir) => PathBuf::from(dir),
            Err(_) if std::env::var("RDB_MOCK").is_ok_and(|value| value == "1") => {
                std::env::temp_dir().join(format!("rdb-mock-{}", std::process::id()))
            }
            Err(_) => ProjectDirs::from("dev", "dbm", "dbm")
                .ok_or("No application config directory")?
                .config_dir()
                .to_owned(),
        };
        Self::open(dir)
    }

    pub fn open(dir: PathBuf) -> Result<Self, String> {
        let path = dir.join("agent.json");
        let data = if path.exists() {
            serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?
        } else {
            StoredData::default()
        };
        let secrets = select_backend(&dir).map_err(|error| error.to_string())?;
        Ok(Self {
            path,
            data,
            secrets,
        })
    }

    pub fn configs(&self) -> &[AgentConfig] {
        &self.data.configs
    }
    pub fn conversations(&self) -> &[Conversation] {
        &self.data.conversations
    }

    pub fn preferences(&self) -> &AgentPreferences {
        &self.data.preferences
    }

    pub fn save_preferences(&mut self, preferences: AgentPreferences) -> Result<(), String> {
        if !(5..=500).contains(&preferences.max_turns) || preferences.max_api_retries > 10 {
            return Err("Agent limits are out of range".into());
        }
        if preferences.global_instructions.chars().count() > 12_000 {
            return Err("Global instructions exceed 12,000 characters".into());
        }
        let old_data = self.data.clone();
        self.data.preferences = preferences;
        if let Err(error) = self.flush() {
            self.data = old_data;
            return Err(error);
        }
        Ok(())
    }

    pub fn api_key(&self, config_id: &str) -> Result<Option<String>, String> {
        self.secrets
            .get(&format!("agent:{config_id}"))
            .map_err(|error| error.to_string())
    }

    pub fn save_config(
        &mut self,
        config: AgentConfig,
        api_key: Option<&str>,
    ) -> Result<(), String> {
        let key_id = format!("agent:{}", config.id);
        let old_data = self.data.clone();
        let mut old_key = None;
        if let Some(key) = api_key.filter(|key| !key.is_empty()) {
            let previous = self
                .secrets
                .get(&key_id)
                .map_err(|error| error.to_string())?;
            self.secrets
                .set(&key_id, key)
                .map_err(|error| error.to_string())?;
            let roundtrip = self
                .secrets
                .get(&key_id)
                .map_err(|error| error.to_string())?;
            if roundtrip.as_deref() != Some(key) {
                match previous {
                    Some(old) => self.secrets.set(&key_id, &old),
                    None => self.secrets.delete(&key_id),
                }
                .map_err(|error| error.to_string())?;
                return Err("API key could not be stored".into());
            }
            old_key = Some(previous);
        }
        if let Some(existing) = self
            .data
            .configs
            .iter_mut()
            .find(|item| item.id == config.id)
        {
            *existing = config;
        } else {
            self.data.configs.push(config);
        }
        if let Err(error) = self.flush() {
            self.data = old_data;
            if let Some(previous) = old_key {
                match previous {
                    Some(key) => self.secrets.set(&key_id, &key),
                    None => self.secrets.delete(&key_id),
                }
                .map_err(|rollback| format!("{error}; key rollback failed: {rollback}"))?;
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn delete_config(&mut self, id: &str) -> Result<(), String> {
        let old_data = self.data.clone();
        self.data.configs.retain(|item| item.id != id);
        if let Err(error) = self.flush() {
            self.data = old_data;
            return Err(error);
        }
        self.secrets
            .delete(&format!("agent:{id}"))
            .map_err(|error| error.to_string())
    }

    pub fn save_conversation(&mut self, conversation: Conversation) -> Result<(), String> {
        let old_data = self.data.clone();
        if let Some(existing) = self
            .data
            .conversations
            .iter_mut()
            .find(|item| item.id == conversation.id)
        {
            *existing = conversation;
        } else {
            self.data.conversations.push(conversation);
        }
        if let Err(error) = self.flush() {
            self.data = old_data;
            return Err(error);
        }
        Ok(())
    }

    pub fn delete_conversation(&mut self, id: &str) -> Result<(), String> {
        let old_data = self.data.clone();
        self.data.conversations.retain(|item| item.id != id);
        if let Err(error) = self.flush() {
            self.data = old_data;
            return Err(error);
        }
        Ok(())
    }

    fn flush(&self) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or("Agent store has no parent directory")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec_pretty(&self.data).map_err(|error| error.to_string())?;
        let temp = self.path.with_extension("json.tmp");
        fs::write(&temp, bytes).map_err(|error| error.to_string())?;
        fs::rename(temp, &self.path).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Provider;

    #[test]
    fn stores_api_key_outside_agent_json() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = AgentStore::open(dir.path().to_owned()).unwrap();
        store
            .save_config(
                AgentConfig {
                    id: "one".into(),
                    name: "Local".into(),
                    provider: Provider::OpenAi,
                    endpoint: "https://example.invalid/v1".into(),
                    model: "example".into(),
                },
                Some("secret-value"),
            )
            .unwrap();
        let json = fs::read_to_string(dir.path().join("agent.json")).unwrap();
        assert!(!json.contains("secret-value"));
        assert_eq!(
            store.api_key("one").unwrap().as_deref(),
            Some("secret-value")
        );
    }

    #[test]
    fn preferences_roundtrip_and_reject_out_of_range_limits() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = AgentStore::open(dir.path().to_owned()).unwrap();
        assert_eq!(store.preferences(), &AgentPreferences::default());
        let preferences = AgentPreferences {
            global_instructions: "Use UTC dates".into(),
            max_turns: 45,
            max_api_retries: 3,
        };
        store.save_preferences(preferences.clone()).unwrap();
        assert_eq!(
            AgentStore::open(dir.path().to_owned())
                .unwrap()
                .preferences(),
            &preferences
        );
        let invalid = AgentPreferences {
            max_turns: 501,
            ..preferences.clone()
        };
        assert!(store.save_preferences(invalid).is_err());
        assert_eq!(store.preferences(), &preferences);
    }
}

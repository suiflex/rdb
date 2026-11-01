use std::{fs, path::PathBuf};

use directories::ProjectDirs;
use rdb_connstore::secret::{select_backend, SecretBackend};
use serde::{Deserialize, Serialize};

use crate::{AgentConfig, AgentPreferences, Conversation, PromptTemplate};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredData {
    configs: Vec<AgentConfig>,
    conversations: Vec<Conversation>,
    preferences: AgentPreferences,
    templates: Vec<PromptTemplate>,
}

pub struct AgentStore {
    path: PathBuf,
    data: StoredData,
    secrets: Box<dyn SecretBackend>,
    notice: Option<String>,
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
        let mut notice = None;
        let data = if path.exists() {
            let bytes = fs::read(&path).map_err(|error| error.to_string())?;
            match serde_json::from_slice(&bytes) {
                Ok(data) => data,
                // A file that no longer parses used to disable the whole
                // panel. Set it aside, untouched, and start empty.
                Err(error) => {
                    let aside = path.with_extension(format!(
                        "json.corrupt-{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |elapsed| elapsed.as_secs())
                    ));
                    fs::rename(&path, &aside).map_err(|rename| {
                        format!("{error}; could not set the file aside: {rename}")
                    })?;
                    notice = Some(format!(
                        "Assistant data could not be read ({error}) and was moved to {}",
                        aside.display()
                    ));
                    StoredData::default()
                }
            }
        } else {
            StoredData::default()
        };
        let secrets = select_backend(&dir).map_err(|error| error.to_string())?;
        Ok(Self {
            path,
            data,
            secrets,
            notice,
        })
    }

    /// A one-time message about recovery done while opening the store.
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
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

    pub fn templates(&self) -> &[PromptTemplate] {
        &self.data.templates
    }

    pub fn save_template(&mut self, template: PromptTemplate) -> Result<(), String> {
        if template.name.trim().is_empty() || template.name.chars().count() > 100 {
            return Err("Template name must be 1 to 100 characters".into());
        }
        if template.content.trim().is_empty() || template.content.chars().count() > 12_000 {
            return Err("Template content must be 1 to 12,000 characters".into());
        }
        let old_data = self.data.clone();
        if let Some(existing) = self
            .data
            .templates
            .iter_mut()
            .find(|item| item.id == template.id)
        {
            *existing = template;
        } else {
            self.data.templates.push(template);
        }
        if let Err(error) = self.flush() {
            self.data = old_data;
            return Err(error);
        }
        Ok(())
    }

    pub fn delete_template(&mut self, id: &str) -> Result<(), String> {
        let old_data = self.data.clone();
        self.data.templates.retain(|item| item.id != id);
        for conversation in &mut self.data.conversations {
            conversation
                .selected_templates
                .retain(|selected| selected != id);
        }
        if let Err(error) = self.flush() {
            self.data = old_data;
            return Err(error);
        }
        Ok(())
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
        crate::provider::discovery_url(&config.endpoint)?;
        let key_id = format!("agent:{}", config.id);
        // A saved key belongs to the endpoint it was entered for. Keeping it
        // after the endpoint or provider changes would send it to a new host
        // nobody typed it in for.
        let moved = self.data.configs.iter().any(|item| {
            item.id == config.id
                && (item.provider != config.provider
                    || item.endpoint.trim_end_matches('/') != config.endpoint.trim_end_matches('/'))
        });
        if moved && api_key.is_none_or(str::is_empty) && self.api_key(&config.id)?.is_some() {
            return Err("Enter the API key again for the new endpoint".into());
        }
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
        // Conversations hold query text and results; keep them owner-only,
        // like the connection store's secret file.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))
                .map_err(|error| error.to_string())?;
        }
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
            share_result_preview: true,
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

    #[test]
    fn templates_roundtrip_and_deletion_clears_conversation_selection() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = AgentStore::open(dir.path().to_owned()).unwrap();
        let template = PromptTemplate {
            id: "template-one".into(),
            name: "Query style".into(),
            content: "Use explicit joins".into(),
            default_engines: vec!["postgres".into()],
        };
        store.save_template(template.clone()).unwrap();
        let mut conversation = Conversation::new(
            "conversation-one".into(),
            "connection-one".into(),
            "db".into(),
            None,
        );
        conversation.selected_templates.push(template.id.clone());
        store.save_conversation(conversation).unwrap();
        let reopened = AgentStore::open(dir.path().to_owned()).unwrap();
        assert_eq!(reopened.templates(), &[template]);
        store.delete_template("template-one").unwrap();
        assert!(store.conversations()[0].selected_templates.is_empty());
    }
    fn sample_config(endpoint: &str) -> AgentConfig {
        AgentConfig {
            id: "one".into(),
            name: "Sample".into(),
            provider: Provider::OpenAi,
            endpoint: endpoint.into(),
            model: "example".into(),
        }
    }

    #[test]
    fn corrupt_store_is_set_aside_and_opens_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("agent.json"), b"{ not json").unwrap();
        let mut store = AgentStore::open(dir.path().to_owned()).unwrap();
        assert!(store.conversations().is_empty());
        let notice = store.take_notice().unwrap();
        assert!(notice.contains("agent.json.corrupt-"), "{notice}");
        assert!(store.take_notice().is_none());
        let aside = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
            .unwrap();
        assert_eq!(fs::read(aside.path()).unwrap(), b"{ not json");
    }

    #[test]
    fn moving_a_config_needs_its_key_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = AgentStore::open(dir.path().to_owned()).unwrap();
        store
            .save_config(
                sample_config("https://example.invalid/v1"),
                Some("secret-value"),
            )
            .unwrap();
        // Same endpoint, empty key field: the stored key stays.
        store
            .save_config(sample_config("https://example.invalid/v1/"), Some(""))
            .unwrap();
        assert!(store
            .save_config(sample_config("https://other.invalid/v1"), Some(""))
            .is_err());
        assert_eq!(store.configs()[0].endpoint, "https://example.invalid/v1/");
        store
            .save_config(
                sample_config("https://other.invalid/v1"),
                Some("new-secret"),
            )
            .unwrap();
        assert_eq!(store.api_key("one").unwrap().as_deref(), Some("new-secret"));
    }

    #[test]
    fn endpoints_with_credentials_or_queries_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = AgentStore::open(dir.path().to_owned()).unwrap();
        for endpoint in [
            "ftp://example.invalid/v1",
            "https://user:pass@example.invalid/v1",
            "https://example.invalid/v1?key=abc",
            "not a url",
        ] {
            assert!(
                store.save_config(sample_config(endpoint), None).is_err(),
                "{endpoint}"
            );
        }
        assert!(store.configs().is_empty());
    }

    #[test]
    fn older_preferences_load_without_sharing_results() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("agent.json"),
            br#"{"preferences":{"global_instructions":"","max_turns":30,"max_api_retries":2}}"#,
        )
        .unwrap();
        let store = AgentStore::open(dir.path().to_owned()).unwrap();
        assert!(!store.preferences().share_result_preview);
    }

    #[cfg(unix)]
    #[test]
    fn store_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mut store = AgentStore::open(dir.path().to_owned()).unwrap();
        store.save_preferences(AgentPreferences::default()).unwrap();
        let mode = fs::metadata(dir.path().join("agent.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

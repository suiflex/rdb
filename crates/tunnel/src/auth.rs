use rdb_core::conn::{SshAuthMode, SshTunnelConfig};
use rdb_core::error::{RdbError, Result};
use russh::client::Handle;
use russh_keys::agent::client::AgentClient;
use russh_keys::decode_secret_key;
use std::path::PathBuf;
use std::sync::Arc;

use crate::forwarder::ClientHandler;

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(path)
}

pub async fn authenticate(
    session: &mut Handle<ClientHandler>,
    cfg: &SshTunnelConfig,
) -> Result<()> {
    match cfg.auth_mode {
        SshAuthMode::Password => {
            let pw = cfg.password.as_deref().unwrap_or("");
            let auth_res = session
                .authenticate_password(&cfg.user, pw)
                .await
                .map_err(|e| RdbError::Connection(format!("SSH password auth failed: {e}")))?;
            if !auth_res {
                return Err(RdbError::Connection(
                    "SSH password authentication rejected by server".into(),
                ));
            }
            Ok(())
        }
        SshAuthMode::KeyFile => {
            let key_path_str = cfg.key_path.as_deref().unwrap_or("~/.ssh/id_rsa");
            let key_path = expand_tilde(key_path_str);
            if !key_path.exists() {
                return Err(RdbError::Connection(format!(
                    "SSH private key file not found: {}",
                    key_path.display()
                )));
            }
            let key_content = std::fs::read_to_string(&key_path)
                .map_err(|e| RdbError::Connection(format!("Failed to read SSH key file: {e}")))?;
            let passphrase = cfg.passphrase.as_deref();
            let key_pair = decode_secret_key(&key_content, passphrase).map_err(|e| {
                RdbError::Connection(format!("Failed to parse SSH private key: {e}"))
            })?;

            let auth_res = session
                .authenticate_publickey(&cfg.user, Arc::new(key_pair))
                .await
                .map_err(|e| RdbError::Connection(format!("SSH key auth failed: {e}")))?;
            if !auth_res {
                return Err(RdbError::Connection(
                    "SSH public key authentication rejected by server".into(),
                ));
            }
            Ok(())
        }
        SshAuthMode::Agent => {
            let mut agent = match AgentClient::connect_env().await {
                Ok(a) => a,
                Err(e) => {
                    return Err(RdbError::Connection(format!(
                        "Could not connect to SSH agent ($SSH_AUTH_SOCK): {e}"
                    )));
                }
            };
            let identities = agent.request_identities().await.map_err(|e| {
                RdbError::Connection(format!("SSH agent request_identities error: {e}"))
            })?;

            if identities.is_empty() {
                return Err(RdbError::Connection(
                    "SSH agent has no keys loaded (run `ssh-add` to add keys)".into(),
                ));
            }

            let mut authenticated = false;
            for pubkey in identities {
                let (agent_back, auth_res) =
                    session.authenticate_future(&cfg.user, pubkey, agent).await;
                agent = agent_back;
                if let Ok(true) = auth_res {
                    authenticated = true;
                    break;
                }
            }

            if !authenticated {
                return Err(RdbError::Connection(
                    "None of the keys in the SSH agent were accepted by the server".into(),
                ));
            }
            Ok(())
        }
    }
}

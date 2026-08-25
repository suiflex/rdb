use rdb_core::conn::{SshAuthMode, SshTunnelConfig};
use rdb_core::error::{RdbError, Result};
use russh::client::Handle;
use russh::keys::agent::client::AgentClient;
use russh::keys::agent::AgentIdentity;
use russh::keys::{decode_secret_key, PrivateKeyWithHashAlg};
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
            if !auth_res.success() {
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

            // RSA keys have to be signed with the hash the server actually
            // accepts (ssh-rsa vs rsa-sha2-256/512); modern servers reject the
            // SHA-1 default. Ignored for every other algorithm.
            let hash_alg = session.best_supported_rsa_hash().await.ok().flatten();
            let key = PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash_alg.flatten());

            let auth_res = session
                .authenticate_publickey(&cfg.user, key)
                .await
                .map_err(|e| RdbError::Connection(format!("SSH key auth failed: {e}")))?;
            if !auth_res.success() {
                return Err(RdbError::Connection(
                    "SSH public key authentication rejected by server".into(),
                ));
            }
            Ok(())
        }
        SshAuthMode::Agent => {
            // Agents speak the same protocol over a different transport per
            // platform: a Unix socket named by $SSH_AUTH_SOCK, or on Windows
            // the OpenSSH named pipe with Pageant as the fallback. `dynamic()`
            // boxes the stream so the identity loop below stays one copy.
            #[cfg(unix)]
            let mut agent = match AgentClient::connect_env().await {
                Ok(a) => a.dynamic(),
                Err(e) => {
                    return Err(RdbError::Connection(format!(
                        "Could not connect to SSH agent ($SSH_AUTH_SOCK): {e}"
                    )));
                }
            };
            #[cfg(windows)]
            let mut agent = {
                // Where Windows' bundled OpenSSH agent listens.
                const OPENSSH_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
                match AgentClient::connect_named_pipe(OPENSSH_PIPE).await {
                    Ok(a) => a.dynamic(),
                    Err(pipe_err) => match AgentClient::connect_pageant().await {
                        Ok(a) => a.dynamic(),
                        Err(pageant_err) => {
                            return Err(RdbError::Connection(format!(
                                "Could not connect to an SSH agent (OpenSSH pipe: \
                                 {pipe_err}; Pageant: {pageant_err})"
                            )));
                        }
                    },
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

            let hash_alg = session.best_supported_rsa_hash().await.ok().flatten();

            let mut authenticated = false;
            for identity in identities {
                let AgentIdentity::PublicKey { key, .. } = identity else {
                    continue;
                };
                let auth_res = session
                    .authenticate_publickey_with(&cfg.user, key, hash_alg.flatten(), &mut agent)
                    .await;
                if matches!(auth_res, Ok(ref r) if r.success()) {
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

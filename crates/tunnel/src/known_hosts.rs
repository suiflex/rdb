use rdb_core::error::{RdbError, Result};
use russh::keys::key::PublicKey;
use std::path::PathBuf;

pub fn default_known_hosts_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts"))
}

pub fn check_known_host(host: &str, port: u16, pubkey: &PublicKey) -> Result<bool> {
    let path = match default_known_hosts_path() {
        Some(p) => p,
        None => return Ok(true), // No home dir, accept key
    };
    check_known_hosts_at_path(host, port, pubkey, &path)
}

pub fn check_known_hosts_at_path(
    host: &str,
    port: u16,
    pubkey: &PublicKey,
    path: &std::path::Path,
) -> Result<bool> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = russh_keys::learn_known_hosts_path(host, port, pubkey, path);
        return Ok(true);
    }

    match russh_keys::check_known_hosts_path(host, port, pubkey, path) {
        Ok(true) => Ok(true),
        Ok(false) => {
            // Host not found in known_hosts: TOFU (Trust-On-First-Use) -> learn key and accept
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = russh_keys::learn_known_hosts_path(host, port, pubkey, path);
            log::info!(
                "Learned new SSH host key for {}:{} in {:?}",
                host,
                port,
                path
            );
            Ok(true)
        }
        Err(russh_keys::Error::KeyChanged { line }) => {
            // Key mismatch against existing known host entry (possible MITM attack)
            Err(RdbError::Connection(format!(
                "Host key mismatch for {}:{}. The server key does not match the known_hosts entry on line {}.",
                host, port, line
            )))
        }
        Err(e) => {
            // Other error reading known_hosts (e.g. unparseable line or hashed format):
            // Attempt learning and proceed with TOFU.
            log::warn!(
                "Could not check known_hosts for {}:{}: {}. Proceeding with TOFU.",
                host,
                port,
                e
            );
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = russh_keys::learn_known_hosts_path(host, port, pubkey, path);
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh_keys::key::KeyPair;

    #[test]
    fn tofu_learns_new_key_and_verifies() {
        let temp_dir = std::env::temp_dir().join(format!("rdb_kh_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let kh_file = temp_dir.join("known_hosts");

        let key1 = KeyPair::generate_ed25519().unwrap();
        let pubkey1 = key1.clone_public_key().unwrap();

        // 1. Initial check (file absent) -> learns key
        let res1 = check_known_hosts_at_path("bastion.example.com", 22, &pubkey1, &kh_file);
        assert!(res1.is_ok());
        assert!(kh_file.exists());

        // 2. Same key -> passes verification
        let res2 = check_known_hosts_at_path("bastion.example.com", 22, &pubkey1, &kh_file);
        assert!(res2.is_ok());

        // 3. Different key for same host/port -> fails with mismatch (anti-MITM)
        let key2 = KeyPair::generate_ed25519().unwrap();
        let pubkey2 = key2.clone_public_key().unwrap();
        let res3 = check_known_hosts_at_path("bastion.example.com", 22, &pubkey2, &kh_file);
        assert!(res3.is_err());
        assert!(res3.unwrap_err().to_string().contains("Host key mismatch"));

        // 4. Another new host when file already exists -> learns key (TOFU)
        let key3 = KeyPair::generate_ed25519().unwrap();
        let pubkey3 = key3.clone_public_key().unwrap();
        let res4 = check_known_hosts_at_path("other-bastion.example.com", 22, &pubkey3, &kh_file);
        assert!(res4.is_ok());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}

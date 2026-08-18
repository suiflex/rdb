use async_trait::async_trait;
use russh::client::{self, Handle, Handler};
use russh::keys::key::PublicKey;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use rdb_core::conn::SshTunnelConfig;
use rdb_core::error::{RdbError, Result};

use crate::auth::authenticate;
use crate::known_hosts::check_known_host;

#[derive(Clone)]
pub struct ClientHandler {
    host: String,
    port: u16,
    key_error: Arc<std::sync::Mutex<Option<String>>>,
}

impl ClientHandler {
    pub fn new(host: String, port: u16, key_error: Arc<std::sync::Mutex<Option<String>>>) -> Self {
        Self {
            host,
            port,
            key_error,
        }
    }
}

#[async_trait]
impl Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        match check_known_host(&self.host, self.port, server_public_key) {
            Ok(valid) => Ok(valid),
            Err(e) => {
                log::error!("SSH host key rejected: {}", e);
                if let Ok(mut g) = self.key_error.lock() {
                    *g = Some(e.to_string());
                }
                Ok(false)
            }
        }
    }
}

/// Handle to an active background SSH tunnel.
pub struct TunnelHandle {
    local_port: u16,
    shutdown_tx: Option<oneshot::Sender<()>>,
    forwarder_task: Option<JoinHandle<()>>,
}

impl TunnelHandle {
    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.forwarder_task.take() {
            task.abort();
        }
    }
}

impl Drop for TunnelHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.forwarder_task.take() {
            task.abort();
        }
    }
}

pub struct SshTunnel;

impl SshTunnel {
    /// Test the SSH connection and target host reachability without establishing a persistent tunnel.
    pub async fn test_connection(
        cfg: &SshTunnelConfig,
        target_host: &str,
        target_port: u16,
    ) -> Result<()> {
        let session = Self::connect_and_auth(cfg).await?;
        let _channel = session
            .channel_open_direct_tcpip(target_host, target_port as u32, "127.0.0.1", 0)
            .await
            .map_err(|e| {
                RdbError::Connection(format!(
                    "SSH bastion connected, but failed to reach target DB {}:{}: {}",
                    target_host, target_port, e
                ))
            })?;
        Ok(())
    }

    /// Establish the SSH connection and start forwarding an ephemeral local TCP port to target_host:target_port.
    pub async fn open(
        cfg: &SshTunnelConfig,
        target_host: &str,
        target_port: u16,
    ) -> Result<TunnelHandle> {
        let session = Self::connect_and_auth(cfg).await?;

        // Bind ephemeral local port on loopback
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| {
            RdbError::Connection(format!("Failed to bind local loopback listener: {e}"))
        })?;

        let local_addr: SocketAddr = listener
            .local_addr()
            .map_err(|e| RdbError::Connection(format!("Failed to get local port: {e}")))?;
        let local_port = local_addr.port();

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let target_host_owned = target_host.to_string();

        let forwarder_task = tokio::spawn(async move {
            let session = Arc::new(session);

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    accept_res = listener.accept() => {
                        match accept_res {
                            Ok((local_stream, _client_addr)) => {
                                let session = Arc::clone(&session);
                                let target_host = target_host_owned.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_stream(session, local_stream, &target_host, target_port, local_port).await {
                                        log::warn!("SSH direct-tcpip forward error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                log::warn!("Listener accept error: {}", e);
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(TunnelHandle {
            local_port,
            shutdown_tx: Some(shutdown_tx),
            forwarder_task: Some(forwarder_task),
        })
    }

    async fn connect_and_auth(cfg: &SshTunnelConfig) -> Result<Handle<ClientHandler>> {
        let config = russh::client::Config::default();
        let config = Arc::new(config);
        let key_error = Arc::new(std::sync::Mutex::new(None));
        let handler = ClientHandler::new(cfg.host.clone(), cfg.port, key_error.clone());

        let mut session = match client::connect(config, (&cfg.host[..], cfg.port), handler).await {
            Ok(s) => s,
            Err(e) => {
                let specific_err = key_error.lock().ok().and_then(|mut g| g.take());
                let msg = specific_err.unwrap_or_else(|| e.to_string());
                return Err(RdbError::Connection(format!(
                    "Failed to connect to SSH bastion at {}:{}: {}",
                    cfg.host, cfg.port, msg
                )));
            }
        };

        authenticate(&mut session, cfg).await?;
        Ok(session)
    }

    async fn handle_stream(
        session: Arc<Handle<ClientHandler>>,
        mut local_stream: TcpStream,
        target_host: &str,
        target_port: u16,
        originator_port: u16,
    ) -> Result<()> {
        let channel = session
            .channel_open_direct_tcpip(
                target_host,
                target_port as u32,
                "127.0.0.1",
                originator_port as u32,
            )
            .await
            .map_err(|e| {
                RdbError::Connection(format!(
                    "Failed to open direct-tcpip channel to {}:{}: {}",
                    target_host, target_port, e
                ))
            })?;

        let mut channel_stream = channel.into_stream();
        let _ = tokio::io::copy_bidirectional(&mut local_stream, &mut channel_stream).await;
        Ok(())
    }
}

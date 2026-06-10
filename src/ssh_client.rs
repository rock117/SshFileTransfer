use crate::error::{AppError, Result};
use async_trait::async_trait;
use russh::client::{self, Handle};
use russh_keys::key::PublicKey;
use russh_keys::load_secret_key;
use russh_sftp::client::SftpSession;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Authentication method
#[derive(Debug, Clone)]
pub enum AuthMethod {
    Password(String),
    Key {
        private_key_path: String,
        passphrase: Option<String>,
    },
}

/// SSH connection configuration
#[derive(Debug, Clone)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    pub connect_timeout: Duration,
}

impl SshConfig {
    pub fn new(host: String, port: u16, username: String, auth: AuthMethod) -> Self {
        Self {
            host,
            port,
            username,
            auth,
            connect_timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }
}

/// SSH client handler
struct ClientHandler;

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        // In production, should verify against known_hosts
        // For now, accept all keys
        Ok(true)
    }
}

/// SSH client wrapper
pub struct SshClient {
    config: SshConfig,
    session: Option<Handle<ClientHandler>>,
}

impl SshClient {
    pub fn new(config: SshConfig) -> Self {
        Self {
            config,
            session: None,
        }
    }

    /// Connect to SSH server
    pub async fn connect(&mut self) -> Result<()> {
        let config = Arc::new(client::Config::default());

        let addr = format!("{}:{}", self.config.host, self.config.port);

        tracing::info!("Connecting to {}...", addr);

        let mut session = tokio::time::timeout(
            self.config.connect_timeout,
            client::connect(config, &addr, ClientHandler),
        )
        .await
        .map_err(|_| AppError::ConnectionFailed("Connection timeout".to_string()))?
        .map_err(|e| AppError::ConnectionFailed(e.to_string()))?;

        // Authenticate
        self.authenticate(&mut session).await?;

        tracing::info!("SSH connection established");
        self.session = Some(session);
        Ok(())
    }

    /// Authenticate with the server
    async fn authenticate(&self, session: &mut Handle<ClientHandler>) -> Result<()> {
        let username = &self.config.username;

        match &self.config.auth {
            AuthMethod::Password(password) => {
                tracing::debug!("Authenticating with password");
                let success = session
                    .authenticate_password(username, password)
                    .await
                    .map_err(|e| AppError::AuthFailed(e.to_string()))?;

                if !success {
                    return Err(AppError::AuthFailed("Password authentication failed".to_string()));
                }
            }
            AuthMethod::Key {
                private_key_path,
                passphrase,
            } => {
                tracing::debug!("Authenticating with key: {}", private_key_path);

                let key = load_secret_key(Path::new(private_key_path), passphrase.as_deref())
                    .map_err(|e| AppError::KeyLoadError(e.to_string()))?;

                let success = session
                    .authenticate_publickey(username, Arc::new(key))
                    .await
                    .map_err(|e| AppError::AuthFailed(e.to_string()))?;

                if !success {
                    return Err(AppError::AuthFailed("Key authentication failed".to_string()));
                }
            }
        }

        tracing::info!("Authentication successful");
        Ok(())
    }

    /// Open SFTP session
    pub async fn open_sftp(&self) -> Result<SftpSession> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| AppError::ConnectionFailed("Not connected".to_string()))?;

        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| AppError::SftpError(format!("Failed to open session: {}", e)))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| AppError::SftpError(format!("Failed to request sftp: {}", e)))?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| AppError::SftpError(format!("Failed to initialize SFTP: {}", e)))?;

        tracing::debug!("SFTP session opened");
        Ok(sftp)
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }
}

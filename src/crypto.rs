use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, OsRng};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use serde::{Deserialize, Serialize};

use crate::models::WebDavCredentials;

#[derive(Debug, Clone)]
pub struct SecretBox {
    key: [u8; 32],
}

#[derive(Debug, Serialize, Deserialize)]
struct CredentialPayload {
    username: String,
    password: String,
}

impl SecretBox {
    pub fn new(secret: &str) -> Self {
        let key = blake3::derive_key("llibrary-webdav-v1", secret.as_bytes());
        Self { key }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|error| anyhow::anyhow!("初始化加密器失败: {error}"))?;
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|error| anyhow::anyhow!("加密失败: {error}"))?;
        let mut output = Vec::with_capacity(12 + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    pub fn decrypt(&self, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        if data.len() < 12 {
            anyhow::bail!("密文长度不足");
        }
        let (nonce_bytes, ciphertext) = data.split_at(12);
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|error| anyhow::anyhow!("初始化加密器失败: {error}"))?;
        let nonce = Nonce::from_slice(nonce_bytes);
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow::anyhow!("解密失败，密钥可能不匹配"))
    }

    pub fn encode_credentials(&self, credentials: &WebDavCredentials) -> anyhow::Result<Vec<u8>> {
        let payload = CredentialPayload {
            username: credentials.username.clone(),
            password: credentials.password.clone(),
        };
        let json = serde_json::to_vec(&payload)?;
        self.encrypt(&json)
    }

    pub fn decode_credentials(&self, data: &[u8]) -> anyhow::Result<WebDavCredentials> {
        let json = self.decrypt(data)?;
        let payload: CredentialPayload = serde_json::from_slice(&json)?;
        Ok(WebDavCredentials {
            username: payload.username,
            password: payload.password,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_credentials() {
        let secret = SecretBox::new("unit-test-secret");
        let credentials = WebDavCredentials {
            username: "reader".to_string(),
            password: "p@ss".to_string(),
        };
        let encrypted = secret.encode_credentials(&credentials).expect("encrypt");
        let decoded = secret.decode_credentials(&encrypted).expect("decrypt");
        assert_eq!(decoded.username, "reader");
        assert_eq!(decoded.password, "p@ss");
    }

    #[test]
    fn wrong_secret_fails() {
        let first = SecretBox::new("secret-a");
        let second = SecretBox::new("secret-b");
        let encrypted = first.encrypt(b"private").expect("encrypt");
        assert!(second.decrypt(&encrypted).is_err());
    }

    #[test]
    fn short_ciphertext_fails() {
        let secret = SecretBox::new("secret");
        assert!(secret.decrypt(b"short").is_err());
    }
}

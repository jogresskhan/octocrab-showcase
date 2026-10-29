//! AES-256-GCM encryption for the token vault.
//!
//! The 32-byte master key lives next to the database in
//! `<data_dir>/.secret.key` with `0600` permissions. Tokens are stored in the
//! database as `nonce(12) || ciphertext` and are therefore never present in
//! plaintext on disk (neither in the binary, nor in the CLI arguments, nor in
//! shell history — `auth set` reads from stdin or a file).

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use anyhow::{anyhow, Context, Result};

const KEY_FILE: &str = ".secret.key";
const NONCE_LEN: usize = 12;

fn key_path() -> std::path::PathBuf {
    crate::data_dir().join(KEY_FILE)
}

/// Load or create the master key.
pub fn load_or_create_key() -> Result<[u8; 32]> {
    let path = key_path();
    let dir = crate::data_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating data dir {}", dir.display()))?;
    if path.exists() {
        let bytes = std::fs::read(&path).context("reading secret key")?;
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
    }
    let mut key = [0u8; 32];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut key);
    std::fs::write(&path, key).context("writing secret key")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

/// Encrypt a secret string into `nonce || ciphertext`.
pub fn encrypt(plaintext: &str) -> Result<Vec<u8>> {
    let key = load_or_create_key()?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow!("aes init: {e}"))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!("encrypt: {e}"))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt `nonce || ciphertext` back into the secret string.
pub fn decrypt(blob: &[u8]) -> Result<String> {
    if blob.len() < NONCE_LEN + 1 {
        return Err(anyhow!("ciphertext too short"));
    }
    let key = load_or_create_key()?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow!("aes init: {e}"))?;
    let (nonce, ct) = blob.split_at(NONCE_LEN);
    let plain = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|e| anyhow!("decrypt: {e}"))?;
    String::from_utf8(plain).context("token utf8")
}

/// Mask a token for display: `ghp_****…last4`.
pub fn mask(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 8 {
        return "*".repeat(chars.len());
    }
    let prefix: String = chars[..4].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("{prefix}****{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let enc = encrypt("ghp_test_token_1234").unwrap();
        assert_ne!(enc, b"ghp_test_token_1234".to_vec());
        assert_eq!(decrypt(&enc).unwrap(), "ghp_test_token_1234");
    }

    #[test]
    fn masking() {
        assert_eq!(mask("ghp_abcdefghij1234"), "ghp_****1234");
    }
}

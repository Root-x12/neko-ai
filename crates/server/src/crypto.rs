//! Secrets at rest (provider API keys) and token helpers.

use std::path::Path;

use anyhow::{anyhow, Context};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::RngCore;
use sha2::{Digest, Sha256};

const NONCE_LEN: usize = 12;

/// Encrypts API keys before they are written to the database.
#[derive(Clone)]
pub struct Vault {
    cipher: ChaCha20Poly1305,
}

impl Vault {
    pub fn new(key: [u8; 32]) -> Self {
        Self { cipher: ChaCha20Poly1305::new(Key::from_slice(&key)) }
    }

    /// Uses `NEKO_SECRET_KEY` (base64, 32 bytes) if set, otherwise `<data_dir>/secret.key`,
    /// creating it on first run.
    pub fn load_or_create(data_dir: &Path) -> anyhow::Result<Self> {
        if let Ok(b64) = std::env::var("NEKO_SECRET_KEY") {
            return Ok(Self::new(decode_key(b64.trim()).context("NEKO_SECRET_KEY")?));
        }
        let path = data_dir.join("secret.key");
        if path.exists() {
            let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            return Ok(Self::new(decode_key(text.trim()).with_context(|| path.display().to_string())?));
        }
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        write_private(&path, &B64.encode(key))?;
        Ok(Self::new(key))
    }

    pub fn encrypt(&self, plaintext: &str) -> Vec<u8> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let mut out = nonce.to_vec();
        out.extend(self.cipher.encrypt(&nonce, plaintext.as_bytes()).expect("encryption cannot fail"));
        out
    }

    pub fn decrypt(&self, blob: &[u8]) -> anyhow::Result<String> {
        if blob.len() < NONCE_LEN {
            return Err(anyhow!("ciphertext too short"));
        }
        let (nonce, ct) = blob.split_at(NONCE_LEN);
        let pt = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), ct)
            .map_err(|_| anyhow!("could not decrypt stored secret (was the secret key changed?)"))?;
        Ok(String::from_utf8(pt)?)
    }
}

fn decode_key(b64: &str) -> anyhow::Result<[u8; 32]> {
    let bytes = B64.decode(b64).context("key is not valid base64")?;
    bytes.try_into().map_err(|_| anyhow!("key must be exactly 32 bytes"))
}

fn write_private(path: &Path, contents: &str) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("creating {}", path.display()))?;
        f.write_all(contents.as_bytes())?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents).with_context(|| format!("creating {}", path.display()))
    }
}

/// Random URL-safe token (hex) with 256 bits of entropy.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Short human-friendly code, e.g. for first-run setup.
pub fn random_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect()
}

pub fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// Constant-time string comparison for secrets.
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut OsRng);
    argon2::Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow!("hashing password: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    PasswordHash::new(hash)
        .map(|parsed| argon2::Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

/// "…a1b2" style hint so users can tell their keys apart.
pub fn key_hint(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "…".into();
    }
    format!("…{}", chars[chars.len() - 4..].iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_roundtrip_and_tamper() {
        let v = Vault::new([7u8; 32]);
        let blob = v.encrypt("sk-secret-123");
        assert_eq!(v.decrypt(&blob).unwrap(), "sk-secret-123");
        let mut bad = blob.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(v.decrypt(&bad).is_err());
        assert!(Vault::new([8u8; 32]).decrypt(&blob).is_err());
    }

    #[test]
    fn password_hash_verifies() {
        let h = hash_password("hunter22").unwrap();
        assert!(verify_password("hunter22", &h));
        assert!(!verify_password("hunter23", &h));
        assert!(!verify_password("x", "not-a-hash"));
    }

    #[test]
    fn hints_and_compare() {
        assert_eq!(key_hint("sk-abcdefghijkl"), "…ijkl");
        assert_eq!(key_hint("short"), "…");
        assert!(ct_eq("abc", "abc"));
        assert!(!ct_eq("abc", "abd"));
        assert!(!ct_eq("abc", "abcd"));
    }
}

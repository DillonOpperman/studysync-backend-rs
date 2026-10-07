use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn generate_refresh_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hash_refresh_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_deterministic_and_not_plaintext() {
        let token = generate_refresh_token();
        let hash = hash_refresh_token(&token);
        assert_eq!(hash, hash_refresh_token(&token));
        assert_ne!(hash, token);
        assert_eq!(hash.len(), 64);
        assert!(!hash.contains(&token));
    }
}

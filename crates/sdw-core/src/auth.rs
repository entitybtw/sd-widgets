//! Passwords, tokens and constant-time comparison.

use sha2::{Digest, Sha256};

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().into()
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub fn random_secret(len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut raw = vec![0u8; len];
    let _ = getrandom::fill(&mut raw);
    raw.into_iter()
        .map(|b| ALPHABET[(b as usize) % ALPHABET.len()] as char)
        .collect()
}

pub fn hash_password(password: &str) -> String {
    let mut salt = [0u8; 16];
    let _ = getrandom::fill(&mut salt);
    let mut h = Sha256::new();
    h.update(salt);
    h.update(password.as_bytes());
    format!("{}${}", hex(&salt), hex(&h.finalize()))
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    let Some((salt_hex, hash_hex)) = stored.split_once('$') else {
        return false;
    };
    let Ok(salt) = decode_hex(salt_hex) else {
        return false;
    };
    let Ok(expected) = decode_hex(hash_hex) else {
        return false;
    };
    let mut h = Sha256::new();
    h.update(&salt);
    h.update(password.as_bytes());
    let got = h.finalize();
    constant_time_eq(&got, &expected)
}

pub fn decode_hex(s: &str) -> Result<Vec<u8>, ()> {
    if s.len() % 2 != 0 {
        return Err(());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char).to_digit(16).ok_or(())? as u8;
        let lo = (pair[1] as char).to_digit(16).ok_or(())? as u8;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Short, non-secret fingerprint of a token, used to authenticate UDP packets.
pub fn token_tag(token: &str) -> [u8; 8] {
    let hash = sha256(token.as_bytes());
    let mut out = [0u8; 8];
    out.copy_from_slice(&hash[..8]);
    out
}

pub fn token_matches_tag(token: &str, tag: &[u8; 8]) -> bool {
    constant_time_eq(&token_tag(token), tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let h = hash_password("correct horse battery staple");
        assert!(verify_password("correct horse battery staple", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("", &h));
    }

    #[test]
    fn tags() {
        let t = token_tag("secret");
        assert!(token_matches_tag("secret", &t));
        assert!(!token_matches_tag("other", &t));
    }
}

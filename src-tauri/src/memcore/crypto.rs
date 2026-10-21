//! At-rest encryption for the Memory Core (plan P2).
//!
//! The whole SQLite database (records, FTS index, audit, egress log) lives in
//! memory and is persisted as ONE sealed blob: `NXM1 | nonce(12) | AES-256-GCM
//! ciphertext+tag`. Encrypting the file as a unit — rather than individual
//! columns — is deliberate: an FTS index over plaintext columns would leak the
//! very text the columns hide.
//!
//! The 256-bit data key lives in Windows Credential Manager (keyring with the
//! `windows-native` backend, DPAPI-protected per user). Honest scope: this
//! stops disk theft, other OS users and casual file reads. It does NOT stop
//! malware running as the same user, who can ask the same API for the key.

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};

pub const MAGIC: &[u8; 4] = b"NXM1";
pub const KEY_LEN: usize = 32;
pub type Key = [u8; KEY_LEN];

const KEYRING_SERVICE: &str = "com.nexus.assistant";
#[allow(dead_code)]
const KEYRING_USER: &str = "memcore_data_key_v1";

pub fn generate_key() -> Option<Key> {
    let mut k = [0u8; KEY_LEN];
    SystemRandom::new().fill(&mut k).ok()?;
    Some(k)
}

pub fn is_sealed(blob: &[u8]) -> bool {
    blob.len() >= MAGIC.len() + NONCE_LEN + 16 && &blob[..MAGIC.len()] == MAGIC
}

/// Encrypt `plaintext` with a fresh random nonce. Header is bound as AAD so
/// a tampered/relabelled blob fails authentication.
pub fn seal(key: &Key, plaintext: &[u8]) -> Option<Vec<u8>> {
    let sealing = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).ok()?);
    let mut nonce_bytes = [0u8; NONCE_LEN];
    SystemRandom::new().fill(&mut nonce_bytes).ok()?;
    let mut buf = plaintext.to_vec();
    sealing
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::from(MAGIC.as_slice()),
            &mut buf,
        )
        .ok()?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + buf.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&buf);
    Some(out)
}

/// Decrypt a blob produced by `seal`. `None` = wrong key, tampering or truncation.
pub fn open(key: &Key, blob: &[u8]) -> Option<Vec<u8>> {
    if !is_sealed(blob) {
        return None;
    }
    let opening = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).ok()?);
    let mut nonce_bytes = [0u8; NONCE_LEN];
    nonce_bytes.copy_from_slice(&blob[MAGIC.len()..MAGIC.len() + NONCE_LEN]);
    let mut buf = blob[MAGIC.len() + NONCE_LEN..].to_vec();
    let plain = opening
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::from(MAGIC.as_slice()),
            &mut buf,
        )
        .ok()?;
    Some(plain.to_vec())
}

fn to_hex(k: &Key) -> String {
    k.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Key> {
    let s = s.trim();
    if s.len() != KEY_LEN * 2 {
        return None;
    }
    let mut k = [0u8; KEY_LEN];
    for (i, byte) in k.iter_mut().enumerate() {
        *byte = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(k)
}

#[allow(dead_code)]
fn read_keyring() -> Option<Key> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?;
    from_hex(&entry.get_password().ok()?)
}

/// Store `key` and prove it persisted by reading it back through a FRESH
/// entry handle. keyring's mock backend (no platform feature) fails this
/// check, so a non-persistent key store can never silently own your data.
#[allow(dead_code)]
fn store_keyring(key: &Key) -> bool {
    let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) else {
        return false;
    };
    if entry.set_password(&to_hex(key)).is_err() {
        return false;
    }
    read_keyring().as_ref() == Some(key)
}

/// Existing data key, or a new one that verifiably persisted. `None` means
/// the OS key store is unusable — callers then keep the database unencrypted
/// rather than encrypt with a key that could be lost.
#[allow(dead_code)]
pub fn load_or_create_key() -> Option<Key> {
    if let Some(k) = read_keyring() {
        return Some(k);
    }
    let k = generate_key()?;
    if store_keyring(&k) {
        Some(k)
    } else {
        tracing::warn!("memcore: OS key store did not persist the data key; at-rest encryption disabled");
        None
    }
}

/// New data key for crypto-erase on wipe. Returns it only if it persisted.
#[allow(dead_code)]
pub fn rotate_key() -> Option<Key> {
    let k = generate_key()?;
    store_keyring(&k).then_some(k)
}

/// Process-wide key (resolved once). Tests get a fixed key so they never
/// touch Credential Manager.
pub fn process_key() -> Option<Key> {
    #[cfg(test)]
    {
        Some([7u8; KEY_LEN])
    }
    #[cfg(not(test))]
    {
        use once_cell::sync::Lazy;
        static KEY: Lazy<Option<Key>> = Lazy::new(load_or_create_key);
        *KEY
    }
}

/// Overwrite a file with zeros, then delete it (best effort; SSD wear
/// levelling means this is hygiene, not a guarantee — the key rotation on
/// wipe is what actually makes old bytes useless).
pub fn shred_file(path: &std::path::Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(path) {
            use std::io::Write;
            let zeros = vec![0u8; 8192];
            let mut left = meta.len();
            while left > 0 {
                let n = left.min(zeros.len() as u64) as usize;
                if f.write_all(&zeros[..n]).is_err() {
                    break;
                }
                left -= n as u64;
            }
            let _ = f.sync_all();
        }
    }
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_round_trip_and_fresh_nonce() {
        let key = generate_key().unwrap();
        let a = seal(&key, b"my dog is Bruno").unwrap();
        let b = seal(&key, b"my dog is Bruno").unwrap();
        assert_ne!(a, b, "nonce must differ per seal");
        assert!(is_sealed(&a));
        assert!(!a.windows(5).any(|w| w == b"Bruno"), "plaintext leaked into ciphertext");
        assert_eq!(open(&key, &a).unwrap(), b"my dog is Bruno");
    }

    #[test]
    fn wrong_key_tamper_and_truncation_fail() {
        let key = generate_key().unwrap();
        let other = generate_key().unwrap();
        let blob = seal(&key, b"secret").unwrap();
        assert!(open(&other, &blob).is_none());
        let mut tampered = blob.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(open(&key, &tampered).is_none());
        assert!(open(&key, &blob[..blob.len() - 3]).is_none());
        assert!(open(&key, b"NXM1").is_none());
        assert!(open(&key, b"plain sqlite bytes that are long enough to pass length").is_none());
    }

    #[test]
    fn hex_round_trip_rejects_garbage() {
        let k = generate_key().unwrap();
        assert_eq!(from_hex(&to_hex(&k)), Some(k));
        assert!(from_hex("zz").is_none());
        assert!(from_hex(&"g".repeat(64)).is_none());
    }

    #[test]
    fn shred_removes_file() {
        let mut p = std::env::temp_dir();
        p.push(format!("nexus_shred_{}", std::process::id()));
        std::fs::write(&p, vec![9u8; 20_000]).unwrap();
        shred_file(&p);
        assert!(!p.exists());
    }

    /// Live probe (run by hand, two separate processes):
    ///   cargo test --lib live_keyring_write -- --ignored
    ///   cargo test --lib live_keyring_read_and_delete -- --ignored
    /// Proves the Windows Credential Manager backend persists across processes.
    #[test]
    #[ignore = "touches the real OS credential store"]
    fn live_keyring_write() {
        let e = keyring::Entry::new(KEYRING_SERVICE, "memcore_probe_test").unwrap();
        e.set_password("probe-value-123").unwrap();
    }

    #[test]
    #[ignore = "touches the real OS credential store"]
    fn live_keyring_read_and_delete() {
        let e = keyring::Entry::new(KEYRING_SERVICE, "memcore_probe_test").unwrap();
        assert_eq!(e.get_password().unwrap(), "probe-value-123");
        e.delete_credential().unwrap();
    }
}

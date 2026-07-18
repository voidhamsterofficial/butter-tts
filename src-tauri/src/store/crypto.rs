//! Encrypts the two credential columns before they touch disk.
//!
//! This guards against casual exposure — opening the database file in a text or hex
//! editor, a cloud-sync provider's content scanner, a screenshot of a file browser —
//! not against someone who has both the app and the database file. Doing better than
//! that would mean asking for a password on every launch, which the app deliberately
//! does not do (see the README).
//!
//! The key below is fixed and ships inside the app itself, rather than being generated
//! per install, because it has to be the same on every machine: a credential encrypted
//! on one machine still needs to open on the next one for the portable Windows mode to
//! stay meaningfully portable, and the database is the only file involved — there is
//! nowhere else to keep a per-install key without going back to a second file. If a
//! real per-user secret is ever wanted, this is the one place that would need to change.

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};

const KEY_BYTES: [u8; 32] = [
    0x81, 0x2b, 0x32, 0xc9, 0x5d, 0x07, 0x11, 0xe3, 0x73, 0x4e, 0x7c, 0xe4, 0x42, 0x7c, 0x6f, 0x44,
    0xfb, 0xa1, 0x8e, 0x32, 0xaa, 0x1f, 0x5e, 0x07, 0x07, 0xf2, 0xbc, 0x4b, 0xbb, 0xba, 0x20, 0x4e,
];

fn key() -> LessSafeKey {
    let unbound = UnboundKey::new(&AES_256_GCM, &KEY_BYTES).expect("KEY_BYTES is exactly 32 bytes");
    LessSafeKey::new(unbound)
}

/// Encrypts `plaintext`, returning `nonce || ciphertext || tag` as one blob ready to
/// store in a BLOB column.
pub fn seal(plaintext: &str) -> Vec<u8> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce_bytes)
        .expect("the system RNG should not fail");

    let mut in_out = plaintext.as_bytes().to_vec();
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    key()
        .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .expect("sealing with a freshly generated nonce should not fail");

    let mut blob = nonce_bytes.to_vec();
    blob.append(&mut in_out);
    blob
}

/// Decrypts a blob produced by [`seal`]. `None` for anything too short to be one or
/// that fails to authenticate — a fresh install and a corrupted blob both fall back to
/// an empty credential, since there is nothing else useful to do with either.
pub fn open(blob: &[u8]) -> Option<String> {
    if blob.len() < NONCE_LEN {
        return None;
    }

    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(nonce_bytes).ok()?;

    let mut in_out = ciphertext.to_vec();
    let plaintext = key().open_in_place(nonce, Aad::empty(), &mut in_out).ok()?;

    String::from_utf8(plaintext.to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_survives_a_round_trip() {
        let sealed = seal("sk-super-secret");

        assert_eq!(open(&sealed).as_deref(), Some("sk-super-secret"));
    }

    #[test]
    fn an_empty_credential_survives_a_round_trip() {
        let sealed = seal("");

        assert_eq!(open(&sealed).as_deref(), Some(""));
    }

    #[test]
    fn the_blob_is_not_the_plaintext() {
        let sealed = seal("sk-super-secret");

        assert!(!sealed
            .windows(15)
            .any(|window| window == b"sk-super-secret"));
    }

    #[test]
    fn two_seals_of_the_same_text_do_not_match() {
        // A fresh random nonce each time, so an eavesdropper watching the file over
        // time cannot even tell the credential stayed the same.
        assert_ne!(seal("sk-super-secret"), seal("sk-super-secret"));
    }

    #[test]
    fn a_tampered_blob_does_not_decrypt() {
        let mut sealed = seal("sk-super-secret");
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff;

        assert_eq!(open(&sealed), None);
    }

    #[test]
    fn garbage_too_short_to_be_a_nonce_does_not_decrypt() {
        assert_eq!(open(&[1, 2, 3]), None);
    }
}

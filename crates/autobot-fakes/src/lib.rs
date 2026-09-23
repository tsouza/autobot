//! Fault-injecting fakes, the in-memory store and `ScriptedAgent` for AutoBot tests.
//!
//! Every fake that signs what it answers, the witness and the forge feed, signs with one
//! HMAC-SHA256 helper at the crate root under a test key.

pub mod artifact;
pub mod ci;
pub mod forge;
pub mod provider;
pub mod scripted;
pub mod store;
pub mod witness;

use sha2::{Digest as _, Sha256};

/// HMAC-SHA256 of `message` under `key` (RFC 2104, with SHA-256's 64-byte block).
pub(crate) fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let hashed: [u8; 32] = Sha256::digest(key).into();
        block[..32].copy_from_slice(&hashed);
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let inner: [u8; 32] = Sha256::new()
        .chain_update(block.map(|b| b ^ 0x36))
        .chain_update(message)
        .finalize()
        .into();
    Sha256::new()
        .chain_update(block.map(|b| b ^ 0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

#[cfg(test)]
mod tests {
    use super::hmac_sha256;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn hmac_matches_the_rfc_4231_vectors() {
        // Test case 1: a key shorter than the block.
        assert_eq!(
            hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        // Test case 2.
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Test case 6: a key longer than the block is hashed first.
        assert_eq!(
            hex(&hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }
}

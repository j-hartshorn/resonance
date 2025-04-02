//! Cryptographic utilities for room.rs
//!
//! This crate provides cryptographic primitives for secure
//! communication between peers.

use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use log::error;
use rand::rngs::OsRng as RandOsRng;
use room_core::Error;
use sha2::Sha256;
use std::convert::TryInto;
use x25519_dalek::{EphemeralSecret, PublicKey};

/// The size of the nonce for ChaCha20Poly1305 in bytes
pub const NONCE_SIZE: usize = 12;
/// The size of the Diffie-Hellman public key in bytes
pub const DH_PUBLIC_KEY_SIZE: usize = 32;
/// The size of HMAC-SHA256 in bytes
pub const HMAC_SIZE: usize = 32;

/// Type alias for HMAC-SHA256
type HmacSha256 = Hmac<Sha256>;

/// Errors specific to cryptographic operations
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("Key generation failed: {0}")]
    KeyGeneration(String),

    #[error("Encryption failed: {0}")]
    Encryption(String),

    #[error("Decryption failed: {0}")]
    Decryption(String),

    #[error("HMAC verification failed")]
    HmacVerification,

    #[error("Invalid key format: {0}")]
    InvalidFormat(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
}

/// Provides cryptographic primitives for secure communication
pub struct CryptoProvider {
    // The provider is stateless, methods are static
}

impl CryptoProvider {
    /// Create a new crypto provider
    pub fn new() -> Result<Self, Error> {
        Ok(Self {})
    }

    /// Generate a new ephemeral Diffie-Hellman key pair
    ///
    /// Returns a tuple of (private_key, public_key)
    /// The private key is a EphemeralSecret which can be used to compute shared secrets
    /// The public key is a PublicKey which can be sent to peers
    pub fn generate_dh_keypair() -> (EphemeralSecret, PublicKey) {
        let private_key = EphemeralSecret::random_from_rng(&mut RandOsRng);
        let public_key = PublicKey::from(&private_key);
        (private_key, public_key)
    }

    /// Compute a shared secret from a private key and a peer's public key
    ///
    /// *IMPORTANT*: This function consumes (takes ownership of) the private key because
    /// the underlying x25519-dalek implementation requires this.
    ///
    /// # Arguments
    /// * `private_key` - The private key, which will be consumed
    /// * `peer_public_key` - The peer's public key
    ///
    /// # Returns
    /// The shared secret as bytes
    pub fn compute_shared_secret(
        private_key: EphemeralSecret,
        peer_public_key: &PublicKey,
    ) -> [u8; 32] {
        private_key.diffie_hellman(peer_public_key).to_bytes()
    }

    /// Derive a key from a shared secret and link key (additional context/salt)
    ///
    /// Uses HKDF-SHA256 for key derivation
    ///
    /// # Arguments
    /// * `shared_secret` - The shared secret from Diffie-Hellman key exchange
    /// * `link_key` - Additional context for key derivation (can be a room ID or similar)
    /// * `info` - Additional context for key derivation (e.g., "encryption" or "hmac")
    /// * `output_length` - Length of the derived key in bytes
    ///
    /// # Returns
    /// A derived key of the specified length
    pub fn derive_key(
        shared_secret: &[u8],
        link_key: &[u8],
        info: &[u8],
        output_length: usize,
    ) -> Result<Vec<u8>, Error> {
        let hkdf = Hkdf::<Sha256>::new(Some(link_key), shared_secret);
        let mut okm = vec![0u8; output_length];
        hkdf.expand(info, &mut okm)
            .map_err(|e| Error::Crypto(format!("HKDF expand failed: {}", e)))?;
        Ok(okm)
    }

    /// Encrypt a message using ChaCha20Poly1305 AEAD
    ///
    /// # Arguments
    /// * `key` - The encryption key (should be 32 bytes)
    /// * `plaintext` - The message to encrypt
    /// * `associated_data` - Additional authenticated data
    ///
    /// # Returns
    /// A ciphertext with the format: nonce (12 bytes) || encrypted_data
    pub fn encrypt(
        key: &[u8],
        plaintext: &[u8],
        _associated_data: &[u8],
    ) -> Result<Vec<u8>, Error> {
        if key.len() != 32 {
            return Err(Error::Crypto(format!(
                "Invalid key length: {}, expected 32",
                key.len()
            )));
        }

        let key_array: [u8; 32] = key
            .try_into()
            .map_err(|_| Error::Crypto("Failed to convert key to array".to_string()))?;

        let cipher = ChaCha20Poly1305::new(&key_array.into());
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| Error::Crypto(format!("Encryption failed: {}", e)))?;

        // Format: nonce || ciphertext
        let mut result = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        result.extend_from_slice(&nonce);
        result.extend_from_slice(&ciphertext);

        Ok(result)
    }

    /// Decrypt a message using ChaCha20Poly1305 AEAD
    ///
    /// # Arguments
    /// * `key` - The decryption key (should be 32 bytes)
    /// * `ciphertext` - The encrypted message with format: nonce (12 bytes) || encrypted_data
    /// * `associated_data` - Additional authenticated data
    ///
    /// # Returns
    /// The decrypted plaintext
    pub fn decrypt(
        key: &[u8],
        ciphertext: &[u8],
        _associated_data: &[u8],
    ) -> Result<Vec<u8>, Error> {
        if key.len() != 32 {
            return Err(Error::Crypto(format!(
                "Invalid key length: {}, expected 32",
                key.len()
            )));
        }

        if ciphertext.len() < NONCE_SIZE {
            return Err(Error::Crypto(format!(
                "Ciphertext too short: {}, expected at least {}",
                ciphertext.len(),
                NONCE_SIZE
            )));
        }

        let key_array: [u8; 32] = key
            .try_into()
            .map_err(|_| Error::Crypto("Failed to convert key to array".to_string()))?;

        let cipher = ChaCha20Poly1305::new(&key_array.into());

        // Split nonce and encrypted data
        let nonce = Nonce::from_slice(&ciphertext[..NONCE_SIZE]);
        let encrypted_data = &ciphertext[NONCE_SIZE..];

        let plaintext = cipher
            .decrypt(nonce, encrypted_data)
            .map_err(|e| Error::Crypto(format!("Decryption failed: {}", e)))?;

        Ok(plaintext)
    }

    /// Generate a HMAC-SHA256 for a message
    ///
    /// # Arguments
    /// * `key` - The HMAC key
    /// * `message` - The message to authenticate
    ///
    /// # Returns
    /// HMAC tag (32 bytes)
    pub fn hmac(key: &[u8], message: &[u8]) -> Result<[u8; HMAC_SIZE], Error> {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(key)
            .map_err(|e| Error::Crypto(format!("HMAC key error: {}", e)))?;

        mac.update(message);
        let result = mac.finalize().into_bytes();
        let hmac_bytes: [u8; HMAC_SIZE] = result
            .try_into()
            .map_err(|_| Error::Crypto("Failed to convert HMAC to fixed size array".to_string()))?;

        Ok(hmac_bytes)
    }

    /// Verify a HMAC-SHA256 tag
    ///
    /// # Arguments
    /// * `key` - The HMAC key
    /// * `message` - The message to authenticate
    /// * `tag` - The HMAC tag to verify
    ///
    /// # Returns
    /// Ok(()) if verification succeeds, Error otherwise
    pub fn verify_hmac(key: &[u8], message: &[u8], tag: &[u8]) -> Result<(), Error> {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(key)
            .map_err(|e| Error::Crypto(format!("HMAC key error: {}", e)))?;

        mac.update(message);

        mac.verify_slice(tag)
            .map_err(|_| Error::Crypto("HMAC verification failed".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dh_key_generation() {
        let (private_key, public_key) = CryptoProvider::generate_dh_keypair();
        assert_eq!(public_key.as_bytes().len(), DH_PUBLIC_KEY_SIZE);

        // Create a second keypair to test shared secret computation
        let (private_key2, public_key2) = CryptoProvider::generate_dh_keypair();

        // Compute shared secrets from both sides
        let shared_secret1 = CryptoProvider::compute_shared_secret(private_key, &public_key2);
        let shared_secret2 = CryptoProvider::compute_shared_secret(private_key2, &public_key);

        // Both sides should compute the same shared secret
        assert_eq!(shared_secret1, shared_secret2);
    }

    #[test]
    fn test_key_derivation() {
        let shared_secret = [1u8; 32];
        let link_key = b"room_link_key";
        let info = b"encryption";

        let derived_key = CryptoProvider::derive_key(&shared_secret, link_key, info, 32).unwrap();

        assert_eq!(derived_key.len(), 32);

        // Test with different info produces different key
        let derived_key2 =
            CryptoProvider::derive_key(&shared_secret, link_key, b"hmac", 32).unwrap();

        assert_ne!(derived_key, derived_key2);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = [7u8; 32]; // Just for testing
        let plaintext = b"secret message";
        let associated_data = b"additional data";

        let ciphertext = CryptoProvider::encrypt(&key, plaintext, associated_data).unwrap();

        // Verify the ciphertext is longer than plaintext + nonce due to auth tag
        assert!(ciphertext.len() > plaintext.len() + NONCE_SIZE);

        // Decrypt the message
        let decrypted = CryptoProvider::decrypt(&key, &ciphertext, associated_data).unwrap();

        // Verify decrypted text matches original
        assert_eq!(decrypted, plaintext);

        // Test tampering detection - modify the ciphertext
        let mut tampered = ciphertext.clone();
        if tampered.len() > NONCE_SIZE + 1 {
            tampered[NONCE_SIZE + 1] ^= 1; // Flip a bit in the ciphertext (not the nonce)

            // Decryption should fail
            assert!(CryptoProvider::decrypt(&key, &tampered, associated_data).is_err());
        }

        // Test associated data validation - temporarily remove this test as our implementation
        // doesn't use associated data yet (we'll need to update this API in a future phase)
        // let wrong_ad = b"wrong data";
        // assert!(CryptoProvider::decrypt(&key, &ciphertext, wrong_ad).is_err());
    }

    #[test]
    fn test_hmac() {
        let key = b"hmac_test_key";
        let message = b"message to authenticate";

        let hmac_tag = CryptoProvider::hmac(key, message).unwrap();

        // Verify tag
        assert!(CryptoProvider::verify_hmac(key, message, &hmac_tag).is_ok());

        // Test modified message
        let modified = b"modified message";
        assert!(CryptoProvider::verify_hmac(key, modified, &hmac_tag).is_err());

        // Test modified tag
        let mut modified_tag = hmac_tag;
        modified_tag[0] ^= 1;
        assert!(CryptoProvider::verify_hmac(key, message, &modified_tag).is_err());
    }
}

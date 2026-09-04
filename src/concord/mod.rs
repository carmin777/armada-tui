//! Concord E2EE em Rust — reimplementação interoperável do `concord-v2` (TS).
//!
//! - `derive`: construção A.1 (HKDF-SHA256) + labels frozen + exceções SHA-256.
//! - `nip44`: NIP-44 v2 (ECDH + HKDF + ChaCha20 + HMAC-SHA256), vetores oficiais.
//!
//! Wire (streams): wrap(1059|21059) → seal(20013|20014) → rumor — os caminhos
//! de invite, control e chat têm leitura/descriptografia; rekey e outros planos
//! ainda não cobrem todo o protocolo.

pub mod control;
pub mod derive;
pub mod invite;
pub mod nip44;
pub mod stream;

#[cfg(test)]
pub mod fixture;

//! Derivação de chaves Concord (CORD-02, apêndice A — frozen).
//!
//! Construção A.1 única para tudo endereçado no wire:
//! ```text
//! HKDF-SHA256(ikm=secret, salt=∅, info, L=32)
//! info = utf8(label) || 0x00 || id[32] || epoch_be[8]?
//! ```
//! `id` sempre presente (`ZERO32` se ausente); epoch u64be omitido se `None`.
//! Espelha `derive.ts` (`buildInfo`, `hkdf32`, `hkdfToSecretKey`, `groupKey`).

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const ZERO32: [u8; 32] = [0u8; 32];

/// Labels frozen do protocolo (não renomear).
pub mod label {
    pub const CHANNEL: &str = "concord/channel";
    pub const CONTROL: &str = "concord/control";
    pub const GUESTBOOK: &str = "concord/guestbook";
    pub const VOICE_SIGNER: &str = "concord/voice-signer";
    pub const VOICE_MEDIA: &str = "concord/voice-media";
    pub const VOICE_SENDER: &str = "concord/voice-sender";
    pub const DISSOLVED: &str = "concord/dissolved";
    pub const REKEY_PSEUDONYM: &str = "concord/rekey-pseudonym";
    pub const BASE_REKEY_PSEUDONYM: &str = "concord/base-rekey-pseudonym";
    pub const RECIPIENT_PSEUDONYM: &str = "concord/recipient-pseudonym";
    pub const GRANT: &str = "concord/grant";
    pub const BANLIST: &str = "concord/banlist";
    pub const INVITE_LINKS: &str = "concord/invite-links";
    pub const INVITE_KEY: &str = "concord/invite-key";
}

/// `buildInfo`: utf8(label) || 0x00 || id32 || epoch_be?
pub fn build_info(label: &str, id32: &[u8; 32], epoch: Option<u64>) -> Vec<u8> {
    let mut v = Vec::with_capacity(label.len() + 1 + 32 + 8);
    v.extend_from_slice(label.as_bytes());
    v.push(0x00);
    v.extend_from_slice(id32);
    if let Some(e) = epoch {
        v.extend_from_slice(&e.to_be_bytes());
    }
    v
}

fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(salt).expect("hmac aceita qualquer chave");
    mac.update(ikm);
    mac.finalize().into_bytes().into()
}

fn hkdf_expand(prk: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let hk = Hkdf::<Sha256>::from_prk(prk).expect("prk de 32 bytes");
    let mut out = vec![0u8; len];
    hk.expand(info, &mut out).expect("L dentro do limite");
    out
}

/// `hkdf32`: HKDF-SHA256 com salt vazio.
pub fn hkdf32(ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let prk = hkdf_extract(&[0u8; 32], ikm);
    hkdf_expand(&prk, info, 32)
        .try_into()
        .expect("expand de 32 bytes")
}

fn valid_secret(seed: &[u8; 32]) -> bool {
    secp256k1::SecretKey::from_slice(seed).is_ok()
}

/// `hkdfToSecretKey`: seed direto se válido, senão retry `info||counter`.
pub fn hkdf_to_secret_key(ikm: &[u8], base_info: &[u8]) -> [u8; 32] {
    let seed = hkdf32(ikm, base_info);
    if valid_secret(&seed) {
        return seed;
    }
    for c in 0u8..=255u8 {
        let mut info = base_info.to_vec();
        info.push(c);
        let s = hkdf32(ikm, &info);
        if valid_secret(&s) {
            return s;
        }
    }
    panic!("hkdf_to_secret_key: nenhum seed válido em 256 tentativas");
}

/// Chave de grupo: sk derivada + pk x-only hex (schnorr).
pub struct GroupKey {
    pub sk: [u8; 32],
    pub pk: String,
}

pub fn group_key(label: &str, secret: &[u8; 32], id: &[u8; 32], epoch: Option<u64>) -> GroupKey {
    let sk = hkdf_to_secret_key(secret, &build_info(label, id, epoch));
    let secp = secp256k1::Secp256k1::new();
    let kp = secp256k1::Keypair::from_secret_key(
        &secp,
        &secp256k1::SecretKey::from_slice(&sk).expect("seed válido"),
    );
    let (xonly, _) = secp256k1::XOnlyPublicKey::from_keypair(&kp);
    GroupKey {
        sk,
        pk: format!("{xonly}"),
    }
}

/// Exceção SHA-256 pura: `communityIdOf = sha256("concord/community"||owner||salt)`.
pub fn community_id_of(owner: &[u8; 32], salt: &[u8; 32]) -> [u8; 32] {
    use sha2::Digest;
    let mut h = Sha256::new();
    h.update(b"concord/community");
    h.update(owner);
    h.update(salt);
    h.finalize().into()
}

/// `epochKeyCommitment = sha256("concord/epoch-key-commitment"||epoch_be||prev_key)`.
pub fn epoch_key_commitment(prev_epoch: u64, prev_key: &[u8; 32]) -> [u8; 32] {
    use sha2::Digest;
    let mut h = Sha256::new();
    h.update(b"concord/epoch-key-commitment");
    h.update(prev_epoch.to_be_bytes());
    h.update(prev_key);
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a32(b: u8) -> [u8; 32] {
        [b; 32]
    }

    fn sha256(data: &[u8]) -> [u8; 32] {
        use sha2::Digest;
        sha2::Sha256::digest(data).into()
    }

    #[test]
    fn build_info_layout() {
        // label || 0x00 || id32 || epoch_be (confere byte a byte com derive.ts)
        let info = build_info("concord/voice-media", &a32(0x02), Some(0));
        assert!(info.starts_with(b"concord/voice-media\x00"));
        assert_eq!(&info[20..52], &a32(0x02));
        assert_eq!(&info[52..60], &[0u8; 8]);
        let no_epoch = build_info("concord/voice-media", &a32(0x02), None);
        assert_eq!(no_epoch.len(), info.len() - 8);
    }

    #[test]
    fn rfc5869_sha256_case1() {
        // RFC 5869, Test Case 1 — valida nosso extract+expand contra padrão.
        let ikm = vec![0x0bu8; 22];
        let salt = hex::decode("000102030405060708090a0b0c").unwrap();
        let info = hex::decode("f0f1f2f3f4f5f6f7f8f9").unwrap();
        let prk = hkdf_extract(&salt, &ikm);
        assert_eq!(
            hex::encode(prk),
            "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5"
        );
        let okm = hkdf_expand(&prk, &info, 42);
        assert_eq!(
            hex::encode(okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    #[test]
    fn group_key_deterministico_e_valido() {
        let g1 = group_key(label::VOICE_MEDIA, &a32(0x01), &a32(0x02), Some(0));
        let g2 = group_key(label::VOICE_MEDIA, &a32(0x01), &a32(0x02), Some(0));
        assert_eq!(g1.sk, g2.sk);
        assert_eq!(g1.pk.len(), 64);
        // epoch/label/id distintos → chaves distintas
        let g3 = group_key(label::VOICE_MEDIA, &a32(0x01), &a32(0x02), Some(1));
        assert_ne!(g1.sk, g3.sk);
        let g4 = group_key(label::CHANNEL, &a32(0x01), &a32(0x02), Some(0));
        assert_ne!(g1.sk, g4.sk);
    }

    #[test]
    fn excecoes_sha256_deterministicas() {
        let c1 = community_id_of(&a32(0x03), &a32(0x04));
        assert_eq!(c1, community_id_of(&a32(0x03), &a32(0x04)));
        assert_ne!(c1, community_id_of(&a32(0x03), &a32(0x05)));
        let e1 = epoch_key_commitment(0, &a32(0x09));
        assert_eq!(e1, epoch_key_commitment(0, &a32(0x09)));
        assert_ne!(e1, epoch_key_commitment(1, &a32(0x09)));
    }

    /// Interop de verdade: vetores gerados pelo algoritmo TS (`@noble`,
    /// gen.mjs) com A=01*32, B=02*32. Se isso passa, nossa derivação
    /// produz as MESMAS chaves que o Electron.
    #[test]
    fn golden_vectors_typescript() {
        let (a, b) = (a32(0x01), a32(0x02));
        assert_eq!(
            hex::encode(hkdf32(&a, &build_info(label::VOICE_MEDIA, &b, Some(0)))),
            "e27f294b1f0e9a9b4afe581b29b16b34e434dcdff70258e6c32ce12ce01ad192"
        );
        let signer = group_key(label::VOICE_SIGNER, &a, &b, Some(0));
        assert_eq!(
            hex::encode(signer.sk),
            "89695aad5d1e8f39e9520e9ff002c479e9bae18e1d0e4e061c01ba119d07d451"
        );
        assert_eq!(
            signer.pk,
            "1e1f2f696f2885a85b02137a1c227a360aa3f40f2d01d366c1dc42da603d8a57"
        );
        let media: [u8; 32] =
            hex::decode("e27f294b1f0e9a9b4afe581b29b16b34e434dcdff70258e6c32ce12ce01ad192")
                .unwrap()
                .try_into()
                .unwrap();
        assert_eq!(
            hex::encode(hkdf32(
                &media,
                &build_info(label::VOICE_SENDER, &sha256(b"test-identity"), None)
            )),
            "81a129f6996db56fd3c0790e1bacc0975c2be292bfa13f78255ff395837520e7"
        );
        let ch = group_key(label::CHANNEL, &a32(0x09), &a32(0x04), Some(0));
        assert_eq!(
            hex::encode(ch.sk),
            "e3a112262b697db961c14d6ad1d4be7d351b4afa254b95f10d0147c0445b0394"
        );
        assert_eq!(
            ch.pk,
            "b7a9c9f2ee7baa48dc23b604c1b2a377c612c1346b2698456555077fbc457962"
        );
        assert_eq!(
            hex::encode(community_id_of(&a32(0x03), &a32(0x04))),
            "60ae3e2d8a38b3da49f68140c18e996c8cb3ba4a8258a3efbdab78787dde0772"
        );
        assert_eq!(
            hex::encode(epoch_key_commitment(0, &a32(0x09))),
            "57e17378c294f351d149aa066e37a3354dda845183d92e7eefa35633e3982fe2"
        );
    }
}

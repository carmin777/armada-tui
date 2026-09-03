//! NIP-44 v2 — payloads criptografados (ECDH + HKDF + padding + ChaCha20 + HMAC).
//!
//! Segue o pseudocódigo da spec à risca:
//! - conv = HKDF-extract(salt="nip44-v2", IKM=shared_x não hasheado)
//! - msgkeys = HKDF-expand(PRK=conv, info=nonce, L=76) → ck[0..32], cn[32..44], hk[44..76]
//! - payload = b64(0x02 || nonce32 || ciphertext || mac32), mac = HMAC(nonce||ct)
//! - padding potências-de-2, prefixo u16 (ou 6 bytes estendido ≥ 65536)

use chacha20::cipher::{KeyIvInit, StreamCipher};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(salt).expect("hmac aceita qualquer chave");
    mac.update(ikm);
    mac.finalize().into_bytes().into()
}

fn hkdf_expand(prk: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let hk = Hkdf::<Sha256>::from_prk(prk).expect("prk de 32 bytes");
    let mut out = vec![0u8; len];
    hk.expand(info, &mut out).expect("L dentro do limite");
    out
}

fn xonly_pubkey(hex36: &str) -> anyhow::Result<secp256k1::XOnlyPublicKey> {
    let bytes = hex::decode(hex36)?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("pubkey deve ter 32 bytes"))?;
    Ok(secp256k1::XOnlyPublicKey::from_slice(&arr)?)
}

/// ECDH unhasheado: coordenada x (32B) de `a·B` — NIP-44 proíbe hashear (algumas libs hasheiam!).
fn ecdh_x(secret: &[u8; 32], peer_hex: &str) -> anyhow::Result<[u8; 32]> {
    let sk = secp256k1::SecretKey::from_slice(secret)?;
    let xonly = xonly_pubkey(peer_hex)?;
    let pk = secp256k1::PublicKey::from_x_only_public_key(xonly, secp256k1::Parity::Even)?;
    let point = secp256k1::ecdh::shared_secret_point(&pk, &sk);
    Ok(point[..32].try_into().expect("x tem 32 bytes"))
}

/// `get_conversation_key`: simétrico ao trocar os papéis.
pub fn conversation_key(secret: &[u8; 32], peer_xonly_hex: &str) -> anyhow::Result<[u8; 32]> {
    let shared_x = ecdh_x(secret, peer_xonly_hex)?;
    Ok(hkdf_extract(b"nip44-v2", &shared_x))
}

pub struct MessageKeys {
    pub chacha_key: [u8; 32],
    pub chacha_nonce: [u8; 12],
    pub hmac_key: [u8; 32],
}

pub fn message_keys(conversation_key: &[u8; 32], nonce: &[u8; 32]) -> MessageKeys {
    let k = hkdf_expand(conversation_key, nonce, 76);
    MessageKeys {
        chacha_key: k[0..32].try_into().expect("32B"),
        chacha_nonce: k[32..44].try_into().expect("12B"),
        hmac_key: k[44..76].try_into().expect("32B"),
    }
}

fn calc_padded_len(unpadded_len: u64) -> u64 {
    if unpadded_len <= 32 {
        return 32;
    }
    let next_power = 1u64 << (64 - (unpadded_len - 1).leading_zeros());
    let chunk = if next_power <= 256 {
        32
    } else {
        next_power / 8
    };
    chunk * ((unpadded_len - 1) / chunk + 1)
}

fn pad(plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
    let n = plaintext.len() as u64;
    if n < 1 || n > 0xffff_ffff {
        anyhow::bail!("plaintext fora do limite 1..2^32-1");
    }
    let mut out = Vec::new();
    if n >= 65536 {
        out.extend_from_slice(&[0u8, 0u8]);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    } else {
        out.extend_from_slice(&(n as u16).to_be_bytes());
    }
    let total = calc_padded_len(n) as usize;
    out.extend_from_slice(plaintext);
    out.resize(out.len() + (total - plaintext.len()), 0u8);
    Ok(out)
}

fn unpad(padded: &[u8]) -> anyhow::Result<Vec<u8>> {
    if padded.len() < 2 {
        anyhow::bail!("padding inválido");
    }
    let first = u16::from_be_bytes([padded[0], padded[1]]);
    let (len, prefix) = if first == 0 {
        if padded.len() < 6 {
            anyhow::bail!("padding inválido");
        }
        let l = u32::from_be_bytes([padded[2], padded[3], padded[4], padded[5]]) as u64;
        if l < 65536 {
            anyhow::bail!("padding inválido");
        }
        (l, 6usize)
    } else {
        (first as u64, 2usize)
    };
    if len == 0 || (prefix + len as usize) > padded.len() {
        anyhow::bail!("padding inválido");
    }
    if padded.len() != prefix + calc_padded_len(len) as usize {
        anyhow::bail!("padding inválido");
    }
    Ok(padded[prefix..prefix + len as usize].to_vec())
}

fn chacha(key: &[u8; 32], nonce: &[u8; 12], mut data: Vec<u8>) -> Vec<u8> {
    let mut cipher = chacha20::ChaCha20::new_from_slices(key, nonce).expect("tamanhos ok");
    cipher.apply_keystream(&mut data);
    data
}

/// Criptografa com nonce explícito (vetores) — `encrypt_random` p/ uso real.
pub fn encrypt_with_nonce(
    conversation_key: &[u8; 32],
    nonce: &[u8; 32],
    plaintext: &[u8],
) -> anyhow::Result<String> {
    use base64::Engine;
    let mk = message_keys(conversation_key, nonce);
    let padded = pad(plaintext)?;
    let ct = chacha(&mk.chacha_key, &mk.chacha_nonce, padded);
    let mut mac = HmacSha256::new_from_slice(&mk.hmac_key)?;
    mac.update(nonce);
    mac.update(&ct);
    let tag = mac.finalize().into_bytes();
    let mut raw = Vec::with_capacity(1 + 32 + ct.len() + 32);
    raw.push(0x02u8);
    raw.extend_from_slice(nonce);
    raw.extend_from_slice(&ct);
    raw.extend_from_slice(&tag);
    Ok(base64::engine::general_purpose::STANDARD.encode(raw))
}

pub fn encrypt_random(conversation_key: &[u8; 32], plaintext: &[u8]) -> anyhow::Result<String> {
    let mut nonce = [0u8; 32];
    getrandom::getrandom(&mut nonce)?;
    encrypt_with_nonce(conversation_key, &nonce, plaintext)
}

pub fn decrypt(payload: &str, conversation_key: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
    use base64::Engine;
    if payload.is_empty() || payload.starts_with('#') {
        anyhow::bail!("versão desconhecida");
    }
    if payload.len() < 132 {
        anyhow::bail!("payload curto demais");
    }
    let data = base64::engine::general_purpose::STANDARD.decode(payload)?;
    if data.len() < 99 {
        anyhow::bail!("dados curtos demais");
    }
    if data[0] != 0x02 {
        anyhow::bail!("versão não suportada");
    }
    let nonce: [u8; 32] = data[1..33].try_into().expect("32B");
    let ct = &data[33..data.len() - 32];
    let mac: [u8; 32] = data[data.len() - 32..].try_into().expect("32B");
    let mk = message_keys(conversation_key, &nonce);
    let mut verifier = HmacSha256::new_from_slice(&mk.hmac_key)?;
    verifier.update(&nonce);
    verifier.update(ct);
    verifier
        .verify_slice(&mac)
        .map_err(|_| anyhow::anyhow!("MAC inválido"))?;
    let padded = chacha(&mk.chacha_key, &mk.chacha_nonce, ct.to_vec());
    unpad(&padded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }
    fn h32(s: &str) -> [u8; 32] {
        h(s).try_into().expect("32B")
    }

    #[test]
    fn conv_key_vetor_oficial() {
        // nip44.vectors.json valid.get_conversation_key
        let sec1 = h32("315e59ff51cb9209768cf7da80791ddcaae56ac9775eb25b6dee1234bc5d2268");
        let out = conversation_key(
            &sec1,
            "c2f9d9948dc8c7c38321e4b85c8558872eafa0641cd269db76848a6073e69133",
        )
        .unwrap();
        assert_eq!(
            hex::encode(out),
            "3dfef0ce2a4d80a25e7a328accf73448ef67096f65f79588e358d9a0eb9013f1"
        );
    }

    #[test]
    fn conv_key_simetrico() {
        // conv(a,B) == conv(b,A) com sec1/sec2 do exemplo da spec
        let sec1 = h32("0000000000000000000000000000000000000000000000000000000000000001");
        let sec2 = h32("0000000000000000000000000000000000000000000000000000000000000002");
        let secp = secp256k1::Secp256k1::new();
        let pk = |s: &[u8; 32]| {
            let kp = secp256k1::Keypair::from_secret_key(
                &secp,
                &secp256k1::SecretKey::from_slice(s).unwrap(),
            );
            let (x, _) = secp256k1::XOnlyPublicKey::from_keypair(&kp);
            format!("{x}")
        };
        let c1 = conversation_key(&sec1, &pk(&sec2)).unwrap();
        let c2 = conversation_key(&sec2, &pk(&sec1)).unwrap();
        assert_eq!(c1, c2);
        assert_eq!(
            hex::encode(c1),
            "c41c775356fd92eadc63ff5a0dc1da211b268cbea22316767095b2871ea1412d"
        );
    }

    #[test]
    fn message_keys_vetor_oficial() {
        let conv = h32("a1a3d60f3470a8612633924e91febf96dc5366ce130f658b1f0fc652c20b3b54");
        let nonce = h32("e1e6f880560d6d149ed83dcc7e5861ee62a5ee051f7fde9975fe5d25d2a02d72");
        let mk = message_keys(&conv, &nonce);
        assert_eq!(
            hex::encode(mk.chacha_key),
            "f145f3bed47cb70dbeaac07f3a3fe683e822b3715edb7c4fe310829014ce7d76"
        );
        assert_eq!(hex::encode(mk.chacha_nonce), "c4ad129bb01180c0933a160c");
        assert_eq!(
            hex::encode(mk.hmac_key),
            "027c1db445f05e2eee864a0975b0ddef5b7110583c8c192de3732571ca5838c4"
        );
    }

    #[test]
    fn decrypt_vetor_oficial_e_roundtrip() {
        // Payload dourado da spec (sec1=..01, sec2=..02, nonce=..01, plaintext "a")
        let conv = h32("c41c775356fd92eadc63ff5a0dc1da211b268cbea22316767095b2871ea1412d");
        let payload = "AgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABee0G5VSK0/9YypIObAtDKfYEAjD35uVkHyB0F4DwrcNaCXlCWZKaArsGrY6M9wnuTMxWfp1RTN9Xga8no+kF5Vsb";
        assert_eq!(decrypt(payload, &conv).unwrap(), b"a");
        // Criptografar com o mesmo nonce reproduz o payload dourado
        let nonce = h32("0000000000000000000000000000000000000000000000000000000000000001");
        assert_eq!(encrypt_with_nonce(&conv, &nonce, b"a").unwrap(), payload);
        // Roundtrip aleatório
        let p2 = encrypt_random(&conv, "olá frota 🌊".as_bytes()).unwrap();
        assert_eq!(decrypt(&p2, &conv).unwrap(), "olá frota 🌊".as_bytes());
    }

    #[test]
    fn padded_lens_batendo_com_spec() {
        assert_eq!(calc_padded_len(1), 32);
        assert_eq!(calc_padded_len(32), 32);
        assert_eq!(calc_padded_len(33), 64);
        assert_eq!(calc_padded_len(65535), 65536);
        assert_eq!(calc_padded_len(65537), 81920);
    }

    #[test]
    fn mac_tamper_falha() {
        let conv = h32("c41c775356fd92eadc63ff5a0dc1da211b268cbea22316767095b2871ea1412d");
        let mut p = encrypt_random(&conv, b"segredo").unwrap();
        let last = p.pop().unwrap();
        p.push(if last == 'A' { 'B' } else { 'A' });
        assert!(decrypt(&p, &conv).is_err());
    }
}

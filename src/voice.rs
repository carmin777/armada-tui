//! Voz V2 (sinalização): token LiveKit via NIP-29 §"Live audio/video" + NIP-98.
//!
//! Fluxo (spec):
//! 1. anúncio 39000 com tag `livekit` → grupo tem sala AV;
//! 2. `GET https://<relay>/.well-known/nip29/livekit/<group-id>` com
//!    `Authorization: Nostr <base64(evento 27235 com tags u + method)>`;
//! 3. relay responde JWT LiveKit + URL do servidor; `sub` do JWT começa
//!    com a pubkey hex do usuário (64 chars + sufixo aleatório).
//!
//! Detecção de suporte: `GET /.well-known/nip29/livekit` → `204`.
//!
//! Honestidade de escopo: isto é a SINALIZAÇÃO (token + presença 39004).
//! Áudio WebRTC de verdade continua fora do escopo do terminal
//! (precisaria SDK LiveKit + microfone); o JWT nunca é logado.

use std::time::Duration;

/// Resposta do token endpoint (campos tolerantes: relays variam nomes).
#[derive(Debug, Clone)]
pub struct VoiceToken {
    /// JWT LiveKit (tratar como segredo: nunca logar inteiro).
    pub jwt: String,
    /// URL do servidor LiveKit (`wss://…`), pode vir vazia.
    pub server_url: String,
    /// `sub` do JWT (identidade; deve começar com a pubkey hex).
    pub identity: String,
    /// `exp` do JWT (unix), se presente.
    pub exp: Option<i64>,
}

/// Valida group-id p/ uso em path (sem injeção `../`, sem query).
pub fn check_group_id(gid: &str) -> anyhow::Result<()> {
    if gid.is_empty() || gid.len() > 128 {
        anyhow::bail!("group-id inválido");
    }
    if !gid
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        anyhow::bail!("group-id com caracteres inválidos");
    }
    Ok(())
}

/// `wss://host[:porta]` → base `https://host[:porta]` (path descartado:
/// well-known mora na raiz do relay).
pub fn http_base(relay_ws: &str) -> anyhow::Result<String> {
    let u: url::Url = relay_ws.parse()?;
    let https = match u.scheme() {
        "wss" => "https",
        "ws" => "http",
        _ => anyhow::bail!("relay deve ser ws(s)"),
    };
    let host = u
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("relay sem host"))?;
    let base = match u.port() {
        Some(p) => format!("{https}://{host}:{p}"),
        None => format!("{https}://{host}"),
    };
    crate::netpolicy::check_http_url(&base)?;
    Ok(base)
}

/// URL do token endpoint p/ um grupo.
pub fn token_url(relay_ws: &str, group_id: &str) -> anyhow::Result<String> {
    check_group_id(group_id)?;
    let url = format!(
        "{}/.well-known/nip29/livekit/{group_id}",
        http_base(relay_ws)?
    );
    crate::netpolicy::check_http_url(&url)?;
    Ok(url)
}

/// URL de detecção de suporte AV do relay.
pub fn support_url(relay_ws: &str) -> anyhow::Result<String> {
    let url = format!("{}/.well-known/nip29/livekit", http_base(relay_ws)?);
    crate::netpolicy::check_http_url(&url)?;
    Ok(url)
}

/// Monta o header `Authorization: Nostr …` (NIP-98): evento 27235 efêmero
/// com tags `u` (URL exata) + `method`, conteúdo vazio, assinado.
pub fn nip98_header(url: &str, method: &str, secret: &[u8; 32]) -> anyhow::Result<String> {
    let ev = crate::nostr::sign_event(
        secret,
        27235,
        vec![
            vec!["u".to_string(), url.to_string()],
            vec!["method".to_string(), method.to_string()],
        ],
        "",
    )?;
    let raw = serde_json::to_string(&ev)?;
    use base64::Engine;
    Ok(format!(
        "Nostr {}",
        base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
    ))
}

/// Suporte AV do relay? `204` = sim, `404` = não; outros status = erro.
pub fn support_probe(relay_ws: &str) -> anyhow::Result<bool> {
    let url = support_url(relay_ws)?;
    let agent: ureq::Agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .redirects(0)
        .build();
    match agent.get(&url).call() {
        Ok(res) => Ok(res.status() == 204),
        Err(ureq::Error::Status(404, _)) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Pede o JWT LiveKit do grupo (precisa estar logado: NIP-98 assina com
/// a chave da sessão). `403` = relay negou (não-membro etc.) — isso TAMBÉM
/// é resposta válida do controle de acesso, não bug.
pub fn fetch_token(
    relay_ws: &str,
    group_id: &str,
    keys: &crate::nostr::Keys,
) -> anyhow::Result<VoiceToken> {
    let url = token_url(relay_ws, group_id)?;
    let raw: [u8; 32] = *keys.secret;
    let auth = nip98_header(&url, "GET", &raw)?;
    let agent: ureq::Agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .redirects(0)
        .build();
    let res = agent
        .get(&url)
        .set("Authorization", &auth)
        .call()
        .map_err(|e| {
            if let ureq::Error::Status(code, _) = e {
                return anyhow::anyhow!("token endpoint HTTP {code}");
            }
            anyhow::anyhow!(e)
        })?;
    let body = res.into_string()?;
    let (jwt, server_url) = parse_token_body(&body)?;
    let (identity, exp) = jwt_claims(&jwt)?;
    if !identity.starts_with(&keys.pubkey_hex) {
        anyhow::bail!("JWT com sub inesperado (não começa com minha pubkey)");
    }
    Ok(VoiceToken {
        jwt,
        server_url,
        identity,
        exp,
    })
}

/// Corpo do token endpoint: JSON `{token|jwt, url|server_url|…}` ou JWT puro.
pub fn parse_token_body(body: &str) -> anyhow::Result<(String, String)> {
    let t = body.trim();
    if t.split('.').count() == 3 && !t.starts_with('{') {
        return Ok((t.to_string(), String::new()));
    }
    let v: serde_json::Value = serde_json::from_str(t)?;
    let jwt = ["token", "jwt", "access_token", "accessToken"]
        .into_iter()
        .filter_map(|k| v.get(k)?.as_str())
        .next()
        .ok_or_else(|| anyhow::anyhow!("resposta sem JWT"))?
        .to_string();
    let server_url = [
        "url",
        "server_url",
        "serverUrl",
        "livekit_url",
        "livekitUrl",
    ]
    .into_iter()
    .filter_map(|k| v.get(k)?.as_str())
    .next()
    .unwrap_or("")
    .to_string();
    Ok((jwt, server_url))
}

/// Extrai `(sub, exp)` do payload do JWT (base64url, sem padding).
/// Não verifica assinatura (transporte é TLS + NIP-98); valida identidade.
pub fn jwt_claims(jwt: &str) -> anyhow::Result<(String, Option<i64>)> {
    use base64::Engine;
    let mut parts = jwt.split('.');
    let (_h, p, _s) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(h), Some(p), Some(s), None) => (h, p, s),
        _ => anyhow::bail!("JWT malformado"),
    };
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p)?;
    let v: serde_json::Value = serde_json::from_slice(&payload)?;
    let sub = v
        .get("sub")
        .or_else(|| v.get("identity"))
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("JWT sem sub"))?
        .to_string();
    Ok((sub, v.get("exp").and_then(|e| e.as_i64())))
}

/// Resumo seguro p/ status (nunca o JWT): `sub[0:8]… + exp`.
pub fn short(t: &VoiceToken) -> String {
    let id: String = t.identity.chars().take(8).collect();
    match t.exp {
        Some(e) => format!("{id}… exp {e}"),
        None => format!("{id}… sem exp"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grupo_invalido_rejeitado() {
        assert!(check_group_id("").is_err());
        assert!(check_group_id("../x").is_err());
        assert!(check_group_id("a/b").is_err());
        assert!(check_group_id("a?b").is_err());
        assert!(check_group_id("geral").is_ok());
        assert!(check_group_id("relay-tools-test_group.v2-1").is_ok());
    }

    #[test]
    fn base_http_converte() {
        // netpolicy exige DNS real: só hosts públicos de verdade.
        assert_eq!(
            http_base("wss://relay.ditto.pub").unwrap(),
            "https://relay.ditto.pub"
        );
        assert_eq!(
            http_base("wss://relay.ditto.pub:8443/relay").unwrap(),
            "https://relay.ditto.pub:8443"
        );
        assert!(http_base("https://relay.ditto.pub").is_err());
        assert!(http_base("wss://x.test").is_err());
    }

    #[test]
    fn url_token_monta() {
        let u = token_url("wss://relay.ditto.pub", "geral").unwrap();
        assert_eq!(u, "https://relay.ditto.pub/.well-known/nip29/livekit/geral");
        assert!(token_url("wss://relay.ditto.pub", "../x").is_err());
    }

    #[test]
    fn nip98_tem_forma() {
        let me = crate::nostr::generate().unwrap();
        let raw: [u8; 32] = *me.secret;
        let h = nip98_header("https://r.test/.well-known/nip29/livekit/g", "GET", &raw).unwrap();
        assert!(h.starts_with("Nostr "));
        use base64::Engine;
        let ev: serde_json::Value = serde_json::from_slice(
            &base64::engine::general_purpose::STANDARD
                .decode(h["Nostr ".len()..].trim())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(ev["kind"], 27235);
        assert_eq!(ev["content"], "");
        assert_eq!(ev["pubkey"], me.pubkey_hex);
        let tags = ev["tags"].as_array().unwrap();
        assert!(tags.contains(&serde_json::json!([
            "u",
            "https://r.test/.well-known/nip29/livekit/g"
        ])));
        assert!(tags.contains(&serde_json::json!(["method", "GET"])));
        // id/sig conferem (recalcula id e verifica schnorr).
        let secp = secp256k1::Secp256k1::new();
        let commit = serde_json::json!([0, ev["pubkey"], ev["created_at"], 27235, ev["tags"], ""]);
        use sha2::Digest;
        let digest = sha2::Sha256::digest(serde_json::to_string(&commit).unwrap().as_bytes());
        assert_eq!(hex::encode(digest), ev["id"].as_str().unwrap());
        let pk =
            secp256k1::XOnlyPublicKey::from_slice(&hex::decode(me.pubkey_hex).unwrap()).unwrap();
        let msg = secp256k1::Message::from_digest(digest.into());
        let sig = secp256k1::schnorr::Signature::from_slice(
            &hex::decode(ev["sig"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        secp.verify_schnorr(&sig, &msg, &pk).unwrap();
    }

    #[test]
    fn corpo_token_tolerante() {
        let (j, u) = parse_token_body(r#"{"token":"a.b.c","url":"wss://lk.x"}"#).unwrap();
        assert_eq!((j, u), ("a.b.c".into(), "wss://lk.x".into()));
        let (j, u) = parse_token_body(r#"{"jwt":"a.b.c","server_url":"wss://y"}"#).unwrap();
        assert_eq!((j, u), ("a.b.c".into(), "wss://y".into()));
        let (j, u) = parse_token_body("aaa.bbb.ccc").unwrap();
        assert_eq!((j, u), ("aaa.bbb.ccc".into(), String::new()));
        assert!(parse_token_body(r#"{"nada":1}"#).is_err());
    }

    fn fake_jwt(sub: &str) -> String {
        use base64::Engine;
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!(
            "{}.{}.{}",
            e.encode(r#"{"alg":"none"}"#),
            e.encode(format!(r#"{{"sub":"{sub}","exp":1999999999}}"#)),
            e.encode("sig")
        )
    }

    #[test]
    fn claims_e_identidade() {
        let me = crate::nostr::generate().unwrap();
        let sub = format!("{}abcd", me.pubkey_hex);
        let (got, exp) = jwt_claims(&fake_jwt(&sub)).unwrap();
        assert_eq!(got, sub);
        assert_eq!(exp, Some(1999999999));
        assert!(jwt_claims("só.duas").is_err());
    }
}

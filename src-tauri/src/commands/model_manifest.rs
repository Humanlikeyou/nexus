//! The model manifest that ships inside the app (`resources/model-manifest.json`, embedded with
//! `include_bytes!`). It is verified before any field is read: a compiled-in SHA-256 pin today
//! (`PinnedSha256`), with an Ed25519 verifier (`Ed25519Verifier`, ring) ready to replace it. Any
//! verify, parse or schema failure is the fixed error `manifest_invalid`; unverified data is never
//! returned. Model names are the live ones only. No network, no file IO.

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const MANIFEST_INVALID: &str = "manifest_invalid";
const MANIFEST_BYTES: &[u8] = include_bytes!("../../resources/model-manifest.json");
/// SHA-256 (hex) of `resources/model-manifest.json`. Update together with the file.
pub(crate) const MANIFEST_SHA256: &str =
    "35ca870d30a41301a745987f3ea97d080330797cb738044f047f739bdfe9778d";

const TIER_IDS: [&str; 3] = ["compact", "standard", "large"];
const ROLES: [&str; 3] = ["fast", "deep", "embed"];
/// No single model file may claim more than 80 GB.
pub(crate) const MAX_MODEL_BYTES: u64 = 80 * 1_000_000_000;
const MAX_MANIFEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub schema: u32,
    #[allow(dead_code)]
    pub issued: String,
    pub tiers: Vec<ManifestTier>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ManifestTier {
    pub id: String,
    pub name: String,
    pub min_ram_gb: u32,
    pub models: Vec<ManifestModel>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestModel {
    pub role: String,
    pub name: String,
    /// None until the real size is known (never invented).
    #[serde(default)]
    pub bytes: Option<u64>,
    /// Lowercase hex SHA-256 of the file; None until known.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Empty until Taisun logs a host.
    pub url: String,
}

/// Decides whether manifest bytes are trusted. Swap `PinnedSha256` for `Ed25519Verifier` later.
pub(crate) trait ManifestVerifier: Sync {
    fn verify(&self, bytes: &[u8]) -> bool;
}

pub(crate) struct PinnedSha256(pub &'static str);

impl ManifestVerifier for PinnedSha256 {
    fn verify(&self, bytes: &[u8]) -> bool {
        hex::encode(Sha256::digest(bytes)) == self.0
    }
}

/// Detached Ed25519 signature over the manifest bytes. Not used in production yet.
#[allow(dead_code)]
pub(crate) struct Ed25519Verifier {
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

impl ManifestVerifier for Ed25519Verifier {
    fn verify(&self, bytes: &[u8]) -> bool {
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &self.public_key)
            .verify(bytes, &self.signature)
            .is_ok()
    }
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 96
        && !s.contains("gemma4-12b")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':'))
}

/// Empty, or https with a domain host (no IP literal), or http only for exactly 127.0.0.1
/// (test fixtures; production also refuses http). Never userinfo.
fn url_ok(u: &str) -> bool {
    if u.is_empty() {
        return true;
    }
    if u.len() > 2048 || !u.is_ascii() {
        return false;
    }
    let Ok(p) = reqwest::Url::parse(u) else {
        return false;
    };
    if !p.username().is_empty() || p.password().is_some() {
        return false;
    }
    match p.scheme() {
        "https" => p.domain().is_some(),
        "http" => p.host_str() == Some("127.0.0.1") && p.domain().is_none(),
        _ => false,
    }
}

fn validate(m: &Manifest) -> bool {
    if m.schema != 1 || m.tiers.is_empty() || m.tiers.len() > TIER_IDS.len() {
        return false;
    }
    let mut seen = Vec::new();
    for t in &m.tiers {
        if !TIER_IDS.contains(&t.id.as_str())
            || seen.contains(&t.id)
            || t.name.is_empty()
            || t.name.len() > 32
            || t.min_ram_gb == 0
            || t.min_ram_gb > 4096
            || t.models.is_empty()
            || t.models.len() > 8
        {
            return false;
        }
        seen.push(t.id.clone());
        for md in &t.models {
            let url_ok = url_ok(&md.url);
            let sha_ok = md.sha256.as_deref().is_none_or(is_hex64);
            if !ROLES.contains(&md.role.as_str())
                || !valid_name(&md.name)
                || !url_ok
                || !sha_ok
                || md.bytes == Some(0)
                || md.bytes.is_some_and(|b| b > MAX_MODEL_BYTES)
            {
                return false;
            }
        }
    }
    true
}

/// Verify, then parse and schema-check. Every failure is `manifest_invalid`.
pub(crate) fn load_with(
    bytes: &[u8],
    verifier: &dyn ManifestVerifier,
) -> Result<Manifest, &'static str> {
    if bytes.len() > MAX_MANIFEST_BYTES || !verifier.verify(bytes) {
        return Err(MANIFEST_INVALID);
    }
    let m: Manifest = serde_json::from_slice(bytes).map_err(|_| MANIFEST_INVALID)?;
    if validate(&m) {
        Ok(m)
    } else {
        Err(MANIFEST_INVALID)
    }
}

/// The shipped manifest, checked against the compiled-in pin.
pub(crate) fn load() -> Result<Manifest, &'static str> {
    load_with(MANIFEST_BYTES, &PinnedSha256(MANIFEST_SHA256))
}

pub(crate) fn shipped_bytes() -> &'static [u8] {
    MANIFEST_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    struct AcceptAll;
    impl ManifestVerifier for AcceptAll {
        fn verify(&self, _: &[u8]) -> bool {
            true
        }
    }

    #[test]
    fn shipped_manifest_verifies_and_has_live_names_only() {
        let m = load().unwrap();
        assert_eq!(m.tiers.len(), 3);
        let names: Vec<&str> = m
            .tiers
            .iter()
            .flat_map(|t| t.models.iter().map(|x| x.name.as_str()))
            .collect();
        for live in [
            "gemma4:latest",
            "qwen3.8-uncensored:latest",
            "nomic-embed-text:latest",
        ] {
            assert!(names.contains(&live), "{live}");
        }
        assert!(!String::from_utf8_lossy(MANIFEST_BYTES).contains("gemma4-12b-uncensored"));
        // No invented sizes, digests or hosts.
        assert!(m
            .tiers
            .iter()
            .flat_map(|t| &t.models)
            .all(|x| x.bytes.is_none() && x.sha256.is_none() && x.url.is_empty()));
    }

    #[test]
    fn flipped_byte_is_invalid() {
        let mut b = MANIFEST_BYTES.to_vec();
        let i = b.iter().position(|c| *c == b'8').unwrap();
        b[i] = b'9';
        assert_eq!(
            load_with(&b, &PinnedSha256(MANIFEST_SHA256)).unwrap_err(),
            MANIFEST_INVALID
        );
    }

    #[test]
    fn truncated_is_invalid_even_if_verifier_accepts() {
        let b = &MANIFEST_BYTES[..MANIFEST_BYTES.len() / 2];
        assert_eq!(load_with(b, &AcceptAll).unwrap_err(), MANIFEST_INVALID);
        assert_eq!(
            load_with(b, &PinnedSha256(MANIFEST_SHA256)).unwrap_err(),
            MANIFEST_INVALID
        );
        assert_eq!(load_with(b"", &AcceptAll).unwrap_err(), MANIFEST_INVALID);
    }

    #[test]
    fn bad_schema_is_invalid_even_if_verifier_accepts() {
        let text = String::from_utf8(MANIFEST_BYTES.to_vec()).unwrap();
        let cases = [
            text.replace("\"schema\": 1", "\"schema\": 2"),
            text.replace("\"compact\"", "\"tiny\""),
            text.replace("gemma4:latest", "gemma4-12b-uncensored:latest"),
            text.replace("\"url\": \"\"", "\"url\": \"http://example.invalid/x\""),
            text.replace("\"url\": \"\"", "\"url\": \"http://127.0.0.1.evil.com/x\""),
            text.replace("\"url\": \"\"", "\"url\": \"http://127.0.0.1@evil.com/x\""),
            text.replace("\"url\": \"\"", "\"url\": \"https://1.2.3.4/x\""),
            text.replace("\"url\": \"\"", "\"url\": \"https://u:p@example.com/x\""),
            text.replace("\"bytes\": null", "\"bytes\": 80000000001"),
            text.replace("\"sha256\": null", "\"sha256\": \"../../etc/passwd\""),
            text.replacen("\"role\": \"fast\"", "\"role\": \"boss\"", 1),
            text.replacen("{\n  \"schema\"", "{\n  \"extra\": 1,\n  \"schema\"", 1),
        ];
        for c in cases {
            assert_eq!(
                load_with(c.as_bytes(), &AcceptAll).unwrap_err(),
                MANIFEST_INVALID,
                "{c}"
            );
        }
    }

    #[test]
    fn good_urls_are_accepted() {
        let text = String::from_utf8(MANIFEST_BYTES.to_vec()).unwrap();
        for u in [
            "https://models.example.com/a.bin",
            "http://127.0.0.1:8080/a",
        ] {
            let t = text.replace("\"url\": \"\"", &format!("\"url\": \"{u}\""));
            assert!(load_with(t.as_bytes(), &AcceptAll).is_ok(), "{u}");
        }
        let t = text.replace("\"bytes\": null", "\"bytes\": 80000000000");
        assert!(load_with(t.as_bytes(), &AcceptAll).is_ok());
    }

    #[test]
    fn ed25519_verifier_accepts_good_and_rejects_tampered() {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let kp = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let sig = kp.sign(MANIFEST_BYTES).as_ref().to_vec();
        let v = Ed25519Verifier {
            public_key: kp.public_key().as_ref().to_vec(),
            signature: sig,
        };
        assert!(load_with(MANIFEST_BYTES, &v).is_ok());
        let mut b = MANIFEST_BYTES.to_vec();
        b[10] ^= 1;
        assert_eq!(load_with(&b, &v).unwrap_err(), MANIFEST_INVALID);
        let other =
            Ed25519KeyPair::from_pkcs8(Ed25519KeyPair::generate_pkcs8(&rng).unwrap().as_ref())
                .unwrap();
        let wrong = Ed25519Verifier {
            public_key: other.public_key().as_ref().to_vec(),
            signature: v.signature.clone(),
        };
        assert!(load_with(MANIFEST_BYTES, &wrong).is_err());
    }
}

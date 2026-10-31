//! Generic webhook authentication; the route and atomic occurrence/job
//! acceptance are separate so an authenticated request is never an early 202.

use axum::http::StatusCode;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::api::error::ApiError;

pub const MAX_WEBHOOK_BYTES: usize = 64 * 1024;
/// GitHub deliveries carry richer JSON than the generic envelope; keep a
/// separate ceiling instead of silently truncating or accepting unbounded input.
pub const MAX_GITHUB_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    event_id: String,
    payload: Value,
}

#[derive(Debug)]
pub struct VerifiedWebhook {
    pub event_id: String,
    pub payload: Value,
    pub payload_hash: String,
}

#[derive(Debug)]
pub struct VerifiedGithub {
    pub event: String,
    pub delivery_id: String,
    pub payload: Value,
    pub payload_hash: String,
}

fn hex_digest(encoded: &str) -> Result<[u8; 32], ApiError> {
    if encoded.len() != 64 {
        return Err(unauthorized());
    }
    let mut digest = [0u8; 32];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let high = (pair[0] as char).to_digit(16).ok_or_else(unauthorized)?;
        let low = (pair[1] as char).to_digit(16).ok_or_else(unauthorized)?;
        digest[index] = (high * 16 + low) as u8;
    }
    Ok(digest)
}

fn valid_github_header(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && !value.chars().any(char::is_control)
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

/// GitHub signs the raw body only; the delivery and event names are headers.
/// Deduplication therefore also matches normalized payloads, so changing an
/// unsigned delivery header cannot turn one captured delivery into new work.
pub fn verify_github(
    secret: &[u8],
    event: &str,
    delivery_id: &str,
    signature: &str,
    body: &[u8],
) -> Result<VerifiedGithub, ApiError> {
    if body.len() > MAX_GITHUB_BYTES {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "webhook_too_large",
            "GitHub body exceeds 1 MiB",
        ));
    }
    if secret.len() < 32
        || !valid_github_header(event, 64)
        || !valid_github_header(delivery_id, 128)
    {
        return Err(unauthorized());
    }
    let encoded = signature.strip_prefix("sha256=").ok_or_else(unauthorized)?;
    let digest = hex_digest(encoded)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).map_err(|_| unauthorized())?;
    mac.update(body);
    mac.verify_slice(&digest).map_err(|_| unauthorized())?;
    let payload: Value = serde_json::from_slice(body)
        .map_err(|_| ApiError::invalid_input("GitHub webhook body must be JSON"))?;
    let canonical = crate::canonical_json::to_vec(&payload)
        .map_err(|_| ApiError::invalid_input("invalid GitHub payload"))?;
    Ok(VerifiedGithub {
        event: event.to_string(),
        delivery_id: delivery_id.to_string(),
        payload,
        payload_hash: format!("{:x}", Sha256::digest(canonical)),
    })
}

fn unauthorized() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "webhook_unauthorized",
        "Webhook authentication failed",
    )
}

pub fn verify(
    secret: &[u8],
    timestamp: &str,
    signature: &str,
    body: &[u8],
    now: i64,
) -> Result<VerifiedWebhook, ApiError> {
    if body.len() > MAX_WEBHOOK_BYTES {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "webhook_too_large",
            "Webhook body exceeds 64 KiB",
        ));
    }
    let signed_at = timestamp.parse::<i64>().map_err(|_| unauthorized())?;
    if (i128::from(now) - i128::from(signed_at)).abs() > 300 || secret.len() < 32 {
        return Err(unauthorized());
    }
    let encoded = signature.strip_prefix("sha256=").ok_or_else(unauthorized)?;
    if encoded.len() != 64 {
        return Err(unauthorized());
    }
    let mut digest = [0u8; 32];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let high = (pair[0] as char).to_digit(16).ok_or_else(unauthorized)?;
        let low = (pair[1] as char).to_digit(16).ok_or_else(unauthorized)?;
        digest[index] = (high * 16 + low) as u8;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).map_err(|_| unauthorized())?;
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    mac.verify_slice(&digest).map_err(|_| unauthorized())?;
    let envelope: Envelope = serde_json::from_slice(body)
        .map_err(|_| ApiError::invalid_input("webhook body must contain event_id and payload"))?;
    if envelope.event_id.is_empty()
        || envelope.event_id.len() > 128
        || envelope.event_id.chars().any(char::is_control)
    {
        return Err(ApiError::invalid_input(
            "event_id must contain 1–128 bytes without control characters",
        ));
    }
    // Note: an unsigned event-id header lets a captured valid request create
    // fresh dedup keys. Identity comes only from this authenticated JSON body.
    let canonical = crate::canonical_json::to_vec(
        &serde_json::json!({"event_id":envelope.event_id,"payload":envelope.payload}),
    )
    .map_err(|_| ApiError::invalid_input("invalid webhook payload"))?;
    Ok(VerifiedWebhook {
        event_id: envelope.event_id,
        payload: envelope.payload,
        payload_hash: format!("{:x}", Sha256::digest(canonical)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const SECRET: &[u8] = b"fixture-secret-key-at-least-32-bytes";
    const TIME: &str = "1700000000";
    const BODY: &[u8] = br#"{"event_id":"delivery-1","payload":{"text":"fixture"}}"#;
    // Independently generated with Python's standard-library hmac/hashlib.
    const SIGNATURE: &str =
        "sha256=5de76afcbcea626f15dd6e2ab31b040770bc3461052e5960c6f21aebf2b86c27";

    fn sign(body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(SECRET).unwrap();
        mac.update(TIME.as_bytes());
        mac.update(b".");
        mac.update(body);
        format!("sha256={:x}", mac.finalize().into_bytes())
    }

    #[test]
    fn verifies_independent_vector_and_extracts_signed_identity() {
        let result = verify(SECRET, TIME, SIGNATURE, BODY, 1_700_000_000).unwrap();
        assert_eq!(result.event_id, "delivery-1");
        assert_eq!(result.payload["text"], "fixture");
    }
    #[test]
    fn changing_identity_or_body_invalidates_a_captured_signature() {
        let changed = br#"{"event_id":"delivery-2","payload":{"text":"fixture"}}"#;
        assert_eq!(
            verify(SECRET, TIME, SIGNATURE, changed, 1_700_000_000)
                .unwrap_err()
                .code(),
            "webhook_unauthorized"
        );
        assert!(verify(
            b"different-fixture-secret-at-least-32-bytes",
            TIME,
            SIGNATURE,
            BODY,
            1_700_000_000
        )
        .is_err());
    }
    #[test]
    fn enforces_past_and_future_clock_skew_without_integer_overflow() {
        for now in [1_700_000_301, 1_699_999_699, i64::MIN, i64::MAX] {
            assert!(verify(SECRET, TIME, SIGNATURE, BODY, now).is_err());
        }
        assert!(verify(SECRET, TIME, SIGNATURE, BODY, 1_700_000_300).is_ok());
        assert!(verify(SECRET, TIME, SIGNATURE, BODY, 1_699_999_700).is_ok());
    }
    #[test]
    fn dedup_digest_ignores_json_whitespace_but_rejects_changed_content() {
        let alternate = br#"{ "payload": { "text": "fixture" }, "event_id": "delivery-1" }"#;
        let original = verify(SECRET, TIME, SIGNATURE, BODY, 1_700_000_000).unwrap();
        assert_eq!(
            original.payload_hash,
            verify(SECRET, TIME, &sign(alternate), alternate, 1_700_000_000)
                .unwrap()
                .payload_hash
        );
        let changed = br#"{"event_id":"delivery-1","payload":{"text":"different"}}"#;
        assert_ne!(
            original.payload_hash,
            verify(SECRET, TIME, &sign(changed), changed, 1_700_000_000)
                .unwrap()
                .payload_hash
        );
    }
    #[test]
    fn malformed_and_oversized_requests_are_rejected() {
        let oversized = vec![b' '; MAX_WEBHOOK_BYTES + 1];
        assert_eq!(
            verify(SECRET, TIME, SIGNATURE, &oversized, 1_700_000_000)
                .unwrap_err()
                .status_code(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let missing = br#"{"payload":{}}"#;
        assert_eq!(
            verify(SECRET, TIME, &sign(missing), missing, 1_700_000_000)
                .unwrap_err()
                .code(),
            "invalid_input"
        );
        assert!(verify(SECRET, TIME, "sha256=not-hex", BODY, 1_700_000_000).is_err());
    }

    const GITHUB_BODY: &[u8] = br#"{"zen":"Keep it logically awesome.","hook_id":1}"#;
    // Independently generated with Python's standard-library hmac/hashlib.
    const GITHUB_SIGNATURE: &str =
        "sha256=90cb9f3cf8c2b689016a1ad2b2e70f2aaefe908463c7a2d791e53d61356b26d2";
    // SHA-256 of the normalized object (sorted keys), not the raw bytes.
    const GITHUB_HASH: &str = "bdf615153929e27017e6fd4de3065872bfa0485debf44238bed21c9dff4d335b";

    fn sign_github(body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(SECRET).unwrap();
        mac.update(body);
        format!("sha256={:x}", mac.finalize().into_bytes())
    }

    #[test]
    fn github_native_signature_and_delivery_identity_are_verified() {
        let result = verify_github(
            SECRET,
            "ping",
            "72d3162e-cc78-11e3-81ab-4c9367dc0958",
            GITHUB_SIGNATURE,
            GITHUB_BODY,
        )
        .unwrap();
        assert_eq!(result.event, "ping");
        assert_eq!(result.payload["hook_id"], 1);
        assert_eq!(result.payload_hash, GITHUB_HASH);
        // Changing the unsigned delivery header cannot create a new content
        // identity; the normalized body hash is what enqueue dedups on.
        let changed = verify_github(
            SECRET,
            "ping",
            "92d3162e-cc78-11e3-81ab-4c9367dc0958",
            GITHUB_SIGNATURE,
            GITHUB_BODY,
        )
        .unwrap();
        assert_eq!(changed.payload_hash, result.payload_hash);
        assert_ne!(changed.delivery_id, result.delivery_id);
    }

    #[test]
    fn github_rejects_wrong_signature_secret_headers_and_non_json() {
        for signature in [
            "90cb9f3cf8c2b689016a1ad2b2e70f2aaefe908463c7a2d791e53d61356b26d2",
            "sha256=not-hex",
            "sha256=90cb9f3c",
        ] {
            assert!(verify_github(SECRET, "ping", "delivery-1", signature, GITHUB_BODY).is_err());
        }
        assert!(verify_github(
            b"different-fixture-secret-at-least-32-bytes",
            "ping",
            "delivery-1",
            GITHUB_SIGNATURE,
            GITHUB_BODY
        )
        .is_err());
        for event in [String::new(), "pull request".to_string(), "x".repeat(65)] {
            assert_eq!(
                verify_github(SECRET, &event, "delivery-1", GITHUB_SIGNATURE, GITHUB_BODY)
                    .unwrap_err()
                    .code(),
                "webhook_unauthorized"
            );
        }
        let malformed = b"not json";
        assert_eq!(
            verify_github(
                SECRET,
                "push",
                "delivery-1",
                &sign_github(malformed),
                malformed
            )
            .unwrap_err()
            .code(),
            "invalid_input"
        );
        let oversized = vec![b' '; MAX_GITHUB_BYTES + 1];
        assert_eq!(
            verify_github(
                SECRET,
                "push",
                "delivery-1",
                &sign_github(&oversized),
                &oversized
            )
            .unwrap_err()
            .status_code(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[test]
    fn github_canonical_hash_ignores_key_order_and_spacing() {
        let alternate = br#"{ "hook_id": 1, "zen": "Keep it logically awesome." }"#;
        let first =
            verify_github(SECRET, "ping", "delivery-1", GITHUB_SIGNATURE, GITHUB_BODY).unwrap();
        let second = verify_github(
            SECRET,
            "ping",
            "delivery-2",
            &sign_github(alternate),
            alternate,
        )
        .unwrap();
        assert_eq!(first.payload_hash, second.payload_hash);
    }

    #[test]
    fn generic_webhook_digest_sorts_nested_payload_objects() {
        let first = br#"{"event_id":"delivery-1","payload":{"z":{"b":2,"a":1},"a":[3,2]}}"#;
        let second = br#"{"payload":{"a":[3,2],"z":{"a":1,"b":2}},"event_id":"delivery-1"}"#;
        let a = verify(SECRET, TIME, &sign(first), first, 1_700_000_000).unwrap();
        let b = verify(SECRET, TIME, &sign(second), second, 1_700_000_000).unwrap();
        assert_eq!(a.payload_hash, b.payload_hash);
        let sorted = br#"{"event_id":"delivery-1","payload":{"a":[3,2],"z":{"a":1,"b":2}}}"#;
        assert_eq!(a.payload_hash, format!("{:x}", Sha256::digest(sorted)));
    }
}

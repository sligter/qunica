//! Stable identity bytes, not a serializer for provider messages.

use serde_json::Value;

pub(crate) fn to_vec(value: &Value) -> serde_json::Result<Vec<u8>> {
    // Note: Rig enables preserve_order process-wide. Durable identity hashes
    // must retain the old sorted-object encoding without mutating opaque history.
    let mut canonical = value.clone();
    canonical.sort_all_objects();
    serde_json::to_vec(&canonical)
}

/// Most floats a payload may carry for [`legacy_variants`] to enumerate them.
pub(crate) const LEGACY_FLOAT_LIMIT: usize = 3;

/// How far, in ULPs, the pre-pin legacy parser can land from the exact double.
/// Measured at 2 over ~10M probes (long mantissas, subnormals, huge exponents);
/// one more for margin.
const LEGACY_DRIFT_ULPS: u64 = 3;

/// Identity bytes for every double the legacy parser could have read for each
/// float in `value`, the exact reading included.
///
/// For deduplication against rows hashed before the exact parser was pinned:
/// it errs toward recognising a duplicate, so it must never authorize an
/// action. Returns `None` past [`LEGACY_FLOAT_LIMIT`] floats.
// Note: enumerating neighbours instead of re-implementing the legacy parser
// keeps no copy of serde_json's old algorithm, at ≤ 7^3 encodings per payload.
pub(crate) fn legacy_variants(value: &Value) -> Option<Vec<Vec<u8>>> {
    let mut work = value.clone();
    work.sort_all_objects();
    let mut floats = Vec::new();
    collect_floats(&work, String::new(), &mut floats);
    if floats.len() > LEGACY_FLOAT_LIMIT {
        return None;
    }
    let mut out = Vec::new();
    vary(&mut work, &floats, &mut out);
    Some(out)
}

fn collect_floats(value: &Value, pointer: String, out: &mut Vec<String>) {
    match value {
        Value::Number(number) if number.is_f64() => out.push(pointer),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_floats(item, format!("{pointer}/{index}"), out);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                let key = key.replace('~', "~0").replace('/', "~1");
                collect_floats(item, format!("{pointer}/{key}"), out);
            }
        }
        _ => {}
    }
}

fn vary(work: &mut Value, floats: &[String], out: &mut Vec<Vec<u8>>) {
    let Some((pointer, rest)) = floats.split_first() else {
        if let Ok(bytes) = serde_json::to_vec(work) {
            out.push(bytes);
        }
        return;
    };
    let Some(exact) = work.pointer(pointer).and_then(Value::as_f64) else {
        return;
    };
    let bits = exact.to_bits();
    for neighbour in bits.saturating_sub(LEGACY_DRIFT_ULPS)..=bits + LEGACY_DRIFT_ULPS {
        let candidate = f64::from_bits(neighbour);
        // Same sign and finite: the legacy parser never flips either.
        let Some(number) = serde_json::Number::from_f64(candidate)
            .filter(|_| candidate.is_sign_negative() == exact.is_sign_negative())
        else {
            continue;
        };
        if let Some(slot) = work.pointer_mut(pointer) {
            *slot = Value::Number(number);
        }
        vary(work, rest, out);
    }
    if let Some(slot) = work.pointer_mut(pointer) {
        *slot = serde_json::json!(exact);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_objects_are_sorted_without_mutating_input_or_array_order() {
        let value: Value =
            serde_json::from_str(r#"{"z":{"b":2,"a":1},"a":[{"z":true,"a":null},3,2]}"#).unwrap();
        let original = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            to_vec(&value).unwrap(),
            br#"{"a":[{"a":null,"z":true},3,2],"z":{"a":1,"b":2}}"#
        );
        assert_eq!(serde_json::to_vec(&value).unwrap(), original);
    }

    #[test]
    fn scalar_and_number_encoding_keeps_legacy_vectors() {
        for encoded in ["null", "true", "1", "1.25", "-0.0", "18446744073709551615"] {
            let value: Value = serde_json::from_str(encoded).unwrap();
            assert_eq!(to_vec(&value).unwrap(), encoded.as_bytes());
        }
    }

    // Lexemes the legacy parser rounds 1–2 ULPs off. If this fails, the
    // `float_roundtrip` pin was dropped and Rig builds disagree on digests.
    const LEGACY_MISROUNDED: [&str; 3] = [
        "8.971135692166839e-150",
        "1.8339321837204014e-258",
        "1.603964615428183e+143",
    ];

    #[test]
    fn number_parsing_is_exact_in_every_build() {
        for lexeme in LEGACY_MISROUNDED {
            let parsed: f64 = serde_json::from_str(lexeme).unwrap();
            assert_eq!(parsed, lexeme.parse::<f64>().unwrap(), "{lexeme}");
            assert_eq!(serde_json::to_string(&parsed).unwrap(), lexeme);
        }
    }

    #[test]
    fn legacy_variants_cover_the_legacy_reading_and_leave_the_rest_exact() {
        let exact = serde_json::json!({"z":{"ratio":8.971135692166839e-150},"n":[1,"x"]});
        let legacy = serde_json::json!({"z":{"ratio":8.97113569216684e-150},"n":[1,"x"]});
        let variants = legacy_variants(&exact).unwrap();
        assert_eq!(variants.len(), 7);
        assert!(variants.contains(&to_vec(&exact).unwrap()));
        assert!(variants.contains(&to_vec(&legacy).unwrap()));

        // Integers and strings are never varied, floats multiply out.
        assert_eq!(
            legacy_variants(&serde_json::json!({"a":1,"b":"1.5"})).unwrap(),
            vec![br#"{"a":1,"b":"1.5"}"#.to_vec()]
        );
        assert_eq!(
            legacy_variants(&serde_json::json!([0.1, 0.2]))
                .unwrap()
                .len(),
            49
        );
        assert!(legacy_variants(&serde_json::json!([0.1, 0.2, 0.3, 0.4])).is_none());
    }

    #[test]
    fn different_float_values_keep_different_identity_bytes() {
        // What the legacy parser produced for the first vector.
        let legacy = serde_json::json!({"args":{"ratio":8.97113569216684e-150,"n":[1]}});
        let exact = serde_json::json!({"args":{"ratio":8.971135692166839e-150,"n":[1]}});
        assert_ne!(to_vec(&legacy).unwrap(), to_vec(&exact).unwrap());
        for (a, b) in [
            (
                serde_json::json!(1.0),
                serde_json::json!(f64::from_bits(1.0_f64.to_bits() + 1)),
            ),
            (serde_json::json!(0.0), serde_json::json!(-0.0)),
            (serde_json::json!(1), serde_json::json!(1.0)),
        ] {
            assert_ne!(to_vec(&a).unwrap(), to_vec(&b).unwrap(), "{a} vs {b}");
        }
    }
}

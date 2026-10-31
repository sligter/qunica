//! Stable identity bytes, not a serializer for provider messages.

use serde_json::Value;

pub(crate) fn to_vec(value: &Value) -> serde_json::Result<Vec<u8>> {
    // Note: Rig enables preserve_order process-wide. Durable identity hashes
    // must retain the old sorted-object encoding without mutating opaque history.
    let mut canonical = value.clone();
    canonical.sort_all_objects();
    serde_json::to_vec(&canonical)
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
}

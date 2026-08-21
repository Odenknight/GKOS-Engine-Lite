use std::fmt::Write;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{RetrievalError, RetrievalResult};

pub fn sha256(bytes: impl AsRef<[u8]>) -> String {
    let digest = Sha256::digest(bytes.as_ref());
    let mut output = String::with_capacity(71);
    output.push_str("sha256:");
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

pub fn canonical_json<T: Serialize>(value: &T) -> RetrievalResult<String> {
    // JSON serializers commonly normalize non-finite floats to null. Preserve
    // scalar types in a serde-value preflight so that cannot alter a digest.
    let preflight = serde_value::to_value(value).map_err(|error| {
        RetrievalError::InvalidEnvelope(format!("canonical JSON preflight failed: {error}"))
    })?;
    assert_jcs_safe(&preflight)?;
    let canonical = serde_jcs::to_string(value)?;
    Ok(canonical)
}

pub fn canonical_digest<T: Serialize>(value: &T) -> RetrievalResult<String> {
    Ok(sha256(canonical_json(value)?.as_bytes()))
}

fn assert_jcs_safe(value: &serde_value::Value) -> RetrievalResult<()> {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
    match value {
        serde_value::Value::U64(number) if *number > MAX_SAFE_INTEGER => {
            return Err(RetrievalError::InvalidEnvelope(
                "JCS integer exceeds the IEEE-754 safe range".to_owned(),
            ));
        }
        serde_value::Value::I64(number) if number.unsigned_abs() > MAX_SAFE_INTEGER => {
            return Err(RetrievalError::InvalidEnvelope(
                "JCS integer exceeds the IEEE-754 safe range".to_owned(),
            ));
        }
        serde_value::Value::F32(number) if !number.is_finite() => {
            return Err(RetrievalError::InvalidEnvelope(
                "JCS input contains a non-finite number".to_owned(),
            ));
        }
        serde_value::Value::F64(number) if !number.is_finite() => {
            return Err(RetrievalError::InvalidEnvelope(
                "JCS input contains a non-finite number".to_owned(),
            ));
        }
        serde_value::Value::F32(number)
            if number.fract() == 0.0 && f64::from(number.abs()) > MAX_SAFE_INTEGER as f64 =>
        {
            return Err(RetrievalError::InvalidEnvelope(
                "JCS integer-valued float exceeds the IEEE-754 safe range".to_owned(),
            ));
        }
        serde_value::Value::F64(number)
            if number.fract() == 0.0 && number.abs() > MAX_SAFE_INTEGER as f64 =>
        {
            return Err(RetrievalError::InvalidEnvelope(
                "JCS integer-valued float exceeds the IEEE-754 safe range".to_owned(),
            ));
        }
        serde_value::Value::Seq(values) => {
            for value in values {
                assert_jcs_safe(value)?;
            }
        }
        serde_value::Value::Map(values) => {
            for (key, value) in values {
                assert_jcs_safe(key)?;
                assert_jcs_safe(value)?;
            }
        }
        serde_value::Value::Option(value) => {
            if let Some(value) = value.as_deref() {
                assert_jcs_safe(value)?;
            }
        }
        serde_value::Value::Newtype(value) => assert_jcs_safe(value)?,
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn canonical_json_sorts_object_keys_without_reordering_arrays() {
        assert_eq!(
            canonical_json(&json!({ "z": [2, 1], "a": { "d": true, "c": null } })).unwrap(),
            r#"{"a":{"c":null,"d":true},"z":[2,1]}"#
        );
    }

    #[test]
    fn canonical_json_uses_jcs_number_and_utf16_key_rules() {
        let value = json!({
            "one": 1.0,
            "negative_zero": -0.0,
            "small": 0.0000001,
            "\u{e000}": "bmp",
            "\u{10000}": "astral"
        });
        assert_eq!(
            canonical_json(&value).unwrap(),
            "{\"negative_zero\":0,\"one\":1,\"small\":1e-7,\"𐀀\":\"astral\",\"\":\"bmp\"}"
        );
    }

    #[test]
    fn canonical_json_rejects_integers_outside_the_safe_binary64_range() {
        assert!(canonical_json(&json!({ "unsafe": 9_007_199_254_740_992_u64 })).is_err());
        assert!(canonical_json(&json!({ "unsafe": 1e21_f64 })).is_err());
    }

    #[test]
    fn canonical_json_rejects_nonfinite_numbers() {
        #[derive(Serialize)]
        struct Value {
            number: f64,
        }
        assert!(canonical_json(&Value { number: f64::NAN }).is_err());
        assert!(canonical_json(&Value {
            number: f64::INFINITY
        })
        .is_err());
    }

    #[test]
    fn sha256_is_prefixed_and_stable() {
        assert_eq!(
            sha256(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}

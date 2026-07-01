use mongodb::bson::{Bson, Document};

use rdbs_core::error::{RdbsError, Result};

/// Convert a JSON object into a BSON `Document`. Filters, insert payloads, and
/// aggregation stages all arrive as JSON objects, so a non-object is a usage
/// error and is rejected.
pub fn json_to_document(value: &serde_json::Value) -> Result<Document> {
    let bson: Bson = mongodb::bson::to_bson(value)
        .map_err(|e| RdbsError::Query(format!("invalid BSON: {e}")))?;
    match bson {
        Bson::Document(d) => Ok(d),
        other => Err(RdbsError::Query(format!(
            "expected a JSON object, got {:?}",
            other.element_type()
        ))),
    }
}

/// Convert a BSON `Document` back into a `serde_json::Value`. Starts from BSON's
/// relaxed extended-JSON form, then collapses the `$`-prefixed wrappers it emits
/// (`$oid`, `$date`, `$numberLong`, …) into plain scalars so both the table and
/// JSON preview read like Compass instead of leaking `{"$oid":"…"}`.
pub fn document_to_json(doc: Document) -> serde_json::Value {
    simplify(Bson::Document(doc).into_relaxed_extjson())
}

/// Recursively collapse single-key extended-JSON wrapper objects into the plain
/// scalar they represent. Anything that isn't a recognized wrapper is walked
/// through unchanged.
fn simplify(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Array(items) => Value::Array(items.into_iter().map(simplify).collect()),
        Value::Object(map) => {
            // Only single-key objects can be wrappers; collapse the ones we know.
            if map.len() == 1 {
                let (k, inner) = map.into_iter().next().unwrap();
                return match k.as_str() {
                    "$oid" => inner, // already a string
                    // Relaxed ext-JSON dates are ISO strings; pre-1970/post-9999
                    // dates fall back to {"$date":{"$numberLong":"ms"}}.
                    "$date" => simplify(inner),
                    "$numberLong" | "$numberInt" => str_to_number(inner),
                    "$numberDouble" | "$numberDecimal" => str_to_number(inner),
                    _ => {
                        // Not a wrapper: rebuild the one-key object, simplifying its value.
                        let mut m = serde_json::Map::new();
                        m.insert(k, simplify(inner));
                        Value::Object(m)
                    }
                };
            }
            Value::Object(map.into_iter().map(|(k, val)| (k, simplify(val))).collect())
        }
        other => other,
    }
}

/// Parse a numeric ext-JSON string into a JSON number; keep the string if it
/// isn't finite/parseable (e.g. "Infinity", "NaN", Decimal128 precision).
fn str_to_number(v: serde_json::Value) -> serde_json::Value {
    match &v {
        serde_json::Value::String(s) => {
            if let Ok(i) = s.parse::<i64>() {
                serde_json::Value::from(i)
            } else if let Ok(f) = s.parse::<f64>() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(v)
            } else {
                v
            }
        }
        _ => v,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;

    #[test]
    fn json_object_becomes_bson_document() {
        let json = serde_json::json!({ "name": "alice", "age": 30 });
        let doc = json_to_document(&json).unwrap();
        assert_eq!(doc.get_str("name").unwrap(), "alice");
        assert_eq!(
            doc.get_i64("age")
                .or_else(|_| doc.get_i32("age").map(|v| v as i64))
                .unwrap(),
            30
        );
    }

    #[test]
    fn empty_json_object_becomes_empty_document() {
        let json = serde_json::json!({});
        let doc = json_to_document(&json).unwrap();
        assert_eq!(doc.len(), 0);
    }

    #[test]
    fn non_object_json_is_rejected() {
        let json = serde_json::json!([1, 2, 3]);
        assert!(json_to_document(&json).is_err());
    }

    #[test]
    fn bson_document_roundtrips_to_json() {
        let d = doc! { "name": "bob", "score": 1.5 };
        let json = document_to_json(d);
        assert_eq!(json["name"], serde_json::json!("bob"));
        assert_eq!(json["score"], serde_json::json!(1.5));
    }

    #[test]
    fn objectid_collapses_to_hex_string() {
        use mongodb::bson::oid::ObjectId;
        let oid = ObjectId::new();
        let d = doc! { "_id": oid };
        let json = document_to_json(d);
        assert_eq!(json["_id"], serde_json::json!(oid.to_hex()));
    }

    #[test]
    fn date_collapses_to_iso_string() {
        use mongodb::bson::DateTime;
        let dt = DateTime::from_millis(1_700_000_000_000);
        let d = doc! { "at": dt };
        let json = document_to_json(d);
        // Plain ISO string, not {"$date": ...}.
        assert!(json["at"].is_string(), "got {}", json["at"]);
        assert!(json["at"].as_str().unwrap().starts_with("2023-"));
    }

    #[test]
    fn nested_array_and_object_wrappers_collapse() {
        use mongodb::bson::oid::ObjectId;
        let oid = ObjectId::new();
        let d = doc! { "refs": [oid], "child": { "_id": oid } };
        let json = document_to_json(d);
        assert_eq!(json["refs"][0], serde_json::json!(oid.to_hex()));
        assert_eq!(json["child"]["_id"], serde_json::json!(oid.to_hex()));
    }
}

use mongodb::bson::{Bson, Document};

use rdbs_core::error::{RdbsError, Result};

/// Convert a JSON object into a BSON `Document`. Filters, insert payloads, and
/// aggregation stages all arrive as JSON objects, so a non-object is a usage
/// error and is rejected.
pub fn json_to_document(value: &serde_json::Value) -> Result<Document> {
    let bson: Bson =
        mongodb::bson::to_bson(value).map_err(|e| RdbsError::Query(format!("invalid BSON: {e}")))?;
    match bson {
        Bson::Document(d) => Ok(d),
        other => Err(RdbsError::Query(format!(
            "expected a JSON object, got {:?}",
            other.element_type()
        ))),
    }
}

/// Convert a BSON `Document` back into a `serde_json::Value`. Uses BSON's
/// relaxed extended-JSON form so ordinary numbers/strings stay plain and only
/// exotic types (ObjectId, dates) carry `$`-prefixed wrappers.
pub fn document_to_json(doc: Document) -> serde_json::Value {
    Bson::Document(doc).into_relaxed_extjson()
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
}

//! Serde serialization/deserialization for [`QueryError`](super::QueryError).

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::types::{QueryError, QueryErrorKind};

impl Serialize for QueryError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Helper {
            kind: QueryErrorKind,
            message: String,
        }
        Helper {
            kind: self.kind,
            message: self.message.to_string(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for QueryError {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Helper {
            kind: QueryErrorKind,
            message: String,
        }
        let h = Helper::deserialize(deserializer)?;
        Ok(Self {
            kind: h.kind,
            message: h.message.into(),
        })
    }
}

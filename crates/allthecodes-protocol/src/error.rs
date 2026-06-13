#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApiError {
    #[error("bad request: {message}")]
    BadRequest { code: &'static str, message: String },
    #[error("forbidden: {message}")]
    Forbidden { code: &'static str, message: String },
    #[error("{entity} not found: {id}")]
    NotFound { entity: &'static str, id: String },
    #[error("conflict: {reason}")]
    Conflict { reason: String },
    #[error("validation failed for {field}: {message}")]
    Validation { field: String, message: String },
    #[error("engine is busy")]
    EngineBusy,
    #[error("experimental API is not enabled: {0}")]
    Experimental(String),
    #[error("API endpoint is not implemented: {capability}")]
    NotImplemented { capability: String },
    #[error("internal error: {message}")]
    Internal { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ApiErrorBody {
    pub error: String,
    pub code: String,
    pub details: Value,
}

impl ApiError {
    pub const fn status_code(&self) -> u16 {
        match self {
            Self::BadRequest { .. } => 400,
            Self::Forbidden { .. } => 403,
            Self::NotFound { .. } => 404,
            Self::Conflict { .. } => 409,
            Self::Validation { .. } => 422,
            Self::EngineBusy => 503,
            Self::Experimental(_) => 403,
            Self::NotImplemented { .. } => 501,
            Self::Internal { .. } => 500,
        }
    }

    pub const fn code(&self) -> &'static str {
        match self {
            Self::BadRequest { code, .. } => code,
            Self::Forbidden { code, .. } => code,
            Self::NotFound { .. } => "not_found",
            Self::Conflict { .. } => "conflict",
            Self::Validation { .. } => "validation",
            Self::EngineBusy => "engine_busy",
            Self::Experimental(_) => "experimental",
            Self::NotImplemented { .. } => "capability_not_implemented",
            Self::Internal { .. } => "internal",
        }
    }

    pub fn into_body(self) -> ApiErrorBody {
        let error = self.to_string();
        let code = self.code().to_string();
        let details = self.details();

        ApiErrorBody {
            error,
            code,
            details,
        }
    }

    pub fn to_body(&self) -> ApiErrorBody {
        ApiErrorBody {
            error: self.to_string(),
            code: self.code().to_string(),
            details: self.details(),
        }
    }

    fn details(&self) -> Value {
        match self {
            Self::BadRequest { message, .. } => json!({
                "message": message,
            }),
            Self::Forbidden { message, .. } => json!({
                "message": message,
            }),
            Self::NotFound { entity, id } => json!({
                "entity": entity,
                "id": id,
            }),
            Self::Conflict { reason } => json!({
                "reason": reason,
            }),
            Self::Validation { field, message } => json!({
                "field": field,
                "message": message,
            }),
            Self::EngineBusy => json!({}),
            Self::Experimental(reason) => json!({
                "reason": reason,
            }),
            Self::NotImplemented { capability } => json!({
                "capability": capability,
            }),
            Self::Internal { message } => json!({
                "message": message,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_match_error_variants() {
        assert_eq!(
            ApiError::NotFound {
                entity: "session",
                id: "session-1".to_string(),
            }
            .status_code(),
            404
        );
        assert_eq!(
            ApiError::Conflict {
                reason: "already archived".to_string(),
            }
            .status_code(),
            409
        );
        assert_eq!(
            ApiError::Validation {
                field: "title".to_string(),
                message: "required".to_string(),
            }
            .status_code(),
            422
        );
        assert_eq!(ApiError::EngineBusy.status_code(), 503);
        assert_eq!(
            ApiError::Experimental("session-mutations".to_string()).status_code(),
            403
        );
        assert_eq!(
            ApiError::Internal {
                message: "unexpected".to_string(),
            }
            .status_code(),
            500
        );
    }

    #[test]
    fn error_body_preserves_code_and_structured_details() {
        let body = ApiError::NotFound {
            entity: "session",
            id: "session-1".to_string(),
        }
        .into_body();

        assert_eq!(body.code, "not_found");
        assert_eq!(body.details["entity"], "session");
        assert_eq!(body.details["id"], "session-1");
        assert!(body.error.contains("session"));
    }
}

use serde_json::Value;

use crate::v1;

pub const fn split_route(route: &'static str) -> (&'static str, &'static str) {
    let bytes = route.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b' ' {
            let (http_method, path_with_space) = route.split_at(index);
            let (_, path) = path_with_space.split_at(1);
            return (http_method, path);
        }

        index += 1;
    }

    (route, "")
}

pub fn serialization_key(value: Value) -> String {
    match value {
        Value::String(value) => value,
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

crate::api_definitions! {
    /// List active and archived sessions.
    SessionList => "GET /api/sessions" {
        response: v1::SessionListResponse,
    },
    /// Create a new session.
    SessionCreate => "POST /api/sessions/new" {
        params: v1::SessionCreateParams,
        response: v1::SessionCreateResponse,
    },
    /// Fetch one session by id.
    SessionDetail => "GET /api/sessions/{id}" {
        params: v1::SessionDetailParams,
        response: v1::SessionDetailResponse,
        errors: [NotFound],
        serialization: PerKey("id"),
    },
    /// Resume an existing session.
    SessionResume => "POST /api/sessions/{id}/resume" {
        params: v1::SessionResumeParams,
        response: v1::SessionResumeResponse,
        errors: [NotFound, Conflict, EngineBusy],
        serialization: PerKey("id"),
    },
    /// Archive an existing session.
    SessionArchive => "POST /api/sessions/{id}/archive" {
        params: v1::SessionArchiveParams,
        response: v1::SessionArchiveResponse,
        errors: [NotFound, Conflict],
        serialization: PerKey("id"),
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_metadata_matches_session_definitions() {
        let endpoints = [
            (ApiMethod::SessionList, "GET", "/api/sessions"),
            (ApiMethod::SessionCreate, "POST", "/api/sessions/new"),
            (ApiMethod::SessionDetail, "GET", "/api/sessions/{id}"),
            (
                ApiMethod::SessionResume,
                "POST",
                "/api/sessions/{id}/resume",
            ),
            (
                ApiMethod::SessionArchive,
                "POST",
                "/api/sessions/{id}/archive",
            ),
        ];

        assert_eq!(ALL_ENDPOINTS.len(), endpoints.len());

        for (index, (operation, http_method, path)) in endpoints.into_iter().enumerate() {
            assert_eq!(ALL_ENDPOINTS[index].operation, operation);
            assert_eq!(ALL_ENDPOINTS[index].http_method, http_method);
            assert_eq!(ALL_ENDPOINTS[index].path, path);
            assert_eq!(operation.endpoint(), ALL_ENDPOINTS[index]);
        }
    }

    #[test]
    fn client_request_serde_roundtrip() {
        let request = ClientRequest::SessionDetail(v1::SessionDetailParams {
            id: "session-1".to_string(),
        });

        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: ClientRequest = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, request);
        assert_eq!(decoded.endpoint().path, "/api/sessions/{id}");
    }

    #[test]
    fn client_response_serde_roundtrip() {
        let response = ClientResponse::SessionList(v1::SessionListResponse {
            sessions: vec![v1::SessionSummary {
                id: "session-1".to_string(),
                title: Some("Planning".to_string()),
                archived: false,
            }],
        });

        let encoded = serde_json::to_string(&response).unwrap();
        let decoded: ClientResponse = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, response);
    }

    #[test]
    fn serialization_scope_defaults_to_none_and_reads_per_key_id() {
        assert_eq!(
            ClientRequest::SessionList(NoParams {}).serialization_scope(),
            SerializationScope::None
        );

        assert_eq!(
            ClientRequest::SessionResume(v1::SessionResumeParams {
                id: "session-2".to_string(),
            })
            .serialization_scope(),
            SerializationScope::PerKey {
                field: "id",
                key: "session-2".to_string(),
            }
        );
    }

    #[test]
    fn normal_endpoints_are_not_experimental() {
        assert_eq!(
            ClientRequest::SessionList(NoParams {}).experimental_reason(),
            None
        );
        assert_eq!(
            ClientRequest::SessionArchive(v1::SessionArchiveParams {
                id: "session-3".to_string(),
            })
            .experimental_reason(),
            None
        );
    }
}

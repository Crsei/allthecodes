//! Shared API protocol definitions for allthecodes transports.

pub mod error;
pub mod macros;
pub mod notification;
pub mod request;
pub mod response;
pub mod v1;

pub use error::{ApiError, ApiErrorBody};
pub use notification::ServerNotification;
pub use request::{
    ApiEndpoint, ApiMethod, ClientRequest, EmptyResponse, NoParams, SerializationScope,
    ALL_ENDPOINTS,
};
pub use response::ClientResponse;

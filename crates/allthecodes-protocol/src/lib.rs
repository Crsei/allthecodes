//! Shared API protocol definitions for allthecodes transports.

pub mod codegen;
pub mod error;
pub mod macros;
pub mod notification;
pub mod request;
pub mod response;
pub mod transport;
pub mod v1;

pub use error::{ApiError, ApiErrorBody};
pub use notification::ServerNotification;
pub use request::{
    ApiEndpoint, ApiMethod, ApiOperationMetadata, ApiTypeMetadata, ClientRequest, EmptyResponse,
    NoParams, SerializationPolicy, SerializationScope, ALL_ENDPOINTS, API_METADATA,
};
pub use response::ClientResponse;
pub use transport::{
    DirectTransport, JsonRpcFrame, MessageProcessor, Transport, TransportError, TransportRequestId,
};

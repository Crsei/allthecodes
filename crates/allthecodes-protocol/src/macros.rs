#[macro_export]
macro_rules! api_definitions {
    (
        $(
            $(#[$meta:meta])*
            $variant:ident => $route:literal {
                $(params: $params:ty,)?
                $(response: $response:ty,)?
                $(stream: [$($stream_event:ty),* $(,)?],)?
                $(errors: [$($errors:ident),* $(,)?],)?
                $(serialization: $serialization:ident $(($serialization_arg:literal))?,)?
                $(#[experimental($experimental:literal)])?
            }
        ),* $(,)?
    ) => {
        #[derive(
            Debug,
            Default,
            Clone,
            Copy,
            PartialEq,
            Eq,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[cfg_attr(feature = "schema", derive(::schemars::JsonSchema))]
        pub struct NoParams {}

        #[derive(
            Debug,
            Default,
            Clone,
            Copy,
            PartialEq,
            Eq,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[cfg_attr(feature = "schema", derive(::schemars::JsonSchema))]
        pub struct EmptyResponse {}

        #[derive(
            Debug,
            Clone,
            PartialEq,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[cfg_attr(feature = "schema", derive(::schemars::JsonSchema))]
        #[serde(tag = "method", content = "params")]
        pub enum ClientRequest {
            $(
                $(#[$meta])*
                $variant($crate::__api_params_type!($($params)?)),
            )*
        }

        #[derive(
            Debug,
            Clone,
            PartialEq,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[cfg_attr(feature = "schema", derive(::schemars::JsonSchema))]
        #[serde(tag = "method", content = "result")]
        pub enum ClientResponse {
            $(
                $(#[$meta])*
                $variant($crate::__api_response_type!($($response)?)),
            )*
        }

        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[cfg_attr(feature = "schema", derive(::schemars::JsonSchema))]
        pub enum ApiMethod {
            $(
                $(#[$meta])*
                $variant,
            )*
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct ApiEndpoint {
            pub operation: ApiMethod,
            pub http_method: &'static str,
            pub path: &'static str,
        }

        impl ApiEndpoint {
            pub const fn from_route(operation: ApiMethod, route: &'static str) -> Self {
                let (http_method, path) = $crate::request::split_route(route);

                Self {
                    operation,
                    http_method,
                    path,
                }
            }
        }

        #[derive(Debug, Clone, Copy)]
        pub struct ApiTypeMetadata {
            pub rust_type: &'static str,
            #[cfg(feature = "schema")]
            pub schema: fn() -> ::schemars::schema::RootSchema,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum SerializationPolicy {
            Concurrent,
            PerProcess,
            PerConnection,
            PerKey {
                field: &'static str,
            },
        }

        impl SerializationPolicy {
            pub fn access_mode(&self) -> AccessMode {
                match self {
                    SerializationPolicy::Concurrent => AccessMode::SharedRead,
                    _ => AccessMode::Exclusive,
                }
            }
        }

        #[derive(Debug, Clone, Copy)]
        pub struct ApiOperationMetadata {
            pub endpoint: ApiEndpoint,
            pub params: Option<ApiTypeMetadata>,
            pub response: ApiTypeMetadata,
            /// Additional event DTOs emitted by streaming transports whose
            /// HTTP response body cannot name every event variant directly.
            pub stream_events: &'static [ApiTypeMetadata],
            pub errors: &'static [&'static str],
            pub serialization: SerializationPolicy,
            pub experimental: Option<&'static str>,
        }

        impl ApiOperationMetadata {
            pub fn access_mode(&self) -> AccessMode {
                self.serialization.access_mode()
            }
        }

        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum SerializationScope {
            Concurrent,
            PerProcess,
            PerConnection,
            PerKey {
                field: &'static str,
                key: String,
            },
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum AccessMode {
            SharedRead,
            Exclusive,
        }

        impl SerializationScope {
            pub fn access_mode(&self) -> AccessMode {
                match self {
                    SerializationScope::Concurrent => AccessMode::SharedRead,
                    SerializationScope::PerProcess => AccessMode::Exclusive,
                    SerializationScope::PerConnection => AccessMode::Exclusive,
                    SerializationScope::PerKey { .. } => AccessMode::Exclusive,
                }
            }

            pub fn per_key<T>(params: &T, field: &'static str) -> Self
            where
                T: ::serde::Serialize,
            {
                let key = ::serde_json::to_value(params)
                    .ok()
                    .and_then(|value| value.get(field).cloned())
                    .map($crate::request::serialization_key)
                    .unwrap_or_default();

                Self::PerKey { field, key }
            }
        }

        impl ApiMethod {
            pub const fn endpoint(self) -> ApiEndpoint {
                match self {
                    $(
                        Self::$variant => ApiEndpoint::from_route(Self::$variant, $route),
                    )*
                }
            }
        }

        impl ClientRequest {
            pub const fn method(&self) -> ApiMethod {
                match self {
                    $(
                        Self::$variant(..) => ApiMethod::$variant,
                    )*
                }
            }

            pub const fn endpoint(&self) -> ApiEndpoint {
                self.method().endpoint()
            }

            pub fn serialization_scope(&self) -> SerializationScope {
                match self {
                    $(
                        Self::$variant(_params) => {
                            $crate::__api_serialization_scope!(
                                _params;
                                $($serialization $(($serialization_arg))?)?
                            )
                        }
                    )*
                }
            }

            pub const fn experimental_reason(&self) -> Option<&'static str> {
                match self {
                    $(
                        Self::$variant(..) => {
                            $crate::__api_experimental_reason!($($experimental)?)
                        }
                    )*
                }
            }

            pub fn access_mode(&self) -> AccessMode {
                self.serialization_scope().access_mode()
            }
        }

        pub const ALL_ENDPOINTS: &[ApiEndpoint] = &[
            $(
                ApiEndpoint::from_route(ApiMethod::$variant, $route),
            )*
        ];

        pub const API_METADATA: &[ApiOperationMetadata] = &[
            $(
                ApiOperationMetadata {
                    endpoint: ApiEndpoint::from_route(ApiMethod::$variant, $route),
                    params: $crate::__api_params_metadata!($($params)?),
                    response: $crate::__api_response_metadata!($($response)?),
                    stream_events: $crate::__api_stream_metadata!($($($stream_event),*)?),
                    errors: &[$($(stringify!($errors)),*)?],
                    serialization: $crate::__api_serialization_policy!(
                        $($serialization $(($serialization_arg))?)?
                    ),
                    experimental: $crate::__api_experimental_reason!($($experimental)?),
                },
            )*
        ];
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __api_params_type {
    () => {
        NoParams
    };
    ($params:ty) => {
        $params
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __api_response_type {
    () => {
        EmptyResponse
    };
    ($response:ty) => {
        $response
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __api_serialization_scope {
    (; ) => {
        SerializationScope::Concurrent
    };
    ($params:ident; ) => {
        SerializationScope::Concurrent
    };
    (; None) => {
        SerializationScope::Concurrent
    };
    ($params:ident; None) => {
        SerializationScope::Concurrent
    };
    (; Concurrent) => {
        SerializationScope::Concurrent
    };
    ($params:ident; Concurrent) => {
        SerializationScope::Concurrent
    };
    (; PerProcess) => {
        SerializationScope::PerProcess
    };
    ($params:ident; PerProcess) => {
        SerializationScope::PerProcess
    };
    (; PerConnection) => {
        SerializationScope::PerConnection
    };
    ($params:ident; PerConnection) => {
        SerializationScope::PerConnection
    };
    (; Global($key:literal)) => {
        SerializationScope::PerProcess
    };
    ($params:ident; Global($key:literal)) => {
        SerializationScope::PerProcess
    };
    ($params:ident; PerKey($field:literal)) => {
        SerializationScope::per_key($params, $field)
    };
    (; PerKey($field:literal)) => {
        SerializationScope::PerKey {
            field: $field,
            key: String::new(),
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __api_params_metadata {
    () => {
        None
    };
    ($params:ty) => {
        Some(ApiTypeMetadata {
            rust_type: stringify!($params),
            #[cfg(feature = "schema")]
            schema: $crate::request::schema_for::<$params>,
        })
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __api_response_metadata {
    () => {
        ApiTypeMetadata {
            rust_type: stringify!(EmptyResponse),
            #[cfg(feature = "schema")]
            schema: $crate::request::schema_for::<EmptyResponse>,
        }
    };
    ($response:ty) => {
        ApiTypeMetadata {
            rust_type: stringify!($response),
            #[cfg(feature = "schema")]
            schema: $crate::request::schema_for::<$response>,
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __api_stream_metadata {
    () => {
        &[]
    };
    ($($stream_event:ty),+ $(,)?) => {
        &[
            $(
                ApiTypeMetadata {
                    rust_type: stringify!($stream_event),
                    #[cfg(feature = "schema")]
                    schema: $crate::request::schema_for::<$stream_event>,
                },
            )+
        ]
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __api_serialization_policy {
    () => {
        SerializationPolicy::Concurrent
    };
    (None) => {
        SerializationPolicy::Concurrent
    };
    (Concurrent) => {
        SerializationPolicy::Concurrent
    };
    (PerProcess) => {
        SerializationPolicy::PerProcess
    };
    (PerConnection) => {
        SerializationPolicy::PerConnection
    };
    (Global($key:literal)) => {
        SerializationPolicy::PerProcess
    };
    (PerKey($field:literal)) => {
        SerializationPolicy::PerKey { field: $field }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __api_experimental_reason {
    () => {
        None
    };
    ($reason:literal) => {
        Some($reason)
    };
}

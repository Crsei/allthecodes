//! Generated API artifacts from protocol metadata.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::error::ApiErrorBody;
use crate::notification::ServerNotification;
use crate::request::{ApiOperationMetadata, ApiTypeMetadata, SerializationPolicy, API_METADATA};
use crate::request::{ClientRequest, ClientResponse, EmptyResponse, NoParams};

const FRONTEND_GENERATED_DIR_ENV: &str = "ALLTHECODES_FRONTEND_GENERATED_DIR";
const BACKEND_ROUTES_FILE: &str = "docs/api/routes.md";
const BACKEND_SCHEMA_FILE: &str = "docs/api/schema.json";
const BACKEND_OPENAPI_FILE: &str = "docs/api/openapi.json";
const FRONTEND_TYPES_FILE: &str = "api-types.ts";
const FRONTEND_ROUTES_FILE: &str = "api-routes.ts";
const FRONTEND_SCHEMA_FILE: &str = "api-schema.json";
const BACKEND_REGENERATION_COMMAND: &str = "cargo run --locked -p allthecodes-protocol --features codegen --bin codegen -- --target backend-docs";

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArtifactOwner {
    Backend,
    Frontend,
}

impl fmt::Display for ArtifactOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Backend => "backend",
            Self::Frontend => "frontend",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactTarget {
    BackendDocs,
    Frontend,
    All,
}

impl ArtifactTarget {
    pub fn parse(value: &str) -> Result<Self, CodegenError> {
        match value {
            "backend-docs" => Ok(Self::BackendDocs),
            "frontend" => Ok(Self::Frontend),
            "all" => Ok(Self::All),
            _ => Err(CodegenError::InvalidTarget {
                target: value.to_string(),
            }),
        }
    }

    fn includes(self, owner: ArtifactOwner) -> bool {
        matches!(
            (self, owner),
            (Self::BackendDocs, ArtifactOwner::Backend)
                | (Self::Frontend, ArtifactOwner::Frontend)
                | (Self::All, _)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedArtifact {
    pub owner: ArtifactOwner,
    pub relative_path: &'static str,
    pub contents: String,
}

#[derive(Debug)]
struct ResolvedArtifact {
    artifact: GeneratedArtifact,
    path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("failed to serialize generated JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("failed to {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("protocol manifest directory has no repository root: {manifest_dir}")]
    InvalidManifestDirectory { manifest_dir: PathBuf },
    #[error(
        "frontend target requires --frontend-dir <path> or ALLTHECODES_FRONTEND_GENERATED_DIR"
    )]
    MissingFrontendDirectory,
    #[error("unknown generation target '{target}'; expected backend-docs, frontend, or all")]
    InvalidTarget { target: String },
    #[error("duplicate generated output path: {path}")]
    DuplicateOutputPath { path: PathBuf },
    #[error("duplicate API operation metadata: {operation}")]
    DuplicateOperation { operation: String },
    #[error("duplicate API method/path metadata: {method} {path}")]
    DuplicateEndpoint { method: String, path: String },
    #[error("conflicting generated schema name '{name}'")]
    DuplicateSchema { name: String },
    #[error("generated artifacts are stale or missing:\n{details}")]
    StaleArtifacts { details: String },
}

pub fn generate_all_artifacts() -> Result<Vec<GeneratedArtifact>, CodegenError> {
    validate_protocol_metadata(API_METADATA)?;
    let digest = protocol_digest();
    let schema = generate_schema_json_pretty_with_digest(&digest)?;
    let artifacts = vec![
        GeneratedArtifact {
            owner: ArtifactOwner::Backend,
            relative_path: BACKEND_ROUTES_FILE,
            contents: generate_route_markdown_with_digest(&digest),
        },
        GeneratedArtifact {
            owner: ArtifactOwner::Backend,
            relative_path: BACKEND_SCHEMA_FILE,
            contents: schema.clone(),
        },
        GeneratedArtifact {
            owner: ArtifactOwner::Backend,
            relative_path: BACKEND_OPENAPI_FILE,
            contents: generate_openapi_json_pretty_with_digest(&digest)?,
        },
        GeneratedArtifact {
            owner: ArtifactOwner::Frontend,
            relative_path: FRONTEND_TYPES_FILE,
            contents: generate_typescript_types_with_digest(&digest),
        },
        GeneratedArtifact {
            owner: ArtifactOwner::Frontend,
            relative_path: FRONTEND_ROUTES_FILE,
            contents: generate_typescript_routes_with_digest(&digest),
        },
        GeneratedArtifact {
            owner: ArtifactOwner::Frontend,
            relative_path: FRONTEND_SCHEMA_FILE,
            contents: schema,
        },
    ];
    validate_artifact_manifest(&artifacts)?;
    Ok(artifacts)
}

pub fn protocol_digest() -> String {
    hex::encode(Sha256::digest(canonical_protocol_value().to_string()))
}

pub fn backend_repository_root(manifest_dir: &Path) -> Result<PathBuf, CodegenError> {
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| CodegenError::InvalidManifestDirectory {
            manifest_dir: manifest_dir.to_path_buf(),
        })
}

pub fn resolve_frontend_generated_dir(explicit: Option<&Path>) -> Result<PathBuf, CodegenError> {
    resolve_frontend_generated_dir_with_env(
        explicit,
        std::env::var_os(FRONTEND_GENERATED_DIR_ENV).map(PathBuf::from),
    )
}

fn resolve_frontend_generated_dir_with_env(
    explicit: Option<&Path>,
    environment: Option<PathBuf>,
) -> Result<PathBuf, CodegenError> {
    let path = explicit
        .map(Path::to_path_buf)
        .or(environment)
        .ok_or(CodegenError::MissingFrontendDirectory)?;
    absolute_path(path)
}

pub fn artifact_paths(
    manifest_dir: &Path,
    target: ArtifactTarget,
    frontend_dir: Option<&Path>,
) -> Result<Vec<PathBuf>, CodegenError> {
    Ok(resolve_artifacts(manifest_dir, target, frontend_dir)?
        .into_iter()
        .map(|artifact| artifact.path)
        .collect())
}

pub fn write_api_artifacts(
    manifest_dir: &Path,
    target: ArtifactTarget,
    frontend_dir: Option<&Path>,
) -> Result<Vec<PathBuf>, CodegenError> {
    let resolved = resolve_artifacts(manifest_dir, target, frontend_dir)?;
    write_resolved_artifacts(&resolved)?;
    Ok(resolved.into_iter().map(|artifact| artifact.path).collect())
}

pub fn check_api_artifacts_up_to_date(
    manifest_dir: &Path,
    target: ArtifactTarget,
    frontend_dir: Option<&Path>,
) -> Result<Vec<PathBuf>, CodegenError> {
    let resolved = resolve_artifacts(manifest_dir, target, frontend_dir)?;
    let mut stale = Vec::new();

    for artifact in &resolved {
        match std::fs::read(&artifact.path) {
            Ok(actual) if actual == artifact.artifact.contents.as_bytes() => {}
            Ok(_) => stale.push(format_stale_artifact(artifact, frontend_dir, "stale")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                stale.push(format_stale_artifact(artifact, frontend_dir, "missing"));
            }
            Err(source) => {
                return Err(CodegenError::Io {
                    action: "read",
                    path: artifact.path.clone(),
                    source,
                });
            }
        }
    }

    if !stale.is_empty() {
        return Err(CodegenError::StaleArtifacts {
            details: stale.join("\n"),
        });
    }

    Ok(resolved.into_iter().map(|artifact| artifact.path).collect())
}

fn resolve_artifacts(
    manifest_dir: &Path,
    target: ArtifactTarget,
    frontend_dir: Option<&Path>,
) -> Result<Vec<ResolvedArtifact>, CodegenError> {
    let artifacts = generate_all_artifacts()?;
    let backend_root = absolute_path(backend_repository_root(manifest_dir)?)?;
    let frontend_root = if target.includes(ArtifactOwner::Frontend) {
        Some(resolve_frontend_generated_dir(frontend_dir)?)
    } else {
        None
    };

    let mut resolved = Vec::new();
    for artifact in artifacts
        .into_iter()
        .filter(|artifact| target.includes(artifact.owner))
    {
        let path = match artifact.owner {
            ArtifactOwner::Backend => backend_root.join(artifact.relative_path),
            ArtifactOwner::Frontend => {
                let root = frontend_root
                    .as_ref()
                    .ok_or(CodegenError::MissingFrontendDirectory)?;
                root.join(artifact.relative_path)
            }
        };
        resolved.push(ResolvedArtifact { artifact, path });
    }
    validate_resolved_paths(&resolved)?;
    Ok(resolved)
}

fn absolute_path(path: PathBuf) -> Result<PathBuf, CodegenError> {
    if path.is_absolute() {
        return Ok(path);
    }
    let current_dir = std::env::current_dir().map_err(|source| CodegenError::Io {
        action: "resolve current directory for",
        path: path.clone(),
        source,
    })?;
    Ok(current_dir.join(path))
}

fn validate_artifact_manifest(artifacts: &[GeneratedArtifact]) -> Result<(), CodegenError> {
    let mut paths = BTreeSet::new();
    for artifact in artifacts {
        if !paths.insert((artifact.owner, artifact.relative_path)) {
            return Err(CodegenError::DuplicateOutputPath {
                path: PathBuf::from(artifact.relative_path),
            });
        }
    }
    Ok(())
}

fn validate_resolved_paths(artifacts: &[ResolvedArtifact]) -> Result<(), CodegenError> {
    let mut paths = BTreeSet::new();
    for artifact in artifacts {
        if !paths.insert(artifact.path.clone()) {
            return Err(CodegenError::DuplicateOutputPath {
                path: artifact.path.clone(),
            });
        }
    }
    Ok(())
}

fn write_resolved_artifacts(artifacts: &[ResolvedArtifact]) -> Result<(), CodegenError> {
    validate_resolved_paths(artifacts)?;
    let mut staged = Vec::new();

    for artifact in artifacts {
        if std::fs::read(&artifact.path)
            .is_ok_and(|current| current == artifact.artifact.contents.as_bytes())
        {
            continue;
        }
        match stage_resolved_artifact(artifact) {
            Ok(temp_path) => staged.push((temp_path, artifact.path.clone())),
            Err(error) => {
                for (temp_path, _) in staged {
                    let _ = std::fs::remove_file(temp_path);
                }
                return Err(error);
            }
        }
    }

    for (index, (temp_path, output_path)) in staged.iter().enumerate() {
        if let Err(source) = std::fs::rename(temp_path, output_path) {
            for (remaining_temp, _) in staged.iter().skip(index) {
                let _ = std::fs::remove_file(remaining_temp);
            }
            return Err(CodegenError::Io {
                action: "atomically replace",
                path: output_path.clone(),
                source,
            });
        }
    }
    Ok(())
}

fn stage_resolved_artifact(artifact: &ResolvedArtifact) -> Result<PathBuf, CodegenError> {
    let parent = artifact.path.parent().ok_or_else(|| CodegenError::Io {
        action: "resolve parent directory for",
        path: artifact.path.clone(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "generated artifact has no parent directory",
        ),
    })?;
    std::fs::create_dir_all(parent).map_err(|source| CodegenError::Io {
        action: "create directory for",
        path: artifact.path.clone(),
        source,
    })?;
    stage_atomic_file(&artifact.path, artifact.artifact.contents.as_bytes())
}

fn stage_atomic_file(path: &Path, contents: &[u8]) -> Result<PathBuf, CodegenError> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact");

    for _ in 0..100 {
        let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = path.with_file_name(format!(
            ".{file_name}.tmp-{}-{sequence}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(mut file) => {
                if let Err(source) = file.write_all(contents).and_then(|()| file.sync_all()) {
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(CodegenError::Io {
                        action: "stage",
                        path: path.to_path_buf(),
                        source,
                    });
                }
                return Ok(temp_path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(CodegenError::Io {
                    action: "stage",
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }

    Err(CodegenError::Io {
        action: "stage",
        path: path.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a unique temporary file",
        ),
    })
}

fn format_stale_artifact(
    artifact: &ResolvedArtifact,
    frontend_dir: Option<&Path>,
    status: &str,
) -> String {
    let command = match artifact.artifact.owner {
        ArtifactOwner::Backend => BACKEND_REGENERATION_COMMAND.to_string(),
        ArtifactOwner::Frontend => {
            let directory = frontend_dir
                .map(Path::to_path_buf)
                .or_else(|| artifact.path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| PathBuf::from("."));
            format!(
                "cargo run --locked -p allthecodes-protocol --features codegen --bin codegen -- --target frontend --frontend-dir {}",
                directory.display()
            )
        }
    };
    format!(
        "- [{}] {} ({status})\n  regenerate: {command}",
        artifact.artifact.owner,
        artifact.path.display()
    )
}

pub fn generate_typescript_types() -> String {
    generate_typescript_types_with_digest(&protocol_digest())
}

fn generate_typescript_types_with_digest(digest: &str) -> String {
    let schemas = collect_typescript_schemas(API_METADATA);
    let mut output = String::new();

    output.push_str("// AUTO-GENERATED by allthecodes-protocol. Do not edit by hand.\n");
    output.push_str(&format!("// Protocol digest: {digest}\n"));
    output.push_str("// Run `cargo run --locked -p allthecodes-protocol --features codegen --bin codegen -- --target frontend --frontend-dir <path>` from the backend repo.\n\n");
    output.push_str(
        "export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };\n",
    );
    output.push_str("export type NoParams = never;\n");
    output.push_str("export type EmptyResponse = Record<string, never>;\n\n");

    for (name, schema) in schemas {
        if name == "NoParams" || name == "EmptyResponse" {
            continue;
        }

        let ts = schema_to_typescript(&schema, 0);
        if ts.starts_with("{\n") {
            output.push_str("export interface ");
            output.push_str(&name);
            output.push(' ');
            output.push_str(&ts);
            output.push_str("\n\n");
        } else {
            output.push_str("export type ");
            output.push_str(&name);
            output.push_str(" = ");
            output.push_str(&ts);
            output.push_str(";\n\n");
        }
    }

    output.push_str("export type V1ApiRequest =\n");
    for endpoint in API_METADATA {
        output.push_str("  | { method: ");
        output.push_str(&ts_string_literal(&method_literal(endpoint)));
        if let Some(params) = endpoint.params {
            output.push_str("; params: ");
            output.push_str(&typescript_type_for_rust(params.rust_type));
        } else {
            output.push_str("; params?: never");
        }
        output.push_str(" }\n");
    }
    output.push_str(";\n\n");

    output.push_str("export type V1ApiResponse =\n");
    for endpoint in API_METADATA {
        output.push_str("  | { method: ");
        output.push_str(&ts_string_literal(&method_literal(endpoint)));
        let response_type = typescript_type_for_rust(endpoint.response.rust_type);
        if response_type == "EmptyResponse" {
            output.push_str("; data?: never");
        } else {
            output.push_str("; data: ");
            output.push_str(&response_type);
        }
        output.push_str(" }\n");
    }
    output.push_str(";\n");

    output
}

pub fn generate_typescript_routes() -> String {
    generate_typescript_routes_with_digest(&protocol_digest())
}

fn generate_typescript_routes_with_digest(digest: &str) -> String {
    let mut output = String::new();

    output.push_str("// AUTO-GENERATED by allthecodes-protocol. Do not edit by hand.\n");
    output.push_str(&format!("// Protocol digest: {digest}\n"));
    output.push_str("// Run `cargo run --locked -p allthecodes-protocol --features codegen --bin codegen -- --target frontend --frontend-dir <path>` from the backend repo.\n\n");
    output.push_str("export interface V1ApiRoute {\n");
    output.push_str("  operation: string;\n");
    output.push_str("  method: string;\n");
    output.push_str("  path: string;\n");
    output.push_str("  params?: string;\n");
    output.push_str("  response: string;\n");
    output.push_str("  serialization: string;\n");
    output.push_str("  errors: readonly string[];\n");
    output.push_str("  streamEvents?: readonly string[];\n");
    output.push_str("  transport?: 'websocket';\n");
    output.push_str("  experimental?: string;\n");
    output.push_str("}\n\n");

    output.push_str("export const V1_API_ROUTES = [\n");
    for endpoint in API_METADATA {
        output.push_str("  {\n");
        output.push_str("    operation: ");
        output.push_str(&ts_string_literal(&format!(
            "{:?}",
            endpoint.endpoint.operation
        )));
        output.push_str(",\n    method: ");
        output.push_str(&ts_string_literal(endpoint.endpoint.http_method));
        output.push_str(",\n    path: ");
        output.push_str(&ts_string_literal(endpoint.endpoint.path));
        output.push_str(",\n");
        if let Some(params) = endpoint.params {
            output.push_str("    params: ");
            output.push_str(&ts_string_literal(&typescript_type_for_rust(
                params.rust_type,
            )));
            output.push_str(",\n");
        }
        output.push_str("    response: ");
        output.push_str(&ts_string_literal(&typescript_type_for_rust(
            endpoint.response.rust_type,
        )));
        output.push_str(",\n    serialization: ");
        output.push_str(&ts_string_literal(&serialization_label(
            endpoint.serialization,
        )));
        output.push_str(",\n    errors: ");
        output.push_str(&typescript_string_array(endpoint.errors));
        if !endpoint.stream_events.is_empty() {
            let stream_events = endpoint
                .stream_events
                .iter()
                .map(|event| typescript_type_for_rust(event.rust_type))
                .collect::<Vec<_>>();
            output.push_str(",\n    streamEvents: ");
            output.push_str(&typescript_owned_string_array(&stream_events));
        }
        if let Some(transport) = endpoint_transport(endpoint) {
            output.push_str(",\n    transport: ");
            output.push_str(&ts_string_literal(transport));
        }
        if let Some(reason) = endpoint.experimental {
            output.push_str(",\n    experimental: ");
            output.push_str(&ts_string_literal(reason));
        }
        output.push_str(",\n  },\n");
    }
    output.push_str("] as const satisfies readonly V1ApiRoute[];\n");
    output.push_str("\nexport type V1ApiOperation = typeof V1_API_ROUTES[number]['operation'];\n");
    output.push_str("export type V1ApiRoutePath = typeof V1_API_ROUTES[number]['path'];\n");

    output
}

pub fn generate_schema_json_pretty() -> Result<String, CodegenError> {
    generate_schema_json_pretty_with_digest(&protocol_digest())
}

fn generate_schema_json_pretty_with_digest(digest: &str) -> Result<String, CodegenError> {
    let schema = generate_schema_value_with_digest(digest);
    Ok(format!("{}\n", serde_json::to_string_pretty(&schema)?))
}

pub fn generate_schema_value() -> Value {
    generate_schema_value_with_digest(&protocol_digest())
}

fn generate_schema_value_with_digest(digest: &str) -> Value {
    let mut schema = canonical_protocol_value();
    if let Some(object) = schema.as_object_mut() {
        object.insert(
            "_generated".to_string(),
            json!({
                "generator": "allthecodes-protocol",
                "version": "v1",
                "protocol_digest": digest,
            }),
        );
    }
    schema
}

fn canonical_protocol_value() -> Value {
    let mut schemas = Map::new();

    insert_schema(
        &mut schemas,
        "ClientRequest",
        schema_value::<ClientRequest>(),
    );
    insert_schema(
        &mut schemas,
        "ClientResponse",
        schema_value::<ClientResponse>(),
    );
    insert_schema(
        &mut schemas,
        "ServerNotification",
        schema_value::<ServerNotification>(),
    );
    insert_schema(&mut schemas, "ApiErrorBody", schema_value::<ApiErrorBody>());
    insert_schema(&mut schemas, "NoParams", schema_value::<NoParams>());
    insert_schema(
        &mut schemas,
        "EmptyResponse",
        schema_value::<EmptyResponse>(),
    );

    for endpoint in API_METADATA {
        if let Some(params) = endpoint.params {
            insert_schema(
                &mut schemas,
                &typescript_type_for_rust(params.rust_type),
                root_schema_value(params),
            );
        }
        insert_schema(
            &mut schemas,
            &typescript_type_for_rust(endpoint.response.rust_type),
            root_schema_value(endpoint.response),
        );
        for event in endpoint.stream_events {
            insert_schema(
                &mut schemas,
                &typescript_type_for_rust(event.rust_type),
                root_schema_value(*event),
            );
        }
    }

    json!({
        "version": "v1",
        "endpoints": endpoint_values(),
        "schemas": schemas,
    })
}

pub fn generate_route_markdown() -> String {
    generate_route_markdown_with_digest(&protocol_digest())
}

fn generate_route_markdown_with_digest(digest: &str) -> String {
    let mut output = String::new();
    output.push_str("# allthecodes API Routes\n\n");
    output.push_str("Generated from `allthecodes-protocol` metadata.\n\n");
    output.push_str(&format!("Protocol digest: `{digest}`.\n\n"));
    output.push_str("| Operation | Method | Path | Transport | Params | Response | Stream events | Serialization | Errors | Experimental |\n");
    output.push_str("|---|---|---|---|---|---|---|---|---|---|\n");

    for endpoint in API_METADATA {
        output.push('|');
        output.push_str(&format!(" `{:?}` |", endpoint.endpoint.operation));
        output.push_str(&format!(" `{}` |", endpoint.endpoint.http_method));
        output.push_str(&format!(" `{}` |", endpoint.endpoint.path));
        output.push_str(&format!(
            " {} |",
            endpoint_transport(endpoint)
                .map(|transport| format!("`{transport}`"))
                .unwrap_or_else(|| "-".to_string())
        ));
        output.push_str(&format!(
            " {} |",
            endpoint
                .params
                .map(|params| format!("`{}`", typescript_type_for_rust(params.rust_type)))
                .unwrap_or_else(|| "-".to_string())
        ));
        output.push_str(&format!(
            " `{}` |",
            typescript_type_for_rust(endpoint.response.rust_type)
        ));
        output.push_str(&format!(
            " {} |",
            if endpoint.stream_events.is_empty() {
                "-".to_string()
            } else {
                endpoint
                    .stream_events
                    .iter()
                    .map(|event| format!("`{}`", typescript_type_for_rust(event.rust_type)))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        output.push_str(&format!(
            " `{}` |",
            serialization_label(endpoint.serialization)
        ));
        output.push_str(&format!(
            " {} |",
            if endpoint.errors.is_empty() {
                "-".to_string()
            } else {
                endpoint
                    .errors
                    .iter()
                    .map(|error| format!("`{error}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        output.push_str(&format!(
            " {} |\n",
            endpoint
                .experimental
                .map(|reason| format!("`{reason}`"))
                .unwrap_or_else(|| "-".to_string())
        ));
    }

    output
}

pub fn generate_openapi_json_pretty() -> Result<String, CodegenError> {
    generate_openapi_json_pretty_with_digest(&protocol_digest())
}

fn generate_openapi_json_pretty_with_digest(digest: &str) -> Result<String, CodegenError> {
    let openapi = generate_openapi_value_with_digest(digest);
    Ok(format!("{}\n", serde_json::to_string_pretty(&openapi)?))
}

pub fn generate_openapi_value() -> Value {
    generate_openapi_value_with_digest(&protocol_digest())
}

fn generate_openapi_value_with_digest(digest: &str) -> Value {
    let mut paths = Map::new();
    let mut components = Map::new();
    components.insert(
        "ApiErrorBody".to_string(),
        openapi_schema(&schema_value::<ApiErrorBody>()),
    );

    for endpoint in API_METADATA {
        if let Some(params) = endpoint.params {
            collect_component_schema(&mut components, params);
        }
        collect_component_schema(&mut components, endpoint.response);
        for event in endpoint.stream_events {
            collect_component_schema(&mut components, *event);
        }

        let path = openapi_path(endpoint.endpoint.path);
        let method = endpoint.endpoint.http_method.to_ascii_lowercase();
        let method_key = if method == "any" {
            "get"
        } else {
            method.as_str()
        };
        let operation = openapi_operation(endpoint);

        let path_item = paths
            .entry(path)
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(path_item) = path_item {
            path_item.insert(method_key.to_string(), operation);
        }
    }

    let mut schemas = Map::new();
    for (name, schema) in components {
        schemas.insert(name, schema);
    }

    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "allthecodes Web API",
            "version": "v1",
            "x-allthecodes-protocol-digest": digest,
        },
        "paths": paths,
        "components": {
            "schemas": schemas,
        },
    })
}

fn collect_typescript_schemas(metadata: &[ApiOperationMetadata]) -> BTreeMap<String, Value> {
    let mut schemas = BTreeMap::new();

    for endpoint in metadata {
        if let Some(params) = endpoint.params {
            collect_typescript_schema(&mut schemas, params);
        }
        collect_typescript_schema(&mut schemas, endpoint.response);
        for event in endpoint.stream_events {
            collect_typescript_schema(&mut schemas, *event);
        }
    }

    schemas
}

fn collect_typescript_schema(schemas: &mut BTreeMap<String, Value>, metadata: ApiTypeMetadata) {
    let rust_type = metadata.rust_type;
    let ts_type = typescript_type_for_rust(rust_type);
    if ts_type == "JsonValue" || ts_type.ends_with("[]") {
        collect_definitions(schemas, &root_schema_value(metadata));
        return;
    }

    if ts_type == "NoParams" || ts_type == "EmptyResponse" {
        return;
    }

    let root = root_schema_value(metadata);
    collect_definitions(schemas, &root);
    schemas.entry(ts_type).or_insert(root);
}

fn collect_definitions(schemas: &mut BTreeMap<String, Value>, root: &Value) {
    let Some(definitions) = root.get("definitions").and_then(Value::as_object) else {
        return;
    };

    for (name, schema) in definitions {
        schemas
            .entry(sanitize_type_name(name))
            .or_insert(schema.clone());
    }
}

fn collect_component_schema(components: &mut Map<String, Value>, metadata: ApiTypeMetadata) {
    let root = root_schema_value(metadata);
    if let Some(definitions) = root.get("definitions").and_then(Value::as_object) {
        for (name, schema) in definitions {
            components
                .entry(sanitize_type_name(name))
                .or_insert_with(|| openapi_schema(schema));
        }
    }

    let name = typescript_type_for_rust(metadata.rust_type);
    if name == "JsonValue" || name.ends_with("[]") {
        return;
    }

    components
        .entry(name)
        .or_insert_with(|| openapi_schema(&root));
}

fn endpoint_values() -> Vec<Value> {
    API_METADATA
        .iter()
        .map(|endpoint| {
            json!({
                "operation": format!("{:?}", endpoint.endpoint.operation),
                "method": endpoint.endpoint.http_method,
                "path": endpoint.endpoint.path,
                "params": endpoint.params.map(|params| typescript_type_for_rust(params.rust_type)),
                "response": typescript_type_for_rust(endpoint.response.rust_type),
                "stream_events": endpoint.stream_events.iter().map(|event| typescript_type_for_rust(event.rust_type)).collect::<Vec<_>>(),
                "transport": endpoint_transport(endpoint),
                "errors": endpoint.errors,
                "serialization": serialization_label(endpoint.serialization),
                "experimental": endpoint.experimental,
            })
        })
        .collect()
}

fn openapi_operation(endpoint: &ApiOperationMetadata) -> Value {
    let mut operation = Map::new();
    operation.insert(
        "operationId".to_string(),
        Value::String(format!("{:?}", endpoint.endpoint.operation)),
    );
    operation.insert(
        "x-allthecodes-serialization".to_string(),
        Value::String(serialization_label(endpoint.serialization)),
    );
    if endpoint.endpoint.http_method == "ANY" {
        operation.insert(
            "x-allthecodes-source-method".to_string(),
            Value::String("ANY".to_string()),
        );
        operation.insert(
            "x-allthecodes-transport".to_string(),
            Value::String("websocket".to_string()),
        );
    }
    if !endpoint.stream_events.is_empty() {
        operation.insert(
            "x-allthecodes-stream-events".to_string(),
            Value::Array(
                endpoint
                    .stream_events
                    .iter()
                    .map(|event| Value::String(typescript_type_for_rust(event.rust_type)))
                    .collect(),
            ),
        );
    }
    if let Some(reason) = endpoint.experimental {
        operation.insert(
            "x-allthecodes-experimental".to_string(),
            Value::String(reason.to_string()),
        );
    }

    let parameters = openapi_parameters(endpoint);
    if !parameters.is_empty() {
        operation.insert("parameters".to_string(), Value::Array(parameters));
    }

    if let Some(params) = endpoint.params {
        if endpoint.endpoint.http_method != "GET" {
            operation.insert(
                "requestBody".to_string(),
                json!({
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": openapi_type_ref(params),
                        },
                    },
                }),
            );
        }
    }

    operation.insert(
        "responses".to_string(),
        json!({
            "200": {
                "description": "OK",
                "content": {
                    "application/json": {
                        "schema": openapi_type_ref(endpoint.response),
                    },
                },
            },
            "default": {
                "description": "API error",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ApiErrorBody" },
                    },
                },
            },
        }),
    );

    Value::Object(operation)
}

fn openapi_parameters(endpoint: &ApiOperationMetadata) -> Vec<Value> {
    let mut parameters = Vec::new();
    let mut path_names = BTreeSet::new();
    for segment in endpoint.endpoint.path.split('/') {
        if let Some(name) = segment
            .strip_prefix('{')
            .and_then(|value| value.strip_suffix('}'))
        {
            path_names.insert(name.trim_start_matches('*').to_string());
        }
    }

    for name in path_names {
        parameters.push(json!({
            "name": name,
            "in": "path",
            "required": true,
            "schema": { "type": "string" },
        }));
    }

    if let Some(params) = endpoint.params {
        if endpoint.endpoint.http_method == "GET" {
            parameters.push(json!({
                "name": "params",
                "in": "query",
                "required": false,
                "schema": openapi_type_ref(params),
                "style": "deepObject",
                "explode": true,
            }));
        }
    }

    parameters
}

fn openapi_type_ref(metadata: ApiTypeMetadata) -> Value {
    let name = typescript_type_for_rust(metadata.rust_type);
    if name == "JsonValue" {
        json!({})
    } else if let Some(inner) = name.strip_suffix("[]") {
        json!({
            "type": "array",
            "items": { "$ref": format!("#/components/schemas/{inner}") },
        })
    } else {
        json!({ "$ref": format!("#/components/schemas/{name}") })
    }
}

fn openapi_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(object) => {
            let mut result = Map::new();
            for (key, value) in object {
                let key = if key == "definitions" {
                    continue;
                } else if key == "$ref" {
                    "$ref"
                } else {
                    key
                };

                let value = if key == "$ref" {
                    value
                        .as_str()
                        .map(|reference| {
                            Value::String(
                                reference.replace("#/definitions/", "#/components/schemas/"),
                            )
                        })
                        .unwrap_or_else(|| openapi_schema(value))
                } else {
                    openapi_schema(value)
                };
                result.insert(key.to_string(), value);
            }
            Value::Object(result)
        }
        Value::Array(items) => Value::Array(items.iter().map(openapi_schema).collect()),
        other => other.clone(),
    }
}

fn openapi_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if let Some(name) = segment
                .strip_prefix("{*")
                .and_then(|value| value.strip_suffix('}'))
            {
                format!("{{{name}}}")
            } else {
                segment.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn root_schema_value(metadata: ApiTypeMetadata) -> Value {
    serde_json::to_value((metadata.schema)()).unwrap_or_else(|_| json!({}))
}

fn schema_value<T>() -> Value
where
    T: schemars::JsonSchema,
{
    serde_json::to_value(schemars::schema_for!(T)).unwrap_or_else(|_| json!({}))
}

fn insert_schema(schemas: &mut Map<String, Value>, name: &str, schema: Value) {
    if name == "JsonValue" || name.ends_with("[]") {
        return;
    }
    schemas.entry(name.to_string()).or_insert(schema);
}

fn schema_to_typescript(schema: &Value, depth: usize) -> String {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return reference_to_type(reference);
    }

    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return values
            .iter()
            .map(value_to_literal)
            .collect::<Vec<_>>()
            .join(" | ");
    }

    if let Some(value) = schema.get("const") {
        return value_to_literal(value);
    }

    for key in ["oneOf", "anyOf"] {
        if let Some(items) = schema.get(key).and_then(Value::as_array) {
            return join_schema_types(items, depth, " | ");
        }
    }

    if let Some(items) = schema.get("allOf").and_then(Value::as_array) {
        return join_schema_types(items, depth, " & ");
    }

    match schema.get("type") {
        Some(Value::String(kind)) => schema_kind_to_typescript(kind, schema, depth),
        Some(Value::Array(kinds)) => kinds
            .iter()
            .filter_map(Value::as_str)
            .map(|kind| schema_kind_to_typescript(kind, schema, depth))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(" | "),
        _ => {
            if schema.get("properties").is_some() {
                object_to_typescript(schema, depth)
            } else if schema.get("items").is_some() {
                array_to_typescript(schema, depth)
            } else {
                "JsonValue".to_string()
            }
        }
    }
}

fn schema_kind_to_typescript(kind: &str, schema: &Value, depth: usize) -> String {
    match kind {
        "null" => "null".to_string(),
        "boolean" => "boolean".to_string(),
        "integer" | "number" => "number".to_string(),
        "string" => "string".to_string(),
        "array" => array_to_typescript(schema, depth),
        "object" => object_to_typescript(schema, depth),
        _ => "JsonValue".to_string(),
    }
}

fn array_to_typescript(schema: &Value, depth: usize) -> String {
    let item_type = schema
        .get("items")
        .map(|items| {
            if let Some(tuple_items) = items.as_array() {
                let tuple = tuple_items
                    .iter()
                    .map(|item| schema_to_typescript(item, depth + 1))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{tuple}]")
            } else {
                schema_to_typescript(items, depth + 1)
            }
        })
        .unwrap_or_else(|| "JsonValue".to_string());

    if item_type.contains(" | ") || item_type.contains(" & ") {
        format!("({item_type})[]")
    } else {
        format!("{item_type}[]")
    }
}

fn object_to_typescript(schema: &Value, depth: usize) -> String {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    if properties.is_empty() {
        if let Some(additional) = schema.get("additionalProperties") {
            if additional == &Value::Bool(false) {
                return "Record<string, never>".to_string();
            }
            if additional == &Value::Bool(true) {
                return "Record<string, JsonValue>".to_string();
            }
            return format!(
                "Record<string, {}>",
                schema_to_typescript(additional, depth + 1)
            );
        }
        return "Record<string, JsonValue>".to_string();
    }

    let indent = "  ".repeat(depth);
    let child_indent = "  ".repeat(depth + 1);
    let mut output = String::from("{\n");

    for (name, property) in properties {
        output.push_str(&child_indent);
        output.push_str(&ts_property_name(&name));
        if !required.contains(name.as_str()) {
            output.push('?');
        }
        output.push_str(": ");
        output.push_str(&schema_to_typescript(&property, depth + 1));
        output.push_str(";\n");
    }

    output.push_str(&indent);
    output.push('}');
    output
}

fn join_schema_types(items: &[Value], depth: usize, separator: &str) -> String {
    let values = items
        .iter()
        .map(|item| schema_to_typescript(item, depth + 1))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if values.is_empty() {
        "JsonValue".to_string()
    } else {
        values.join(separator)
    }
}

fn reference_to_type(reference: &str) -> String {
    reference
        .rsplit('/')
        .next()
        .map(sanitize_type_name)
        .unwrap_or_else(|| "JsonValue".to_string())
}

fn sanitize_type_name(name: &str) -> String {
    let mut result = String::new();
    let mut uppercase_next = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            if result.is_empty() {
                result.push(character.to_ascii_uppercase());
            } else if uppercase_next {
                result.push(character.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                result.push(character);
            }
        } else {
            uppercase_next = true;
        }
    }

    if result.is_empty() {
        "GeneratedType".to_string()
    } else {
        result
    }
}

fn typescript_type_for_rust(rust_type: &str) -> String {
    let compact = rust_type
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();

    if compact == "Value" || compact == "serde_json::Value" {
        return "JsonValue".to_string();
    }
    if compact == "NoParams" {
        return "NoParams".to_string();
    }
    if compact == "EmptyResponse" {
        return "EmptyResponse".to_string();
    }
    if let Some(inner) = compact
        .strip_prefix("Vec<")
        .and_then(|value| value.strip_suffix('>'))
    {
        return format!("{}[]", typescript_type_for_rust(inner));
    }

    compact
        .rsplit("::")
        .next()
        .map(sanitize_type_name)
        .unwrap_or_else(|| "JsonValue".to_string())
}

fn validate_protocol_metadata(metadata: &[ApiOperationMetadata]) -> Result<(), CodegenError> {
    let mut operations = BTreeSet::new();
    let mut endpoints = BTreeSet::new();
    let mut schemas = BTreeMap::new();

    for endpoint in metadata {
        let operation = format!("{:?}", endpoint.endpoint.operation);
        if !operations.insert(operation.clone()) {
            return Err(CodegenError::DuplicateOperation { operation });
        }
        let endpoint_key = (
            effective_http_method(endpoint).to_string(),
            endpoint.endpoint.path.to_string(),
        );
        if !endpoints.insert(endpoint_key.clone()) {
            return Err(CodegenError::DuplicateEndpoint {
                method: endpoint_key.0,
                path: endpoint_key.1,
            });
        }

        if let Some(params) = endpoint.params {
            validate_type_schema(&mut schemas, params)?;
        }
        validate_type_schema(&mut schemas, endpoint.response)?;
        for event in endpoint.stream_events {
            validate_type_schema(&mut schemas, *event)?;
        }
    }
    Ok(())
}

fn validate_type_schema(
    schemas: &mut BTreeMap<String, Value>,
    metadata: ApiTypeMetadata,
) -> Result<(), CodegenError> {
    let root = root_schema_value(metadata);
    if let Some(definitions) = root.get("definitions").and_then(Value::as_object) {
        for (name, schema) in definitions {
            insert_checked_schema(schemas, sanitize_type_name(name), schema.clone())?;
        }
    }

    let name = typescript_type_for_rust(metadata.rust_type);
    if name != "JsonValue" && !name.ends_with("[]") {
        insert_checked_schema(schemas, name, root)?;
    }
    Ok(())
}

fn insert_checked_schema(
    schemas: &mut BTreeMap<String, Value>,
    name: String,
    schema: Value,
) -> Result<(), CodegenError> {
    let schema = normalized_schema_for_collision(schema);
    match schemas.get(&name) {
        Some(existing) if existing != &schema => Err(CodegenError::DuplicateSchema { name }),
        Some(_) => Ok(()),
        None => {
            schemas.insert(name, schema);
            Ok(())
        }
    }
}

fn normalized_schema_for_collision(mut schema: Value) -> Value {
    if let Some(object) = schema.as_object_mut() {
        object.remove("$schema");
        object.remove("definitions");
        object.remove("title");
    }
    schema
}

fn endpoint_transport(endpoint: &ApiOperationMetadata) -> Option<&'static str> {
    (endpoint.endpoint.http_method == "ANY").then_some("websocket")
}

fn effective_http_method(endpoint: &ApiOperationMetadata) -> &'static str {
    if endpoint.endpoint.http_method == "ANY" {
        "GET"
    } else {
        endpoint.endpoint.http_method
    }
}

fn method_literal(endpoint: &ApiOperationMetadata) -> String {
    format!(
        "{} {}",
        endpoint.endpoint.http_method, endpoint.endpoint.path
    )
}

fn serialization_label(policy: SerializationPolicy) -> String {
    match policy {
        SerializationPolicy::Concurrent => "concurrent".to_string(),
        SerializationPolicy::PerProcess => "per-process".to_string(),
        SerializationPolicy::PerConnection => "per-connection".to_string(),
        SerializationPolicy::PerKey { field } => format!("per-key:{field}"),
    }
}

fn value_to_literal(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => ts_string_literal(value),
        _ => "JsonValue".to_string(),
    }
}

fn ts_property_name(name: &str) -> String {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return "\"\"".to_string();
    };

    let valid_first = first == '_' || first == '$' || first.is_ascii_alphabetic();
    let valid_rest = chars
        .all(|character| character == '_' || character == '$' || character.is_ascii_alphanumeric());

    if valid_first && valid_rest {
        name.to_string()
    } else {
        ts_string_literal(name)
    }
}

fn ts_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn typescript_string_array(values: &[&str]) -> String {
    let values = values
        .iter()
        .map(|value| ts_string_literal(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

fn typescript_owned_string_array(values: &[String]) -> String {
    let values = values
        .iter()
        .map(|value| ts_string_literal(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[allow(dead_code)]
    mod stream_fixture {
        use schemars::JsonSchema;
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
        pub struct FixtureEvent {
            pub message: String,
        }

        crate::api_definitions! {
            Fixture => "GET /fixture" {
                response: serde_json::Value,
                stream: [FixtureEvent],
            },
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct ConflictingFixtureEvent {
        count: u64,
    }

    fn temporary_manifest_dir(root: &Path) -> PathBuf {
        let manifest_dir = root.join("repository/crates/allthecodes-protocol");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        manifest_dir
    }

    #[test]
    fn typescript_output_contains_request_and_response_unions() {
        let output = generate_typescript_types();

        assert!(output.contains("export type V1ApiRequest"));
        assert!(output.contains("export type V1ApiResponse"));
        assert!(output.contains(r#"{ method: "GET /api/sessions"; params?: never }"#));
        assert!(
            output.contains(r#"{ method: "POST /api/sessions/new"; params: SessionCreateParams }"#)
        );
        assert!(output.contains("export interface SessionCreateParams"));
        assert!(!output.contains("export const V1_API_ROUTES"));
    }

    #[test]
    fn typescript_routes_output_contains_route_table() {
        let output = generate_typescript_routes();

        assert!(output.contains("export const V1_API_ROUTES"));
        assert!(output.contains("export type V1ApiOperation"));
        assert!(output.contains(r#"operation: "SessionResume""#));
    }

    #[test]
    fn route_markdown_lists_serialization_metadata() {
        let output = generate_route_markdown();

        assert!(output.contains("`SessionResume`"));
        assert!(output.contains("`per-key:id`"));
    }

    #[test]
    fn openapi_output_contains_paths_and_components() {
        let output = generate_openapi_value();

        assert!(output.pointer("/paths/~1api~1sessions/get").is_some());
        assert!(output
            .pointer("/components/schemas/SessionCreateParams")
            .is_some());
    }

    #[test]
    fn generated_manifest_is_complete_deterministic_and_digest_linked() {
        let first = generate_all_artifacts().unwrap();
        let second = generate_all_artifacts().unwrap();

        assert_eq!(first, second);
        assert_eq!(first.len(), 6);
        assert_eq!(
            first
                .iter()
                .filter(|artifact| artifact.owner == ArtifactOwner::Backend)
                .count(),
            3
        );
        assert_eq!(
            first
                .iter()
                .filter(|artifact| artifact.owner == ArtifactOwner::Frontend)
                .count(),
            3
        );

        let digest = protocol_digest();
        assert_eq!(digest.len(), 64);
        assert!(first
            .iter()
            .all(|artifact| artifact.contents.contains(&digest)));
    }

    #[test]
    fn route_rows_match_metadata_and_any_routes_are_websocket_gets() {
        let markdown = generate_route_markdown();
        let rows = markdown
            .lines()
            .filter(|line| line.starts_with("| `"))
            .count();
        assert_eq!(rows, API_METADATA.len());

        let openapi = generate_openapi_value();
        let websocket_operations = API_METADATA
            .iter()
            .filter(|endpoint| endpoint.endpoint.http_method == "ANY")
            .collect::<Vec<_>>();
        assert!(!websocket_operations.is_empty());
        for endpoint in websocket_operations {
            let path = openapi_path(endpoint.endpoint.path);
            let operation = &openapi["paths"][&path]["get"];
            assert_eq!(operation["x-allthecodes-source-method"], "ANY");
            assert_eq!(operation["x-allthecodes-transport"], "websocket");
            assert!(openapi["paths"][&path].get("any").is_none());
        }
    }

    #[test]
    fn schema_and_openapi_cover_every_declared_operation_and_root_type() {
        let schema = generate_schema_value();
        let endpoints = schema["endpoints"].as_array().unwrap();
        let schemas = schema["schemas"].as_object().unwrap();
        let openapi = generate_openapi_value();

        assert_eq!(endpoints.len(), API_METADATA.len());
        for (declared, generated) in API_METADATA.iter().zip(endpoints) {
            let operation = format!("{:?}", declared.endpoint.operation);
            assert_eq!(generated["operation"], operation);
            assert_eq!(generated["method"], declared.endpoint.http_method);
            assert_eq!(generated["path"], declared.endpoint.path);

            let path = openapi_path(declared.endpoint.path);
            let method = effective_http_method(declared).to_ascii_lowercase();
            assert_eq!(openapi["paths"][&path][&method]["operationId"], operation);

            for metadata in declared
                .params
                .iter()
                .chain(std::iter::once(&declared.response))
                .chain(declared.stream_events.iter())
            {
                let name = typescript_type_for_rust(metadata.rust_type);
                if name != "JsonValue" && !name.ends_with("[]") {
                    assert!(
                        schemas.contains_key(&name),
                        "schema is missing declared type {name} for {operation}"
                    );
                }
            }
        }
    }

    #[test]
    fn stream_metadata_reaches_schema_collectors_and_openapi_extension() {
        assert_eq!(stream_fixture::API_METADATA.len(), 1);
        assert_eq!(
            stream_fixture::API_METADATA[0].stream_events[0].rust_type,
            "FixtureEvent"
        );

        let stream_events: &'static [ApiTypeMetadata] =
            crate::__api_stream_metadata!(stream_fixture::FixtureEvent);
        let metadata = ApiOperationMetadata {
            endpoint: crate::request::ApiEndpoint {
                operation: crate::request::ApiMethod::Chat,
                http_method: "GET",
                path: "/fixture",
            },
            params: None,
            response: ApiTypeMetadata {
                rust_type: "serde_json::Value",
                schema: crate::request::schema_for::<serde_json::Value>,
            },
            stream_events,
            errors: &[],
            serialization: SerializationPolicy::Concurrent,
            experimental: None,
        };

        let schemas = collect_typescript_schemas(&[metadata]);
        assert!(schemas.contains_key("FixtureEvent"));

        let mut components = Map::new();
        collect_component_schema(&mut components, stream_events[0]);
        assert!(components.contains_key("FixtureEvent"));

        let operation = openapi_operation(&metadata);
        assert_eq!(operation["x-allthecodes-stream-events"][0], "FixtureEvent");
    }

    #[test]
    fn backend_resolution_does_not_require_a_frontend_directory() {
        let temp = tempdir().unwrap();
        let manifest_dir = temporary_manifest_dir(temp.path());
        let paths = artifact_paths(&manifest_dir, ArtifactTarget::BackendDocs, None).unwrap();

        assert_eq!(paths.len(), 3);
        assert!(paths
            .iter()
            .all(|path| path.starts_with(temp.path().join("repository/docs/api"))));
    }

    #[test]
    fn explicit_frontend_directory_wins_and_missing_directory_fails() {
        let temp = tempdir().unwrap();
        let explicit = temp.path().join("explicit");
        let environment = temp.path().join("environment");

        assert_eq!(
            resolve_frontend_generated_dir_with_env(Some(&explicit), Some(environment.clone()))
                .unwrap(),
            explicit
        );
        assert_eq!(
            resolve_frontend_generated_dir_with_env(None, Some(environment.clone())).unwrap(),
            environment
        );
        assert!(matches!(
            resolve_frontend_generated_dir_with_env(None, None),
            Err(CodegenError::MissingFrontendDirectory)
        ));
    }

    #[test]
    fn write_and_check_share_the_manifest() {
        let temp = tempdir().unwrap();
        let manifest_dir = temporary_manifest_dir(temp.path());
        let frontend_dir = temp.path().join("frontend-generated");
        let written =
            write_api_artifacts(&manifest_dir, ArtifactTarget::All, Some(&frontend_dir)).unwrap();
        let checked =
            check_api_artifacts_up_to_date(&manifest_dir, ArtifactTarget::All, Some(&frontend_dir))
                .unwrap();

        assert_eq!(written, checked);
        assert_eq!(written.len(), 6);
    }

    #[test]
    fn stale_check_reports_every_path_and_regeneration_command() {
        let temp = tempdir().unwrap();
        let manifest_dir = temporary_manifest_dir(temp.path());
        let paths = write_api_artifacts(&manifest_dir, ArtifactTarget::BackendDocs, None).unwrap();
        std::fs::write(&paths[0], "stale\n").unwrap();
        std::fs::remove_file(&paths[1]).unwrap();

        let error =
            check_api_artifacts_up_to_date(&manifest_dir, ArtifactTarget::BackendDocs, None)
                .unwrap_err()
                .to_string();
        assert!(error.contains(&paths[0].display().to_string()));
        assert!(error.contains("(stale)"));
        assert!(error.contains(&paths[1].display().to_string()));
        assert!(error.contains("(missing)"));
        assert!(error.contains(BACKEND_REGENERATION_COMMAND));
    }

    #[test]
    fn duplicate_manifest_paths_fail_before_writing() {
        let artifact = GeneratedArtifact {
            owner: ArtifactOwner::Backend,
            relative_path: "docs/api/duplicate.json",
            contents: "{}\n".to_string(),
        };
        assert!(matches!(
            validate_artifact_manifest(&[artifact.clone(), artifact]),
            Err(CodegenError::DuplicateOutputPath { .. })
        ));
    }

    #[test]
    fn staging_failure_preserves_existing_artifacts() {
        let temp = tempdir().unwrap();
        let existing = temp.path().join("existing.json");
        std::fs::write(&existing, "old\n").unwrap();
        let blocked_parent = temp.path().join("not-a-directory");
        std::fs::write(&blocked_parent, "block").unwrap();

        let artifacts = vec![
            ResolvedArtifact {
                artifact: GeneratedArtifact {
                    owner: ArtifactOwner::Backend,
                    relative_path: "existing.json",
                    contents: "new\n".to_string(),
                },
                path: existing.clone(),
            },
            ResolvedArtifact {
                artifact: GeneratedArtifact {
                    owner: ArtifactOwner::Backend,
                    relative_path: "blocked.json",
                    contents: "new\n".to_string(),
                },
                path: blocked_parent.join("blocked.json"),
            },
        ];

        assert!(write_resolved_artifacts(&artifacts).is_err());
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "old\n");
        assert!(std::fs::read_dir(temp.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")
        }));
    }

    #[test]
    fn conflicting_schema_names_are_rejected() {
        let first = ApiTypeMetadata {
            rust_type: "stream_fixture::FixtureEvent",
            schema: crate::request::schema_for::<stream_fixture::FixtureEvent>,
        };
        let second = ApiTypeMetadata {
            rust_type: "stream_fixture::FixtureEvent",
            schema: crate::request::schema_for::<ConflictingFixtureEvent>,
        };
        let mut schemas = BTreeMap::new();
        validate_type_schema(&mut schemas, first).unwrap();
        assert!(matches!(
            validate_type_schema(&mut schemas, second),
            Err(CodegenError::DuplicateSchema { .. })
        ));
    }
}

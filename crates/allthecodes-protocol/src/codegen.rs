//! Generated API artifacts from protocol metadata.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::error::ApiErrorBody;
use crate::notification::ServerNotification;
use crate::request::{ApiOperationMetadata, ApiTypeMetadata, SerializationPolicy, API_METADATA};
use crate::request::{ClientRequest, ClientResponse, EmptyResponse, NoParams};

const FRONTEND_GENERATED_RELATIVE_DIR: &str = "../../../allthecodes-web/src/lib/generated";
const FRONTEND_GENERATED_FIX_BUGS_RELATIVE_DIR: &str =
    "../../../allthecodes-web-fix-bugs/src/lib/generated";
const FRONTEND_GENERATED_DIR_ENV: &str = "ALLTHECODES_FRONTEND_GENERATED_DIR";
const FRONTEND_TYPES_FILE: &str = "api-types.ts";
const FRONTEND_ROUTES_FILE: &str = "api-routes.ts";
const FRONTEND_SCHEMA_FILE: &str = "api-schema.json";

#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("failed to serialize generated JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("generated output is stale: {path}")]
    Stale { path: PathBuf },
}

pub fn default_frontend_types_path(manifest_dir: &Path) -> PathBuf {
    default_frontend_generated_dir(manifest_dir).join(FRONTEND_TYPES_FILE)
}

pub fn default_frontend_routes_path(manifest_dir: &Path) -> PathBuf {
    default_frontend_generated_dir(manifest_dir).join(FRONTEND_ROUTES_FILE)
}

pub fn default_frontend_schema_path(manifest_dir: &Path) -> PathBuf {
    default_frontend_generated_dir(manifest_dir).join(FRONTEND_SCHEMA_FILE)
}

pub fn default_frontend_generated_dir(manifest_dir: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os(FRONTEND_GENERATED_DIR_ENV) {
        return PathBuf::from(path);
    }
    let fix_bugs_dir = manifest_dir.join(FRONTEND_GENERATED_FIX_BUGS_RELATIVE_DIR);
    if fix_bugs_dir.exists() {
        return fix_bugs_dir;
    }
    manifest_dir.join(FRONTEND_GENERATED_RELATIVE_DIR)
}

pub fn default_frontend_artifact_paths(manifest_dir: &Path) -> Vec<PathBuf> {
    vec![
        default_frontend_types_path(manifest_dir),
        default_frontend_routes_path(manifest_dir),
        default_frontend_schema_path(manifest_dir),
    ]
}

pub fn write_frontend_api_artifacts(manifest_dir: &Path) -> Result<Vec<PathBuf>, CodegenError> {
    let outputs = frontend_artifact_outputs(manifest_dir)?;
    for (path, contents) in &outputs {
        write_if_changed(path, contents)?;
    }
    Ok(outputs.into_iter().map(|(path, _)| path).collect())
}

pub fn check_frontend_api_artifacts_up_to_date(manifest_dir: &Path) -> Result<(), CodegenError> {
    for (path, expected) in frontend_artifact_outputs(manifest_dir)? {
        let actual = std::fs::read_to_string(&path).map_err(|source| CodegenError::Read {
            path: path.clone(),
            source,
        })?;

        if actual != expected {
            return Err(CodegenError::Stale { path });
        }
    }

    Ok(())
}

pub fn write_frontend_types(manifest_dir: &Path) -> Result<PathBuf, CodegenError> {
    let path = default_frontend_types_path(manifest_dir);
    write_if_changed(&path, &generate_typescript_types())?;
    Ok(path)
}

pub fn check_frontend_types_up_to_date(manifest_dir: &Path) -> Result<(), CodegenError> {
    let path = default_frontend_types_path(manifest_dir);
    let expected = generate_typescript_types();
    let actual = std::fs::read_to_string(&path).map_err(|source| CodegenError::Read {
        path: path.clone(),
        source,
    })?;

    if actual == expected {
        Ok(())
    } else {
        Err(CodegenError::Stale { path })
    }
}

fn frontend_artifact_outputs(manifest_dir: &Path) -> Result<Vec<(PathBuf, String)>, CodegenError> {
    Ok(vec![
        (
            default_frontend_types_path(manifest_dir),
            generate_typescript_types(),
        ),
        (
            default_frontend_routes_path(manifest_dir),
            generate_typescript_routes(),
        ),
        (
            default_frontend_schema_path(manifest_dir),
            generate_schema_json_pretty()?,
        ),
    ])
}

pub fn write_if_changed(path: &Path, contents: &str) -> Result<(), CodegenError> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        if existing == contents {
            return Ok(());
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| CodegenError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    std::fs::write(path, contents).map_err(|source| CodegenError::Write {
        path: path.to_path_buf(),
        source,
    })
}

pub fn generate_typescript_types() -> String {
    let schemas = collect_typescript_schemas();
    let mut output = String::new();

    output.push_str("// AUTO-GENERATED by allthecodes-protocol. Do not edit by hand.\n");
    output.push_str(
        "// Run `cargo run -p allthecodes-protocol --features codegen --bin codegen` from the backend repo.\n\n",
    );
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
    let mut output = String::new();

    output.push_str("// AUTO-GENERATED by allthecodes-protocol. Do not edit by hand.\n");
    output.push_str(
        "// Run `cargo run -p allthecodes-protocol --features codegen --bin codegen` from the backend repo.\n\n",
    );
    output.push_str("export interface V1ApiRoute {\n");
    output.push_str("  operation: string;\n");
    output.push_str("  method: string;\n");
    output.push_str("  path: string;\n");
    output.push_str("  params?: string;\n");
    output.push_str("  response: string;\n");
    output.push_str("  serialization: string;\n");
    output.push_str("  errors: readonly string[];\n");
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
    let schema = generate_schema_value();
    Ok(format!("{}\n", serde_json::to_string_pretty(&schema)?))
}

pub fn generate_schema_value() -> Value {
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
    }

    json!({
        "_generated": {
            "generator": "allthecodes-protocol",
            "version": "v1",
        },
        "version": "v1",
        "endpoints": endpoint_values(),
        "schemas": schemas,
    })
}

pub fn generate_route_markdown() -> String {
    let mut output = String::new();
    output.push_str("# allthecodes API Routes\n\n");
    output.push_str("Generated from `allthecodes-protocol` metadata.\n\n");
    output.push_str("| Operation | Method | Path | Params | Response | Serialization | Errors | Experimental |\n");
    output.push_str("|---|---|---|---|---|---|---|---|\n");

    for endpoint in API_METADATA {
        output.push('|');
        output.push_str(&format!(" `{:?}` |", endpoint.endpoint.operation));
        output.push_str(&format!(" `{}` |", endpoint.endpoint.http_method));
        output.push_str(&format!(" `{}` |", endpoint.endpoint.path));
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
    let openapi = generate_openapi_value();
    Ok(format!("{}\n", serde_json::to_string_pretty(&openapi)?))
}

pub fn generate_openapi_value() -> Value {
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
        },
        "paths": paths,
        "components": {
            "schemas": schemas,
        },
    })
}

fn collect_typescript_schemas() -> BTreeMap<String, Value> {
    let mut schemas = BTreeMap::new();

    for endpoint in API_METADATA {
        if let Some(params) = endpoint.params {
            collect_typescript_schema(&mut schemas, params);
        }
        collect_typescript_schema(&mut schemas, endpoint.response);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

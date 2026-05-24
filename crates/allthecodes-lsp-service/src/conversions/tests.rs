use super::*;
use serde_json::json;

// -- URI helpers --------------------------------------------------------

#[test]
fn test_uri_to_file_path_unix() {
    let uri: lsp_types::Uri = "file:///home/user/project/main.rs".parse().unwrap();
    let path = uri_to_file_path(&uri);
    #[cfg(windows)]
    assert_eq!(path, "home\\user\\project\\main.rs");
    #[cfg(not(windows))]
    assert_eq!(path, "/home/user/project/main.rs");
}

#[cfg(windows)]
#[test]
fn test_uri_to_file_path_windows() {
    let uri: lsp_types::Uri = "file:///C:/Users/dev/project/main.rs".parse().unwrap();
    let path = uri_to_file_path(&uri);
    assert_eq!(path, "C:\\Users\\dev\\project\\main.rs");
}

#[test]
fn test_file_path_to_uri_unix() {
    let uri = file_path_to_uri("/home/user/main.rs").unwrap();
    assert_eq!(uri.as_str(), "file:///home/user/main.rs");
}

#[test]
fn test_file_path_to_uri_windows_backslash() {
    let uri = file_path_to_uri("C:\\Users\\dev\\main.rs").unwrap();
    assert_eq!(uri.as_str(), "file:///C:/Users/dev/main.rs");
}

#[test]
fn test_file_path_to_uri_windows_forward_slash() {
    let uri = file_path_to_uri("D:/projects/src/lib.rs").unwrap();
    assert_eq!(uri.as_str(), "file:///D:/projects/src/lib.rs");
}

#[test]
fn test_uri_roundtrip_unix() {
    let original = "/tmp/test/file.rs";
    let uri = file_path_to_uri(original).unwrap();
    let _back = uri_to_file_path(&uri);
    #[cfg(not(windows))]
    assert_eq!(_back, original);
}

// -- Location conversion -----------------------------------------------

#[test]
fn test_location_to_source_zero_to_one_based() {
    let uri: lsp_types::Uri = "file:///src/main.rs".parse().unwrap();
    let loc = lsp_types::Location {
        uri,
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: lsp_types::Position {
                line: 0,
                character: 5,
            },
        },
    };
    let src = location_to_source(&loc);
    assert_eq!(src.line, 1, "0-based line 0 should become 1");
    assert_eq!(src.character, 1, "0-based char 0 should become 1");
    assert_eq!(src.end_line, Some(1));
    assert_eq!(src.end_character, Some(6));
}

#[test]
fn test_location_to_source_nonzero() {
    let uri: lsp_types::Uri = "file:///src/lib.rs".parse().unwrap();
    let loc = lsp_types::Location {
        uri,
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: 9,
                character: 4,
            },
            end: lsp_types::Position {
                line: 15,
                character: 0,
            },
        },
    };
    let src = location_to_source(&loc);
    assert_eq!(src.line, 10);
    assert_eq!(src.character, 5);
    assert_eq!(src.end_line, Some(16));
    assert_eq!(src.end_character, Some(1));
}

#[test]
fn test_location_link_to_source() {
    let link = lsp_types::LocationLink {
        origin_selection_range: None,
        target_uri: "file:///src/types.rs".parse().unwrap(),
        target_range: lsp_types::Range {
            start: lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: lsp_types::Position {
                line: 50,
                character: 0,
            },
        },
        target_selection_range: lsp_types::Range {
            start: lsp_types::Position {
                line: 3,
                character: 10,
            },
            end: lsp_types::Position {
                line: 3,
                character: 20,
            },
        },
    };
    let src = location_link_to_source(&link);
    assert_eq!(src.line, 4);
    assert_eq!(src.character, 11);
    assert_eq!(src.end_line, Some(4));
    assert_eq!(src.end_character, Some(21));
}

// -- parse_location_response -------------------------------------------

#[test]
fn test_parse_location_response_null() {
    let result = parse_location_response(Value::Null).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_parse_location_response_single() {
    let json = json!({
        "uri": "file:///src/main.rs",
        "range": {
            "start": {"line": 5, "character": 0},
            "end": {"line": 5, "character": 10}
        }
    });
    let result = parse_location_response(json).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].line, 6);
    assert_eq!(result[0].character, 1);
}

#[test]
fn test_parse_location_response_array() {
    let json = json!([
        {
            "uri": "file:///a.rs",
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 5}
            }
        },
        {
            "uri": "file:///b.rs",
            "range": {
                "start": {"line": 10, "character": 3},
                "end": {"line": 10, "character": 8}
            }
        }
    ]);
    let result = parse_location_response(json).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].line, 1);
    assert_eq!(result[1].line, 11);
    assert_eq!(result[1].character, 4);
}

#[test]
fn test_parse_location_response_link_array() {
    let json = json!([
        {
            "targetUri": "file:///src/lib.rs",
            "targetRange": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 20, "character": 0}
            },
            "targetSelectionRange": {
                "start": {"line": 5, "character": 4},
                "end": {"line": 5, "character": 15}
            }
        }
    ]);
    let result = parse_location_response(json).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].line, 6);
    assert_eq!(result[0].character, 5);
}

// -- Hover -------------------------------------------------------------

#[test]
fn test_parse_hover_response_null() {
    let hover = parse_hover_response(Value::Null).unwrap();
    assert!(hover.contents.is_empty());
    assert!(hover.range.is_none());
}

#[test]
fn test_parse_hover_response_markup() {
    let json = json!({
        "contents": {
            "kind": "markdown",
            "value": "```rust\nfn main()\n```"
        }
    });
    let hover = parse_hover_response(json).unwrap();
    assert_eq!(hover.contents, "```rust\nfn main()\n```");
    assert!(hover.range.is_none());
}

#[test]
fn test_parse_hover_response_scalar_string() {
    let json = json!({
        "contents": "Hello hover"
    });
    let hover = parse_hover_response(json).unwrap();
    assert_eq!(hover.contents, "Hello hover");
}

#[test]
fn test_parse_hover_response_language_string() {
    let json = json!({
        "contents": {"language": "rust", "value": "fn foo()"}
    });
    let hover = parse_hover_response(json).unwrap();
    assert!(hover.contents.contains("```rust"));
    assert!(hover.contents.contains("fn foo()"));
}

#[test]
fn test_parse_hover_response_array() {
    let json = json!({
        "contents": [
            "Some doc",
            {"language": "go", "value": "func Bar()"}
        ]
    });
    let hover = parse_hover_response(json).unwrap();
    assert!(hover.contents.contains("Some doc"));
    assert!(hover.contents.contains("```go"));
    assert!(hover.contents.contains("func Bar()"));
}

#[test]
fn test_parse_hover_with_range() {
    let json = json!({
        "contents": {"kind": "plaintext", "value": "int x"},
        "range": {
            "start": {"line": 2, "character": 4},
            "end": {"line": 2, "character": 9}
        }
    });
    let hover = parse_hover_response(json).unwrap();
    assert_eq!(hover.contents, "int x");
    let r = hover.range.unwrap();
    assert_eq!(r.line, 3);
    assert_eq!(r.character, 5);
}

// -- Completion --------------------------------------------------------

#[test]
fn test_parse_completion_response_array() {
    let json = json!([
        {
            "label": "println!",
            "kind": 3,
            "detail": "macro_rules! println",
            "insertText": "println!($0)"
        }
    ]);
    let result = parse_completion_response(json).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].label, "println!");
    assert_eq!(result[0].kind.as_deref(), Some("Function"));
    assert_eq!(result[0].insert_text.as_deref(), Some("println!($0)"));
}

#[test]
fn test_parse_completion_response_list() {
    let json = json!({
        "isIncomplete": false,
        "items": [{
            "label": "String",
            "kind": 7,
            "documentation": {
                "kind": "markdown",
                "value": "Owned UTF-8 string"
            }
        }]
    });
    let result = parse_completion_response(json).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].label, "String");
    assert_eq!(result[0].kind.as_deref(), Some("Class"));
    assert_eq!(
        result[0].documentation.as_deref(),
        Some("Owned UTF-8 string")
    );
}

#[test]
fn test_parse_completion_response_null() {
    let result = parse_completion_response(Value::Null).unwrap();
    assert!(result.is_empty());
}

// -- Symbol kind -------------------------------------------------------

#[test]
fn test_symbol_kind_str_all_variants() {
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::FILE), "File");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::MODULE), "Module");
    assert_eq!(
        symbol_kind_str(lsp_types::SymbolKind::NAMESPACE),
        "Namespace"
    );
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::PACKAGE), "Package");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::CLASS), "Class");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::METHOD), "Method");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::PROPERTY), "Property");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::FIELD), "Field");
    assert_eq!(
        symbol_kind_str(lsp_types::SymbolKind::CONSTRUCTOR),
        "Constructor"
    );
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::ENUM), "Enum");
    assert_eq!(
        symbol_kind_str(lsp_types::SymbolKind::INTERFACE),
        "Interface"
    );
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::FUNCTION), "Function");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::VARIABLE), "Variable");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::CONSTANT), "Constant");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::STRING), "String");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::NUMBER), "Number");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::BOOLEAN), "Boolean");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::ARRAY), "Array");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::OBJECT), "Object");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::KEY), "Key");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::NULL), "Null");
    assert_eq!(
        symbol_kind_str(lsp_types::SymbolKind::ENUM_MEMBER),
        "EnumMember"
    );
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::STRUCT), "Struct");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::EVENT), "Event");
    assert_eq!(symbol_kind_str(lsp_types::SymbolKind::OPERATOR), "Operator");
    assert_eq!(
        symbol_kind_str(lsp_types::SymbolKind::TYPE_PARAMETER),
        "TypeParameter"
    );
}

// -- Document symbols --------------------------------------------------

#[test]
fn test_parse_document_symbols_null() {
    let result = parse_document_symbols_response(Value::Null).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_parse_document_symbols_flat() {
    let json = json!([
        {
            "name": "MyClass",
            "kind": 5,
            "location": {
                "uri": "file:///src/lib.rs",
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 10, "character": 0}
                }
            }
        }
    ]);
    let result = parse_document_symbols_response(json).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "MyClass");
    assert_eq!(result[0].kind, "Class");
    assert_eq!(result[0].location.line, 1);
}

#[test]
#[allow(deprecated)] // DocumentSymbol.deprecated field
fn test_parse_document_symbols_nested_with_children() {
    let json = json!([
        {
            "name": "MyStruct",
            "kind": 23,
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 20, "character": 1}
            },
            "selectionRange": {
                "start": {"line": 0, "character": 4},
                "end": {"line": 0, "character": 12}
            },
            "children": [
                {
                    "name": "field_a",
                    "kind": 8,
                    "range": {
                        "start": {"line": 1, "character": 4},
                        "end": {"line": 1, "character": 20}
                    },
                    "selectionRange": {
                        "start": {"line": 1, "character": 4},
                        "end": {"line": 1, "character": 11}
                    }
                }
            ]
        }
    ]);
    let result = parse_document_symbols_response(json).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "MyStruct");
    assert_eq!(result[0].kind, "Struct");
    assert_eq!(result[0].location.line, 1);
    assert_eq!(result[0].location.character, 5);
    assert_eq!(result[0].children.len(), 1);
    assert_eq!(result[0].children[0].name, "field_a");
    assert_eq!(result[0].children[0].kind, "Field");
    assert_eq!(result[0].children[0].location.line, 2);
}

// -- Workspace symbols -------------------------------------------------

#[test]
fn test_parse_workspace_symbols_null() {
    let result = parse_workspace_symbols_response(Value::Null).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_parse_workspace_symbols_flat() {
    let json = json!([
        {
            "name": "global_var",
            "kind": 13,
            "location": {
                "uri": "file:///src/globals.rs",
                "range": {
                    "start": {"line": 4, "character": 0},
                    "end": {"line": 4, "character": 20}
                }
            }
        }
    ]);
    let result = parse_workspace_symbols_response(json).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "global_var");
    assert_eq!(result[0].kind, "Variable");
    assert_eq!(result[0].location.line, 5);
}

// -- Call hierarchy ----------------------------------------------------

#[test]
fn test_call_hierarchy_item_to_info() {
    let item = lsp_types::CallHierarchyItem {
        name: "do_work".into(),
        kind: lsp_types::SymbolKind::FUNCTION,
        tags: None,
        detail: None,
        uri: "file:///src/work.rs".parse().unwrap(),
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: 10,
                character: 0,
            },
            end: lsp_types::Position {
                line: 25,
                character: 1,
            },
        },
        selection_range: lsp_types::Range {
            start: lsp_types::Position {
                line: 10,
                character: 4,
            },
            end: lsp_types::Position {
                line: 10,
                character: 11,
            },
        },
        data: None,
    };
    let info = call_hierarchy_item_to_info(&item);
    assert_eq!(info.name, "do_work");
    assert_eq!(info.kind, "Function");
    assert_eq!(info.location.line, 11);
    assert_eq!(info.location.character, 5);
}

#[test]
fn test_parse_call_hierarchy_items_null() {
    let result = parse_call_hierarchy_items(Value::Null).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_parse_call_hierarchy_items() {
    let json = json!([
        {
            "name": "init",
            "kind": 12,
            "uri": "file:///src/app.rs",
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 5, "character": 1}
            },
            "selectionRange": {
                "start": {"line": 0, "character": 3},
                "end": {"line": 0, "character": 7}
            }
        }
    ]);
    let result = parse_call_hierarchy_items(json).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "init");
    assert_eq!(result[0].kind, "Function");
    assert_eq!(result[0].location.line, 1);
    assert_eq!(result[0].location.character, 4);
}

#[test]
fn test_parse_incoming_calls() {
    let json = json!([
        {
            "from": {
                "name": "caller_fn",
                "kind": 12,
                "uri": "file:///src/caller.rs",
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 10, "character": 1}
                },
                "selectionRange": {
                    "start": {"line": 0, "character": 3},
                    "end": {"line": 0, "character": 12}
                }
            },
            "fromRanges": [
                {
                    "start": {"line": 5, "character": 4},
                    "end": {"line": 5, "character": 15}
                }
            ]
        }
    ]);
    let result = parse_incoming_calls(json).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "caller_fn");
    assert_eq!(result[0].kind, "Function");
}

#[test]
fn test_parse_outgoing_calls() {
    let json = json!([
        {
            "to": {
                "name": "callee_fn",
                "kind": 12,
                "uri": "file:///src/callee.rs",
                "range": {
                    "start": {"line": 20, "character": 0},
                    "end": {"line": 30, "character": 1}
                },
                "selectionRange": {
                    "start": {"line": 20, "character": 3},
                    "end": {"line": 20, "character": 12}
                }
            },
            "fromRanges": [
                {
                    "start": {"line": 7, "character": 8},
                    "end": {"line": 7, "character": 17}
                }
            ]
        }
    ]);
    let result = parse_outgoing_calls(json).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "callee_fn");
    assert_eq!(result[0].kind, "Function");
    assert_eq!(result[0].location.line, 21);
}

#[test]
fn test_parse_incoming_calls_null() {
    let result = parse_incoming_calls(Value::Null).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_parse_outgoing_calls_null() {
    let result = parse_outgoing_calls(Value::Null).unwrap();
    assert!(result.is_empty());
}

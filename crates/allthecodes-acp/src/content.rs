//! ACP content block conversion.
//!
//! Converts ACP v2 ContentBlock variants into a single allthecodes prompt string
//! for submission via QueryEngine::submit_message_with_overrides.

use agent_client_protocol_schema::v2::ContentBlock;
use agent_client_protocol_schema::v2::Error;

/// Convert ACP content blocks into a flattened prompt string.
///
/// Returns an error for unsupported content block types (image, audio, embedded
/// resource) that have not yet been implemented.
pub fn convert_prompt_blocks(blocks: &[ContentBlock]) -> Result<String, Error> {
    if blocks.is_empty() {
        return Err(Error::invalid_params().data("empty prompt"));
    }

    let mut parts: Vec<String> = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text(text_content) => {
                parts.push(text_content.text.clone());
            }
            ContentBlock::ResourceLink(resource_link) => {
                let uri_str = resource_link.uri.as_str();
                if let Ok(uri) = url::Url::parse(uri_str) {
                    match uri.scheme() {
                        "file" => {
                            if let Ok(path) = uri.to_file_path() {
                                let path_str = path.to_string_lossy().to_string();
                                parts.push(format!("@/{}", path_str));
                            } else {
                                parts.push(format!("[resource_link: {} {}]", resource_link.name, uri_str));
                            }
                        }
                        _ => {
                            parts.push(format!("[resource_link: {} {}]", resource_link.name, uri_str));
                        }
                    }
                } else {
                    parts.push(format!("[resource_link: {} {}]", resource_link.name, uri_str));
                }
            }
            ContentBlock::Image(_) => {
                return Err(Error::invalid_params().data(
                    "image content blocks are not yet supported"
                ));
            }
            ContentBlock::Audio(_) => {
                return Err(Error::invalid_params().data(
                    "audio content blocks are not yet supported"
                ));
            }
            ContentBlock::Resource(_) => {
                return Err(Error::invalid_params().data(
                    "embedded resource content blocks are not yet supported"
                ));
            }
            _ => {
                continue;
            }
        }
    }

    let result = parts.join("\n").trim().to_string();
    if result.is_empty() {
        return Err(Error::invalid_params().data("empty prompt after content conversion"));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol_schema::v2::TextContent;

    #[test]
    fn text_prompt_preserves_lines() {
        let blocks = vec![
            ContentBlock::Text(TextContent::new("Hello world")),
        ];
        let result = convert_prompt_blocks(&blocks).unwrap();
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn resource_link_file_uri_maps_to_at_path() {
        let blocks = vec![
            ContentBlock::ResourceLink(
                agent_client_protocol_schema::v2::ResourceLink::new(
                    "file", "file:///home/user/file.txt",
                ),
            ),
        ];
        let result = convert_prompt_blocks(&blocks).unwrap();
        assert!(result.contains("@/"));
        assert!(result.contains("/home/user/file.txt"));
    }

    #[test]
    fn unsupported_image_is_invalid_params() {
        let blocks = vec![
            ContentBlock::Image(agent_client_protocol_schema::v2::ImageContent::new(
                "AAAA".to_string(),
                "image/png".to_string(),
            )),
        ];
        let result = convert_prompt_blocks(&blocks);
        assert!(result.is_err());
    }

    #[test]
    fn empty_prompt_is_invalid_params() {
        let result = convert_prompt_blocks(&[]);
        assert!(result.is_err());
    }
}

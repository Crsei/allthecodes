//! Search provider implementations (Tavily, Brave).

use anyhow::{Context, Result};
use serde_json::json;

use allthecodes_config::runtime_settings::SettingsJson;

use super::{
    BraveSearchResponse, SearchProvider, SearchResultEntry, TavilySearchResponse, BRAVE_API_URL,
    TAVILY_API_URL,
};

pub(super) fn detect_provider_from_settings(settings: &SettingsJson) -> Option<SearchProvider> {
    let provider = settings
        .web_search_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match provider {
        Some("tavily") => {
            configured_key(settings.web_search_tavily_api_key.clone()).map(SearchProvider::Tavily)
        }
        Some("brave") => {
            configured_key(settings.web_search_brave_api_key.clone()).map(SearchProvider::Brave)
        }
        Some(_) => None,
        None => detect_provider_from_keys(
            settings.web_search_tavily_api_key.clone(),
            settings.web_search_brave_api_key.clone(),
        ),
    }
}

pub(super) fn detect_provider_from_keys(
    tavily_key: Option<String>,
    brave_key: Option<String>,
) -> Option<SearchProvider> {
    configured_key(tavily_key)
        .map(SearchProvider::Tavily)
        .or_else(|| configured_key(brave_key).map(SearchProvider::Brave))
}

fn configured_key(key: Option<String>) -> Option<String> {
    key.map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Execute a Tavily search and return unified results.
pub(super) async fn search_tavily(
    client: &reqwest::Client,
    api_key: &str,
    query: &str,
    max_results: u32,
) -> Result<Vec<SearchResultEntry>> {
    let body = json!({
        "api_key": api_key,
        "query": query,
        "max_results": max_results,
        "search_depth": "basic",
    });

    let resp = client
        .post(TAVILY_API_URL)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("Tavily Search API request failed")?;

    let status = resp.status().as_u16();
    if status != 200 {
        let err_body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Tavily Search API returned HTTP {}: {}", status, err_body);
    }

    let tavily_resp: TavilySearchResponse = resp
        .json()
        .await
        .context("Failed to parse Tavily Search response")?;

    Ok(tavily_resp
        .results
        .into_iter()
        .map(|r| SearchResultEntry {
            title: r.title,
            url: r.url,
            description: r.content,
            age: r.published_date,
        })
        .collect())
}

/// Execute a Brave search and return unified results.
pub(super) async fn search_brave(
    client: &reqwest::Client,
    api_key: &str,
    query: &str,
    max_results: u32,
) -> Result<Vec<SearchResultEntry>> {
    let resp = client
        .get(BRAVE_API_URL)
        .header("Accept", "application/json")
        .header("Accept-Encoding", "gzip")
        .header("X-Subscription-Token", api_key)
        .query(&[("q", query), ("count", &max_results.to_string())])
        .send()
        .await
        .context("Brave Search API request failed")?;

    let status = resp.status().as_u16();
    if status != 200 {
        let err_body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Brave Search API returned HTTP {}: {}", status, err_body);
    }

    let search_resp: BraveSearchResponse = resp
        .json()
        .await
        .context("Failed to parse Brave Search response")?;

    let raw_results = search_resp.web.map(|w| w.results).unwrap_or_default();
    Ok(raw_results
        .into_iter()
        .map(|r| SearchResultEntry {
            title: r.title,
            url: r.url,
            description: r.description,
            age: r.age,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Inline unit tests (logic that lives in this file)
// The broader web_search integration tests are in tests.rs.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Brave response deserialization
    // -----------------------------------------------------------------------

    #[test]
    fn brave_response_full_deserialization() {
        let json_str = r#"{
            "web": {
                "results": [
                    {
                        "title": "Brave test",
                        "url": "https://brave.com",
                        "description": "A privacy-respecting search engine",
                        "age": "1 week ago"
                    }
                ]
            }
        }"#;
        let resp: BraveSearchResponse = serde_json::from_str(json_str).unwrap();
        let web = resp.web.expect("web field should be present");
        assert_eq!(web.results.len(), 1);
        assert_eq!(web.results[0].title, "Brave test");
        assert_eq!(web.results[0].url, "https://brave.com");
        assert_eq!(
            web.results[0].description,
            "A privacy-respecting search engine"
        );
        assert_eq!(web.results[0].age.as_deref(), Some("1 week ago"));
    }

    #[test]
    fn brave_response_missing_web_field_defaults_to_none() {
        let json_str = r#"{}"#;
        let resp: BraveSearchResponse = serde_json::from_str(json_str).unwrap();
        assert!(
            resp.web.is_none(),
            "missing web field should deserialize as None"
        );
    }

    #[test]
    fn brave_response_empty_results_list() {
        let json_str = r#"{"web": {"results": []}}"#;
        let resp: BraveSearchResponse = serde_json::from_str(json_str).unwrap();
        let web = resp.web.unwrap();
        assert!(web.results.is_empty());
    }

    #[test]
    fn brave_result_optional_fields_default_correctly() {
        // description and age are both optional / default
        let json_str = r#"{
            "web": {
                "results": [
                    {
                        "title": "No extras",
                        "url": "https://example.org"
                    }
                ]
            }
        }"#;
        let resp: BraveSearchResponse = serde_json::from_str(json_str).unwrap();
        let result = &resp.web.unwrap().results[0];
        assert_eq!(
            result.description, "",
            "description should default to empty string"
        );
        assert!(result.age.is_none(), "age should default to None");
    }
}

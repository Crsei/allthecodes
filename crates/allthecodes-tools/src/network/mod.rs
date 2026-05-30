//! Network tools and network safety helpers.

use std::net::IpAddr;
use std::sync::Arc;

use url::Url;

use crate::tool::Tools;

pub(crate) mod vault_http_fetch;
pub mod web_fetch;
pub mod web_search;

pub use vault_http_fetch::VaultHttpFetchTool;
pub use web_fetch::WebFetchTool;
pub use web_search::WebSearchTool;

pub fn tools() -> Tools {
    vec![
        Arc::new(WebFetchTool),
        Arc::new(WebSearchTool),
        Arc::new(VaultHttpFetchTool),
    ]
}

pub(crate) fn host_is_blocked(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(ip) => {
                ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
            }
            IpAddr::V6(ip) => ip.is_loopback() || ip.is_unspecified(),
        };
    }
    false
}

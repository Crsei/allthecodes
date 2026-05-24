//! BCP-47 / alias → dictation-language-code normalization (issue #13).
//!
//! Mirrors `claude-code-bun/src/hooks/useVoice.ts :: normalizeLanguageForSTT`.
//! The Anthropic `voice_stream` endpoint only accepts a handful of
//! language codes; anything else silently falls back to English, which
//! surprises users. Surfacing the fallback up-front is the acceptance
//! criterion "language affects dictation language".
//!
//! # Supported matrix
//!
//! | User setting (case-insensitive)        | Normalized code |
//! |----------------------------------------|-----------------|
//! | `en`, `en-us`, `en_us`, `english`      | `en`            |
//! | `zh`, `zh-cn`, `zh-hans`, `chinese`    | `zh`            |
//! | `ja`, `ja-jp`, `japanese`              | `ja`            |
//! | `ko`, `ko-kr`, `korean`                | `ko`            |
//! | `es`, `es-es`, `es-mx`, `spanish`      | `es`            |
//! | `fr`, `fr-fr`, `french`                | `fr`            |
//! | `de`, `de-de`, `german`                | `de`            |
//! | `it`, `it-it`, `italian`               | `it`            |
//! | `pt`, `pt-br`, `pt-pt`, `portuguese`   | `pt`            |
//! | `ru`, `ru-ru`, `russian`               | `ru`            |
//! | anything else                          | `en` (fallback) |
//!
//! When a non-empty input falls back, [`NormalizedLanguage::fell_back_from`]
//! carries the original string so `/voice` can warn the user.

/// Result of normalizing a user's `language` setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedLanguage {
    /// 2-letter ISO-639-1 code understood by the STT endpoint.
    pub code: String,
    /// If set, the raw user value that we couldn't map — useful for
    /// building a "using English; please pick a supported code" hint.
    pub fell_back_from: Option<String>,
}

impl NormalizedLanguage {
    /// Helper for tests / callers that just want the code.
    pub fn as_str(&self) -> &str {
        &self.code
    }
}

/// Best-effort mapping to a supported dictation language code. Falls
/// back to English (`en`) for unknown / empty inputs.
///
/// The matching is loose:
///   - leading / trailing whitespace ignored
///   - case-insensitive
///   - `-` and `_` treated identically (so `en-US`, `en_us`, `EN-us` all work)
///   - full display names work too (`"english"`, `"中文"` (Simplified))
///
/// This deliberately accepts a superset of what the UI offers so users
/// editing `settings.json` by hand can paste `en-GB` or `zh-Hans`
/// without surprises.
pub fn normalize_language_for_stt(raw: Option<&str>) -> NormalizedLanguage {
    let Some(raw) = raw else {
        return NormalizedLanguage {
            code: "en".to_string(),
            fell_back_from: None,
        };
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return NormalizedLanguage {
            code: "en".to_string(),
            fell_back_from: None,
        };
    }

    let normalized = trimmed.to_ascii_lowercase().replace('_', "-");
    let base = normalized
        .split('-')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    // Exact match (both fully-qualified variants and display names).
    let code: Option<&'static str> = match normalized.as_str() {
        "en" | "en-us" | "en-gb" | "en-au" | "en-ca" | "en-in" | "english" => Some("en"),
        "zh" | "zh-cn" | "zh-hans" | "zh-tw" | "zh-hant" | "chinese" | "中文" | "中文（简体）"
        | "中文（繁体）" => Some("zh"),
        "ja" | "ja-jp" | "japanese" | "日本語" => Some("ja"),
        "ko" | "ko-kr" | "korean" | "한국어" => Some("ko"),
        "es" | "es-es" | "es-mx" | "es-us" | "spanish" | "español" => Some("es"),
        "fr" | "fr-fr" | "fr-ca" | "french" | "français" => Some("fr"),
        "de" | "de-de" | "de-at" | "de-ch" | "german" | "deutsch" => Some("de"),
        "it" | "it-it" | "italian" | "italiano" => Some("it"),
        "pt" | "pt-br" | "pt-pt" | "portuguese" | "português" => Some("pt"),
        "ru" | "ru-ru" | "russian" | "русский" => Some("ru"),
        _ => None,
    };
    if let Some(c) = code {
        return NormalizedLanguage {
            code: c.to_string(),
            fell_back_from: None,
        };
    }

    // Base-tag fallback (e.g. `en-XY` → `en` even if `en-xy` isn't listed).
    let base_code: Option<&'static str> = match base.as_str() {
        "en" => Some("en"),
        "zh" => Some("zh"),
        "ja" => Some("ja"),
        "ko" => Some("ko"),
        "es" => Some("es"),
        "fr" => Some("fr"),
        "de" => Some("de"),
        "it" => Some("it"),
        "pt" => Some("pt"),
        "ru" => Some("ru"),
        _ => None,
    };
    if let Some(c) = base_code {
        return NormalizedLanguage {
            code: c.to_string(),
            fell_back_from: None,
        };
    }

    NormalizedLanguage {
        code: "en".to_string(),
        fell_back_from: Some(trimmed.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_language_defaults_to_english_without_fallback_hint() {
        let r = normalize_language_for_stt(None);
        assert_eq!(r.code, "en");
        assert!(r.fell_back_from.is_none());
    }

    #[test]
    fn empty_string_defaults_to_english() {
        let r = normalize_language_for_stt(Some("   "));
        assert_eq!(r.code, "en");
        assert!(r.fell_back_from.is_none());
    }

    #[test]
    fn common_bcp47_variants_map_to_base_code() {
        for raw in ["en-US", "en_us", "EN-us", "English", "english"] {
            assert_eq!(
                normalize_language_for_stt(Some(raw)).code,
                "en",
                "raw={}",
                raw
            );
        }
        for raw in ["zh-CN", "zh_hans", "CHINESE", "中文"] {
            assert_eq!(
                normalize_language_for_stt(Some(raw)).code,
                "zh",
                "raw={}",
                raw
            );
        }
        for raw in ["es-mx", "Español", "ES-ES"] {
            assert_eq!(
                normalize_language_for_stt(Some(raw)).code,
                "es",
                "raw={}",
                raw
            );
        }
    }

    #[test]
    fn unknown_regional_code_falls_back_to_base_tag() {
        let r = normalize_language_for_stt(Some("en-GB"));
        assert_eq!(r.code, "en");
        assert!(r.fell_back_from.is_none());

        let r = normalize_language_for_stt(Some("zh-hant"));
        assert_eq!(r.code, "zh");
    }

    #[test]
    fn totally_unknown_language_falls_back_to_english_with_hint() {
        let r = normalize_language_for_stt(Some("klingon"));
        assert_eq!(r.code, "en");
        assert_eq!(r.fell_back_from.as_deref(), Some("klingon"));
    }

    #[test]
    fn whitespace_is_trimmed_before_matching() {
        let r = normalize_language_for_stt(Some("  FR-ca  "));
        assert_eq!(r.code, "fr");
        assert!(r.fell_back_from.is_none());
    }
}

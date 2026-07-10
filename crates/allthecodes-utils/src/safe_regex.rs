use std::borrow::Cow;

use regex::{Captures, Match, Regex};

/// A lazily compiled regex that fails closed instead of panicking if a static
/// pattern is invalid.
pub(crate) struct SafeRegex(Result<Regex, regex::Error>);

impl SafeRegex {
    pub(crate) fn new(pattern: &str) -> Self {
        Self(Regex::new(pattern))
    }

    pub(crate) fn is_match(&self, haystack: &str) -> bool {
        self.0.as_ref().is_ok_and(|regex| regex.is_match(haystack))
    }

    pub(crate) fn captures<'h>(&self, haystack: &'h str) -> Option<Captures<'h>> {
        self.0
            .as_ref()
            .ok()
            .and_then(|regex| regex.captures(haystack))
    }

    pub(crate) fn find<'h>(&self, haystack: &'h str) -> Option<Match<'h>> {
        self.0.as_ref().ok().and_then(|regex| regex.find(haystack))
    }

    pub(crate) fn replace_all<'h, R>(&self, haystack: &'h str, replacement: R) -> Cow<'h, str>
    where
        R: regex::Replacer,
    {
        self.0.as_ref().map_or_else(
            |_| Cow::Borrowed(haystack),
            |regex| regex.replace_all(haystack, replacement),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::SafeRegex;

    #[test]
    fn invalid_pattern_fails_closed() {
        let regex = SafeRegex::new("(");

        assert!(!regex.is_match("anything"));
        assert!(regex.find("anything").is_none());
        assert!(regex.captures("anything").is_none());
        assert_eq!(regex.replace_all("anything", "replacement"), "anything");
    }

    #[test]
    fn valid_pattern_delegates_to_regex() {
        let regex = SafeRegex::new(r"a+(b)");

        assert!(regex.is_match("aaab"));
        assert_eq!(
            regex.find("xxaaabyy").map(|found| found.as_str()),
            Some("aaab")
        );
        assert_eq!(
            regex
                .captures("aaab")
                .and_then(|captures| captures.get(1))
                .map(|matched| matched.as_str()),
            Some("b")
        );
        assert_eq!(regex.replace_all("aaab", "x"), "x");
    }
}

//! Encode and decode linked mentions stored in prompt history.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedMention {
    pub name: String,
    pub target: String,
    pub start: usize,
    pub end: usize,
}

impl LinkedMention {
    pub fn new(
        name: impl Into<String>,
        target: impl Into<String>,
        start: usize,
        end: usize,
    ) -> Self {
        Self {
            name: name.into(),
            target: target.into(),
            start,
            end,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodedHistoryText {
    pub text: String,
    pub mentions: Vec<LinkedMention>,
}

pub fn encode_history_mentions(text: &str, mentions: &[LinkedMention]) -> String {
    if mentions.is_empty() {
        return text.to_string();
    }

    let mut ordered = mentions.to_vec();
    ordered.sort_by_key(|mention| mention.start);
    let mut out = String::new();
    let mut cursor = 0;
    for mention in ordered {
        if mention.start < cursor || mention.end > text.len() || mention.start >= mention.end {
            continue;
        }
        out.push_str(&text[cursor..mention.start]);
        let visible = &text[mention.start..mention.end];
        out.push_str(&format!(
            "[{}]({})",
            escape_link_text(visible),
            escape_link_target(&mention.target)
        ));
        cursor = mention.end;
    }
    out.push_str(&text[cursor..]);
    out
}

pub fn decode_history_mentions(text: &str) -> DecodedHistoryText {
    let mut decoded = DecodedHistoryText::default();
    let mut cursor = 0;
    while let Some(open_rel) = text[cursor..].find('[') {
        let open = cursor + open_rel;
        let Some(close_rel) = text[open..].find("](") else {
            break;
        };
        let close = open + close_rel;
        let target_start = close + 2;
        let Some(target_end_rel) = text[target_start..].find(')') else {
            break;
        };
        let target_end = target_start + target_end_rel;

        decoded.text.push_str(&text[cursor..open]);
        let start = decoded.text.len();
        let visible = unescape_link_text(&text[open + 1..close]);
        decoded.text.push_str(&visible);
        let end = decoded.text.len();
        let target = unescape_link_target(&text[target_start..target_end]);
        let name = visible.trim_start_matches('@').to_string();
        decoded
            .mentions
            .push(LinkedMention::new(name, target, start, end));
        cursor = target_end + 1;
    }
    decoded.text.push_str(&text[cursor..]);
    decoded
}

fn escape_link_text(text: &str) -> String {
    text.replace('[', "\\[").replace(']', "\\]")
}

fn unescape_link_text(text: &str) -> String {
    text.replace("\\[", "[").replace("\\]", "]")
}

fn escape_link_target(target: &str) -> String {
    target.replace(')', "%29")
}

fn unescape_link_target(target: &str) -> String {
    target.replace("%29", ")")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_linked_mentions() {
        let text = "open @file now";
        let encoded =
            encode_history_mentions(text, &[LinkedMention::new("file", "src/lib.rs", 5, 10)]);
        let decoded = decode_history_mentions(&encoded);
        assert_eq!(decoded.text, text);
        assert_eq!(decoded.mentions[0].target, "src/lib.rs");
    }
}

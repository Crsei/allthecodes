use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchBox<'a> {
    pub query: &'a str,
    pub placeholder: &'a str,
    pub is_focused: bool,
    pub is_terminal_focused: bool,
    pub prefix: &'a str,
    pub cursor_offset: Option<usize>,
    pub borderless: bool,
    pub width: Option<usize>,
}

impl<'a> SearchBox<'a> {
    pub fn new(query: &'a str) -> Self {
        Self {
            query,
            placeholder: "Search...",
            is_focused: true,
            is_terminal_focused: false,
            prefix: "/",
            cursor_offset: None,
            borderless: false,
            width: None,
        }
    }

    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    pub fn focused(mut self, is_focused: bool) -> Self {
        self.is_focused = is_focused;
        self
    }

    pub fn terminal_focused(mut self, is_terminal_focused: bool) -> Self {
        self.is_terminal_focused = is_terminal_focused;
        self
    }

    pub fn prefix(mut self, prefix: &'a str) -> Self {
        self.prefix = prefix;
        self
    }

    pub fn cursor_offset(mut self, cursor_offset: usize) -> Self {
        self.cursor_offset = Some(cursor_offset);
        self
    }

    pub fn borderless(mut self, borderless: bool) -> Self {
        self.borderless = borderless;
        self
    }

    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    pub fn render(&self) -> String {
        render_search_box(self)
    }
}

pub fn render_search_box(search_box: &SearchBox<'_>) -> String {
    let body = render_body(search_box);
    let content = format!("{} {}", search_box.prefix, body);
    let rendered = if search_box.borderless {
        content
    } else {
        format!("[ {content} ]")
    };

    match search_box.width {
        Some(width) => truncate_by_width(&rendered, width),
        None => rendered,
    }
}

fn render_body(search_box: &SearchBox<'_>) -> String {
    let cursor_offset = search_box
        .cursor_offset
        .unwrap_or_else(|| search_box.query.chars().count());

    if !search_box.is_focused {
        return if search_box.query.is_empty() {
            search_box.placeholder.to_string()
        } else {
            search_box.query.to_string()
        };
    }

    if search_box.query.is_empty() {
        return if search_box.is_terminal_focused {
            format!("|{}", search_box.placeholder)
        } else {
            search_box.placeholder.to_string()
        };
    }

    if search_box.is_terminal_focused {
        insert_cursor(search_box.query, cursor_offset)
    } else {
        search_box.query.to_string()
    }
}

fn insert_cursor(query: &str, cursor_offset: usize) -> String {
    let cursor_offset = cursor_offset.min(query.chars().count());
    let mut output = String::new();
    for (idx, ch) in query.chars().enumerate() {
        if idx == cursor_offset {
            output.push('|');
        }
        output.push(ch);
    }
    if cursor_offset == query.chars().count() {
        output.push('|');
    }
    output
}

fn truncate_by_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let mut width = 0usize;
    let mut out = String::new();
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::SearchBox;
    use insta::assert_snapshot;

    #[test]
    fn snapshot_search_box_states() {
        let states = [
            SearchBox::new("")
                .placeholder("Filter commands")
                .terminal_focused(true)
                .render(),
            SearchBox::new("plugin")
                .cursor_offset(2)
                .terminal_focused(true)
                .render(),
            SearchBox::new("plugin").focused(false).render(),
            SearchBox::new("agent")
                .prefix("find")
                .borderless(true)
                .render(),
            SearchBox::new("a very long query").width(12).render(),
        ];

        assert_snapshot!(states.join("\n"));
    }
}

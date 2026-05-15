use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub(super) const TEXT_LIMIT: usize = 3_600;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TelegramTextChunk {
    pub(super) markdown: String,
    pub(super) html: String,
}

pub(super) fn rendered_len(text: &str) -> usize {
    md_to_html(text).len()
}

pub(super) fn render_text_chunks(text: &str) -> Vec<TelegramTextChunk> {
    split_text_into_bubbles(text)
        .into_iter()
        .map(|markdown| {
            let html = md_to_html(&markdown);
            TelegramTextChunk { markdown, html }
        })
        .collect()
}

pub(super) fn split_text_for_bubble(text: &str, limit: usize) -> Option<(String, String)> {
    if rendered_len(text) <= limit {
        return None;
    }
    if let Some(idx) = find_safe_split_index(text, limit) {
        return Some(split_at_boundary(text, idx));
    }
    Some(force_split_text(text, limit))
}

pub(super) fn split_text_into_bubbles(text: &str) -> Vec<String> {
    let mut remaining = text.to_string();
    let mut bubbles = Vec::new();

    while let Some((prefix, suffix)) = split_text_for_bubble(&remaining, TEXT_LIMIT) {
        bubbles.push(prefix);
        remaining = suffix;
    }

    if !remaining.is_empty() {
        bubbles.push(remaining);
    }

    bubbles
}

pub(super) fn is_markdown_closed(text: &str) -> bool {
    markdown_state(text).is_closed()
}

pub(super) fn markdown_to_plain_text(text: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(text, options);
    let mut out = String::with_capacity(text.len());
    let mut list_stack: Vec<Option<u64>> = Vec::new();

    for event in parser {
        match event {
            Event::Start(Tag::List(start)) => list_stack.push(start),
            Event::Start(Tag::Item) => push_plain_list_item_prefix(&mut out, &mut list_stack),
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock) => {
                out.push_str("\n\n");
            }
            Event::End(TagEnd::List(_)) => {
                let _ = list_stack.pop();
                if !out.ends_with("\n\n") {
                    out.push('\n');
                }
            }
            Event::Start(_) | Event::End(_) => {}
            Event::Text(text) | Event::Code(text) => out.push_str(text.as_ref()),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::Rule => out.push_str("\n────────\n"),
            Event::Html(raw)
            | Event::InlineHtml(raw)
            | Event::FootnoteReference(raw)
            | Event::InlineMath(raw)
            | Event::DisplayMath(raw) => out.push_str(raw.as_ref()),
            Event::TaskListMarker(checked) => {
                out.push_str(if checked { "[x] " } else { "[ ] " });
            }
        }
    }

    trim_redundant_blank_lines(&out)
}

/// Convert basic Markdown to Telegram-safe HTML.
fn md_to_html(text: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(text, options);
    let mut html = String::with_capacity(text.len() + text.len() / 4);
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut blockquote_depth = 0usize;

    for event in parser {
        render_markdown_event(&mut html, &mut list_stack, &mut blockquote_depth, event);
    }

    trim_redundant_blank_lines(&html)
}

fn render_markdown_event(
    html: &mut String,
    list_stack: &mut Vec<Option<u64>>,
    blockquote_depth: &mut usize,
    event: Event<'_>,
) {
    match event {
        Event::Start(tag) => {
            render_markdown_start(html, list_stack, blockquote_depth, tag);
        }
        Event::End(tag) => render_markdown_end(html, list_stack, blockquote_depth, tag),
        Event::Text(text) => render_markdown_text(html, *blockquote_depth, text.as_ref()),
        Event::Code(code) => push_inline_code(html, code.as_ref()),
        Event::SoftBreak | Event::HardBreak => html.push('\n'),
        Event::Rule => html.push_str("\n────────\n"),
        Event::Html(raw) | Event::InlineHtml(raw) => {
            html.push_str(&escape_html(raw.as_ref()));
        }
        Event::FootnoteReference(name) => {
            html.push('[');
            html.push_str(&escape_html(name.as_ref()));
            html.push(']');
        }
        Event::TaskListMarker(checked) => {
            html.push_str(if checked { "☑ " } else { "☐ " });
        }
        Event::InlineMath(expr) => push_inline_code(html, expr.as_ref()),
        Event::DisplayMath(expr) => {
            html.push_str("<pre><code>");
            html.push_str(&escape_html(expr.as_ref()));
            html.push_str("</code></pre>");
        }
    }
}

fn render_markdown_start(
    html: &mut String,
    list_stack: &mut Vec<Option<u64>>,
    blockquote_depth: &mut usize,
    tag: Tag<'_>,
) {
    match tag {
        Tag::Heading { level, .. } => {
            let _ = level;
            html.push_str("<b>");
        }
        Tag::BlockQuote(_) => {
            *blockquote_depth += 1;
            if !html.ends_with('\n') && !html.is_empty() {
                html.push('\n');
            }
            html.push_str("&gt; ");
        }
        Tag::CodeBlock(_) => push_code_block_start(html),
        Tag::List(start) => {
            list_stack.push(start);
            if !html.ends_with('\n') && !html.is_empty() {
                html.push('\n');
            }
        }
        Tag::Item => push_list_item_prefix(html, list_stack),
        Tag::Emphasis => html.push_str("<i>"),
        Tag::Strong => html.push_str("<b>"),
        Tag::Strikethrough => html.push_str("<s>"),
        Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
            html.push_str("<a href=\"");
            html.push_str(&escape_html(dest_url.as_ref()));
            html.push_str("\">");
        }
        Tag::Paragraph
        | Tag::FootnoteDefinition(_)
        | Tag::HtmlBlock
        | Tag::DefinitionList
        | Tag::DefinitionListTitle
        | Tag::DefinitionListDefinition
        | Tag::Superscript
        | Tag::Subscript
        | Tag::MetadataBlock(_)
        | Tag::TableHead
        | Tag::TableCell => {}
        Tag::Table(_) => {
            if !html.ends_with('\n') && !html.is_empty() {
                html.push_str("\n\n");
            }
        }
        Tag::TableRow => {
            if !html.ends_with('\n') && !html.is_empty() {
                html.push('\n');
            }
            html.push_str("• ");
        }
    }
}

fn render_markdown_end(
    html: &mut String,
    list_stack: &mut Vec<Option<u64>>,
    blockquote_depth: &mut usize,
    tag: TagEnd,
) {
    match tag {
        TagEnd::Paragraph | TagEnd::Table => html.push_str("\n\n"),
        TagEnd::Heading(_) => html.push_str("</b>\n\n"),
        TagEnd::BlockQuote(_) => {
            *blockquote_depth = blockquote_depth.saturating_sub(1);
            html.push_str("\n\n");
        }
        TagEnd::CodeBlock => html.push_str("</code></pre>\n\n"),
        TagEnd::List(_) => {
            let _ = list_stack.pop();
            if !html.ends_with("\n\n") {
                html.push('\n');
            }
        }
        TagEnd::Emphasis => html.push_str("</i>"),
        TagEnd::Strong => html.push_str("</b>"),
        TagEnd::Strikethrough => html.push_str("</s>"),
        TagEnd::Link => html.push_str("</a>"),
        TagEnd::Image => {
            if html.ends_with("\">") {
                html.push_str("[image]");
            }
            html.push_str("</a>");
        }
        TagEnd::Item
        | TagEnd::FootnoteDefinition
        | TagEnd::HtmlBlock
        | TagEnd::DefinitionList
        | TagEnd::DefinitionListTitle
        | TagEnd::DefinitionListDefinition
        | TagEnd::Superscript
        | TagEnd::Subscript
        | TagEnd::MetadataBlock(_) => {}
        TagEnd::TableHead | TagEnd::TableRow => html.push('\n'),
        TagEnd::TableCell => html.push_str("  |  "),
    }
}

fn render_markdown_text(html: &mut String, blockquote_depth: usize, text: &str) {
    if blockquote_depth > 0 && html.ends_with('\n') {
        html.push_str("&gt; ");
    }
    html.push_str(&escape_html(text));
}

fn push_inline_code(html: &mut String, code: &str) {
    html.push_str("<code>");
    html.push_str(&escape_html(code));
    html.push_str("</code>");
}

fn push_code_block_start(html: &mut String) {
    html.push_str("<pre><code>");
}

fn push_list_item_prefix(html: &mut String, list_stack: &mut [Option<u64>]) {
    if !html.ends_with('\n') && !html.is_empty() {
        html.push('\n');
    }
    let indent = "  ".repeat(list_stack.len().saturating_sub(1));
    html.push_str(&indent);
    match list_stack.last_mut() {
        Some(Some(next)) => {
            html.push_str(&next.to_string());
            html.push_str(". ");
            *next += 1;
        }
        _ => html.push_str("• "),
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn trim_redundant_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut newline_run = 0usize;
    for ch in text.trim().chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push(ch);
            }
        } else {
            newline_run = 0;
            out.push(ch);
        }
    }
    out
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct MarkdownSplitState {
    in_fenced_code_block: bool,
    in_inline_code: bool,
    strong_marker: Option<char>,
}

fn find_safe_split_index(text: &str, limit: usize) -> Option<usize> {
    let (paragraphs, lines, spaces) = split_boundaries(text);
    <[Vec<usize>; 3]>::from((paragraphs, lines, spaces))
        .into_iter()
        .find_map(|candidates| {
            candidates.into_iter().rev().find(|&idx| {
                let prefix = &text[..idx];
                rendered_len(prefix) <= limit && markdown_state(prefix).is_closed()
            })
        })
}

fn split_boundaries(text: &str) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let mut paragraphs = Vec::new();
    let mut lines = Vec::new();
    let mut spaces = Vec::new();
    let mut chars = text.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        match ch {
            '\n' => {
                let mut boundary = idx + ch.len_utf8();
                let mut run_len = 1usize;
                while let Some(&(next_idx, next_ch)) = chars.peek() {
                    if next_ch != '\n' {
                        break;
                    }
                    let _ = chars.next();
                    boundary = next_idx + next_ch.len_utf8();
                    run_len += 1;
                }
                if run_len >= 2 {
                    paragraphs.push(boundary);
                } else {
                    lines.push(boundary);
                }
            }
            ' ' | '\t' => spaces.push(idx + ch.len_utf8()),
            _ => {}
        }
    }

    (paragraphs, lines, spaces)
}

fn force_split_text(text: &str, limit: usize) -> (String, String) {
    let mut boundaries: Vec<usize> = text.char_indices().map(|(idx, _)| idx).collect();
    boundaries.push(text.len());
    let first = boundaries.get(1).copied().unwrap_or(text.len());
    let mut low = 1usize;
    let mut high = boundaries.len() - 1;
    let mut best = first;

    while low <= high {
        let mid = low + (high - low) / 2;
        let candidate = boundaries[mid];
        let (prefix, _) = rebalance_split(&text[..candidate], "");
        if rendered_len(&prefix) <= limit {
            best = candidate;
            low = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }

    loop {
        let (prefix, suffix) = split_at_boundary(text, best);
        if rendered_len(&prefix) <= limit || best == first {
            return (prefix, suffix);
        }
        if let Some(previous) = boundaries.iter().copied().rev().find(|idx| *idx < best) {
            best = previous;
        } else {
            return (prefix, suffix);
        }
    }
}

fn split_at_boundary(text: &str, idx: usize) -> (String, String) {
    let prefix = text[..idx].trim_end_matches(char::is_whitespace);
    let suffix = text[idx..].trim_start_matches(char::is_whitespace);
    rebalance_split(prefix, suffix)
}

fn rebalance_split(prefix: &str, suffix: &str) -> (String, String) {
    let state = markdown_state(prefix);
    let mut left = prefix.to_string();
    let mut right = suffix.to_string();

    if let Some(marker) = state.strong_marker {
        left.push(marker);
        left.push(marker);
        if !right.is_empty() {
            right.insert(0, marker);
            right.insert(0, marker);
        }
    }

    if state.in_inline_code {
        left.push('`');
        if !right.is_empty() {
            right.insert(0, '`');
        }
    }

    if state.in_fenced_code_block {
        if !left.ends_with('\n') {
            left.push('\n');
        }
        left.push_str("```");
        if !right.is_empty() {
            right.insert_str(0, "```\n");
        }
    }

    (left, right)
}

fn markdown_state(text: &str) -> MarkdownSplitState {
    let mut state = MarkdownSplitState::default();
    for line in text.split_inclusive('\n') {
        if toggles_fenced_code_block(line) {
            state.in_fenced_code_block = !state.in_fenced_code_block;
            continue;
        }
        if !state.in_fenced_code_block {
            scan_inline_markdown_state(line, &mut state);
        }
    }
    state
}

fn toggles_fenced_code_block(line: &str) -> bool {
    let trimmed = line.trim_end_matches('\n');
    let without_indent = trimmed.trim_start_matches([' ', '\t']);
    without_indent.starts_with("```")
}

fn scan_inline_markdown_state(line: &str, state: &mut MarkdownSplitState) {
    let mut escaped = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '`' => state.in_inline_code = !state.in_inline_code,
            '*' | '_' if !state.in_inline_code => {
                let marker = ch;
                let mut run_len = 1usize;
                while chars.peek() == Some(&marker) {
                    let _ = chars.next();
                    run_len += 1;
                }
                for _ in 0..(run_len / 2) {
                    toggle_strong_marker(state, marker);
                }
            }
            _ => {}
        }
    }
}

impl MarkdownSplitState {
    const fn is_closed(self) -> bool {
        !self.in_fenced_code_block && !self.in_inline_code && self.strong_marker.is_none()
    }
}

fn toggle_strong_marker(state: &mut MarkdownSplitState, marker: char) {
    if state.strong_marker == Some(marker) {
        state.strong_marker = None;
    } else if state.strong_marker.is_none() {
        state.strong_marker = Some(marker);
    }
}

fn push_plain_list_item_prefix(out: &mut String, list_stack: &mut [Option<u64>]) {
    if !out.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    let indent = "  ".repeat(list_stack.len().saturating_sub(1));
    out.push_str(&indent);
    match list_stack.last_mut() {
        Some(Some(next)) => {
            out.push_str(&next.to_string());
            out.push_str(". ");
            *next += 1;
        }
        _ => out.push_str("- "),
    }
}

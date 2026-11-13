//! 仅用于对话显示的 Markdown 模型，不执行 HTML，也不获取远程内容。

use super::{push_attachment, ChatAttachment, MAX_TEXT_CHARS};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub emphasis: bool,
    pub code: bool,
    pub target: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Block {
    Paragraph(Vec<Span>),
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    ListItem {
        marker: String,
        spans: Vec<Span>,
    },
    Quote(Vec<Span>),
    Code {
        language: String,
        text: String,
    },
    Rule,
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

pub(crate) fn parse(text: &str) -> (Vec<Block>, Vec<ChatAttachment>) {
    // 先识别完整语法和附件，再限制展示长度，避免截断链接后把目标路径当正文。
    let (mut blocks, attachments) = document(text);
    let mut remaining = MAX_TEXT_CHARS;
    let mut visible = 0;
    for block in &mut blocks {
        if remaining == 0 {
            break;
        }
        match block {
            Block::Paragraph(spans)
            | Block::Heading { spans, .. }
            | Block::ListItem { spans, .. }
            | Block::Quote(spans) => {
                let mut count = 0;
                for span in spans.iter_mut() {
                    if remaining == 0 {
                        break;
                    }
                    limit_text(&mut span.text, &mut remaining);
                    count += 1;
                }
                spans.truncate(count);
            }
            Block::Code { text, .. } => limit_text(text, &mut remaining),
            Block::Table { headers, rows } => {
                for cell in headers.iter_mut().chain(rows.iter_mut().flatten()) {
                    limit_text(cell, &mut remaining);
                }
                rows.retain(|row| row.iter().any(|cell| !cell.is_empty()));
            }
            Block::Rule => remaining -= 1,
        }
        visible += 1;
    }
    blocks.truncate(visible);
    (blocks, attachments)
}

fn limit_text(text: &mut String, remaining: &mut usize) {
    if let Some((end, _)) = text.char_indices().nth(*remaining) {
        text.truncate(end);
        *remaining = 0;
    } else {
        *remaining -= text.chars().count();
    }
}

/// 优先提取结论句供小岛和列表复用；规则压缩原文，不调用模型或混入附件路径。
pub(crate) fn summary(text: &str) -> String {
    let (blocks, attachments) = parse(text);
    let mut paragraphs = Vec::new();
    let mut fallback = Vec::new();
    for block in blocks {
        let (spans, prose) = match block {
            Block::Paragraph(spans) | Block::Quote(spans) => (spans, true),
            Block::Heading { spans, .. } | Block::ListItem { spans, .. } => (spans, false),
            Block::Code { .. } | Block::Rule | Block::Table { .. } => continue,
        };
        let content: String = spans
            .into_iter()
            .filter(|span| !(span.code && Path::new(&span.text).is_absolute()))
            .map(|span| span.text)
            .collect();
        let mut content = content.split_whitespace().collect::<Vec<_>>().join(" ");
        for prefix in [
            "结论：",
            "结论:",
            "摘要：",
            "摘要:",
            "总结：",
            "总结:",
            "好的，",
            "好的。",
        ] {
            if let Some(body) = content.strip_prefix(prefix) {
                content = body.trim_start().to_owned();
            }
        }
        if content.is_empty() {
            continue;
        }
        if prose {
            paragraphs.push(content);
        } else {
            fallback.push(content);
        }
    }
    let content = paragraphs.first().or_else(|| fallback.first());
    let Some(content) = content else {
        return if !attachments.is_empty() {
            format!("{} 个附件，点击查看", attachments.len())
        } else {
            "点击查看完整内容".to_owned()
        };
    };
    // 优先使用完整结论句；过长时在短句边界压缩，49 个字符包括省略号。
    let sentence = content
        .split_inclusive(['。', '！', '？'])
        .next()
        .unwrap_or(content);
    if sentence.chars().count() <= 49 {
        return sentence.to_owned();
    }
    let head: String = sentence.chars().take(48).collect();
    let boundary = head
        .char_indices()
        .rev()
        .find(|(_, ch)| matches!(ch, '，' | '；' | '：' | ',' | ';'))
        .map(|(offset, _)| offset)
        .filter(|offset| head[..*offset].chars().count() >= 12);
    let mut result = if let Some(offset) = boundary {
        head[..offset].to_owned()
    } else {
        head
    };
    result.push('…');
    result
}

fn document(text: &str) -> (Vec<Block>, Vec<ChatAttachment>) {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut attachments = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if line.trim().is_empty() {
            index += 1;
            continue;
        }
        if let Some((mark, count, language)) = fence(line) {
            let start = index + 1;
            index = start;
            while index < lines.len()
                && !fence(lines[index]).is_some_and(|(other, length, suffix)| {
                    other == mark && length >= count && suffix.is_empty()
                })
            {
                index += 1;
            }
            blocks.push(Block::Code {
                language: language.to_owned(),
                text: lines[start..index].join("\n"),
            });
            if index < lines.len() {
                index += 1;
            }
            continue;
        }
        if line.starts_with("    ") || line.starts_with('\t') {
            let mut code = Vec::new();
            while index < lines.len() {
                let current = lines[index];
                if let Some(content) = current
                    .strip_prefix("    ")
                    .or_else(|| current.strip_prefix('\t'))
                {
                    code.push(content);
                } else if current.trim().is_empty() {
                    code.push("");
                } else {
                    break;
                }
                index += 1;
            }
            blocks.push(Block::Code {
                language: String::new(),
                text: code.join("\n"),
            });
            continue;
        }
        if line.contains('|') && index + 1 < lines.len() && table_divider(lines[index + 1]) {
            let headers = table_cells(line)
                .into_iter()
                .map(|cell| cell_text(cell, &mut attachments))
                .collect();
            index += 2;
            let mut rows = Vec::new();
            while index < lines.len()
                && lines[index].contains('|')
                && !lines[index].trim().is_empty()
            {
                rows.push(
                    table_cells(lines[index])
                        .into_iter()
                        .map(|cell| cell_text(cell, &mut attachments))
                        .collect(),
                );
                index += 1;
            }
            blocks.push(Block::Table { headers, rows });
            continue;
        }
        if let Some((level, body)) = heading(line) {
            blocks.push(Block::Heading {
                level,
                spans: inline(body, &mut attachments, false, false),
            });
        } else if rule(line) {
            blocks.push(Block::Rule);
        } else if let Some(body) = line.trim_start().strip_prefix('>') {
            blocks.push(Block::Quote(inline(
                body.trim_start(),
                &mut attachments,
                false,
                false,
            )));
        } else if let Some((marker, body)) = list_item(line) {
            blocks.push(Block::ListItem {
                marker,
                spans: inline(body, &mut attachments, false, false),
            });
        } else {
            let mut paragraph = line.to_owned();
            index += 1;
            while index < lines.len() {
                let next = lines[index];
                if next.trim().is_empty()
                    || fence(next).is_some()
                    || heading(next).is_some()
                    || rule(next)
                    || next.trim_start().starts_with('>')
                    || list_item(next).is_some()
                    || next.starts_with("    ")
                    || next.starts_with('\t')
                    || (next.contains('|')
                        && index + 1 < lines.len()
                        && table_divider(lines[index + 1]))
                {
                    break;
                }
                paragraph.push('\n');
                paragraph.push_str(next);
                index += 1;
            }
            blocks.push(Block::Paragraph(inline(
                &paragraph,
                &mut attachments,
                false,
                false,
            )));
            continue;
        }
        index += 1;
    }
    (blocks, attachments)
}

fn fence(line: &str) -> Option<(char, usize, &str)> {
    let body = line.trim_start_matches(' ');
    if line.len() - body.len() > 3 {
        return None;
    }
    let mark = body.chars().next()?;
    if !matches!(mark, '`' | '~') {
        return None;
    }
    let count = body.chars().take_while(|ch| *ch == mark).count();
    if count < 3 {
        return None;
    }
    Some((mark, count, body[count..].trim()))
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let body = line.trim_start_matches(' ');
    if line.len() - body.len() > 3 {
        return None;
    }
    let count = body.bytes().take_while(|byte| *byte == b'#').count();
    if count == 0 || count > 6 || !body[count..].starts_with(char::is_whitespace) {
        return None;
    }
    let content = body[count..].trim();
    let without_hashes = content.trim_end_matches('#');
    let content = if without_hashes.ends_with(char::is_whitespace) {
        without_hashes.trim_end()
    } else {
        content
    };
    Some((count as u8, content))
}

fn rule(line: &str) -> bool {
    let marks: Vec<char> = line.chars().filter(|ch| !ch.is_whitespace()).collect();
    marks.len() >= 3
        && matches!(marks[0], '-' | '*' | '_')
        && marks.iter().all(|ch| *ch == marks[0])
}

fn list_item(line: &str) -> Option<(String, &str)> {
    let body = line.trim_start();
    let (marker, content) = if let Some(rest) = body
        .strip_prefix("- ")
        .or_else(|| body.strip_prefix("* "))
        .or_else(|| body.strip_prefix("+ "))
    {
        ("•".to_owned(), rest)
    } else {
        let digits = body.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0
            || !matches!(body.as_bytes().get(digits), Some(b'.' | b')'))
            || body.as_bytes().get(digits + 1) != Some(&b' ')
        {
            return None;
        }
        (body[..digits + 1].to_owned(), &body[digits + 2..])
    };
    if let Some(rest) = content.strip_prefix("[ ] ") {
        return Some(("☐".to_owned(), rest));
    }
    if let Some(rest) = content
        .strip_prefix("[x] ")
        .or_else(|| content.strip_prefix("[X] "))
    {
        return Some(("☑".to_owned(), rest));
    }
    Some((marker, content))
}

/// 行内代码中的竖线和转义竖线不作为表格分隔符。
fn table_cells(line: &str) -> Vec<&str> {
    let mut line = line.trim();
    if let Some(body) = line.strip_prefix('|') {
        line = body;
    }
    if let Some(body) = line.strip_suffix('|') {
        if body.bytes().rev().take_while(|byte| *byte == b'\\').count() % 2 == 0 {
            line = body;
        }
    }
    let mut cells = Vec::new();
    let mut start = 0;
    let mut code_ticks = 0;
    let mut index = 0;
    while index < line.len() {
        let byte = line.as_bytes()[index];
        if byte == b'\\' {
            index += 1;
            if index < line.len() {
                index += line[index..].chars().next().unwrap().len_utf8();
            }
            continue;
        }
        if byte == b'`' {
            let count = line[index..]
                .bytes()
                .take_while(|byte| *byte == b'`')
                .count();
            if code_ticks == 0 {
                code_ticks = count;
            } else if code_ticks == count {
                code_ticks = 0;
            }
            index += count;
            continue;
        }
        if byte == b'|' && code_ticks == 0 {
            cells.push(line[start..index].trim());
            start = index + 1;
        }
        index += line[index..].chars().next().unwrap().len_utf8();
    }
    cells.push(line[start..].trim());
    cells
}

fn table_divider(line: &str) -> bool {
    line.contains('|')
        && table_cells(line).iter().all(|cell| {
            let body = cell.trim_matches(':');
            body.len() >= 3 && body.bytes().all(|byte| byte == b'-')
        })
}

fn cell_text(cell: &str, attachments: &mut Vec<ChatAttachment>) -> String {
    inline(cell, attachments, false, false)
        .into_iter()
        .map(|span| span.text)
        .collect()
}

fn inline(
    text: &str,
    attachments: &mut Vec<ChatAttachment>,
    bold: bool,
    emphasis: bool,
) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        if rest.starts_with('\\') {
            if let Some(next) = rest[1..].chars().next().filter(char::is_ascii_punctuation) {
                push_span(&mut spans, next.to_string(), bold, emphasis, false, None);
                index += 1 + next.len_utf8();
                continue;
            }
        }
        if rest.starts_with('`') {
            let count = rest.bytes().take_while(|byte| *byte == b'`').count();
            let marker = &rest[..count];
            if let Some(end) = closing_marker(&rest[count..], marker, false) {
                let code = rest[count..count + end].replace('\n', " ");
                let code =
                    if code.starts_with(' ') && code.ends_with(' ') && !code.trim().is_empty() {
                        &code[1..code.len() - 1]
                    } else {
                        &code
                    };
                push_span(&mut spans, code.to_owned(), bold, emphasis, true, None);
                index += count + end + count;
                continue;
            }
        }
        let image = rest.starts_with("![");
        if image || rest.starts_with('[') {
            if let Some((label, destination, used)) = link(rest, image) {
                let local = local_path(&destination);
                let target = if let Some(path) = &local {
                    Some(path.clone())
                } else if destination.starts_with("https://") || destination.starts_with("http://")
                {
                    Some(destination.clone())
                } else {
                    None
                };
                let mut labels = inline(label, attachments, bold, emphasis);
                let name: String = labels.iter().map(|span| span.text.as_str()).collect();
                if image || local.is_some() {
                    let name = if name.trim().is_empty() {
                        local
                            .as_deref()
                            .and_then(|path| Path::new(path).file_name()?.to_str())
                            .unwrap_or("图片附件")
                            .to_owned()
                    } else {
                        name
                    };
                    push_attachment(
                        attachments,
                        ChatAttachment {
                            name,
                            path: local,
                            image,
                        },
                    );
                }
                if !image {
                    for span in &mut labels {
                        span.target = target.clone();
                    }
                    spans.extend(labels);
                }
                index += used;
                continue;
            }
        }
        let mut formatted = false;
        for marker in ["***", "___", "**", "__", "*", "_"] {
            if !rest.starts_with(marker) {
                continue;
            }
            let inside = &rest[marker.len()..];
            if inside.starts_with(char::is_whitespace) || inside.is_empty() {
                continue;
            }
            if marker.starts_with('_')
                && index > 0
                && text[..index]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric)
            {
                continue;
            }
            if let Some(end) = closing_marker(inside, marker, true) {
                spans.extend(inline(
                    &inside[..end],
                    attachments,
                    bold || marker.len() >= 2,
                    emphasis || marker.len() != 2,
                ));
                index += marker.len() + end + marker.len();
                formatted = true;
                break;
            }
        }
        if formatted {
            continue;
        }
        let next = rest.chars().next().unwrap();
        push_span(&mut spans, next.to_string(), bold, emphasis, false, None);
        index += next.len_utf8();
    }
    spans
}

fn push_span(
    spans: &mut Vec<Span>,
    text: String,
    bold: bool,
    emphasis: bool,
    code: bool,
    target: Option<String>,
) {
    if let Some(last) = spans.last_mut() {
        if last.bold == bold
            && last.emphasis == emphasis
            && last.code == code
            && last.target == target
        {
            last.text.push_str(&text);
            return;
        }
    }
    spans.push(Span {
        text,
        bold,
        emphasis,
        code,
        target,
    });
}

fn closing_marker(text: &str, marker: &str, emphasis: bool) -> Option<usize> {
    for (index, _) in text.match_indices(marker) {
        let before = &text[..index];
        if emphasis
            && before
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'\\')
                .count()
                % 2
                == 1
        {
            continue;
        }
        if !emphasis {
            if before.ends_with('`') || text[index + marker.len()..].starts_with('`') {
                continue;
            }
        } else {
            if before.is_empty() || before.ends_with(char::is_whitespace) {
                continue;
            }
            if marker.starts_with('_')
                && text[index + marker.len()..].starts_with(char::is_alphanumeric)
            {
                continue;
            }
        }
        return Some(index);
    }
    None
}

/// 完整行内链接允许括号路径、转义括号及尖括号包裹的空格路径。
fn link(text: &str, image: bool) -> Option<(&str, String, usize)> {
    let start = if image { 2 } else { 1 };
    let mut index = start;
    let mut brackets = 1;
    let label_end;
    loop {
        let byte = *text.as_bytes().get(index)?;
        if byte == b'\\' {
            index += 2;
            continue;
        }
        if byte == b'[' {
            brackets += 1;
        }
        if byte == b']' {
            brackets -= 1;
        }
        if brackets == 0 {
            label_end = index;
            break;
        }
        index += 1;
    }
    if text.as_bytes().get(index + 1) != Some(&b'(') {
        return None;
    }
    let destination_start = index + 2;
    index = destination_start;
    let mut parentheses = 1;
    let mut angle = false;
    while let Some(&byte) = text.as_bytes().get(index) {
        if byte == b'\\' {
            index += 2;
            continue;
        }
        if byte == b'<' {
            angle = true;
        }
        if byte == b'>' {
            angle = false;
        }
        if !angle {
            if byte == b'(' {
                parentheses += 1;
            }
            if byte == b')' {
                parentheses -= 1;
            }
        }
        if parentheses == 0 {
            let raw = text[destination_start..index].trim();
            let destination = if let Some(angle) = raw.strip_prefix('<') {
                angle.split_once('>')?.0
            } else {
                // 可选标题不是文件路径；无标题时允许路径自身含空格。
                let mut end = raw.len();
                for quote in ['"', '\''] {
                    if raw.ends_with(quote) {
                        if let Some(title) = raw[..raw.len() - 1].rfind(quote) {
                            if title > 0 && raw[..title].ends_with(char::is_whitespace) {
                                end = title;
                            }
                        }
                    }
                }
                raw[..end].trim_end()
            };
            return Some((&text[start..label_end], unescape(destination), index + 1));
        }
        index += 1;
    }
    None
}

fn unescape(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(char::is_ascii_punctuation) {
            result.push(chars.next().unwrap());
        } else {
            result.push(ch);
        }
    }
    result
}

fn local_path(destination: &str) -> Option<String> {
    let destination = destination
        .strip_prefix("file://localhost")
        .or_else(|| destination.strip_prefix("file://"))
        .unwrap_or(destination);
    // Windows的file:///C:/...带URI根斜杠，去掉后才能按盘符绝对路径识别。
    #[cfg(windows)]
    let destination = if destination.as_bytes().get(2) == Some(&b':') && destination.starts_with('/') {
        &destination[1..]
    } else {
        destination
    };
    if !Path::new(destination).is_absolute() || destination.contains('\0') {
        return None;
    }
    let mut path = destination;
    if let Some((base, location)) = path.rsplit_once('#') {
        if location.strip_prefix('L').is_some_and(|line| {
            !line.is_empty()
                && line
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'-' || byte == b'L')
        }) {
            path = base;
        }
    }
    // 引用允许 :行号 或 :行号:列号，预览只接收真实文件路径。
    for _ in 0..2 {
        if let Some((base, number)) = path.rsplit_once(':') {
            if !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) {
                path = base;
            }
        }
    }
    let mut decoded = Vec::new();
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = char::from(bytes[index + 1]).to_digit(16);
            let low = char::from(bytes[index + 2]).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                decoded.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    let path = String::from_utf8(decoded).ok()?;
    (!path.contains('\0')).then_some(path)
}

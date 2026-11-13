//! Codex 回复的轻量 Markdown 排版；解析结果在历史读取时生成，绘制只处理可见片段。

use crate::codex_client::markdown::{Block, Span};
use crate::commands::Command;
use crate::gfx::{TextStyle, Weight};
use crate::ui::{wid_n, Ui};
use poni_core::geom::{palette, Color, Point, Rect};

pub(super) struct Layout {
    pub height: f32,
    items: Vec<Item>,
}

enum Item {
    Text {
        rect: Rect,
        text: String,
        style: TextStyle,
        code: bool,
        emphasis: bool,
        target: Option<String>,
    },
    Panel {
        rect: Rect,
        color: Color,
        radius: f32,
    },
    Line {
        from: Point,
        to: Point,
        color: Color,
        width: f32,
    },
}

impl Layout {
    fn empty() -> Self {
        Self {
            height: 0.0,
            items: Vec::new(),
        }
    }

    fn append(&mut self, other: Layout, x: f32, y: f32) {
        for item in other.items {
            self.items.push(match item {
                Item::Text {
                    mut rect,
                    text,
                    style,
                    code,
                    emphasis,
                    target,
                } => {
                    rect.x += x;
                    rect.y += y;
                    Item::Text {
                        rect,
                        text,
                        style,
                        code,
                        emphasis,
                        target,
                    }
                }
                Item::Panel {
                    mut rect,
                    color,
                    radius,
                } => {
                    rect.x += x;
                    rect.y += y;
                    Item::Panel {
                        rect,
                        color,
                        radius,
                    }
                }
                Item::Line {
                    mut from,
                    mut to,
                    color,
                    width,
                } => {
                    from.x += x;
                    from.y += y;
                    to.x += x;
                    to.y += y;
                    Item::Line {
                        from,
                        to,
                        color,
                        width,
                    }
                }
            });
        }
    }
}

/// 使用与绘制相同的样式和真实宽度计算高度，避免气泡和附件的滚动范围失配。
pub(super) fn layout(ui: &mut Ui, blocks: &[Block], width: f32, base: TextStyle) -> Layout {
    let width = width.max(1.0);
    let mut result = Layout::empty();
    for block in blocks {
        let y = result.height;
        let (part, gap) = match block {
            Block::Paragraph(spans) => (flow(ui, spans, width, base), 8.0),
            Block::Heading { level, spans } => {
                let extra = match level {
                    1 => 6.0,
                    2 => 4.0,
                    _ => 2.0,
                };
                let style = TextStyle::new(base.size + extra)
                    .weight(Weight::Bold)
                    .color(base.color);
                (flow(ui, spans, width, style), 9.0)
            }
            Block::ListItem { marker, spans } => {
                let marker_w = (ui.c.measure_raw(marker, &base, 100_000.0).0 + 8.0)
                    .max(20.0)
                    .min(width * 0.35);
                let mut list = Layout::empty();
                let body = flow(ui, spans, (width - marker_w).max(1.0), base);
                list.height = body.height;
                list.items.push(Item::Text {
                    rect: Rect::new(0.0, 0.0, marker_w, base.size * 1.4),
                    text: marker.clone(),
                    style: base.top().line_spacing(1.4),
                    code: false,
                    emphasis: false,
                    target: None,
                });
                list.append(body, marker_w, 0.0);
                (list, 4.0)
            }
            Block::Quote(spans) => {
                let quote_style = base.color(palette::TEXT_DIM);
                let body = flow(ui, spans, (width - 14.0).max(1.0), quote_style);
                let mut quote = Layout::empty();
                quote.height = body.height + 8.0;
                quote.items.push(Item::Panel {
                    rect: Rect::new(0.0, 0.0, width, quote.height),
                    color: Color::white(0.025),
                    radius: 4.0,
                });
                quote.items.push(Item::Line {
                    from: Point::new(2.0, 2.0),
                    to: Point::new(2.0, quote.height - 2.0),
                    color: palette::TEXT_FAINT,
                    width: 2.0,
                });
                quote.append(body, 12.0, 4.0);
                (quote, 8.0)
            }
            Block::Code { language, text } => {
                let code_style = TextStyle::new((base.size - 1.0).max(11.0)).color(base.color);
                let label_h = if language.is_empty() { 0.0 } else { 18.0 };
                let spans = [Span {
                    text: text.replace('\t', "    "),
                    bold: false,
                    emphasis: false,
                    code: true,
                    target: None,
                }];
                let mut body = flow(ui, &spans, (width - 20.0).max(1.0), code_style);
                // 整块已有底色，代码行无需重复画行内代码胶囊。
                for item in &mut body.items {
                    if let Item::Text { code, .. } = item {
                        *code = false;
                    }
                }
                let mut code = Layout::empty();
                code.height = body.height + label_h + 16.0;
                code.items.push(Item::Panel {
                    rect: Rect::new(0.0, 0.0, width, code.height),
                    color: Color::black(0.22),
                    radius: 7.0,
                });
                if !language.is_empty() {
                    code.items.push(Item::Text {
                        rect: Rect::new(10.0, 5.0, width - 20.0, 15.0),
                        text: language.clone(),
                        style: TextStyle::caption().top().color(palette::TEXT_FAINT),
                        code: false,
                        emphasis: false,
                        target: None,
                    });
                }
                code.append(body, 10.0, 8.0 + label_h);
                (code, 9.0)
            }
            Block::Rule => {
                let mut line = Layout::empty();
                line.height = 8.0;
                line.items.push(Item::Line {
                    from: Point::new(0.0, 4.0),
                    to: Point::new(width, 4.0),
                    color: palette::BORDER,
                    width: 1.0,
                });
                (line, 8.0)
            }
            Block::Table { headers, rows } => (table(ui, headers, rows, width, base), 9.0),
        };
        result.height += part.height + gap;
        result.append(part, 0.0, y);
    }
    if !blocks.is_empty() {
        result.height = (result.height - 4.0).max(0.0);
    }
    result
}

fn table(
    ui: &mut Ui,
    headers: &[String],
    rows: &[Vec<String>],
    width: f32,
    base: TextStyle,
) -> Layout {
    let mut result = Layout::empty();
    let columns = headers
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0));
    if columns == 0 {
        return result;
    }
    let column_w = width / columns as f32;
    // 窄岛无法容纳多列时逐字段展示，避免单个汉字已宽于列宽而互相覆盖。
    if column_w < 70.0 {
        for (row_index, row) in rows.iter().enumerate() {
            if row_index > 0 {
                result.height += 6.0;
            }
            for column in 0..columns {
                let label = headers
                    .get(column)
                    .cloned()
                    .unwrap_or_else(|| format!("第 {} 列", column + 1));
                let text = row.get(column).cloned().unwrap_or_default();
                let spans = [
                    Span {
                        text: format!("{label}："),
                        bold: true,
                        emphasis: false,
                        code: false,
                        target: None,
                    },
                    Span {
                        text,
                        bold: false,
                        emphasis: false,
                        code: false,
                        target: None,
                    },
                ];
                let field = flow(ui, &spans, width, base);
                let y = result.height;
                result.height += field.height + 4.0;
                result.append(field, 0.0, y);
            }
        }
        if rows.is_empty() {
            let spans = [Span {
                text: headers.join(" · "),
                bold: true,
                emphasis: false,
                code: false,
                target: None,
            }];
            return flow(ui, &spans, width, base);
        }
        return result;
    }
    for (index, cells) in std::iter::once(headers)
        .chain(rows.iter().map(Vec::as_slice))
        .enumerate()
    {
        let mut layouts = Vec::with_capacity(columns);
        let mut row_h: f32 = base.size * 1.4 + 12.0;
        for column in 0..columns {
            let text = cells.get(column).cloned().unwrap_or_default();
            let span = [Span {
                text,
                bold: index == 0,
                emphasis: false,
                code: false,
                target: None,
            }];
            let cell = flow(ui, &span, (column_w - 12.0).max(1.0), base);
            row_h = row_h.max(cell.height + 12.0);
            layouts.push(cell);
        }
        let y = result.height;
        let fill = if index == 0 {
            Color::white(0.075)
        } else if index % 2 == 0 {
            Color::white(0.025)
        } else {
            Color::white(0.0)
        };
        result.items.push(Item::Panel {
            rect: Rect::new(0.0, y, width, row_h),
            color: fill,
            radius: 0.0,
        });
        for (column, cell) in layouts.into_iter().enumerate() {
            result.append(cell, column as f32 * column_w + 6.0, y + 6.0);
        }
        result.items.push(Item::Line {
            from: Point::new(0.0, y + row_h),
            to: Point::new(width, y + row_h),
            color: palette::BORDER,
            width: 1.0,
        });
        result.height += row_h;
    }
    for column in 0..=columns {
        let x = (column as f32 * column_w).min(width);
        result.items.push(Item::Line {
            from: Point::new(x, 0.0),
            to: Point::new(x, result.height),
            color: palette::BORDER,
            width: 1.0,
        });
    }
    result
}

/// 单词优先换行；无空格的中文或长路径按可用宽度二分截取，不逐字建立测量缓存。
fn flow(ui: &mut Ui, spans: &[Span], width: f32, base: TextStyle) -> Layout {
    let mut result = Layout::empty();
    let line_h = base.size * 1.4;
    let mut x = 0.0;
    let mut y = 0.0;
    let mut has_content = false;
    for span in spans {
        let mut style = base.top().line_spacing(1.4);
        if span.bold {
            style.weight = Weight::Bold;
        } else if span.emphasis {
            style.weight = Weight::Medium;
        }
        let text = span.text.replace('\t', "    ");
        for (line_index, line) in text.split('\n').enumerate() {
            if line_index > 0 {
                x = 0.0;
                y += line_h;
                has_content = true;
            }
            for token in line.split_inclusive(char::is_whitespace) {
                let mut remaining = token;
                while !remaining.is_empty() {
                    if !span.code && x == 0.0 && remaining.trim().is_empty() {
                        break;
                    }
                    let pad = if span.code { 3.0 } else { 0.0 };
                    let natural = ui.c.measure_raw(remaining, &style, 100_000.0).0;
                    let available = (width - x - 2.0 * pad).max(0.0);
                    if natural > available && natural + 2.0 * pad <= width && x > 0.0 {
                        x = 0.0;
                        y += line_h;
                        continue;
                    }
                    let length = if natural <= available {
                        remaining.len()
                    } else {
                        let mut boundaries: Vec<usize> =
                            remaining.char_indices().map(|(i, _)| i).collect();
                        boundaries.push(remaining.len());
                        let mut low = 0;
                        let mut high = boundaries.len() - 1;
                        while low < high {
                            let middle = (low + high + 1) / 2;
                            let w = ui
                                .c
                                .measure_raw(&remaining[..boundaries[middle]], &style, 100_000.0)
                                .0;
                            if w <= available {
                                low = middle;
                            } else {
                                high = middle - 1;
                            }
                        }
                        if low == 0 && x > 0.0 {
                            x = 0.0;
                            y += line_h;
                            continue;
                        }
                        boundaries[low.max(1)]
                    };
                    let piece = &remaining[..length];
                    let measured = ui.c.measure_raw(piece, &style, 100_000.0).0;
                    result.items.push(Item::Text {
                        rect: Rect::new(x + pad, y, measured + 1.0, line_h),
                        text: piece.to_owned(),
                        style,
                        code: span.code,
                        emphasis: span.emphasis,
                        target: span.target.clone(),
                    });
                    x += measured + 2.0 * pad;
                    has_content = true;
                    remaining = &remaining[length..];
                    if !remaining.is_empty() {
                        x = 0.0;
                        y += line_h;
                    }
                }
            }
        }
    }
    if has_content {
        result.height = y + line_h;
    }
    result
}

/// 只对可见链接注册点击区域；链接不会在排版或绘制时自动打开。
pub(super) fn draw(
    ui: &mut Ui,
    layout: &Layout,
    origin: Point,
    view: Rect,
    accent: Color,
    message: u64,
) -> Option<Command> {
    let mut command = None;
    for (index, item) in layout.items.iter().enumerate() {
        match item {
            Item::Text {
                rect,
                text,
                style,
                code,
                emphasis,
                target,
            } => {
                let rect = Rect::new(origin.x + rect.x, origin.y + rect.y, rect.w, rect.h);
                let Some(hit) = intersection(rect, view) else {
                    continue;
                };
                let mut style = *style;
                if *code {
                    ui.c.fill_rrect(
                        Rect::new(rect.x - 2.0, rect.y + 1.0, rect.w + 4.0, rect.h - 2.0),
                        3.0,
                        Color::white(0.075),
                    );
                }
                if *emphasis {
                    style.color = palette::TEXT;
                }
                if let Some(target) = target {
                    let response = ui.button(
                        wid_n(
                            "codex.markdown-link",
                            wid_n(&message.to_string(), index as u64),
                        ),
                        hit,
                    );
                    style.color = accent;
                    if response.clicked {
                        if std::path::Path::new(target).is_absolute() {
                            command = Some(Command::CodexPreviewAttachment(target.clone()));
                        } else if target.starts_with("https://") || target.starts_with("http://") {
                            command = Some(Command::OpenUrl(target.clone()));
                        }
                    }
                    if response.hovered {
                        ui.c.line(
                            Point::new(rect.x, rect.bottom() - 2.0),
                            Point::new(rect.right(), rect.bottom() - 2.0),
                            accent,
                            1.0,
                        );
                    }
                }
                ui.c.text_raw(text, rect, &style);
            }
            Item::Panel {
                rect,
                color,
                radius,
            } => {
                let rect = Rect::new(origin.x + rect.x, origin.y + rect.y, rect.w, rect.h);
                if intersection(rect, view).is_some() {
                    ui.c.fill_rrect(rect, *radius, *color);
                }
            }
            Item::Line {
                from,
                to,
                color,
                width,
            } => {
                let from = Point::new(origin.x + from.x, origin.y + from.y);
                let to = Point::new(origin.x + to.x, origin.y + to.y);
                if from.y.min(to.y) <= view.bottom() && from.y.max(to.y) >= view.y {
                    ui.c.line(from, to, *color, *width);
                }
            }
        }
    }
    if command.is_some() {
        ui.blur();
    }
    command
}

fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let right = a.right().min(b.right());
    let top = a.y.max(b.y);
    let bottom = a.bottom().min(b.bottom());
    if right <= left || bottom <= top {
        return None;
    }
    Some(Rect::new(left, top, right - left, bottom - top))
}

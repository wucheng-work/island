//! 原会话阅读、附件预览和回复输入；服务调用仍由父页面返回 Command。

use super::{Conversation, Entry, Command, Icon, TextStyle, Weight, Ui, PillStyle, PILL_SMALL, MAX_REPLY_CHARS};
use crate::codex_client::Role;
use crate::ui::{wid, wid_n};
use poni_core::design::radius;
use poni_core::geom::{palette, Color, Point, Rect};

#[path = "markdown.rs"]
mod markdown;

pub(super) fn draw(
    ui: &mut Ui,
    entry: &Entry,
    conversation: &mut Conversation,
    compact: bool,
    area: Rect,
    accent: Color,
) -> Option<Command> {
    let thread_id = &entry.message.thread_id;
    let back = Rect::new(area.x, area.y, 30.0, 30.0);
    if ui
        .icon_button(
            wid("codex.chat-back"),
            back,
            Icon::Undo,
            palette::TEXT_DIM,
            17.0,
        )
        .clicked
    {
        ui.blur();
        return Some(Command::CodexBack);
    }

    let mut title_right;
    if compact {
        let width = ui.pill_width("展开对话", false, true);
        let button = Rect::new(area.right() - width, area.y + 1.0, width, PILL_SMALL);
        if ui
            .pill(
                wid("codex.expand-chat"),
                button,
                "展开对话",
                None,
                PillStyle::Utility,
            )
            .clicked
        {
            ui.blur();
            return Some(Command::CodexSelect {
                id: entry.message.id.clone(),
                compact: false,
            });
        }
        title_right = button.x - 10.0;
        if conversation.send_uncertain {
            let open = Rect::new(title_right - 30.0, area.y, 30.0, 30.0);
            if ui
                .icon_button(
                    wid("codex.compact-open"),
                    open,
                    Icon::Open,
                    palette::TEXT_DIM,
                    16.0,
                )
                .clicked
            {
                return Some(Command::CodexOpen(thread_id.clone()));
            }
            title_right = open.x - 8.0;
        }
    } else {
        let refresh = Rect::new(area.right() - 30.0, area.y, 30.0, 30.0);
        if conversation.loading.is_none() {
            if ui
                .icon_button(
                    wid("codex.chat-refresh"),
                    refresh,
                    Icon::Reset,
                    palette::TEXT_DIM,
                    16.0,
                )
                .clicked
            {
                return Some(Command::CodexHistory {
                    thread_id: thread_id.clone(),
                    older: false,
                });
            }
        } else {
            ui.c.icon(
                Icon::Reset,
                Rect::from_center(refresh.center(), 16.0, 16.0),
                palette::TEXT_FAINT,
            );
        }
        let open = Rect::new(refresh.x - 36.0, refresh.y, 30.0, 30.0);
        if ui
            .icon_button(
                wid("codex.chat-open"),
                open,
                Icon::Open,
                palette::TEXT_DIM,
                16.0,
            )
            .clicked
        {
            return Some(Command::CodexOpen(thread_id.clone()));
        }
        title_right = open.x - 10.0;
    }

    let name = conversation.title();
    let title_x = back.right() + 9.0;
    let title_w = (title_right - title_x).max(0.0);
    ui.c.text_raw(
        name,
        Rect::new(title_x, area.y + 2.0, title_w, 24.0),
        &TextStyle::headline(),
    );
    if compact {
        ui.c.text(
            "消息预览",
            Rect::new(title_x, area.y + 30.0, title_w, 17.0),
            &TextStyle::caption().color(palette::TEXT_FAINT),
        );
    }

    let mut body_y = area.y + if compact { 53.0 } else { 34.0 };
    let progress_count = conversation
        .items
        .iter()
        .filter(|item| item.progress)
        .count();
    if !compact
        && (conversation.loading.is_some()
            || conversation.next_cursor.is_some()
            || progress_count > 0)
    {
        let auxiliary = Rect::new(area.x + 2.0, body_y, area.w - 4.0, 22.0);
        if conversation.loading.is_some() {
            ui.c.text(
                "读取中…",
                Rect::new(auxiliary.x, auxiliary.y, 88.0, auxiliary.h),
                &TextStyle::caption().color(palette::TEXT_FAINT),
            );
        } else if conversation.next_cursor.is_some() {
            let older = Rect::new(auxiliary.x, auxiliary.y, 88.0, auxiliary.h);
            let response = ui.button(wid("codex.chat-older"), older);
            ui.c.text(
                "更早消息",
                older,
                &TextStyle::caption().color(if response.hovered {
                    palette::TEXT
                } else {
                    palette::TEXT_DIM
                }),
            );
            if response.clicked {
                return Some(Command::CodexHistory {
                    thread_id: thread_id.clone(),
                    older: true,
                });
            }
        }
        if progress_count > 0 {
            let label = if conversation.show_progress {
                "隐藏进度".to_owned()
            } else {
                format!("显示进度 · {progress_count}")
            };
            let width = ui.c.measure_raw(&label, &TextStyle::caption(), area.w).0 + 8.0;
            let toggle = Rect::new(auxiliary.right() - width, auxiliary.y, width, auxiliary.h);
            let response = ui.button(wid("codex.chat-progress"), toggle);
            ui.c.text_raw(
                &label,
                toggle,
                &TextStyle::caption().right().color(if response.hovered {
                    palette::TEXT
                } else {
                    palette::TEXT_FAINT
                }),
            );
            if response.clicked {
                conversation.show_progress = !conversation.show_progress;
                conversation.scroll_to_end = true;
                ui.blur();
                ui.request_repaint();
            }
        }
        body_y = auxiliary.bottom() + 4.0;
    }

    let status_h = if conversation.send_uncertain
        || conversation.sending.is_some()
        || !conversation.error.is_empty()
        || !conversation.send_status.is_empty()
    {
        40.0
    } else {
        0.0
    };
    let composer_y = area.bottom() - 34.0 - status_h;
    let body = Rect::new(
        area.x,
        body_y,
        area.w,
        (composer_y - body_y - 8.0).max(16.0),
    );
    let attachment_command = if compact {
        draw_message(ui, &entry.message.id, &entry.message.text, &mut conversation.scroll, body, accent)
    } else {
        draw_transcript(ui, conversation, body, accent)
    };

    let send_label = if conversation.send_uncertain {
        "已核对，未收到"
    } else {
        "发送"
    };
    let send_w = ui.pill_width(send_label, false, true).max(58.0);
    let input = Rect::new(area.x, composer_y, (area.w - send_w - 8.0).max(0.0), 34.0);
    let send = Rect::new(input.right() + 8.0, composer_y, send_w, input.h);
    let blocked = conversation.sending.is_some() || conversation.send_uncertain;
    if blocked {
        let pending_text = if conversation.confirmation.is_some() {
            "确认"
        } else {
            &conversation.draft
        };
        ui.c.fill_rrect(input, radius::CONTROL, palette::SURFACE);
        let mut text_area = input.inset(10.0, 0.0);
        if conversation.send_uncertain && !pending_text.is_empty() {
            let copy = Rect::new(input.right() - 49.0, input.y + 3.0, 46.0, 28.0);
            let response = ui.button(wid_n("codex.chat-copy", wid(thread_id)), copy);
            ui.c.text(
                "复制",
                copy,
                &TextStyle::footnote().center().color(if response.hovered {
                    palette::TEXT
                } else {
                    palette::TEXT_DIM
                }),
            );
            if response.clicked {
                return Some(Command::CodexCopy(pending_text.to_owned()));
            }
            text_area.w = (copy.x - text_area.x - 4.0).max(0.0);
        }
        ui.c.text_raw(
            pending_text,
            text_area,
            &TextStyle::body().color(palette::TEXT_DIM),
        );
    } else {
        // 回车只完成输入法确认；发送必须明确点击，不因候选词确认而发出消息。
        ui.text_input(
            wid_n("codex.chat-reply", wid(thread_id)),
            input,
            &mut conversation.draft,
            "继续这个对话…",
            MAX_REPLY_CHARS,
        );
    }
    if conversation.send_uncertain {
        if ui
            .pill(
                wid_n("codex.chat-confirm-queue", wid(thread_id)),
                send,
                send_label,
                None,
                PillStyle::Utility,
            )
            .clicked
        {
            ui.blur();
            return Some(Command::CodexConfirmQueue(thread_id.clone()));
        }
    } else if !blocked && !conversation.draft.trim().is_empty() {
        if ui
            .pill(
                wid_n("codex.chat-send", wid(thread_id)),
                send,
                "发送",
                None,
                PillStyle::Primary(accent),
            )
            .clicked
        {
            ui.blur();
            return Some(Command::CodexSend {
                thread_id: thread_id.clone(),
                text: conversation.draft.trim().to_owned(),
            });
        }
    } else {
        ui.c.fill_rrect(send, radius::CONTROL, palette::SURFACE);
        ui.c.text(
            "发送",
            send,
            &TextStyle::footnote()
                .weight(Weight::Semibold)
                .color(palette::DISABLED)
                .center(),
        );
    }
    let (status, color) = if conversation.send_uncertain {
        (
            if conversation.confirmation.is_some() {
                "确认结果待核对，请先查看原会话；原有草稿已保留。"
            } else {
                "发送结果待确认，请先核对原会话；草稿已保留。"
            },
            palette::ORANGE,
        )
    } else if conversation.sending.is_some() {
        ("正在加入 Codex 队列，请稍候。", palette::TEXT_DIM)
    } else if !conversation.error.is_empty() {
        (conversation.error.as_str(), palette::ORANGE)
    } else if !conversation.send_status.is_empty() {
        (conversation.send_status.as_str(), palette::TEXT_DIM)
    } else {
        ("", palette::TEXT_FAINT)
    };
    if !status.is_empty() {
        let status_area = Rect::new(
            area.x + 2.0,
            input.bottom() + 5.0,
            area.w - 4.0,
            status_h - 5.0,
        );
        ui.c.clip(status_area);
        ui.c.text_raw(
            status,
            status_area,
            &TextStyle::caption()
                .wrap()
                .top()
                .line_spacing(1.3)
                .color(color),
        );
        ui.c.unclip();
    }
    attachment_command
}

fn draw_transcript(ui: &mut Ui, conversation: &mut Conversation, area: Rect, accent: Color) -> Option<Command> {
    let view = area.inset(3.0, 4.0);
    let regular_style = TextStyle::body().wrap().top().line_spacing(1.4);
    let progress_style = TextStyle::footnote()
        .wrap()
        .top()
        .line_spacing(1.35)
        .color(palette::TEXT_DIM);
    let attachment_h = 50.0;
    let attachment_gap = 6.0;
    let max_bubble_w = view.w * 0.84;
    let max_text_w = (max_bubble_w - 24.0).max(1.0);
    let mut layout = Vec::new();
    let mut total_h = 0.0;
    for (index, item) in conversation.items.iter().enumerate() {
        if item.progress && !conversation.show_progress {
            continue;
        }
        let style = if item.progress {
            &progress_style
        } else {
            &regular_style
        };
        let formatted = item.role == Role::Assistant;
        let measured_w = if formatted { max_text_w } else { ui.c.measure_raw(&item.text, style, max_text_w).0 };
        let min_width = if !item.attachments.is_empty() {
            240.0
        } else if item.progress {
            104.0
        } else {
            72.0
        };
        let bubble_w = (measured_w.ceil() + 24.0).max(min_width).min(max_bubble_w);
        let formatted_layout = formatted.then(|| markdown::layout(ui, &item.markdown, (bubble_w - 24.0).max(1.0), *style));
        let text_h = if let Some(layout) = &formatted_layout {
            layout.height
        } else if item.text.is_empty() {
            0.0
        } else {
            ui.c.measure_raw(&item.text, style, (bubble_w - 24.0).max(1.0)).1.max(18.0)
        };
        let attachments_h = if item.attachments.is_empty() {
            0.0
        } else {
            let count = item.attachments.len() as f32;
            count * attachment_h + (count - 1.0) * attachment_gap
                + if text_h > 0.0 { 8.0 } else { 0.0 }
        };
        let bubble_h = text_h + attachments_h + if item.progress { 29.0 } else { 37.0 };
        layout.push((index, bubble_w, bubble_h, text_h, formatted_layout));
        total_h += bubble_h + 10.0;
    }
    if layout.is_empty() {
        let label = if conversation.loading.is_some() {
            "正在读取原会话…"
        } else if conversation.items.iter().any(|item| item.progress) {
            "正在处理中，可查看进度消息"
        } else {
            "暂时没有可显示的对话"
        };
        ui.c.text(
            label,
            view,
            &TextStyle::footnote().color(palette::TEXT_DIM).center(),
        );
        return None;
    }
    total_h -= 10.0;
    let max_scroll = (total_h - view.h).max(0.0);
    if conversation.scroll_to_end {
        conversation.scroll = max_scroll;
        conversation.scroll_to_end = false;
    }
    conversation.scroll = (conversation.scroll - ui.wheel_in(area) * 40.0).clamp(0.0, max_scroll);
    let mut y = view.y - conversation.scroll;
    let mut preview = None;
    ui.c.clip(view);
    for (index, width, height, text_h, formatted_layout) in layout {
        let item = &conversation.items[index];
        if y + height > view.y && y < view.bottom() {
            let user = item.role == Role::User;
            let x = if user { view.right() - width } else { view.x };
            let bubble = Rect::new(x, y, width, height);
            let fill = if user {
                accent.with_alpha(0.12)
            } else if item.progress {
                Color::white(0.028)
            } else {
                Color::white(0.065)
            };
            ui.c.fill_rrect(bubble, radius::PANEL, fill);
            let top_pad = if item.progress { 6.0 } else { 8.0 };
            let label_h = if item.progress { 12.0 } else { 14.0 };
            let label = if user {
                "我"
            } else if item.progress {
                "Codex · 进度"
            } else {
                "Codex"
            };
            ui.c.text(
                label,
                Rect::new(x + 12.0, y + top_pad, width - 24.0, label_h),
                &TextStyle::caption()
                    .weight(Weight::Semibold)
                    .color(if user { accent } else { palette::TEXT_DIM }),
            );
            let text_y = y + top_pad + label_h + 4.0;
            let style = if item.progress {
                &progress_style
            } else {
                &regular_style
            };
            if let Some(layout) = formatted_layout {
                if let Some(command) = markdown::draw(ui, &layout, Point::new(x + 12.0, text_y), view, accent, wid(&item.id)) {
                    preview = Some(command);
                }
            } else if text_h > 0.0 {
                ui.c.text_raw(&item.text, Rect::new(x + 12.0, text_y, width - 24.0, text_h), style);
            }
            let attachment_y = text_y + text_h + if text_h > 0.0 { 8.0 } else { 0.0 };
            if let Some(command) = draw_attachments(ui, &item.attachments, Point::new(x + 12.0, attachment_y), width - 24.0, view, &item.id) {
                preview = Some(command);
            }
        }
        y += height + 10.0;
    }
    ui.c.unclip();
    if max_scroll > 0.0 {
        let h = (view.h * view.h / total_h).max(18.0);
        let y = view.y + (view.h - h) * conversation.scroll / max_scroll;
        ui.c.fill_rrect(
            Rect::new(area.right() - 1.0, y, 2.0, h),
            1.0,
            Color::white(0.20),
        );
    }
    preview
}

/// 紧凑预览和等待详情共用 Markdown 与附件布局，避免切换模式后又显示原始标记。
pub(super) fn draw_message(ui: &mut Ui, message: &str, text: &str, scroll: &mut f32, area: Rect, accent: Color) -> Option<Command> {
    let view = area.inset(3.0, 2.0);
    let (blocks, attachments) = crate::codex_client::markdown::parse(text);
    let layout = markdown::layout(ui, &blocks, view.w, TextStyle::footnote().color(palette::TEXT_DIM));
    let attachments_h = if attachments.is_empty() { 0.0 } else { attachments.len() as f32 * 56.0 - 6.0 + if layout.height > 0.0 { 8.0 } else { 0.0 } };
    let total_h = layout.height + attachments_h;
    *scroll = (*scroll - ui.wheel_in(area) * 40.0).clamp(0.0, (total_h - view.h).max(0.0));
    ui.c.clip(view);
    let origin = Point::new(view.x, view.y - *scroll);
    let mut command = markdown::draw(ui, &layout, origin, view, accent, wid(message));
    let attachment_y = origin.y + layout.height + if layout.height > 0.0 { 8.0 } else { 0.0 };
    if let Some(preview) = draw_attachments(ui, &attachments, Point::new(origin.x, attachment_y), view.w, view, message) { command = Some(preview); }
    if total_h == 0.0 { ui.c.text("此消息没有正文。", view, &TextStyle::footnote().color(palette::TEXT_FAINT)); }
    ui.c.unclip();
    command
}

fn draw_attachments(ui: &mut Ui, attachments: &[crate::codex_client::ChatAttachment], origin: Point, width: f32, view: Rect, message: &str) -> Option<Command> {
    let attachment_h = 50.0;
    let attachment_gap = 6.0;
    let mut attachment_y = origin.y;
    let mut preview = None;
    for (attachment_index, attachment) in attachments.iter().enumerate() {
        let card = Rect::new(origin.x, attachment_y, width, attachment_h);
        attachment_y += attachment_h + attachment_gap;
        let hit_top = card.y.max(view.y);
        let hit_bottom = card.bottom().min(view.bottom());
        if hit_bottom <= hit_top {
            continue;
        }
        let mut hovered = false;
        if let Some(path) = &attachment.path {
            // 点击区域同样裁到视口，隐藏在输入框或标题下的卡片不能响应。
            let hit = Rect::new(card.x, hit_top, card.w, hit_bottom - hit_top);
            let id = wid_n("codex.chat-attachment", wid_n(message, attachment_index as u64));
            let response = ui.button(id, hit);
            hovered = response.hovered;
            if response.clicked {
                ui.blur();
                preview = Some(Command::CodexPreviewAttachment(path.clone()));
            }
        }
        ui.c.fill_rrect(card, radius::CONTROL, if hovered { palette::SURFACE_HOVER } else { palette::SURFACE });
        ui.c.stroke_rrect(card, radius::CONTROL, if hovered { palette::TEXT_FAINT.mul_alpha(0.4) } else { palette::BORDER }, 1.0);
        ui.c.icon(
            Icon::File,
            Rect::new(card.x + 9.0, card.y + 14.0, 22.0, 22.0),
            if attachment.path.is_some() { palette::TEXT_DIM } else { palette::TEXT_FAINT },
        );
        let text_x = card.x + 39.0;
        let text_w = (card.w - 75.0).max(0.0);
        ui.c.text_raw(
            &attachment.name,
            Rect::new(text_x, card.y + 7.0, text_w, 18.0),
            &TextStyle::footnote().color(palette::TEXT),
        );
        let hint = if attachment.path.is_none() {
            "暂无本地文件，无法预览"
        } else if cfg!(windows) {
            "文件 · 在资源管理器中查看"
        } else if attachment.image {
            "图片 · 点击预览"
        } else {
            "文件 · 点击预览"
        };
        ui.c.text(
            hint,
            Rect::new(text_x, card.y + 27.0, text_w, 15.0),
            &TextStyle::caption().color(palette::TEXT_FAINT),
        );
        if attachment.path.is_some() {
            ui.c.icon(
                Icon::Eye,
                Rect::new(card.right() - 27.0, card.y + 17.0, 17.0, 17.0),
                if hovered { palette::TEXT } else { palette::TEXT_DIM },
            );
        }
    }
    preview
}

//! Codex 快捷回复、原会话对话与等待问题回答。消息和草稿均只驻留内存。

use crate::codex::{Conversation, Entry, Kind, ReplyState};
use crate::commands::Command;
use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::state::AppState;
use crate::ui::{wid, wid_n, PillStyle, Ui, PILL_SMALL};
use poni_core::design::radius;
use poni_core::geom::{palette, Color, Point, Rect};

mod conversation;

const HEIGHT: f32 = 350.0;
const CHAT_HEIGHT: f32 = 380.0;
const COMPACT_HEIGHT: f32 = 180.0;
const ROW_H: f32 = 78.0;
const ROW_GAP: f32 = 8.0;
const MAX_REPLY_CHARS: usize = 2000;

pub fn height(st: &AppState) -> f32 {
    let selected = st.codex.selected.as_ref().and_then(|id| {
        st.codex
            .entries
            .iter()
            .find(|entry| entry.message.id == *id)
    });
    let height = if selected.is_some_and(Entry::can_chat) && st.settings.codex_enabled {
        if st.codex.compact {
            COMPACT_HEIGHT
        } else {
            CHAT_HEIGHT
        }
    } else {
        HEIGHT
    };
    if selected.is_some_and(|entry| {
        !entry.can_chat()
            && st.codex.groups().iter().any(|group| {
                group.members.len() > 1
                    && group
                        .members
                        .iter()
                        .any(|&index| st.codex.entries[index].message.id == entry.message.id)
            })
    }) {
        height + 32.0
    } else {
        height
    }
}

pub fn draw(ui: &mut Ui, st: &mut AppState, mut area: Rect) {
    let selected = st.codex.selected.as_ref().and_then(|id| {
        st.codex
            .entries
            .iter()
            .position(|entry| entry.message.id == *id)
    });
    if selected.is_none() && st.codex.selected.is_some() {
        st.codex.selected = None;
        ui.blur();
    }

    let groups = st.codex.groups();
    if st.settings.codex_enabled {
        if let Some(group) =
            selected.and_then(|index| groups.iter().find(|group| group.members.contains(&index)))
        {
            if group.members.len() > 1
                && selected.is_some_and(|index| !st.codex.entries[index].can_chat())
            {
                let position = group
                    .members
                    .iter()
                    .position(|&index| Some(index) == selected)
                    .unwrap_or(0);
                let footer = Rect::new(area.x, area.bottom() - 26.0, area.w, 26.0);
                area.h -= 32.0;
                let next_w = ui.pill_width("更早提醒", false, true);
                let previous_w = ui.pill_width("更新提醒", false, true);
                let next = Rect::new(footer.right() - next_w, footer.y, next_w, footer.h);
                let previous = Rect::new(next.x - previous_w - 8.0, footer.y, previous_w, footer.h);
                ui.c.text_raw(
                    &format!("会话提醒 {}/{}", position + 1, group.members.len()),
                    Rect::new(
                        footer.x + 2.0,
                        footer.y,
                        previous.x - footer.x - 10.0,
                        footer.h,
                    ),
                    &TextStyle::caption().color(palette::TEXT_FAINT),
                );
                let mut target = None;
                if position > 0
                    && ui
                        .pill(
                            wid("codex.group-newer"),
                            previous,
                            "更新提醒",
                            None,
                            PillStyle::Utility,
                        )
                        .clicked
                {
                    target = Some(group.members[position - 1]);
                }
                if position + 1 < group.members.len()
                    && ui
                        .pill(
                            wid("codex.group-older"),
                            next,
                            "更早提醒",
                            None,
                            PillStyle::Utility,
                        )
                        .clicked
                {
                    target = Some(group.members[position + 1]);
                }
                if let Some(index) = target {
                    ui.blur();
                    st.cmd(Command::CodexSelect {
                        id: st.codex.entries[index].message.id.clone(),
                        compact: st.codex.compact,
                    });
                    return;
                }
            }
        }
    }

    if st.settings.codex_enabled {
        if let Some(index) = selected {
            if st.codex.entries[index].can_chat() {
                let entry = &st.codex.entries[index];
                let thread_id = entry.message.thread_id.clone();
                let conversation = st.codex.conversations.entry(thread_id).or_default();
                if let Some(command) =
                    conversation::draw(ui, entry, conversation, st.codex.compact, area, st.accent)
                {
                    st.cmd(command);
                }
                return;
            }
        }
    }

    let setup_w = ui.pill_width("接入配置", true, true);
    let setup = Rect::new(area.right() - setup_w, area.y, setup_w, PILL_SMALL);
    if ui
        .pill(
            wid("codex.setup"),
            setup,
            "接入配置",
            Some(Icon::Gear),
            PillStyle::Utility,
        )
        .clicked
    {
        st.cmd(Command::CodexSetup);
    }
    let mut header_right = setup.x - 14.0;
    let has_finished = st.codex.entries.iter().any(|entry| {
        !matches!(entry.status, ReplyState::Waiting | ReplyState::Sending)
            && !st
                .codex
                .conversations
                .get(&entry.message.thread_id)
                .is_some_and(|conversation| {
                    !conversation.draft.is_empty()
                        || conversation.sending.is_some()
                        || conversation.send_uncertain
                })
    });
    if selected.is_none() && has_finished {
        let width = ui.pill_width("清空已结束", true, true);
        let button = Rect::new(header_right - width, area.y, width, PILL_SMALL);
        if ui
            .pill(
                wid("codex.clear_finished"),
                button,
                "清空已结束",
                Some(Icon::Trash),
                PillStyle::Utility,
            )
            .clicked
        {
            st.cmd(Command::CodexClearFinished);
            return;
        }
        header_right = button.x - 14.0;
    }
    let header = Rect::new(area.x, area.y, header_right - area.x, PILL_SMALL);
    if selected.is_some() {
        let back_w = ui.pill_width("全部消息", false, true);
        if ui
            .pill(
                wid("codex.back"),
                Rect::new(header.x, header.y, back_w, header.h),
                "全部消息",
                None,
                PillStyle::Neutral,
            )
            .clicked
        {
            ui.blur();
            st.cmd(Command::CodexBack);
            return;
        }
    } else {
        let unread = st.codex.entries.iter().filter(|entry| entry.unread).count();
        let status = if !st.settings.codex_enabled {
            "尚未启用 Codex 接入".into()
        } else if !groups.is_empty() && st.codex.connected {
            format!("{} 个会话 · {unread} 条未读", groups.len())
        } else if st.codex.bridge_status.is_empty() {
            "等待 Codex 消息".into()
        } else {
            st.codex.bridge_status.clone()
        };
        ui.c.fill_circle(
            Point::new(header.x + 5.0, header.center().y),
            3.0,
            if !st.settings.codex_enabled {
                palette::TEXT_FAINT
            } else if st.codex.connected {
                palette::GREEN
            } else {
                palette::RED
            },
        );
        ui.c.text_raw(
            &status,
            Rect::new(header.x + 17.0, header.y, header.w - 17.0, header.h),
            &TextStyle::footnote().color(palette::TEXT_DIM),
        );
    }

    let content = Rect::new(area.x, area.y + 42.0, area.w, area.h - 42.0);
    if !st.settings.codex_enabled {
        let empty = Rect::new(content.x, content.y, content.w, content.h - 52.0);
        super::empty_state(
            ui,
            empty,
            Icon::Codex,
            "开启 Codex 提醒",
            "支持快捷回复和原会话对话，审批仍在 Codex 处理",
        );
        let width = ui.pill_width("打开接入设置", true, true);
        let button = Rect::new(
            content.center().x - width / 2.0,
            empty.center().y + 55.0,
            width,
            PILL_SMALL,
        );
        if ui
            .pill(
                wid("codex.enable_setup"),
                button,
                "打开接入设置",
                Some(Icon::Gear),
                PillStyle::Primary(st.accent),
            )
            .clicked
        {
            st.cmd(Command::CodexSetup);
        }
        return;
    }
    if let Some(index) = selected {
        let name = st
            .codex
            .conversations
            .get(&st.codex.entries[index].message.thread_id)
            .map(Conversation::title);
        if let Some(command) = draw_detail(
            ui,
            &mut st.codex.entries[index],
            &mut st.codex.message_scroll,
            content,
            st.accent,
            name,
        ) {
            st.cmd(command);
        }
        return;
    }
    if st.codex.entries.is_empty() {
        super::empty_state(
            ui,
            content,
            Icon::Codex,
            "还没有 Codex 消息",
            "接入后可查看提醒和继续对话，审批仍在 Codex 处理",
        );
        return;
    }

    let total = groups.len() as f32 * (ROW_H + ROW_GAP) - ROW_GAP;
    st.codex.list_scroll = (st.codex.list_scroll - ui.wheel_in(content) * 40.0)
        .clamp(0.0, (total - content.h).max(0.0));
    let mut picked = None;
    ui.c.clip(content);
    for (index, group) in groups.iter().enumerate() {
        let entry = &st.codex.entries[group.representative];
        let latest = &st.codex.entries[group.members[0]];
        let unread = group
            .members
            .iter()
            .filter(|&&index| st.codex.entries[index].unread)
            .count();
        let row = Rect::new(
            content.x,
            content.y + index as f32 * (ROW_H + ROW_GAP) - st.codex.list_scroll,
            content.w,
            ROW_H,
        );
        let top = row.y.max(content.y);
        let bottom = row.bottom().min(content.bottom());
        if bottom <= top {
            continue;
        }
        let hit = Rect::new(row.x, top, row.w, bottom - top);
        let response = ui.button(wid_n("codex.entry", wid(&entry.message.id)), hit);
        let fill = if response.pressed {
            palette::SURFACE_PRESSED
        } else if response.hovered {
            palette::SURFACE_HOVER
        } else {
            palette::SURFACE
        };
        ui.c.fill_rrect(row, radius::PANEL, fill);
        let x = row.x + 16.0;
        let chat = st.codex.conversations.get(&entry.message.thread_id);
        let reply_label = if entry.can_chat() {
            Some(if chat.is_some_and(|chat| chat.sending.is_some()) {
                "发送中"
            } else if chat.is_some_and(|chat| chat.send_uncertain) {
                "待核对"
            } else if entry.confirmed {
                "已确认"
            } else {
                "确认"
            })
        } else if entry.status == ReplyState::Waiting {
            Some("回答")
        } else {
            None
        };
        let reply = reply_label.map(|label| {
            let width = ui.pill_width(label, false, true);
            Rect::new(row.right() - width - 12.0, row.y + 40.0, width, PILL_SMALL)
        });
        let width = reply.map_or(row.w - 32.0, |button| button.x - x - 12.0);
        let time = message_time(latest.received_unix, st);
        let time_w =
            ui.c.measure_raw(&time, &TextStyle::caption(), 150.0)
                .0
                .ceil();
        let time_area = Rect::new(row.right() - 16.0 - time_w, row.y + 13.0, time_w, 18.0);
        ui.c.text_raw(
            &time,
            time_area,
            &TextStyle::caption().right().color(palette::TEXT_FAINT),
        );
        let mut title_right = time_area.x - 12.0;
        if unread > 0 {
            let count = unread.to_string();
            let count_w =
                (ui.c.measure_raw(&count, &TextStyle::caption(), 80.0).0 + 14.0).max(23.0);
            let badge = Rect::new(title_right - count_w, row.y + 11.0, count_w, 21.0);
            ui.c.fill_rrect(badge, 10.5, st.accent.with_alpha(0.22));
            ui.c.text_raw(
                &count,
                badge,
                &TextStyle::caption()
                    .weight(Weight::Semibold)
                    .center()
                    .color(st.accent),
            );
            title_right = badge.x - 10.0;
        }
        if st.settings.codex_preview {
            let title = st
                .codex
                .conversations
                .get(&entry.message.thread_id)
                .map(Conversation::title)
                .unwrap_or(&entry.message.title);
            ui.c.text_raw(
                title,
                Rect::new(x, row.y + 10.0, title_right - x, 24.0),
                &TextStyle::body().weight(if unread > 0 {
                    Weight::Semibold
                } else {
                    Weight::Regular
                }),
            );
            let preview = match chat {
                Some(chat) if entry.can_chat() && !chat.send_status.is_empty() => &chat.send_status,
                _ => &entry.preview,
            };
            let state = if matches!(entry.status, ReplyState::Waiting | ReplyState::Sending) {
                Some((state_label(entry), st.accent))
            } else if entry.message.kind == Kind::Approval {
                Some(("待审批", palette::ORANGE))
            } else {
                None
            };
            let mut preview_x = x;
            if let Some((label, color)) = state {
                let state_w =
                    ui.c.measure_raw(label, &TextStyle::caption(), 100.0)
                        .0
                        .ceil()
                        + 12.0;
                let tag = Rect::new(x, row.y + 43.0, state_w, 20.0);
                ui.c.fill_rrect(tag, 5.0, color.with_alpha(0.11));
                ui.c.text(label, tag, &TextStyle::caption().center().color(color));
                preview_x = tag.right() + 8.0;
            }
            ui.c.text_raw(
                preview,
                Rect::new(preview_x, row.y + 43.0, width - (preview_x - x), 20.0),
                &TextStyle::footnote().color(palette::TEXT_DIM),
            );
        } else {
            ui.c.text(
                "Codex 会话",
                Rect::new(x, row.y + 10.0, title_right - x, 24.0),
                &TextStyle::body(),
            );
            ui.c.text(
                "内容已隐藏，点击查看",
                Rect::new(x, row.y + 43.0, width, 20.0),
                &TextStyle::footnote().color(palette::TEXT_DIM),
            );
        }
        if response.clicked {
            picked = Some(Command::CodexSelect {
                id: entry.message.id.clone(),
                compact: false,
            });
        }
        if let (Some(label), Some(button)) = (reply_label, reply) {
            // 裁切区外不注册按钮，避免滚动时点击到相邻区域。
            if matches!(label, "发送中" | "已确认") {
                ui.c.fill_rrect(button, button.h / 2.0, palette::SURFACE);
                ui.c.text(
                    label,
                    button,
                    &TextStyle::footnote().center().color(palette::TEXT_FAINT),
                );
            } else if button.y >= content.y
                && button.bottom() <= content.bottom()
                && ui
                    .pill(
                        wid_n("codex.entry-reply", wid(&entry.message.id)),
                        button,
                        label,
                        None,
                        PillStyle::Primary(st.accent),
                    )
                    .clicked
            {
                picked = Some(if label == "确认" {
                    Command::CodexConfirm(entry.message.id.clone())
                } else {
                    Command::CodexSelect {
                        id: entry.message.id.clone(),
                        compact: true,
                    }
                });
            }
        }
    }
    ui.c.unclip();
    if let Some(command) = picked {
        ui.blur();
        st.cmd(command);
    }
}

fn draw_detail(
    ui: &mut Ui,
    entry: &mut Entry,
    scroll: &mut f32,
    area: Rect,
    accent: Color,
    name: Option<&str>,
) -> Option<Command> {
    ui.c.text_raw(
        name.unwrap_or(&entry.message.title),
        Rect::new(area.x + 2.0, area.y, area.w - 4.0, 25.0),
        &TextStyle::headline(),
    );
    ui.c.text_raw(
        state_label(entry),
        Rect::new(area.x + 2.0, area.y + 28.0, area.w - 4.0, 18.0),
        &TextStyle::footnote().color(kind_color(entry.message.kind, accent)),
    );

    let waiting = entry.status == ReplyState::Waiting;
    let has_draft = !waiting && !entry.draft.is_empty();
    let footer_h = if waiting {
        102.0
    } else if has_draft {
        88.0
    } else {
        62.0
    };
    let body = Rect::new(
        area.x,
        area.y + 57.0,
        area.w,
        (area.h - 57.0 - footer_h).max(24.0),
    );
    ui.c.fill_rrect(body, radius::PANEL, palette::SURFACE);
    let markdown_command = conversation::draw_message(
        ui,
        &entry.message.id,
        &entry.message.text,
        scroll,
        body.inset(13.0, 10.0),
        accent,
    );

    if waiting {
        let input = Rect::new(area.x, area.bottom() - 91.0, area.w, 34.0);
        // 回车只交给输入法和输入框，发送必须由用户明确点击。
        ui.text_input(
            wid_n("codex.reply", wid(&entry.message.id)),
            input,
            &mut entry.draft,
            "输入简短回答，点击发送",
            MAX_REPLY_CHARS,
        );
        ui.c.text(
            "回答仅返回这条等待中的请求",
            Rect::new(area.x + 2.0, input.bottom() + 3.0, area.w - 4.0, 17.0),
            &TextStyle::caption().color(palette::TEXT_FAINT),
        );
        let y = area.bottom() - PILL_SMALL;
        let send_w = ui.pill_width("发送回答", false, true);
        let cancel_w = ui.pill_width("取消等待", false, true);
        let send = Rect::new(area.right() - send_w, y, send_w, PILL_SMALL);
        let cancel = Rect::new(send.x - cancel_w - 8.0, y, cancel_w, PILL_SMALL);
        if ui
            .pill(
                wid("codex.cancel"),
                cancel,
                "取消等待",
                None,
                PillStyle::Utility,
            )
            .clicked
        {
            ui.blur();
            return Some(Command::CodexCancel(entry.message.id.clone()));
        }
        if !entry.draft.trim().is_empty() {
            if ui
                .pill(
                    wid("codex.send"),
                    send,
                    "发送回答",
                    None,
                    PillStyle::Primary(accent),
                )
                .clicked
            {
                ui.blur();
                return Some(Command::CodexReply {
                    id: entry.message.id.clone(),
                    text: entry.draft.trim().to_owned(),
                });
            }
        } else {
            ui.c.fill_rrect(send, PILL_SMALL / 2.0, palette::SURFACE);
            ui.c.text(
                "发送回答",
                send,
                &TextStyle::footnote()
                    .weight(Weight::Semibold)
                    .color(palette::DISABLED)
                    .center(),
            );
        }
        return markdown_command;
    }

    let hint = match entry.status {
        ReplyState::ReadOnly if entry.message.kind == Kind::Approval => {
            "请在 Codex 中检查并处理审批请求"
        }
        ReplyState::ReadOnly => "此消息可预览，请打开 Codex 继续对话",
        ReplyState::Replied => "回答已发送到原始请求",
        ReplyState::Sending => "正在返回回答，请稍候",
        ReplyState::Expired => "请求已结束，草稿仍为你保留",
        ReplyState::Cancelled => "已取消等待，可打开 Codex 继续",
        ReplyState::Waiting => unreachable!(),
    };
    if has_draft {
        let draft = format!("保留的回答：{}", entry.draft);
        ui.c.text_raw(
            &draft,
            Rect::new(area.x + 2.0, area.bottom() - 80.0, area.w - 4.0, 19.0),
            &TextStyle::footnote().color(palette::TEXT_FAINT),
        );
    }
    ui.c.text(
        hint,
        Rect::new(area.x + 2.0, area.bottom() - 54.0, area.w - 4.0, 18.0),
        &TextStyle::footnote().color(palette::TEXT_DIM),
    );
    let open_w = ui.pill_width("打开 Codex", true, true);
    let open = Rect::new(
        area.right() - open_w,
        area.bottom() - PILL_SMALL,
        open_w,
        PILL_SMALL,
    );
    if ui
        .pill(
            wid("codex.open"),
            open,
            "打开 Codex",
            Some(Icon::Open),
            PillStyle::Neutral,
        )
        .clicked
    {
        return Some(Command::CodexOpen(entry.message.thread_id.clone()));
    }
    if has_draft {
        let copy_w = ui.pill_width("复制回答", false, true);
        let copy = Rect::new(open.x - copy_w - 8.0, open.y, copy_w, PILL_SMALL);
        if ui
            .pill(
                wid("codex.copy"),
                copy,
                "复制回答",
                None,
                PillStyle::Utility,
            )
            .clicked
        {
            return Some(Command::CodexCopy(entry.draft.clone()));
        }
    }
    markdown_command
}

fn message_time(unix: i64, st: &AppState) -> String {
    if unix <= 0 {
        return "时间未知".into();
    }
    let local = crate::clock::local_at(unix);
    let clock = poni_core::datetime::fmt_clock(&local, st.settings.use_24h);
    if local.year != st.local.year {
        return format!("{}/{:02}/{:02} {clock}", local.year, local.month, local.day);
    }
    if (local.month, local.day) == (st.local.month, st.local.day) {
        return clock;
    }
    format!("{:02}/{:02} {clock}", local.month, local.day)
}

fn state_label(entry: &Entry) -> &'static str {
    match entry.status {
        ReplyState::Replied => "已回答",
        ReplyState::Sending => "正在发送",
        ReplyState::Expired => "请求已结束",
        ReplyState::Cancelled => "已取消",
        ReplyState::Waiting => "等待回答",
        ReplyState::ReadOnly => match entry.message.kind {
            Kind::Completed => "已回复",
            Kind::Failed => "执行异常",
            Kind::Approval => "等待审批",
            Kind::Question => "问题提醒",
        },
    }
}

fn kind_color(kind: Kind, accent: Color) -> Color {
    match kind {
        Kind::Failed => palette::RED,
        Kind::Approval => palette::ORANGE,
        Kind::Completed | Kind::Question => accent,
    }
}

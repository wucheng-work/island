//! Codex 接收、对话和提醒的主线程生命周期。
//! 与窗口调度共用同一个 App，不创建另一份状态或后台任务。

use super::*;

impl App {
    pub(super) fn sync_codex(&mut self) {
        if !self.st.settings.codex_enabled {
            self.stop_codex();
            return;
        }
        if self.codex_bridge.is_some() {
            return;
        }
        match crate::codex_bridge::Bridge::start(self.sink.clone()) {
            Ok(bridge) => {
                self.codex_bridge = Some(bridge);
                self.st.codex.bridge_status = "本机接收已开启".into();
                self.st.codex.connected = true;
            }
            Err(error) => {
                pwarn!("codex bridge unavailable: {error}");
                self.st.codex.bridge_status = "接收启动失败，请关闭后重新开启".into();
                self.st.codex.connected = false;
            }
        }
    }

    pub(super) fn stop_codex(&mut self) {
        for (_, sender) in self.codex_pending.drain() {
            let _ = sender.send(crate::codex_bridge::Reply::Cancel);
        }
        // 先取消等待，再关闭本机连接，避免主线程等待尚未退出的请求线程。
        if let Some(mut bridge) = self.codex_bridge.take() {
            bridge.stop();
        }
        self.codex_notices.clear();
        self.st.codex = crate::codex::Inbox::default();
        if self.codex_notice_shown() {
            self.st.notice = None;
            self.outgoing = None;
        }
        if self.st.page == PageKind::Codex {
            self.blur_text();
        }
    }

    pub(super) fn codex_notice_shown(&self) -> bool {
        self.st.notice.as_ref().is_some_and(|notice| {
            matches!(
                notice.action,
                Some(NoticeAction::OpenPage(PageKind::Codex) | NoticeAction::OpenCodex(_))
            )
        })
    }

    pub(super) fn on_codex(&mut self, event: crate::codex_bridge::BridgeEvent) {
        use crate::codex::{Entry, ReplyState};
        use crate::codex_bridge::{BridgeEvent, Reply};
        match event {
            BridgeEvent::Client(event) => self.on_codex_client(event),
            BridgeEvent::Incoming { message, reply } => {
                if !self.st.settings.codex_enabled || self.codex_bridge.is_none() {
                    if let Some(sender) = reply {
                        let _ = sender.send(Reply::Cancel);
                    }
                    return;
                }
                let duplicate = self.st.codex.entries.iter().any(|entry| {
                    entry.message.id == message.id
                        || (reply.is_none()
                            && !message.thread_id.is_empty()
                            && !message.turn_id.is_empty()
                            && entry.message.thread_id == message.thread_id
                            && entry.message.turn_id == message.turn_id
                            && entry.message.kind == message.kind)
                });
                if duplicate {
                    if let Some(sender) = reply {
                        let _ = sender.send(Reply::Cancel);
                    }
                    return;
                }
                if self.st.codex.entries.len() >= 40 {
                    let removable = self.st.codex.entries.iter().rposition(|entry| {
                        !matches!(entry.status, ReplyState::Waiting | ReplyState::Sending)
                            && self
                                .st
                                .codex
                                .conversations
                                .get(&entry.message.thread_id)
                                .is_none_or(|chat| {
                                    chat.draft.is_empty()
                                        && chat.sending.is_none()
                                        && !chat.send_uncertain
                                })
                    });
                    let Some(index) = removable else {
                        if let Some(sender) = reply {
                            let _ = sender.send(Reply::Cancel);
                        }
                        self.st.notify(
                            Icon::Codex,
                            self.st.accent,
                            "Codex 消息列表已满",
                            "请先处理等待中的回答或保留的草稿",
                            6.0,
                            Some(NoticeAction::OpenPage(PageKind::Codex)),
                        );
                        return;
                    };
                    let old = self.st.codex.entries.remove(index);
                    if let Some(sender) = self.codex_pending.remove(&old.message.id) {
                        let _ = sender.send(Reply::Cancel);
                    }
                    self.codex_notices.retain(|id| *id != old.message.id);
                }
                let status = if let Some(sender) = reply {
                    self.codex_pending.insert(message.id.clone(), sender);
                    ReplyState::Waiting
                } else {
                    ReplyState::ReadOnly
                };
                let thread_id = message.thread_id.clone();
                self.codex_notices.push_back(message.id.clone());
                self.st.codex.entries.insert(
                    0,
                    Entry {
                        preview: crate::codex_client::markdown::summary(&message.text),
                        message,
                        received_unix: clock::unix_now(),
                        draft: String::new(),
                        status,
                        confirmed: false,
                        unread: true,
                    },
                );
                let retained_threads: std::collections::HashSet<_> = self
                    .st
                    .codex
                    .entries
                    .iter()
                    .map(|entry| entry.message.thread_id.clone())
                    .collect();
                // 仅保留仍有入口或未提交内容的会话，不因通知轮换丢掉草稿。
                self.st.codex.conversations.retain(|id, chat| {
                    retained_threads.contains(id)
                        || !chat.draft.is_empty()
                        || chat.sending.is_some()
                        || chat.send_uncertain
                });
                if crate::codex_client::valid_thread_id(&thread_id) {
                    let chat = self
                        .st
                        .codex
                        .conversations
                        .entry(thread_id.clone())
                        .or_default();
                    if chat.sending.is_none() && !chat.send_uncertain {
                        chat.send_status.clear();
                    }
                    let selected_thread = self
                        .st
                        .codex
                        .selected
                        .as_ref()
                        .and_then(|id| {
                            self.st
                                .codex
                                .entries
                                .iter()
                                .find(|entry| entry.message.id == *id)
                        })
                        .map(|entry| &entry.message.thread_id);
                    let include_messages = self.expanded
                        && self.st.page == PageKind::Codex
                        && !self.st.codex.compact
                        && selected_thread == Some(&thread_id);
                    // 提醒到达时只取名称；完整历史仍只在对话页可见时读取。
                    self.load_codex(&thread_id, false, include_messages);
                }
                self.mark_visible_codex_read();
                self.show_next_codex_notice();
            }
            BridgeEvent::Closed { id, answered } => {
                self.codex_pending.remove(&id);
                if let Some(entry) = self
                    .st
                    .codex
                    .entries
                    .iter_mut()
                    .find(|entry| entry.message.id == id)
                {
                    if matches!(entry.status, ReplyState::Waiting | ReplyState::Sending) {
                        entry.status = if answered {
                            ReplyState::Replied
                        } else {
                            ReplyState::Expired
                        };
                    }
                }
                if self.st.page == PageKind::Codex && self.st.codex.selected.as_ref() == Some(&id) {
                    self.blur_text();
                }
            }
        }
    }

    pub(super) fn select_codex(&mut self, id: &str, compact: bool) {
        let Some(entry) = self
            .st
            .codex
            .entries
            .iter()
            .find(|entry| entry.message.id == id)
        else {
            return;
        };
        let thread_id = entry.message.thread_id.clone();
        let conversation = entry.status == crate::codex::ReplyState::ReadOnly
            && entry.message.kind != crate::codex::Kind::Approval
            && crate::codex_client::valid_thread_id(&thread_id);
        self.blur_text();
        self.st.page = PageKind::Codex;
        self.st.codex.selected = Some(id.to_owned());
        self.st.codex.compact = compact;
        self.st.codex.message_scroll = 0.0;
        self.mark_visible_codex_read();
        if conversation {
            self.st
                .codex
                .conversations
                .entry(thread_id.clone())
                .or_default();
            if !compact && self.expanded {
                self.load_codex(&thread_id, false, true);
            }
        }
        self.request_frame();
    }

    fn mark_visible_codex_read(&mut self) {
        if !self.expanded
            || self.st.page != PageKind::Codex
            || self.locked
            || self.display_off
            || self.fullscreen_hidden
        {
            return;
        }
        let Some(selected) = self.st.codex.selected.as_ref().and_then(|id| {
            self.st
                .codex
                .entries
                .iter()
                .find(|entry| entry.message.id == *id)
        }) else {
            return;
        };
        let selected_id = selected.message.id.clone();
        let thread_id = selected.message.thread_id.clone();
        let full_conversation = !self.st.codex.compact && selected.can_chat();
        for entry in &mut self.st.codex.entries {
            if entry.message.id == selected_id
                || (full_conversation && entry.can_chat() && entry.message.thread_id == thread_id)
            {
                entry.unread = false;
            }
        }
    }

    pub(super) fn refresh_codex_conversation(&mut self) {
        self.mark_visible_codex_read();
        if !self.expanded || self.st.page != PageKind::Codex || self.st.codex.compact {
            return;
        }
        let thread_id = self
            .st
            .codex
            .selected
            .as_ref()
            .and_then(|id| {
                self.st
                    .codex
                    .entries
                    .iter()
                    .find(|entry| entry.message.id == *id)
            })
            .filter(|entry| {
                entry.status == crate::codex::ReplyState::ReadOnly
                    && entry.message.kind != crate::codex::Kind::Approval
            })
            .map(|entry| entry.message.thread_id.clone());
        if let Some(thread_id) = thread_id {
            self.load_codex(&thread_id, false, true);
        }
    }

    pub(super) fn load_codex(&mut self, thread_id: &str, older: bool, include_messages: bool) {
        if !self.st.settings.codex_enabled
            || self.locked
            || self.display_off
            || !crate::codex_client::valid_thread_id(thread_id)
        {
            return;
        }
        let busy = self
            .st
            .codex
            .conversations
            .values()
            .filter(|chat| chat.loading.is_some())
            .count();
        let Some(chat) = self.st.codex.conversations.get_mut(thread_id) else {
            return;
        };
        if chat.loading.is_some() {
            if include_messages && !older {
                chat.refresh_after_load = true;
            }
            return;
        }
        if busy >= 2 {
            chat.error = "其他对话正在加载，请稍后点刷新".into();
            return;
        }
        let cursor = if older {
            let Some(cursor) = chat.next_cursor.clone() else {
                return;
            };
            Some(cursor)
        } else {
            None
        };
        let request = crate::codex::request_id();
        chat.loading = Some(request);
        chat.loading_older = older;
        chat.error.clear();
        crate::codex_client::history(
            thread_id.to_owned(),
            request,
            cursor,
            include_messages,
            self.sink.clone(),
        );
    }

    pub(super) fn confirm_codex(&mut self, id: &str) -> bool {
        let Some(entry) = self
            .st
            .codex
            .entries
            .iter()
            .find(|entry| entry.message.id == id)
        else {
            return false;
        };
        if !entry.can_chat() || entry.confirmed {
            return false;
        }
        let thread_id = entry.message.thread_id.clone();
        self.send_codex(&thread_id, "确认".into(), Some(id.to_owned()))
    }

    pub(super) fn send_codex(
        &mut self,
        thread_id: &str,
        text: String,
        confirmation: Option<String>,
    ) -> bool {
        if !self.st.settings.codex_enabled || self.locked || self.display_off {
            return false;
        }
        let Some(chat) = self.st.codex.conversations.get_mut(thread_id) else {
            return false;
        };
        if chat.sending.is_some() || chat.send_uncertain {
            return false;
        }
        let request = crate::codex::request_id();
        chat.sending = Some(request);
        chat.confirmation = confirmation;
        chat.send_status = "正在加入 Codex 队列…".into();
        crate::codex_client::queue(thread_id.to_owned(), request, text, self.sink.clone());
        self.blur_text();
        true
    }

    fn on_codex_client(&mut self, event: crate::codex_client::ClientEvent) {
        use crate::codex_client::{ClientEvent, QueueOutcome};
        if !self.st.settings.codex_enabled {
            return;
        }
        match event {
            ClientEvent::Queued {
                thread_id,
                request_id,
                outcome,
            } => {
                let Some(chat) = self.st.codex.conversations.get_mut(&thread_id) else {
                    return;
                };
                if chat.sending != Some(request_id) {
                    return;
                }
                chat.sending = None;
                let confirmation = chat.confirmation.clone();
                let mut notice_result = None;
                match outcome {
                    QueueOutcome::Queued => {
                        if let Some(id) = chat.confirmation.take() {
                            if let Some(entry) = self
                                .st
                                .codex
                                .entries
                                .iter_mut()
                                .find(|entry| entry.message.id == id)
                            {
                                entry.confirmed = true;
                                entry.unread = false;
                            }
                            // 保留动作及点击占位，成功反馈期间也不因悬停而展开。
                            notice_result = Some((
                                "确认已加入 Codex 队列",
                                Icon::Check,
                                palette::GREEN,
                                Some(InlineAction::CodexConfirm),
                            ));
                        } else {
                            chat.draft.clear();
                        }
                        chat.send_status = "已加入 Codex 队列；未继续时请打开原会话".into();
                    }
                    QueueOutcome::NotSent(error) => {
                        chat.confirmation = None;
                        chat.send_status = error;
                        notice_result = Some((
                            "确认未发送，可点击重试",
                            Icon::Codex,
                            palette::RED,
                            Some(InlineAction::CodexConfirm),
                        ));
                    }
                    QueueOutcome::Uncertain(error) => {
                        chat.send_uncertain = true;
                        chat.send_status = error;
                        notice_result = Some((
                            "结果待确认，请点击查看并核对原会话",
                            Icon::Codex,
                            palette::ORANGE,
                            Some(InlineAction::Open),
                        ));
                    }
                }
                if let Some(id) = confirmation {
                    if let Some(notice) = self
                        .st
                        .notice
                        .as_mut()
                        .filter(|notice| notice.action == Some(NoticeAction::OpenCodex(id)))
                    {
                        if let Some((subtitle, icon, color, inline)) = notice_result {
                            notice.subtitle = subtitle.into();
                            notice.icon = icon;
                            notice.color = color;
                            notice.inline = inline;
                            notice.until = self.st.now + 6.0;
                        }
                    }
                }
            }
            ClientEvent::History {
                thread_id,
                request_id,
                result,
            } => {
                let Some(chat) = self.st.codex.conversations.get_mut(&thread_id) else {
                    return;
                };
                if chat.loading != Some(request_id) {
                    return;
                }
                chat.loading = None;
                match result {
                    Ok(history) => {
                        chat.name = history.name.unwrap_or_default();
                        chat.name_loaded = true;
                        if let Some(mut items) = history.items {
                            if chat.loading_older {
                                items
                                    .retain(|item| !chat.items.iter().any(|old| old.id == item.id));
                                items.append(&mut chat.items);
                                items.truncate(120);
                                chat.items = items;
                            } else {
                                chat.items = items;
                                chat.scroll_to_end = true;
                            }
                            chat.next_cursor = history.next_cursor;
                            chat.loaded = true;
                        }
                        chat.error.clear();
                    }
                    Err(error) => chat.error = error,
                }
                let name = chat.title().to_owned();
                let refresh = std::mem::take(&mut chat.refresh_after_load);
                if refresh
                    && !self.st.codex.compact
                    && self.st.page == PageKind::Codex
                    && self.expanded
                {
                    self.load_codex(&thread_id, false, true);
                }
                if self.st.settings.codex_preview && !name.is_empty() {
                    if let Some(notice) = self.st.notice.as_mut() {
                        if let Some(NoticeAction::OpenCodex(id)) = &notice.action {
                            if self.st.codex.entries.iter().any(|entry| {
                                entry.message.id == *id && entry.message.thread_id == thread_id
                            }) {
                                notice.title = name;
                            }
                        }
                    }
                }
            }
        }
        self.request_frame();
    }

    pub(super) fn reply_codex(&mut self, id: &str, text: Option<String>) {
        use crate::codex::ReplyState;
        let Some(entry) = self
            .st
            .codex
            .entries
            .iter_mut()
            .find(|entry| entry.message.id == id)
        else {
            return;
        };
        if entry.status != ReplyState::Waiting {
            return;
        }
        if text
            .as_ref()
            .is_some_and(|text| text.trim().is_empty() || text.chars().count() > 2000)
        {
            self.st.notify(
                Icon::Bell,
                palette::ORANGE,
                "回答未发送",
                "请输入 1–2000 字的回答",
                4.0,
                None,
            );
            return;
        }
        let reply = match text {
            Some(text) => crate::codex_bridge::Reply::Answer(text),
            None => crate::codex_bridge::Reply::Cancel,
        };
        let cancelled = matches!(reply, crate::codex_bridge::Reply::Cancel);
        let sent = self
            .codex_pending
            .remove(id)
            .is_some_and(|sender| sender.send(reply).is_ok());
        entry.status = if !sent {
            ReplyState::Expired
        } else if cancelled {
            ReplyState::Cancelled
        } else {
            ReplyState::Sending
        };
        self.blur_text();
    }

    pub(super) fn show_next_codex_notice(&mut self) {
        if self.st.notice.is_some() || self.locked || self.display_off || self.fullscreen_hidden {
            return;
        }
        while let Some(id) = self.codex_notices.pop_front() {
            let Some(entry) = self
                .st
                .codex
                .entries
                .iter()
                .find(|entry| entry.message.id == id)
            else {
                continue;
            };
            let title = if self.st.settings.codex_preview {
                self.st
                    .codex
                    .conversations
                    .get(&entry.message.thread_id)
                    .map(|chat| chat.title().to_owned())
                    .unwrap_or_else(|| entry.message.title.clone())
            } else {
                "Codex 有新消息".into()
            };
            let text = if self.st.settings.codex_preview {
                entry.preview.clone()
            } else {
                "点击查看消息或回答等待中的问题".into()
            };
            let thread_id = entry.message.thread_id.clone();
            let inline = if entry.can_chat() && !entry.confirmed {
                if self
                    .st
                    .codex
                    .conversations
                    .get(&thread_id)
                    .is_some_and(|chat| chat.send_uncertain)
                {
                    Some(InlineAction::Open)
                } else {
                    Some(InlineAction::CodexConfirm)
                }
            } else if entry.status == crate::codex::ReplyState::Waiting {
                Some(InlineAction::Open)
            } else {
                None
            };
            self.st.notify(
                Icon::Codex,
                self.st.accent,
                title,
                text,
                6.0,
                Some(NoticeAction::OpenCodex(id)),
            );
            if let Some(inline) = inline {
                self.st.set_notice_inline(inline);
            }
            if self.st.settings.codex_preview
                && self
                    .st
                    .codex
                    .conversations
                    .get(&thread_id)
                    .is_some_and(|chat| chat.name.is_empty() && chat.loading.is_none())
            {
                self.load_codex(&thread_id, false, false);
            }
            if self.st.settings.codex_sound {
                self.st.cmd(Command::Play(crate::commands::Sound::Codex));
            }
            break;
        }
    }
}

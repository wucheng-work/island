//! 多邮箱配置、预览、翻译与提醒的主线程编排。
//! 服务仍负责网络和凭据；这里接收结果并更新唯一的 AppState。

use super::*;

impl App {
    pub(super) fn clear_mail_translation(&mut self) {
        self.mail_translation_id = self.mail_translation_id.wrapping_add(1);
        self.mail_translation = None;
        self.st.mail.translation = None;
        self.st.mail.translation_loading = false;
        self.st.mail.show_translation = false;
    }

    fn translate_mail(&mut self) {
        if !self.expanded
            || self.st.page != PageKind::Mail
            || self.away.0
            || self.away.1
            || self.st.power.saver
            || self.smoke.is_some()
            || self.st.mail.translation_loading
        {
            return;
        }
        let Some((account_id, message_id)) = &self.st.mail.selected_message else {
            return;
        };
        let Some(account) = self
            .st
            .settings
            .mail
            .accounts
            .iter()
            .find(|account| account.id == *account_id && account.enabled)
            .cloned()
        else {
            return;
        };
        let Some(Ok(preview)) = self.st.mail.preview.clone() else {
            return;
        };
        if preview.message.id != *message_id {
            return;
        }
        self.clear_mail_translation();
        self.st.mail.translation_loading = true;
        match services::translation::start(
            self.mail_translation_id,
            &account,
            &preview,
            self.sink.clone(),
        ) {
            Ok(request) => self.mail_translation = Some(request),
            Err(error) => {
                self.st.mail.translation_loading = false;
                self.st.mail.translation = Some(Err(error));
            }
        }
    }

    pub(super) fn configure_mail(&mut self) {
        let suspended = self.away.0 || self.away.1 || self.st.power.saver || self.smoke.is_some();
        let invalid_preview = self
            .st
            .mail
            .selected_message
            .as_ref()
            .is_some_and(|(id, _)| {
                !self
                    .st
                    .settings
                    .mail
                    .accounts
                    .iter()
                    .any(|account| account.id == *id && account.enabled)
            });
        if suspended || !self.st.settings.mail.enabled || invalid_preview {
            self.clear_mail_translation();
        }
        if self.mail.is_none()
            && self.st.settings.mail.enabled
            && self.st.settings.mail.accounts.iter().any(|a| a.enabled)
        {
            self.mail = Some(services::mail::MailService::new(self.sink.clone()));
        }
        if let Some(service) = &mut self.mail {
            service.configure(&self.st.settings.mail, suspended);
        }
        if !self.st.settings.mail.enabled {
            self.mail_notices.clear();
            self.mail_notice_at = None;
            self.clear_mail_notice();
            self.st.mail = Default::default();
            return;
        }
        let accounts = &self.st.settings.mail.accounts;
        self.mail_notices.retain(|id, _| {
            accounts
                .iter()
                .any(|a| a.id == *id && a.enabled && a.notify)
        });
        self.st
            .mail
            .messages
            .retain(|id, _| accounts.iter().any(|a| a.id == *id));
        self.st
            .mail
            .statuses
            .retain(|id, _| accounts.iter().any(|a| a.id == *id));
        self.st
            .mail
            .unread_counts
            .retain(|id, _| accounts.iter().any(|a| a.id == *id && a.enabled));
        self.st
            .mail
            .unseen
            .retain(|id, _| accounts.iter().any(|a| a.id == *id && a.enabled));
        for account in accounts {
            if !account.enabled {
                self.st
                    .mail
                    .statuses
                    .insert(account.id.clone(), MailConnection::Off);
            } else if suspended {
                self.st
                    .mail
                    .statuses
                    .insert(account.id.clone(), MailConnection::Paused);
            }
        }
        if self
            .st
            .mail
            .selected_message
            .as_ref()
            .is_some_and(|(id, _)| !accounts.iter().any(|a| a.id == *id && a.enabled))
        {
            self.st.mail.selected_message = None;
            self.st.mail.preview = None;
            self.st.mail.preview_loading = false;
        }
        if self.mail_notices.is_empty() {
            self.mail_notice_at = None;
        }
        if let Some((_, id)) = &self.mail_notice {
            if suspended
                || !accounts
                    .iter()
                    .any(|a| a.id == *id && a.enabled && a.notify)
            {
                self.clear_mail_notice();
            }
        }
    }

    pub(super) fn close_mail_editor(&mut self) {
        let was_open = self.st.mail.editor.take().is_some();
        self.st.mail.editor_rect = None;
        self.st.mail.editor_clip = None;
        settings_host::clear_mail_editor();
        if was_open {
            if let Some(ui) = &mut self.settings_ui {
                ui.view.dirty = true;
            }
        }
    }

    pub(super) fn mail_action(&mut self, action: MailAction) {
        if action == MailAction::CloseEditor {
            self.close_mail_editor();
            settings_host::redraw();
            return;
        }
        if !self.st.settings.mail.enabled {
            return;
        }
        match action {
            MailAction::Translate => self.translate_mail(),
            MailAction::CancelTranslation => self.clear_mail_translation(),
            MailAction::ClosePreview => {
                self.clear_mail_translation();
                self.st.mail.selected_message = None;
                self.st.mail.preview = None;
                self.st.mail.preview_loading = false;
                self.st.mail.preview_scroll = 0.0;
            }
            MailAction::EditAccount(id) => {
                if id
                    .as_ref()
                    .is_some_and(|id| self.st.mail.busy_accounts.contains(id))
                {
                    return;
                }
                let account = id.and_then(|id| {
                    self.st
                        .settings
                        .mail
                        .accounts
                        .iter()
                        .find(|a| a.id == id)
                        .cloned()
                });
                let editor = account
                    .as_ref()
                    .map_or(crate::mail::MailEditor::New, |account| {
                        crate::mail::MailEditor::Edit(account.id.clone())
                    });
                if self.st.mail.editor.as_ref() == Some(&editor) {
                    self.close_mail_editor();
                    settings_host::redraw();
                    return;
                }
                self.close_mail_editor();
                self.st.mail.editor = Some(editor);
                self.blur_text();
                self.collapse();
                self.open_settings(Some(Section::Mail));
                self.pending.push(Deferred::EditMailAccount {
                    account,
                    sink: self.sink.clone(),
                });
            }
            MailAction::ConfirmRemove(id) => {
                if self.st.mail.busy_accounts.contains(&id) {
                    return;
                }
                if let Some(account) = self
                    .st
                    .settings
                    .mail
                    .accounts
                    .iter()
                    .find(|a| a.id == id)
                    .cloned()
                {
                    self.blur_text();
                    self.pending.push(Deferred::RemoveMailAccount {
                        account,
                        sink: self.sink.clone(),
                    });
                }
            }
            MailAction::OpenMailbox(id) => {
                if let Some(a) = self.st.settings.mail.accounts.iter().find(|a| a.id == id) {
                    services::open(a.provider.web_url());
                }
            }
            mut action => {
                if matches!(&action, MailAction::Save { .. } | MailAction::Remove(_))
                    && !self.settings_writable
                {
                    self.saved(
                        crate::persistence::SaveFile::Settings,
                        Err(crate::persistence::SaveError::Protected),
                    );
                    return;
                }
                match &mut action {
                    MailAction::Authorize(id) => {
                        if self.st.mail.busy_accounts.contains(id) {
                            return;
                        }
                        let Some(account) = self
                            .st
                            .settings
                            .mail
                            .accounts
                            .iter()
                            .find(|a| a.id == *id && a.enabled)
                        else {
                            return;
                        };
                        self.st
                            .mail
                            .authorizing_accounts
                            .insert(id.clone(), account.revision);
                        self.st.mail.busy_accounts.insert(id.clone());
                        self.st.mail.account_error = None;
                        self.st
                            .mail
                            .statuses
                            .insert(id.clone(), MailConnection::Authorizing);
                    }
                    MailAction::Save { account, .. } => {
                        if self.st.mail.busy_accounts.contains(&account.id) {
                            return;
                        }
                        let current = self
                            .st
                            .settings
                            .mail
                            .accounts
                            .iter()
                            .find(|a| a.id == account.id)
                            .map_or(0, |a| a.revision);
                        account.revision = account.revision.max(current.saturating_add(1));
                        self.mail_removing.remove(&account.id);
                        self.mail_pending_saves
                            .insert(account.id.clone(), account.revision);
                        self.st.mail.busy_accounts.insert(account.id.clone());
                        self.st.mail.account_error = None;
                        self.st
                            .mail
                            .statuses
                            .insert(account.id.clone(), MailConnection::Authorizing);
                    }
                    MailAction::Remove(id) => {
                        if self.st.mail.busy_accounts.contains(id) {
                            return;
                        }
                        self.mail_pending_saves.remove(id);
                        self.mail_removing.insert(id.clone());
                        self.st.mail.busy_accounts.insert(id.clone());
                    }
                    MailAction::Preview {
                        account_id,
                        message_id,
                    } => {
                        self.clear_mail_translation();
                        if self.away.0 || self.away.1 || self.st.power.saver {
                            self.st.mail.preview_loading = false;
                            self.st.mail.preview =
                                Some(Err("收信已暂停，请返回桌面或关闭低电量模式后重试".into()));
                            return;
                        }
                        self.st.mail.selected_message =
                            Some((account_id.clone(), message_id.clone()));
                        self.st.mail.preview = None;
                        self.st.mail.preview_loading = true;
                    }
                    _ => {}
                }
                let service = self
                    .mail
                    .get_or_insert_with(|| services::mail::MailService::new(self.sink.clone()));
                service.submit(action);
            }
        }
    }

    pub(super) fn on_mail(&mut self, event: MailEvent) {
        if !self.st.settings.mail.enabled
            && matches!(
                &event,
                MailEvent::Status { .. }
                    | MailEvent::Cached { .. }
                    | MailEvent::Synced { .. }
                    | MailEvent::Preview { .. }
                    | MailEvent::Translated { .. }
                    | MailEvent::AuthorizationUrl(_)
            )
        {
            return;
        }
        match event {
            MailEvent::Authorized {
                account_id,
                revision,
                result,
            } => {
                if self.st.mail.authorizing_accounts.get(&account_id) != Some(&revision) {
                    return;
                }
                self.st.mail.authorizing_accounts.remove(&account_id);
                self.st.mail.busy_accounts.remove(&account_id);
                let Some(account) = self
                    .st
                    .settings
                    .mail
                    .accounts
                    .iter()
                    .find(|a| a.id == account_id && a.revision == revision)
                else {
                    return;
                };
                let state = match result {
                    Ok(()) => MailConnection::Connecting,
                    Err(error) => MailConnection::AuthorizationRequired(error),
                };
                if account.enabled {
                    self.st.mail.statuses.insert(account_id, state);
                }
                self.configure_mail();
            }
            MailEvent::AccountSaved(account) => {
                let pending = self.mail_pending_saves.get(&account.id) == Some(&account.revision);
                let newer = self
                    .st
                    .settings
                    .mail
                    .accounts
                    .iter()
                    .any(|a| a.id == account.id && a.revision >= account.revision);
                if !pending || newer || self.mail_removing.contains(&account.id) {
                    return;
                }
                self.mail_pending_saves.remove(&account.id);
                self.st.mail.busy_accounts.remove(&account.id);
                let identity_changed = self.st.settings.mail.accounts.iter().any(|a| {
                    a.id == account.id
                        && (a.address != account.address
                            || a.provider != account.provider
                            || a.host != account.host
                            || a.port != account.port)
                });
                if identity_changed {
                    self.st.mail.messages.remove(&account.id);
                    self.st.mail.unread_counts.remove(&account.id);
                    self.st.mail.statuses.remove(&account.id);
                    self.st.mail.unseen.remove(&account.id);
                    self.mail_notices.remove(&account.id);
                    if self
                        .st
                        .mail
                        .selected_message
                        .as_ref()
                        .is_some_and(|(id, _)| id == &account.id)
                    {
                        self.st.mail.selected_message = None;
                        self.st.mail.preview = None;
                        self.st.mail.preview_loading = false;
                    }
                    if self
                        .mail_notice
                        .as_ref()
                        .is_some_and(|(_, id)| id == &account.id)
                    {
                        self.clear_mail_notice();
                    }
                }
                if let Some(existing) = self
                    .st
                    .settings
                    .mail
                    .accounts
                    .iter_mut()
                    .find(|a| a.id == account.id)
                {
                    *existing = account;
                } else {
                    self.st.settings.mail.accounts.push(account);
                }
                self.save_settings();
                self.st.mail.account_error = None;
                self.configure_mail();
            }
            MailEvent::AccountRemoved(id) => {
                if !self.mail_removing.remove(&id) {
                    return;
                }
                self.st.mail.busy_accounts.remove(&id);
                self.st.mail.expanded_accounts.remove(&id);
                if self.st.mail.editor.as_ref() == Some(&crate::mail::MailEditor::Edit(id.clone()))
                {
                    self.close_mail_editor();
                }
                self.st.settings.mail.accounts.retain(|a| a.id != id);
                if self.st.mail.selected_account.as_ref() == Some(&id) {
                    self.st.mail.selected_account = None;
                }
                self.save_settings();
                self.st.mail.account_error = None;
                self.configure_mail();
            }
            MailEvent::AccountError {
                account_id,
                revision,
                error,
            } => {
                let save = revision.is_some()
                    && self.mail_pending_saves.get(&account_id).copied() == revision;
                let remove = revision.is_none() && self.mail_removing.remove(&account_id);
                if save || remove {
                    self.mail_pending_saves.remove(&account_id);
                    self.st.mail.busy_accounts.remove(&account_id);
                    self.st.mail.account_error = Some(error);
                    self.configure_mail();
                }
            }
            MailEvent::Status {
                account_id,
                revision,
                state,
            } => {
                let Some(account) = self
                    .st
                    .settings
                    .mail
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id && account.revision == revision)
                else {
                    return;
                };
                if self.mail_pending_saves.contains_key(&account_id)
                    || self.mail_removing.contains(&account_id)
                    || self.st.mail.authorizing_accounts.contains_key(&account_id)
                {
                    return;
                }
                let suspended =
                    self.away.0 || self.away.1 || self.st.power.saver || self.smoke.is_some();
                let current = match &state {
                    MailConnection::Off => !account.enabled,
                    MailConnection::Paused => account.enabled && suspended,
                    _ => account.enabled && !suspended,
                };
                if !current {
                    return;
                }
                self.st.mail.statuses.insert(account_id, state);
            }
            MailEvent::Cached {
                account_id,
                revision,
                messages,
            } => {
                if self.away.0
                    || self.away.1
                    || self.st.power.saver
                    || self.mail_pending_saves.contains_key(&account_id)
                    || self.mail_removing.contains(&account_id)
                    || self.st.mail.authorizing_accounts.contains_key(&account_id)
                    || !self.st.settings.mail.accounts.iter().any(|account| {
                        account.id == account_id && account.revision == revision && account.enabled
                    })
                {
                    return;
                }
                // 本地快照只恢复列表，不能作为服务器连接成功或未读数为零的依据。
                self.st.mail.messages.insert(account_id, messages);
            }
            MailEvent::Synced {
                account_id,
                revision,
                messages,
                new_ids,
                unread,
            } => {
                let Some(account) = self
                    .st
                    .settings
                    .mail
                    .accounts
                    .iter()
                    .find(|a| a.id == account_id && a.revision == revision && a.enabled)
                else {
                    return;
                };
                if self.mail_pending_saves.contains_key(&account_id)
                    || self.mail_removing.contains(&account_id)
                    || self.st.mail.authorizing_accounts.contains_key(&account_id)
                    || self.away.0
                    || self.away.1
                    || self.st.power.saver
                {
                    return;
                }
                self.st
                    .mail
                    .unread_counts
                    .insert(account_id.clone(), unread);
                self.st
                    .mail
                    .statuses
                    .insert(account_id.clone(), MailConnection::Ready { unread });
                let visible = self.expanded
                    && self.st.page == PageKind::Mail
                    && self
                        .st
                        .mail
                        .selected_account
                        .as_ref()
                        .is_none_or(|id| id == &account_id);
                if !new_ids.is_empty() && !visible {
                    let count = self.st.mail.unseen.entry(account_id.clone()).or_default();
                    *count = count.saturating_add(new_ids.len() as u32);
                    if account.notify {
                        let pending = self.mail_notices.entry(account_id.clone()).or_default();
                        for message in &messages {
                            if new_ids.contains(&message.id)
                                && !pending.iter().any(|m| m.id == message.id)
                            {
                                pending.push(message.clone());
                            }
                        }
                        // 同一轮多账户事件合并一秒，避免连续覆盖正在显示的提示。
                        self.mail_notice_at.get_or_insert(self.st.now + 1.0);
                    }
                }
                self.st.mail.messages.insert(account_id, messages);
            }
            MailEvent::Preview {
                account_id,
                revision,
                message_id,
                result,
            } => {
                if !self.away.0
                    && !self.away.1
                    && !self.st.power.saver
                    && self
                        .st
                        .settings
                        .mail
                        .accounts
                        .iter()
                        .any(|a| a.id == account_id && a.revision == revision && a.enabled)
                    && self.st.mail.selected_message.as_ref() == Some(&(account_id, message_id))
                {
                    self.st.mail.preview_loading = false;
                    self.st.mail.preview = Some(result);
                }
            }
            MailEvent::Translated {
                account_id,
                revision,
                message_id,
                request_id,
                result,
            } => {
                if request_id != self.mail_translation_id {
                    return;
                }
                if !self.expanded
                    || self.st.page != PageKind::Mail
                    || self.away.0
                    || self.away.1
                    || self.st.power.saver
                    || !self.st.settings.mail.accounts.iter().any(|account| {
                        account.id == account_id && account.revision == revision && account.enabled
                    })
                    || self.st.mail.selected_message.as_ref() != Some(&(account_id, message_id))
                {
                    self.clear_mail_translation();
                    return;
                }
                self.mail_translation = None;
                self.st.mail.translation_loading = false;
                self.st.mail.show_translation = result.is_ok();
                self.st.mail.translation = Some(result);
                self.st.mail.preview_scroll = 0.0;
            }
            MailEvent::AuthorizationUrl(url) => services::open(&url),
        }
        if let Some(ui) = &mut self.settings_ui {
            ui.view.dirty = true;
        }
    }

    pub(super) fn clear_mail_notice(&mut self) {
        if self.mail_notice.take().is_some() {
            // 收起面板会延长提示时长，归属必须按动作判断，不能比较截止时间。
            if self
                .st
                .notice
                .as_ref()
                .is_some_and(|n| n.action == Some(NoticeAction::OpenPage(PageKind::Mail)))
            {
                self.st.notice = None;
            }
        }
    }

    pub(super) fn show_mail_notice(&mut self) {
        let Some(at) = self.mail_notice_at else {
            return;
        };
        if self.st.now < at || self.away.0 || self.away.1 || self.st.power.saver {
            return;
        }
        let focused = self.st.settings.mail.quiet_during_focus
            && !self.st.pomodoro.phase.is_break()
            && matches!(
                self.st.pomodoro.run,
                poni_core::pomodoro::RunState::Running { .. }
            );
        if self.expanded && self.st.page == PageKind::Mail {
            self.mail_notices.clear();
            self.mail_notice_at = None;
            return;
        }
        if focused {
            // 专注结束本身会唤醒；保留汇总，不额外轮询等待计时结束。
            self.mail_notice_at = Some(self.st.now);
            return;
        }
        if self.expanded || self.fullscreen_hidden || self.front_full_screen {
            // 展开或全屏期间不启动重复计时，由下一次 UI 状态变化唤起。
            self.mail_notice_at = Some(self.st.now);
            return;
        }
        if self.st.notice.is_some() || self.st.hud.is_some() {
            let until = self
                .st
                .notice
                .as_ref()
                .map_or(self.st.now, |n| n.until)
                .max(self.st.hud.as_ref().map_or(self.st.now, |h| h.until));
            self.mail_notice_at = Some(until + 0.1);
            return;
        }
        let Some(account) = self
            .st
            .settings
            .mail
            .accounts
            .iter()
            .find(|a| a.enabled && a.notify && self.mail_notices.contains_key(&a.id))
            .cloned()
        else {
            self.mail_notices.clear();
            self.mail_notice_at = None;
            return;
        };
        let messages = self.mail_notices.remove(&account.id).unwrap_or_default();
        if messages.is_empty() {
            self.mail_notice_at = (!self.mail_notices.is_empty()).then_some(self.st.now + 1.0);
            return;
        }
        let title = format!("{} · {} 封新邮件", account.name, messages.len());
        let message = &messages[0];
        let detail = match account.privacy {
            poni_core::mail::MailPrivacy::CountOnly => "点击查看邮件".into(),
            poni_core::mail::MailPrivacy::SenderAndSubject => {
                format!("{} · {}", message.sender, message.subject)
            }
            poni_core::mail::MailPrivacy::Preview => format!(
                "{} · {} · {}",
                message.sender, message.subject, message.snippet
            ),
        };
        self.st.notify(
            Icon::Mail,
            self.st.accent,
            title,
            detail,
            6.0,
            Some(NoticeAction::OpenPage(PageKind::Mail)),
        );
        self.mail_notice = Some((self.st.now + 6.0, account.id));
        self.mail_notice_at = (!self.mail_notices.is_empty()).then_some(self.st.now + 6.1);
        if self.st.settings.mail.sound && account.sound {
            services::play_tone(
                self.st
                    .settings
                    .alert_sounds
                    .get(poni_core::alert_sounds::SoundModule::Mail),
            );
        }
    }
}

//! 多邮箱统一列表与只读预览。网络和凭据操作交给后台服务。

use crate::commands::Command;
use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::mail::{MailAction, MailConnection, MailMessage, UnreadSummary};
use crate::state::AppState;
use crate::ui::{wid, KeyInput, PillStyle, Ui};
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::mail::MailAccount;

const PAGE_H: f32 = 330.0;
const PREVIEW_H: f32 = 360.0;
const ROW_H: f32 = 74.0;
const ACCENT: Color = palette::BLUE;

pub fn height(st: &AppState) -> f32 {
    if st.settings.mail.enabled && st.mail.selected_message.is_some() {
        PREVIEW_H
    } else {
        PAGE_H
    }
}

fn time_label(unix: i64, st: &AppState, full: bool) -> String {
    if unix <= 0 {
        return "时间未知".into();
    }
    let local = crate::clock::local_at(unix);
    let clock = poni_core::datetime::fmt_clock(&local, st.settings.use_24h);
    if full || local.year != st.local.year {
        return format!("{}年{}月{}日 {clock}", local.year, local.month, local.day);
    }
    if (local.month, local.day) == (st.local.month, st.local.day) {
        return clock;
    }
    format!("{}月{}日 {clock}", local.month, local.day)
}

fn panel(ui: &mut Ui, area: Rect) {
    ui.c.fill_rrect(area, 12.0, Color::white(0.035));
    ui.c.stroke_rrect(area, 12.0, palette::BORDER, 1.0);
}

fn info(ui: &mut Ui, area: Rect, title: &str, detail: &str) {
    let icon = Rect::new(area.center().x - 17.0, area.y + 38.0, 34.0, 34.0);
    ui.c.icon(Icon::Mail, icon, palette::TEXT_DIM);
    ui.c.text(
        title,
        Rect::new(area.x + 20.0, icon.bottom() + 12.0, area.w - 40.0, 24.0),
        &TextStyle::body().weight(Weight::Semibold).center(),
    );
    ui.c.text(
        detail,
        Rect::new(area.x + 24.0, icon.bottom() + 44.0, area.w - 48.0, 58.0),
        &TextStyle::footnote()
            .wrap()
            .top()
            .center()
            .color(palette::TEXT_DIM),
    );
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    if !st.settings.mail.enabled {
        st.mail.account_filter_open = false;
        panel(ui, area);
        info(
            ui,
            area,
            "邮件已停用",
            "账户配置已保留，当前不收取邮件或显示账户\n可在高级设置的邮件模块中重新启用",
        );
        let setup_x = if cfg!(windows) {
            area.center().x - 65.0
        } else {
            area.center().x - 140.0
        };
        if ui
            .pill(
                wid("mail.setup"),
                Rect::new(setup_x, area.y + 200.0, 130.0, 30.0),
                "邮件设置",
                Some(Icon::Gear),
                PillStyle::UtilityAccent(ACCENT),
            )
            .clicked
        {
            st.cmd(Command::MailSetup);
        }
        #[cfg(target_os = "macos")]
        if ui
            .pill(
                wid("mail.open-native"),
                Rect::new(area.center().x + 10.0, area.y + 200.0, 130.0, 30.0),
                "打开邮件 App",
                Some(Icon::Mail),
                PillStyle::Utility,
            )
            .clicked
        {
            st.cmd(Command::OpenAppleMail);
        }
        return;
    }
    if st.mail.selected_message.is_some() {
        st.mail.account_filter_open = false;
        draw_preview(ui, st, area);
    } else {
        draw_inbox(ui, st, area);
    }
}

fn draw_inbox(ui: &mut Ui, st: &mut AppState, area: Rect) {
    // 账户被移除后，筛选回到全部，避免空列表留在失效账户上。
    if st.mail.selected_account.as_ref().is_some_and(|id| {
        !st.settings
            .mail
            .accounts
            .iter()
            .any(|a| &a.id == id && a.enabled)
    }) {
        st.mail.selected_account = None;
        st.mail.scroll = 0.0;
    }
    let page_hits = ui.hits.len();
    let selected = st.mail.selected_account.clone();
    let account_name = selected
        .as_ref()
        .and_then(|id| st.settings.mail.accounts.iter().find(|a| &a.id == id))
        .map_or("全部邮箱", |a| a.name.as_str());
    let controls_w = if cfg!(windows) { 78.0 } else { 114.0 };
    let selector = Rect::new(area.x, area.y, (area.w - controls_w).max(100.0), 30.0);
    let mut badge_width: f32 = 40.0;
    let badge_style = TextStyle::new(11.0);
    for id in st
        .settings
        .mail
        .accounts
        .iter()
        .filter(|account| account.enabled)
        .map(|account| Some(account.id.as_str()))
        .chain(std::iter::once(None))
    {
        let unread = st.mail.unread_summary(&st.settings.mail.accounts, id);
        badge_width = badge_width.max(
            ui.c.measure_raw(&unread.label(), &badge_style, selector.w)
                .0
                + 29.0,
        );
    }
    badge_width = badge_width.min(selector.w * 0.5);
    let selector_response = ui.button(wid("mail.account-filter"), selector);
    ui.c.fill_rrect(
        selector,
        15.0,
        if selector_response.hovered {
            palette::SURFACE_HOVER
        } else {
            palette::SURFACE
        },
    );
    ui.c.stroke_rrect(selector, 15.0, Color::white(0.08), 1.0);
    let name_color = selected.as_ref().map_or(palette::TEXT, |id| {
        st.mail
            .statuses
            .get(id)
            .unwrap_or(&MailConnection::Off)
            .color()
    });
    let text_x = selector.x + 13.0;
    let badge = Rect::new(
        selector.right() - 34.0 - badge_width,
        selector.center().y - 8.0,
        badge_width,
        16.0,
    );
    ui.c.text_raw(
        account_name,
        Rect::new(
            text_x,
            selector.y,
            (badge.x - text_x - 8.0).max(0.0),
            selector.h,
        ),
        &TextStyle::body().weight(Weight::Semibold).color(name_color),
    );
    draw_unread_badge(
        ui,
        st.mail
            .unread_summary(&st.settings.mail.accounts, selected.as_deref()),
        badge,
    );
    let arrow = Point::new(selector.right() - 18.0, selector.center().y);
    let direction = if st.mail.account_filter_open {
        -1.0
    } else {
        1.0
    };
    ui.c.line(
        Point::new(arrow.x - 4.0, arrow.y - direction * 2.0),
        Point::new(arrow.x, arrow.y + direction * 2.0),
        palette::TEXT_DIM,
        1.6,
    );
    ui.c.line(
        Point::new(arrow.x, arrow.y + direction * 2.0),
        Point::new(arrow.x + 4.0, arrow.y - direction * 2.0),
        palette::TEXT_DIM,
        1.6,
    );
    if selector_response.clicked {
        st.mail.account_filter_open = !st.mail.account_filter_open;
        st.mail.account_filter_scroll = 0.0;
        ui.request_repaint();
    }
    let mut tip = None;
    #[cfg(target_os = "macos")]
    if ui
        .icon_button(
            wid("mail.open-native"),
            Rect::new(area.right() - 100.0, area.y + 1.0, 28.0, 28.0),
            Icon::Mail,
            palette::TEXT_DIM,
            16.0,
        )
        .clicked
    {
        st.cmd(Command::OpenAppleMail);
    }
    if ui
        .icon_button(
            wid("mail.refresh"),
            Rect::new(area.right() - 64.0, area.y + 1.0, 28.0, 28.0),
            Icon::Reset,
            palette::TEXT_DIM,
            16.0,
        )
        .clicked
    {
        st.cmd(Command::Mail(MailAction::Refresh(
            st.mail.selected_account.clone(),
        )));
    }
    if ui
        .icon_button(
            wid("mail.setup"),
            Rect::new(area.right() - 28.0, area.y + 1.0, 28.0, 28.0),
            Icon::Gear,
            palette::TEXT_DIM,
            16.0,
        )
        .clicked
    {
        st.cmd(Command::MailSetup);
    }

    if !st.mail.account_filter_open && !ui.input.down {
        for (id, label) in [
            #[cfg(target_os = "macos")]
            ("mail.open-native", "打开 macOS 邮件"),
            ("mail.refresh", "刷新邮件"),
            ("mail.setup", "邮件账户设置"),
        ] {
            if let Some(hit) = ui.hits.iter().find(|hit| hit.id == wid(id)) {
                if ui.hovered(hit.rect) {
                    tip = Some(super::Tip {
                        anchor: hit.rect,
                        text: label,
                        alpha: 1.0,
                    });
                    break;
                }
            }
        }
    }
    draw_messages(ui, st, area, &selected);
    if let Some(tip) = tip {
        super::draw_tip(ui, Rect::new(area.x, area.y, area.w, 30.0), &tip);
    }
    if st.mail.account_filter_open {
        // 下拉层独占页面内的点击，防止选账户时误开底下的邮件或工具按钮。
        ui.hits.truncate(page_hits);
        draw_account_filter(ui, st, area, selector, badge_width);
    }
}

fn draw_unread_badge(ui: &mut Ui, unread: UnreadSummary, badge: Rect) {
    // 部分合计保留≥，未知数量不当作0或高优先级。
    let color = match unread.count() {
        None | Some(0) => palette::TEXT_DIM,
        Some(1..=4) => palette::BLUE,
        Some(5..=9) => palette::ORANGE,
        Some(_) => palette::RED,
    };
    let muted = unread.count().is_none_or(|count| count == 0);
    let fill = if muted {
        Color::white(0.045)
    } else {
        color.with_alpha(0.12)
    };
    let border = if muted {
        Color::white(0.07)
    } else {
        color.with_alpha(0.24)
    };
    ui.c.fill_rrect(badge, 8.0, fill);
    ui.c.stroke_rrect(badge, 8.0, border, 0.7);
    ui.c.icon(
        Icon::Mail,
        Rect::new(badge.x + 6.0, badge.y + 2.5, 11.0, 11.0),
        color,
    );
    ui.c.text_raw(
        &unread.label(),
        Rect::new(badge.x + 22.0, badge.y, (badge.w - 28.0).max(0.0), badge.h),
        &TextStyle::new(11.0).right().color(color),
    );
}

fn draw_account_filter(
    ui: &mut Ui,
    st: &mut AppState,
    area: Rect,
    selector: Rect,
    badge_width: f32,
) {
    let accounts: Vec<_> = st
        .settings
        .mail
        .accounts
        .iter()
        .filter(|account| account.enabled)
        .collect();
    let row_h = 43.0;
    let total_h = (accounts.len() + 1) as f32 * row_h;
    let menu = Rect::new(
        selector.x,
        selector.bottom() + 5.0,
        selector.w,
        (total_h + 12.0).min(area.bottom() - selector.bottom() - 5.0),
    );
    let outside = ui.button(wid("mail.account-filter.dismiss"), area);
    ui.button(wid("mail.account-filter"), selector);
    if outside.clicked || ui.input.keys.contains(&KeyInput::Escape) {
        st.mail.account_filter_open = false;
        ui.request_repaint();
        return;
    }
    ui.c.fill_rrect(menu, 12.0, Color::rgba(0.10, 0.10, 0.10, 1.0));
    ui.c.stroke_rrect(menu, 12.0, Color::white(0.14), 1.0);
    // 菜单留白也属于菜单，点击它不应关闭或穿透。
    ui.button(wid("mail.account-filter.panel"), menu);
    let view = menu.inset(6.0, 6.0);
    let max_scroll = (total_h - view.h).max(0.0);
    let wheel = ui.wheel_in(menu);
    if wheel != 0.0 {
        st.mail.account_filter_scroll =
            (st.mail.account_filter_scroll - wheel * row_h).clamp(0.0, max_scroll);
        ui.request_repaint();
    }
    st.mail.account_filter_scroll = st.mail.account_filter_scroll.clamp(0.0, max_scroll);
    ui.c.clip(view);
    let mut selection = None;
    for index in 0..=accounts.len() {
        let account = index.checked_sub(1).map(|index| accounts[index]);
        let id = account.map(|account| account.id.clone());
        let current = st.mail.selected_account == id;
        let row = Rect::new(
            view.x,
            view.y + index as f32 * row_h - st.mail.account_filter_scroll,
            view.w,
            row_h,
        );
        if row.bottom() <= view.y || row.y >= view.bottom() {
            continue;
        }
        let hit = Rect::new(
            row.x,
            row.y.max(view.y),
            row.w,
            row.bottom().min(view.bottom()) - row.y.max(view.y),
        );
        let key = account.map_or("all", |account| account.id.as_str());
        let response = ui.button(wid(&format!("mail.account-option.{key}")), hit);
        if current || response.hovered {
            ui.c.fill_rrect(
                row,
                8.0,
                if response.hovered {
                    palette::SURFACE_HOVER
                } else {
                    palette::SURFACE
                },
            );
        }
        let (name, detail, color) = if let Some(account) = account {
            let color = st
                .mail
                .statuses
                .get(&account.id)
                .unwrap_or(&MailConnection::Off)
                .color();
            (account.name.as_str(), account.address.clone(), color)
        } else {
            (
                "全部邮箱",
                format!("{} 个已启用邮箱", accounts.len()),
                palette::TEXT,
            )
        };
        if current {
            ui.c.icon(
                Icon::Check,
                Rect::new(row.x + 4.0, row.center().y - 7.0, 14.0, 14.0),
                color,
            );
        }
        let badge = Rect::new(
            row.right() - 9.0 - badge_width,
            row.center().y - 8.0,
            badge_width,
            16.0,
        );
        let text_width = (badge.x - row.x - 29.0).max(0.0);
        ui.c.text_raw(
            name,
            Rect::new(row.x + 21.0, row.y + 3.0, text_width, 20.0),
            &TextStyle::body().weight(Weight::Semibold).color(color),
        );
        ui.c.text_raw(
            &detail,
            Rect::new(row.x + 21.0, row.y + 24.0, text_width, 16.0),
            &TextStyle::new(10.0).color(palette::TEXT_DIM),
        );
        draw_unread_badge(
            ui,
            st.mail.unread_summary(
                &st.settings.mail.accounts,
                account.map(|account| account.id.as_str()),
            ),
            badge,
        );
        if response.clicked {
            selection = Some(id);
        }
    }
    ui.c.unclip();
    if max_scroll > 0.0 {
        let height = (view.h * view.h / total_h).max(18.0);
        let y = view.y + (view.h - height) * st.mail.account_filter_scroll / max_scroll;
        ui.c.fill_rrect(
            Rect::new(menu.right() - 4.0, y, 2.0, height),
            1.0,
            Color::white(0.30),
        );
    }
    if let Some(id) = selection {
        st.mail.selected_account = id;
        st.mail.scroll = 0.0;
        st.mail.account_filter_open = false;
        ui.request_repaint();
    }
}

fn draw_messages(ui: &mut Ui, st: &mut AppState, area: Rect, selected: &Option<String>) {
    let list = Rect::new(area.x, area.y + 65.0, area.w, area.h - 65.0);
    panel(ui, list);
    let accounts: Vec<MailAccount> = st
        .settings
        .mail
        .accounts
        .iter()
        .filter(|a| a.enabled && selected.as_ref().is_none_or(|id| &a.id == id))
        .cloned()
        .collect();
    let mut rows: Vec<(MailAccount, MailMessage)> = accounts
        .iter()
        .flat_map(|account| {
            st.mail
                .messages
                .get(&account.id)
                .into_iter()
                .flatten()
                .map(|message| (account.clone(), message.clone()))
        })
        .collect();
    rows.sort_by(|(a, x), (b, y)| {
        y.received_unix
            .cmp(&x.received_unix)
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| x.id.cmp(&y.id))
    });

    let connected = accounts
        .iter()
        .filter(|a| {
            matches!(
                st.mail.statuses.get(&a.id),
                Some(MailConnection::Ready { .. } | MailConnection::Syncing)
            )
        })
        .count();
    let status = if let Some(account) = accounts.first().filter(|_| selected.is_some()) {
        st.mail
            .statuses
            .get(&account.id)
            .unwrap_or(&MailConnection::Off)
            .label()
    } else {
        format!(
            "{} 个邮箱 · {connected} 个已连接 · 最近 {} 封邮件",
            accounts.len(),
            rows.len()
        )
    };
    let status_color = selected.as_ref().map_or(palette::TEXT_DIM, |id| {
        st.mail
            .statuses
            .get(id)
            .unwrap_or(&MailConnection::Off)
            .color()
    });
    ui.c.text_raw(
        &status,
        Rect::new(area.x + 2.0, area.y + 37.0, area.w - 4.0, 19.0),
        &TextStyle::footnote().color(status_color),
    );
    if accounts.is_empty() {
        let detail = st
            .mail
            .account_error
            .as_deref()
            .unwrap_or("在高级设置的邮件模块添加或启用账户\n支持 QQ、Gmail、iCloud 和公司邮箱");
        info(ui, list, "把邮件带到岛上", detail);
        return;
    }
    if rows.is_empty() {
        let state = accounts
            .iter()
            .filter_map(|a| st.mail.statuses.get(&a.id))
            .find(|s| {
                #[cfg(target_os = "macos")]
                if matches!(s, MailConnection::AuthorizationRequired(_)) {
                    return true;
                }
                matches!(s, MailConnection::Error(_))
            })
            .cloned();
        #[cfg(target_os = "macos")]
        let needs_authorization = matches!(state, Some(MailConnection::AuthorizationRequired(_)));
        #[cfg(not(target_os = "macos"))]
        let needs_authorization = false;
        let detail = if needs_authorization {
            "请到高级设置 → 邮件，展开账户并点击“授权”\n后台收信和普通刷新不会弹出授权框".into()
        } else if let Some(state) = state {
            state.label()
        } else {
            "连接成功后显示最近邮件；首次连接不会弹出历史提醒".into()
        };
        info(
            ui,
            list,
            if needs_authorization {
                "邮箱等待授权"
            } else {
                "暂无邮件"
            },
            &detail,
        );
        return;
    }
    let view = list.inset(7.0, 7.0);
    let total_h = rows.len() as f32 * ROW_H;
    let max_scroll = (total_h - view.h).max(0.0);
    let wheel = if st.mail.account_filter_open {
        0.0
    } else {
        ui.wheel_in(list)
    };
    if wheel != 0.0 {
        st.mail.scroll = (st.mail.scroll - wheel * 46.0).clamp(0.0, max_scroll);
        ui.request_repaint();
    }
    st.mail.scroll = st.mail.scroll.clamp(0.0, max_scroll);
    ui.c.clip(view);
    let mut open = None;
    for (i, (account, message)) in rows.iter().enumerate() {
        let row = Rect::new(
            view.x,
            view.y + i as f32 * ROW_H - st.mail.scroll,
            view.w,
            ROW_H,
        );
        if row.bottom() < view.y || row.y > view.bottom() {
            continue;
        }
        let hit = Rect::new(
            row.x,
            row.y.max(view.y),
            row.w,
            row.bottom().min(view.bottom()) - row.y.max(view.y),
        );
        let response = ui.button(
            wid(&format!("mail.message.{}.{}", account.id, message.id)),
            hit,
        );
        let fill = if response.pressed {
            palette::SURFACE_PRESSED
        } else if response.hovered {
            palette::SURFACE_HOVER
        } else if message.unread {
            palette::SURFACE
        } else {
            Color::white(0.0)
        };
        ui.c.fill_rrect(row, 6.0, fill);
        if i + 1 < rows.len() {
            ui.c.line(
                Point::new(row.x + 21.0, row.bottom()),
                Point::new(row.right() - 12.0, row.bottom()),
                palette::BORDER,
                1.0,
            );
        }
        if message.unread {
            ui.c.fill_circle(Point::new(row.x + 10.0, row.y + 18.0), 2.8, ACCENT);
        }
        let x = row.x + 21.0;
        let sender = if message.sender.is_empty() {
            "未知发件人"
        } else {
            &message.sender
        };
        ui.c.text_raw(
            sender,
            Rect::new(x, row.y + 6.0, row.w - 166.0, 20.0),
            &TextStyle::body().weight(if message.unread {
                Weight::Semibold
            } else {
                Weight::Regular
            }),
        );
        ui.c.text_raw(
            &time_label(message.received_unix, st, false),
            Rect::new(row.right() - 126.0, row.y + 6.0, 115.0, 20.0),
            &TextStyle::footnote().right().color(palette::TEXT_DIM),
        );
        let subject = if message.subject.is_empty() {
            "（无主题）"
        } else {
            &message.subject
        };
        ui.c.text_raw(
            subject,
            Rect::new(x, row.y + 27.0, row.w - 42.0, 20.0),
            &TextStyle::body(),
        );
        let summary = if message.snippet.is_empty() {
            account.name.clone()
        } else {
            format!("{} · {}", account.name, message.snippet)
        };
        ui.c.text_raw(
            &summary,
            Rect::new(x, row.y + 49.0, row.w - 42.0, 17.0),
            &TextStyle::footnote().color(palette::TEXT_DIM),
        );
        if response.clicked {
            open = Some((account.id.clone(), message.id.clone()));
        }
    }
    ui.c.unclip();
    if max_scroll > 0.0 {
        let h = (view.h * view.h / total_h).max(18.0);
        let y = view.y + (view.h - h) * st.mail.scroll / max_scroll;
        ui.c.fill_rrect(
            Rect::new(list.right() - 4.0, y, 2.0, h),
            1.0,
            Color::white(0.22),
        );
    }
    if let Some((account_id, message_id)) = open {
        st.mail.selected_message = Some((account_id.clone(), message_id.clone()));
        st.mail.preview = None;
        st.mail.preview_loading = true;
        st.mail.preview_scroll = 0.0;
        st.mail.unseen.remove(&account_id);
        st.cmd(Command::Mail(MailAction::Preview {
            account_id,
            message_id,
        }));
        ui.request_repaint();
    }
}

fn draw_preview(ui: &mut Ui, st: &mut AppState, area: Rect) {
    let Some((account_id, _)) = st.mail.selected_message.clone() else {
        return;
    };
    if ui
        .pill(
            wid("mail.back"),
            Rect::new(area.x, area.y, 74.0, 28.0),
            "返回",
            Some(Icon::Prev),
            PillStyle::Utility,
        )
        .clicked
    {
        st.cmd(Command::Mail(MailAction::ClosePreview));
        ui.request_repaint();
        return;
    }
    let account = st
        .settings
        .mail
        .accounts
        .iter()
        .find(|a| a.id == account_id)
        .cloned();
    let label = account.as_ref().map_or("邮箱已移除", |a| a.name.as_str());
    ui.c.text_raw(
        label,
        Rect::new(
            area.x + 87.0,
            area.y,
            (area.w - if cfg!(windows) { 210.0 } else { 330.0 }).max(0.0),
            28.0,
        ),
        &TextStyle::body()
            .weight(Weight::Semibold)
            .color(palette::TEXT_DIM),
    );
    #[cfg(target_os = "macos")]
    if ui
        .pill(
            wid("mail.open-native"),
            Rect::new(area.right() - 232.0, area.y, 110.0, 28.0),
            "邮件 App",
            Some(Icon::Mail),
            PillStyle::Utility,
        )
        .clicked
    {
        st.cmd(Command::OpenAppleMail);
    }
    if account
        .as_ref()
        .is_some_and(|a| !a.provider.web_url().is_empty())
        && ui
            .pill(
                wid("mail.open-web"),
                Rect::new(area.right() - 112.0, area.y, 112.0, 28.0),
                "网页邮箱",
                Some(Icon::Open),
                PillStyle::Utility,
            )
            .clicked
    {
        st.cmd(Command::Mail(MailAction::OpenMailbox(account_id)));
    }
    let body = Rect::new(area.x, area.y + 42.0, area.w, area.h - 42.0);
    panel(ui, body);
    if st.mail.preview_loading {
        info(
            ui,
            body,
            "正在获取邮件内容…",
            "只读取所选邮件，预览不会自动标为已读",
        );
        return;
    }
    let preview = match st.mail.preview.clone() {
        Some(Ok(preview)) => preview,
        Some(Err(error)) => {
            info(ui, body, "暂时无法预览", &error);
            return;
        }
        None => {
            info(
                ui,
                body,
                "请选择一封邮件",
                "返回列表后重新选择邮件即可加载正文",
            );
            return;
        }
    };
    #[cfg(target_os = "macos")]
    let (error_h, translation) = {
        let translation_label = if st.mail.translation_loading {
            "正在本机翻译…"
        } else if st.mail.show_translation {
            "简体中文译文"
        } else {
            "邮件原文"
        };
        ui.c.text(
            translation_label,
            Rect::new(body.x + 16.0, body.y + 10.0, body.w - 152.0, 28.0),
            &TextStyle::footnote().color(palette::TEXT_DIM),
        );
        let button_label = if st.mail.translation_loading {
            "取消翻译"
        } else if st.mail.show_translation {
            "查看原文"
        } else if matches!(st.mail.translation, Some(Ok(_))) {
            "查看译文"
        } else {
            "翻译为中文"
        };
        if ui
            .pill(
                wid("mail.translate"),
                Rect::new(body.right() - 130.0, body.y + 10.0, 114.0, 28.0),
                button_label,
                None,
                PillStyle::UtilityAccent(ACCENT),
            )
            .clicked
        {
            if st.mail.translation_loading {
                st.cmd(Command::Mail(MailAction::CancelTranslation));
            } else if matches!(st.mail.translation, Some(Ok(_))) {
                st.mail.show_translation = !st.mail.show_translation;
                st.mail.preview_scroll = 0.0;
            } else {
                st.cmd(Command::Mail(MailAction::Translate));
            }
            ui.request_repaint();
        }
        let error_style = TextStyle::footnote().wrap().top().color(palette::TEXT_DIM);
        let error = st
            .mail
            .translation
            .as_ref()
            .and_then(|result| result.as_ref().err());
        let error_h = error.map_or(0.0, |error| {
            ui.c.measure_raw(error, &error_style, body.w - 32.0).1 + 10.0
        });
        if let Some(error) = error {
            ui.c.text_raw(
                error,
                Rect::new(body.x + 16.0, body.y + 47.0, body.w - 32.0, error_h),
                &error_style,
            );
        }
        let translation = st
            .mail
            .translation
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .filter(|_| st.mail.show_translation);
        (error_h, translation)
    };
    #[cfg(windows)]
    let error_h = 0.0;
    let view = Rect::new(
        body.x + 16.0,
        body.y + 52.0 + error_h,
        body.w - 32.0,
        body.h - 65.0 - error_h,
    );
    #[cfg(target_os = "macos")]
    let subject = translation.map_or(preview.message.subject.as_str(), |translation| {
        translation.subject.as_str()
    });
    #[cfg(windows)]
    let subject = preview.message.subject.as_str();
    #[cfg(windows)]
    ui.c.text(
        "邮件原文",
        Rect::new(body.x + 16.0, body.y + 10.0, body.w - 32.0, 28.0),
        &TextStyle::footnote().color(palette::TEXT_DIM),
    );
    let subject = if subject.is_empty() {
        "（无主题）"
    } else {
        subject
    };
    let title_style = TextStyle::new(16.0).weight(Weight::Semibold).wrap().top();
    let title_h = ui.c.measure_raw(subject, &title_style, view.w).1.max(22.0);
    let meta_style = TextStyle::footnote()
        .color(palette::TEXT_DIM)
        .wrap()
        .top()
        .line_spacing(1.35);
    let meta = format!(
        "发件人：{}\n收件人：{}\n{}",
        preview.message.sender,
        preview.to,
        time_label(preview.message.received_unix, st, true)
    );
    let meta_h = ui.c.measure_raw(&meta, &meta_style, view.w).1;
    #[cfg(target_os = "macos")]
    let text = translation.map_or(preview.body.as_str(), |translation| {
        translation.body.as_str()
    });
    #[cfg(windows)]
    let text = preview.body.as_str();
    let text = if text.trim().is_empty() {
        "（没有可显示的正文）"
    } else {
        text
    };
    let text_style = TextStyle::body().wrap().top().line_spacing(1.55);
    let text_h = ui.c.measure_raw(text, &text_style, view.w).1.max(22.0);
    let mut footer = String::new();
    if !preview.attachments.is_empty() {
        footer.push_str("附件");
        for attachment in &preview.attachments {
            footer.push('\n');
            footer.push_str(&attachment.name);
            if let Some(size) = attachment.size {
                footer.push_str(&format!(" · {} KB", size.div_ceil(1024)));
            }
        }
    }
    if preview.truncated {
        if !footer.is_empty() {
            footer.push_str("\n\n");
        }
        footer.push_str("正文较长，已显示部分内容。完整邮件请在邮箱中查看。");
    }
    let footer_h = if footer.is_empty() {
        0.0
    } else {
        ui.c.measure_raw(&footer, &meta_style, view.w).1 + 20.0
    };
    let total = title_h + 12.0 + meta_h + 24.0 + text_h + footer_h + 8.0;
    let max_scroll = (total - view.h).max(0.0);
    let wheel = ui.wheel_in(body);
    if wheel != 0.0 {
        st.mail.preview_scroll = (st.mail.preview_scroll - wheel * 46.0).clamp(0.0, max_scroll);
        ui.request_repaint();
    }
    st.mail.preview_scroll = st.mail.preview_scroll.clamp(0.0, max_scroll);
    ui.c.clip(view);
    let y = view.y - st.mail.preview_scroll;
    ui.c.text_raw(subject, Rect::new(view.x, y, view.w, title_h), &title_style);
    let meta_y = y + title_h + 12.0;
    ui.c.text_raw(
        &meta,
        Rect::new(view.x, meta_y, view.w, meta_h),
        &meta_style,
    );
    let text_y = meta_y + meta_h + 24.0;
    ui.c.fill_rect(
        Rect::new(view.x, text_y - 12.0, view.w, 1.0),
        Color::white(0.10),
    );
    ui.c.text_raw(text, Rect::new(view.x, text_y, view.w, text_h), &text_style);
    if !footer.is_empty() {
        ui.c.text_raw(
            &footer,
            Rect::new(view.x, text_y + text_h + 20.0, view.w, footer_h),
            &meta_style,
        );
    }
    ui.c.unclip();
    if max_scroll > 0.0 {
        let h = (view.h * view.h / total).max(18.0);
        let y = view.y + (view.h - h) * st.mail.preview_scroll / max_scroll;
        ui.c.fill_rrect(
            Rect::new(body.right() - 5.0, y, 2.0, h),
            1.0,
            Color::white(0.22),
        );
    }
}

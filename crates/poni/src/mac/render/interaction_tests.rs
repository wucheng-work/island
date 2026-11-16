//! 以实际绘图、命中区域与命令队列检查关键页面交互；不启动平台服务。

use super::{base_state, media, sample_mail};
use crate::commands::{Command, MediaCmd};
use crate::gfx::{Bitmap, Gfx};
use crate::mail::MailAction;
use crate::pages::{self, PageKind};
use crate::state::AppState;
use crate::ui::{wid, wid_n, Hit, Input, KeyInput, MouseButton, Ui, WidgetId};
use poni_core::datetime::days_from_civil;
use poni_core::geom::Rect;
use poni_core::settings::Theme;

/// 使用真实 Canvas；每帧收集页面生成的命中区域，再向该区域发送输入。
struct PageHarness {
    gfx: Gfx,
    bitmap: Bitmap,
    input: Input,
}

impl PageHarness {
    fn new() -> Self {
        Self {
            gfx: Gfx::new(1.0),
            bitmap: Bitmap::new(1000, 800, 1.0).expect("test bitmap"),
            input: Input::default(),
        }
    }

    fn draw(&mut self, st: &mut AppState, page: PageKind, width: f32) -> Vec<Hit> {
        st.page = page;
        st.settings.expanded_width = width;
        self.gfx.set_look(poni_core::look::of(st.settings.theme));
        let body = Rect::new(
            0.0,
            0.0,
            pages::expanded_width(page, st),
            pages::HEADER_H + pages::content_height(page, st) + pages::PAD,
        );
        let mut hits = Vec::new();
        {
            let mut canvas = self.gfx.canvas(self.bitmap.ctx());
            let mut ui = Ui::new(&mut canvas, &mut self.input, &mut hits, st.now);
            pages::draw_expanded(&mut ui, st, body, page, None);
        }
        self.input.end_frame();
        hits
    }

    fn click(&mut self, hits: &[Hit], id: WidgetId) {
        let hit = hit(hits, id);
        self.input.mouse = Some(hit.rect.center());
        self.input.clicked = Some((hit.id, MouseButton::Left));
    }
}

fn hit(hits: &[Hit], id: WidgetId) -> &Hit {
    hits.iter()
        .find(|hit| hit.id == id)
        .unwrap_or_else(|| panic!("missing hit {id}"))
}

fn within(hit: &Hit, width: f32, height: f32) {
    let rect = hit.rect;
    assert!(rect.w > 0.0 && rect.h > 0.0, "invalid hit {rect:?}");
    assert!(
        rect.x >= 0.0
            && rect.y >= 0.0
            && rect.right() <= width + 0.5
            && rect.bottom() <= height + 0.5,
        "hit outside page: {rect:?} in {width}×{height}"
    );
}

#[test]
fn header_navigation_and_all_settings_entry_remain_reachable_at_supported_widths() {
    let mut harness = PageHarness::new();
    for theme in [Theme::Classic, Theme::Pill] {
        for width in [480.0, 620.0, 900.0] {
            let mut st = base_state(theme);
            media(&mut st);
            let index = pages::tabs(&st)
                .iter()
                .position(|page| *page == PageKind::Today)
                .unwrap();
            let hits = harness.draw(&mut st, PageKind::Media, width);
            let tab = wid_n("tab", index as u64);
            within(hit(&hits, tab), width, 100.0);
            harness.click(&hits, tab);
            harness.draw(&mut st, PageKind::Media, width);
            assert!(
                st.cmds.contains(&Command::SetPage(PageKind::Today)),
                "{theme:?} at {width}"
            );

            st.cmds.clear();
            let hits = harness.draw(&mut st, PageKind::Settings, width);
            let advanced = wid(pages::settings::ADVANCED_ID);
            within(
                hit(&hits, advanced),
                width,
                pages::HEADER_H + pages::settings::height(&st) + pages::PAD,
            );
            harness.click(&hits, advanced);
            harness.draw(&mut st, PageKind::Settings, width);
            assert!(
                st.cmds.contains(&Command::OpenSettings),
                "{theme:?} at {width}"
            );
        }
    }
}

#[test]
fn every_advanced_settings_category_responds_to_real_hit_testing() {
    use crate::settings_model::Section;
    use crate::settings_window::{Frame, View};
    let mut st = base_state(Theme::Pill);
    sample_mail(&mut st);
    let mut view = View::new(Section::General);
    let mut gfx = Gfx::new(1.0);
    let bitmap = Bitmap::new(840, 580, 1.0).expect("test settings bitmap");
    let frame = Frame {
        card: Rect::new(0.0, 0.0, 840.0, 580.0),
        own_chrome: false,
        top: 28.0,
        still: true,
    };
    let mut draw = |view: &mut View, st: &mut AppState| {
        let now = st.now;
        let mut canvas = gfx.canvas(bitmap.ctx());
        view.draw(&mut canvas, st, now, frame, false)
    };
    draw(&mut view, &mut st);
    for (index, section) in Section::ALL.into_iter().enumerate() {
        let rect = view
            .hit_rect(wid_n("settingswin.nav", index as u64))
            .expect("settings navigation hit");
        view.press(rect.center());
        view.release(rect.center());
        draw(&mut view, &mut st);
        assert_eq!(view.section, section);
        draw(&mut view, &mut st);
    }
}

#[test]
fn selected_date_and_chinese_city_input_use_the_actual_page_controls() {
    let mut harness = PageHarness::new();
    for width in [480.0, 620.0, 900.0] {
        let mut st = base_state(Theme::Pill);
        let tomorrow = days_from_civil(st.local.year, st.local.month, st.local.day) + 1;
        let hits = harness.draw(&mut st, PageKind::Today, width);
        harness.click(&hits, wid_n("today.date", tomorrow as u64));
        let hits = harness.draw(&mut st, PageKind::Today, width);
        assert_eq!(st.ui.today_pick, Some(tomorrow));

        harness.click(&hits, wid("weather.city"));
        harness.draw(&mut st, PageKind::Today, width);
        assert!(st.ui.weather_city_editing);
        let hits = harness.draw(&mut st, PageKind::Today, width);
        harness.click(&hits, wid("weather.city.input"));
        harness.draw(&mut st, PageKind::Today, width);
        assert_eq!(harness.input.focused, Some(wid("weather.city.input")));
        st.ui.weather_city_input.clear();
        harness.input.caret = 0;
        harness.input.keys = vec![KeyInput::Char('长'), KeyInput::Char('沙')];
        harness.draw(&mut st, PageKind::Today, width);
        assert_eq!(st.ui.weather_city_input, "长沙");
        st.now += 0.4;
        harness.draw(&mut st, PageKind::Today, width);
        assert!(st
            .cmds
            .iter()
            .any(|command| matches!(command, Command::SearchWeatherCities { query, .. } if query == "长沙")));
        assert!(!st
            .cmds
            .iter()
            .any(|command| matches!(command, Command::LocateWeather)));
    }
}

#[test]
fn music_mute_and_seek_emit_commands_without_faking_a_confirmed_position() {
    let mut harness = PageHarness::new();
    for theme in [Theme::Classic, Theme::Pill] {
        for width in [480.0, 620.0, 900.0] {
            let mut st = base_state(theme);
            media(&mut st);
            let hits = harness.draw(&mut st, PageKind::Media, width);
            harness.click(&hits, wid("media_mute"));
            let hits = harness.draw(&mut st, PageKind::Media, width);
            assert!(st.cmds.contains(&Command::SetOutputMute(true)));
            st.cmds.clear();

            let seek = hit(&hits, wid("media_seek"));
            harness.input.released = Some((seek.id, seek.rect.center()));
            let confirmed = st.media.as_ref().unwrap().timeline.position_s;
            harness.draw(&mut st, PageKind::Media, width);
            assert!(st
                .cmds
                .iter()
                .any(|command| matches!(command, Command::Media(MediaCmd::Seek(seconds)) if (*seconds - 100.5).abs() < 0.01)));
            assert_eq!(st.media.as_ref().unwrap().timeline.position_s, confirmed);
        }
    }
}

#[test]
fn mail_account_menu_owns_input_and_keeps_preview_and_back_actions_working() {
    let mut harness = PageHarness::new();
    for width in [480.0, 620.0, 900.0] {
        let mut st = base_state(Theme::Pill);
        sample_mail(&mut st);
        st.settings.mail.enabled = true;
        let hits = harness.draw(&mut st, PageKind::Mail, width);
        harness.input.mouse = Some(
            hit(&hits, wid("mail.message.demo-mail-qq.qq-101"))
                .rect
                .center(),
        );
        harness.input.wheel = -1.0;
        let hits = harness.draw(&mut st, PageKind::Mail, width);
        assert!(st.mail.scroll > 0.0);
        harness.click(&hits, wid("mail.account-filter"));
        let hits = harness.draw(&mut st, PageKind::Mail, width);
        assert!(st.mail.account_filter_open);
        assert!(
            !hits
                .iter()
                .any(|hit| hit.id == wid("mail.message.demo-mail-qq.qq-101")),
            "dropdown must not expose the message beneath it"
        );
        harness.click(&hits, wid("mail.account-option.demo-mail-aruba"));
        harness.draw(&mut st, PageKind::Mail, width);
        assert_eq!(st.mail.selected_account.as_deref(), Some("demo-mail-aruba"));
        assert!(!st.mail.account_filter_open);
        assert_eq!(st.mail.scroll, 0.0);

        let hits = harness.draw(&mut st, PageKind::Mail, width);
        harness.click(&hits, wid("mail.message.demo-mail-aruba.aruba-303"));
        harness.draw(&mut st, PageKind::Mail, width);
        assert_eq!(
            st.mail.selected_message,
            Some(("demo-mail-aruba".into(), "aruba-303".into()))
        );
        assert!(st.mail.preview_loading);
        assert!(st
            .cmds
            .iter()
            .any(|command| matches!(command, Command::Mail(MailAction::Preview { account_id, message_id }) if account_id == "demo-mail-aruba" && message_id == "aruba-303")));

        // 页面刚选择邮件的这一帧仍是列表；下一帧才进入预览。
        let hits = harness.draw(&mut st, PageKind::Mail, width);
        harness.click(&hits, wid("mail.back"));
        harness.draw(&mut st, PageKind::Mail, width);
        assert!(st.cmds.contains(&Command::Mail(MailAction::ClosePreview)));
    }
}

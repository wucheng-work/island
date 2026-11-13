//! 专注阶段的应用生命周期：统计、完成提醒与声音，不依赖页面绘制。

use crate::icons::Icon;
use crate::pages::PageKind;
use crate::state::{AppState, InlineAction, NoticeAction};
use poni_core::geom::{palette, Color};
use poni_core::pomodoro::{Phase, PhaseEnded};

/// 专注统计保留一年的历史，沿用已有日期裁剪规则。
const KEEP_STATS_DAYS: i64 = 366;

pub fn phase_color(phase: Phase) -> Color {
    match phase {
        Phase::Focus => palette::BLUE,
        Phase::ShortBreak => palette::GREEN,
        Phase::LongBreak => palette::PURPLE,
    }
}

/// 仅处理自然结束的阶段；主动跳过不增加次数，也不发完成提醒。
pub fn on_phase_ended(st: &mut AppState, ended: PhaseEnded) {
    let key = st.day_key();
    if ended.ended == Phase::Focus {
        let count = st.store.focus_sessions.entry(key.clone()).or_insert(0);
        *count = count.saturating_add(1);
        let cutoff = st.local.add_days(-KEEP_STATS_DAYS).day_key();
        st.store.focus_sessions.retain(|day, _| day.as_str() >= cutoff.as_str());
        st.save();
    }
    let today = st.store.focus_sessions.get(&key).copied().unwrap_or(0);
    let minutes = (st.pomodoro.phase_duration(ended.next) / 60.0).round() as u32;
    let (icon, color, title, subtitle) = match ended.next {
        Phase::Focus => (
            Icon::Timer,
            phase_color(Phase::Focus),
            "休息结束".to_string(),
            if ended.auto_started { format!("已开始 {minutes} 分钟专注") } else { "点击开始下一次专注".to_string() },
        ),
        Phase::ShortBreak | Phase::LongBreak => {
            let title = if ended.next == Phase::LongBreak {
                format!("本轮完成 · 长休息 {minutes} 分钟")
            } else {
                format!("专注完成 · 休息 {minutes} 分钟")
            };
            let action = if ended.auto_started { "休息已开始" } else { "点击开始休息" };
            (Icon::Coffee, phase_color(ended.next), title, format!("今天已专注 {today} 次 · {action}"))
        }
    };
    // 自动开始时打开页面；等待开始时保留无需展开岛的一键操作。
    let action = if ended.auto_started { NoticeAction::OpenPage(PageKind::Focus) } else { NoticeAction::StartFocus };
    st.notify(icon, color, title, subtitle, 6.0, Some(action));
    if !ended.auto_started {
        st.set_notice_inline(InlineAction::StartFocus);
    }
    st.chime();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{Command, Sound};
    use poni_core::datetime::LocalDateTime;
    use poni_core::settings::Settings;
    use poni_core::store::Store;

    #[test]
    fn completed_focus_saves_statistics_and_keeps_manual_next_action() {
        let mut settings = Settings::default();
        settings.focus_sound = true;
        settings.pomodoro.auto_start_breaks = false;
        let mut st = AppState::new(settings, Store::default());
        st.local = LocalDateTime { year: 2026, month: 11, day: 12, ..LocalDateTime::default() };
        st.store.focus_sessions.insert("2020-01-01".into(), 9);
        st.pomodoro.start(0.0);
        let ended = st.pomodoro.tick(st.pomodoro.current_duration()).unwrap();

        on_phase_ended(&mut st, ended);

        assert_eq!(st.store.focus_sessions.get(&st.day_key()), Some(&1));
        assert!(!st.store.focus_sessions.contains_key("2020-01-01"));
        assert!(st.cmds.contains(&Command::SaveStore));
        assert!(st.cmds.contains(&Command::Play(Sound::Chime)));
        assert!(matches!(st.notice.as_ref().and_then(|notice| notice.action.as_ref()), Some(NoticeAction::StartFocus)));
        assert!(matches!(st.notice.as_ref().and_then(|notice| notice.inline.as_ref()), Some(InlineAction::StartFocus)));
    }

    #[test]
    fn completed_break_opens_auto_started_focus_without_incrementing_statistics() {
        let mut settings = Settings::default();
        settings.focus_sound = false;
        settings.pomodoro.auto_start_focus = true;
        let mut st = AppState::new(settings, Store::default());
        st.pomodoro.set_phase(Phase::ShortBreak);
        st.pomodoro.start(0.0);
        let ended = st.pomodoro.tick(st.pomodoro.current_duration()).unwrap();

        on_phase_ended(&mut st, ended);

        assert!(st.store.focus_sessions.is_empty());
        assert!(!st.cmds.contains(&Command::SaveStore));
        assert!(!st.cmds.contains(&Command::Play(Sound::Chime)));
        assert!(matches!(st.notice.as_ref().and_then(|notice| notice.action.as_ref()), Some(NoticeAction::OpenPage(PageKind::Focus))));
        assert!(st.notice.as_ref().unwrap().inline.is_none());
        assert!(st.pomodoro.is_running());
    }
}

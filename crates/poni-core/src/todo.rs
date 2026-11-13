//! A small personal task list.

use serde::{Deserialize, Serialize};

pub const MAX_TASK_LEN: usize = 200;
pub const MAX_TASKS: usize = 200;

/// 日历日期标记，与所选日期的待办列表使用相同的可见性规则。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DayTasks {
    pub has_tasks: bool,
    pub has_open: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub text: String,
    pub done: bool,
    pub created_unix: i64,
    #[serde(default)]
    pub done_unix: Option<i64>,
    /// 计划日期（公历自 1970-01-01 起的天数）；旧数据由应用按本地创建日补齐。
    #[serde(default)]
    pub scheduled_day: Option<i64>,
    #[serde(default)]
    pub completed_day: Option<i64>,
}

impl Task {
    fn visible_on(&self, day: i64, today: i64) -> bool {
        if day != today {
            return self.scheduled_day == Some(day);
        }
        if self.done {
            return self.completed_day == Some(today);
        }
        self.scheduled_day.is_some_and(|scheduled| scheduled <= today)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskList {
    pub tasks: Vec<Task>,
    pub next_id: u64,
}

impl Default for TaskList {
    fn default() -> Self {
        Self { tasks: Vec::new(), next_id: 1 }
    }
}

impl TaskList {
    /// Add a task; returns its id, or `None` for blank text / a full list.
    pub fn add(&mut self, text: &str, now_unix: i64, day: i64) -> Option<u64> {
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() || self.tasks.len() >= MAX_TASKS {
            return None;
        }
        let text: String = text.chars().take(MAX_TASK_LEN).collect();
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.tasks.push(Task { id, text, done: false, created_unix: now_unix, done_unix: None, scheduled_day: Some(day), completed_day: None });
        Some(id)
    }

    pub fn toggle(&mut self, id: u64, now_unix: i64, today: i64) -> bool {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.done = !t.done;
            t.done_unix = if t.done { Some(now_unix) } else { None };
            t.completed_day = if t.done { Some(today) } else { None };
            true
        } else {
            false
        }
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        self.tasks.len() != before
    }

    pub fn clear_done(&mut self, day: i64, today: i64) -> usize {
        let before = self.tasks.len();
        self.tasks.retain(|t| !t.done || !t.visible_on(day, today));
        before - self.tasks.len()
    }

    /// 所选日期的完成数和总数；今天包含此前未完成事项，其他日期按计划日显示。
    pub fn progress(&self, day: i64, today: i64) -> (usize, usize) {
        let mut done = 0;
        let mut total = 0;
        for task in self.tasks.iter().filter(|task| task.visible_on(day, today)) {
            total += 1;
            if task.done {
                done += 1;
            }
        }
        (done, total)
    }

    /// 单次遍历聚合连续日期的标记；今天包含结转，其他日期只显示计划日事项。
    pub fn day_marks(&self, first_day: i64, day_count: usize, today: i64) -> Vec<DayTasks> {
        let mut marks = vec![DayTasks::default(); day_count];
        let end = first_day + day_count as i64;
        for task in &self.tasks {
            // 一项待办最多出现在计划日和今天，避免逐日重新扫描整份列表。
            let scheduled = task.scheduled_day.filter(|day| *day != today);
            for day in scheduled.into_iter().chain(std::iter::once(today)) {
                if day < first_day || day >= end || !task.visible_on(day, today) {
                    continue;
                }
                let mark = &mut marks[(day - first_day) as usize];
                mark.has_tasks = true;
                mark.has_open |= !task.done;
            }
        }
        marks
    }

    /// 所选日未完成事项在前（按创建顺序），已完成事项在后。
    pub fn display_order(&self, day: i64, today: i64) -> Vec<&Task> {
        let mut open: Vec<&Task> = self.tasks.iter().filter(|t| !t.done && t.visible_on(day, today)).collect();
        let mut done: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| t.done && t.visible_on(day, today))
            .collect();
        open.sort_by_key(|t| (t.created_unix, t.id));
        done.sort_by_key(|t| std::cmp::Reverse(t.done_unix));
        open.extend(done);
        open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_toggle_remove() {
        let mut l = TaskList::default();
        assert_eq!(l.add("   ", 0, 50), None);
        let a = l.add("  buy   milk ", 10, 50).unwrap();
        let b = l.add("ship poni", 20, 50).unwrap();
        assert_eq!(l.tasks[0].text, "buy milk");
        assert!(l.toggle(a, 100, 50));
        assert_eq!(l.progress(50, 50), (1, 2));
        assert_eq!(l.progress(200, 200), (0, 1));
        let order: Vec<u64> = l.display_order(50, 50).iter().map(|t| t.id).collect();
        assert_eq!(order, vec![b, a]);
        assert_eq!(l.clear_done(50, 50), 1);
        assert!(l.remove(b));
        assert!(!l.remove(b));
    }

    #[test]
    fn truncates_long_text() {
        let mut l = TaskList::default();
        let id = l.add(&"x".repeat(500), 0, 0).unwrap();
        assert_eq!(l.tasks.iter().find(|t| t.id == id).unwrap().text.len(), MAX_TASK_LEN);
    }

    #[test]
    fn ids_survive_default_zero() {
        let mut l = TaskList { tasks: vec![], next_id: 0 };
        assert_eq!(l.add("a", 0, 0), Some(1));
        assert_eq!(l.add("b", 0, 0), Some(2));
    }

    #[test]
    fn calendar_marks_follow_the_same_dates_as_the_task_list() {
        let mut list = TaskList::default();
        let earlier = list.add("以前未完成", 1, 95).unwrap();
        let completed_before = list.add("以前已完成", 2, 98).unwrap();
        let completed_today = list.add("今天完成的旧事项", 3, 99).unwrap();
        list.add("今天待办", 4, 100).unwrap();
        list.add("未来待办", 5, 105).unwrap();
        list.toggle(completed_before, 20, 99);
        list.toggle(completed_today, 21, 100);

        let marks = list.day_marks(98, 10, 100);
        assert_eq!(marks[0], DayTasks { has_tasks: true, has_open: false });
        assert_eq!(marks[2], DayTasks { has_tasks: true, has_open: true });
        assert_eq!(marks[7], DayTasks { has_tasks: true, has_open: true });
        assert_eq!(marks[9], DayTasks::default());
        for (index, mark) in marks.iter().enumerate() {
            let (done, total) = list.progress(98 + index as i64, 100);
            assert_eq!(*mark, DayTasks { has_tasks: total > 0, has_open: done < total });
        }

        // 结转事项完成后，昨天不再结转到今天；空范围也不产生标记。
        list.toggle(earlier, 22, 99);
        list.clear_done(100, 100);
        assert_eq!(list.day_marks(100, 1, 100)[0], DayTasks { has_tasks: true, has_open: true });
        assert!(list.day_marks(100, 0, 100).is_empty());
        // 今日不在可见月份时，仍保留该月计划日的完成标记。
        assert_eq!(list.day_marks(95, 1, 200)[0], DayTasks { has_tasks: true, has_open: false });
    }
}

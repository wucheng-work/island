//! 天气和显式单次定位的Windows主线程编排。

use super::*;

impl App {
    pub(super) fn refresh_weather(&mut self, force: bool) {
        if !self.expanded || self.st.page != PageKind::Today || self.locked || self.display_off || self.st.power.saver || self.selftest.is_some() {
            return;
        }
        // 打开页面、定时刷新和重试均不触发定位权限。
        if self.st.settings.weather_city.is_empty() || self.st.weather.loading {
            return;
        }
        let interval = if self.st.weather.snapshot.is_some() { 30 * 60 } else { 10 * 60 };
        if !force && self.weather_attempt_unix > 0 && self.st.unix - self.weather_attempt_unix < interval {
            return;
        }
        self.weather_attempt_unix = self.st.unix;
        self.st.weather.request_id = self.st.weather.request_id.wrapping_add(1);
        self.st.weather.loading = true;
        self.st.weather.message.clear();
        let request_id = self.st.weather.request_id;
        if let Some((latitude, longitude)) = self.st.settings.weather_city_coordinates {
            services::weather::fetch_location(latitude, longitude, self.st.settings.weather_city.clone(), request_id, self.sink.clone());
        } else {
            services::weather::fetch_city(self.st.settings.weather_city.clone(), request_id, self.sink.clone());
        }
    }

    pub(super) fn set_weather_city(&mut self, city: String, coordinates: (f64, f64)) {
        // 新选择使旧定位、天气和城市结果失效，旧响应不能覆盖它。
        self.location = None;
        self.st.weather.request_id = self.st.weather.request_id.wrapping_add(1);
        self.st.weather.snapshot = None;
        self.st.weather.loading = false;
        self.st.weather.message.clear();
        self.weather_attempt_unix = 0;
        self.st.settings.weather_city = city;
        self.st.settings.weather_city_coordinates = Some(coordinates);
        self.save_settings();
        self.refresh_weather(true);
    }

    pub(super) fn locate_weather(&mut self) {
        if !self.expanded || self.st.page != PageKind::Today || self.locked || self.display_off || self.st.power.saver || self.selftest.is_some() || self.location.is_some() {
            return;
        }
        self.weather_attempt_unix = self.st.unix;
        self.st.weather.request_id = self.st.weather.request_id.wrapping_add(1);
        self.st.weather.loading = true;
        self.st.weather.message.clear();
        // 系统权限请求必须来自前台UI线程，且仅响应这次“定位”点击。
        self.set_noactivate(false);
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
        let result = services::location::LocationService::start(self.sink.clone(), self.st.weather.request_id);
        self.set_noactivate(!self.text_focus);
        match result {
            Ok(location) => self.location = Some(location),
            Err(error) => {
                pwarn!("weather location: {error}");
                self.st.weather.loading = false;
                self.st.weather.message = "定位暂时不可用，请稍后重试或手动选择城市".into();
            }
        }
    }

    pub(super) fn cancel_weather_location(&mut self, keep_permission_prompt: bool) {
        if keep_permission_prompt && self.location.as_ref().is_some_and(|location| location.awaiting_access()) {
            return;
        }
        if self.location.take().is_some() {
            self.st.weather.request_id = self.st.weather.request_id.wrapping_add(1);
            self.st.weather.loading = false;
            self.weather_attempt_unix = 0;
        }
    }
}

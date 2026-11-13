//! 天气请求与显式定位的主线程编排。
//! 保留页面可见、系统休眠和请求身份检查，不在静止时增加工作。

use super::*;

impl App {
    pub(super) fn refresh_weather(&mut self, force: bool) {
        if !self.expanded || self.st.page != PageKind::Today || self.away.0 || self.away.1 || self.st.power.saver || self.smoke.is_some() {
            return;
        }
        // 未选择城市时保持空状态，打开页面和重试都不能触发系统定位授权。
        if self.st.settings.weather_city.is_empty() || self.st.weather.loading {
            return;
        }
        // 成功时半小时更新一次；失败后十分钟再试。只在页面可见时运行。
        let interval = if self.st.weather.snapshot.is_some() { 30 * 60 } else { 10 * 60 };
        if !force && self.weather_attempt_unix > 0 && self.st.unix - self.weather_attempt_unix < interval {
            return;
        }
        self.weather_attempt_unix = self.st.unix;
        self.st.weather.request_id = self.st.weather.request_id.wrapping_add(1);
        let request_id = self.st.weather.request_id;
        self.st.weather.loading = true;
        self.st.weather.message.clear();
        if let Some((latitude, longitude)) = self.st.settings.weather_city_coordinates {
            services::weather::fetch_location(latitude, longitude, self.st.settings.weather_city.clone(), request_id, self.sink.clone());
        } else {
            services::weather::fetch_city(self.st.settings.weather_city.clone(), request_id, self.sink.clone());
        }
    }

    pub(super) fn set_weather_city(&mut self, city: String, coordinates: (f64, f64)) {
        // 即使重新选择同一城市，也要丢弃之前的定位回调，避免覆盖用户的选择。
        self.location = None;
        self.weather_initial_location = false;
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
        if !self.expanded || self.st.page != PageKind::Today || self.away.0 || self.away.1 || self.st.power.saver || self.smoke.is_some() || self.location.is_some() {
            return;
        }
        // 保留已保存的城市，定位成功后才替换；失败时仍可刷新原城市。
        self.weather_attempt_unix = self.st.unix;
        self.st.weather.request_id = self.st.weather.request_id.wrapping_add(1);
        self.st.weather.loading = true;
        self.st.weather.message.clear();
        let location = services::location::LocationService::new(self.sink.clone());
        self.weather_initial_location = location.needs_authorization();
        if self.weather_initial_location {
            // 首次系统授权需要应用处于前台；授权面板收起岛时保留这次请求。
            NSApplication::sharedApplication(self.mtm).activate();
        }
        location.request(self.st.weather.request_id);
        self.location = Some(location);
    }

    pub(super) fn cancel_weather_location(&mut self) {
        if self.weather_initial_location {
            return;
        }
        if self.location.take().is_some() {
            self.st.weather.request_id = self.st.weather.request_id.wrapping_add(1);
            self.st.weather.loading = false;
            self.weather_attempt_unix = 0;
        }
    }
}

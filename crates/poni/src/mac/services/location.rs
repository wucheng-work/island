//! 仅在用户点击天气的“定位”按钮后请求一次，结果经事件队列回到主线程。

use crate::events::{AppEvent, EventSink};
use std::cell::Cell;
use std::ffi::{c_char, c_void, CStr};

extern "C" {
    fn poni_location_needs_authorization() -> i32;
    fn poni_location_request(callback: extern "C" fn(*mut c_void, f64, f64, i32, *const c_char), context: *mut c_void);
    fn poni_location_stop();
}

struct Context {
    sink: EventSink,
    request_id: Cell<u64>,
}

pub struct LocationService {
    context: Box<Context>,
}

impl LocationService {
    pub fn new(sink: EventSink) -> Self {
        Self { context: Box::new(Context { sink, request_id: Cell::new(0) }) }
    }

    pub fn request(&self, request_id: u64) {
        self.context.request_id.set(request_id);
        unsafe { poni_location_request(location_result, self.context.as_ref() as *const Context as *mut c_void) }
    }

    pub fn needs_authorization(&self) -> bool {
        unsafe { poni_location_needs_authorization() != 0 }
    }
}

impl Drop for LocationService {
    fn drop(&mut self) {
        unsafe { poni_location_stop() }
    }
}

extern "C" fn location_result(context: *mut c_void, latitude: f64, longitude: f64, status: i32, place: *const c_char) {
    let context = unsafe { &*(context as *const Context) };
    if status != 0 {
        let detail = if place.is_null() { "no error detail".into() } else { unsafe { CStr::from_ptr(place) }.to_string_lossy() };
        pwarn!("weather location: status {status}, {detail}");
    }
    let result = match status {
        0 => {
            let place = if place.is_null() { "当前位置".to_string() } else { unsafe { CStr::from_ptr(place) }.to_string_lossy().into_owned() };
            Ok((latitude, longitude, place))
        }
        1 => Err("无法获取位置，请在系统设置中允许定位，或手动选择城市".to_string()),
        3 => Err("系统定位服务已关闭，请在系统设置中开启".to_string()),
        4 => Err("定位服务网络不可用，请检查网络后重试".to_string()),
        _ => Err("定位暂时不可用，请稍后重试或手动选择城市".to_string()),
    };
    context.sink.send(AppEvent::WeatherLocation { request_id: context.request_id.get(), result });
}

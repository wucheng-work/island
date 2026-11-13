//! 点击“定位”后只请求一次；WinRT完成回调返回结果，取消时释放请求。

use crate::events::{AppEvent, EventSink};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use windows::core::Result;
use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, Geoposition};
use windows::Foundation::{AsyncOperationCompletedHandler, IAsyncOperation, TimeSpan};

struct Request {
    sink: EventSink,
    request_id: u64,
    canceled: AtomicBool,
    awaiting_access: AtomicBool,
    position: Mutex<Option<IAsyncOperation<Geoposition>>>,
}

impl Request {
    fn finish(&self, result: std::result::Result<(f64, f64, String), String>) {
        self.position.lock().unwrap_or_else(|error| error.into_inner()).take();
        if !self.canceled.load(Ordering::Acquire) {
            self.sink.send(AppEvent::WeatherLocation { request_id: self.request_id, result });
        }
    }
}

pub struct LocationService {
    access: IAsyncOperation<GeolocationAccessStatus>,
    request: Arc<Request>,
}

impl LocationService {
    /// RequestAccessAsync必须在前台UI线程调用，由用户点击触发。
    pub fn start(sink: EventSink, request_id: u64) -> Result<Self> {
        let access = Geolocator::RequestAccessAsync()?;
        let request = Arc::new(Request { sink, request_id, canceled: AtomicBool::new(false), awaiting_access: AtomicBool::new(true), position: Mutex::new(None) });
        let pending = Arc::clone(&request);
        access.SetCompleted(&AsyncOperationCompletedHandler::<GeolocationAccessStatus>::new(move |operation, _status| {
            pending.awaiting_access.store(false, Ordering::Release);
            if pending.canceled.load(Ordering::Acquire) {
                return Ok(());
            }
            let allowed = operation.and_then(|operation| operation.GetResults().ok()) == Some(GeolocationAccessStatus::Allowed);
            if !allowed {
                pending.finish(Err("无法获取位置，请在 Windows 设置中允许定位，或手动选择城市".into()));
                return Ok(());
            }
            let result = (|| -> Result<()> {
                let locator = Geolocator::new()?;
                // 最多复用一分钟内的位置，十秒无结果即结束，不注册持续定位。
                let position = locator.GetGeopositionAsyncWithAgeAndTimeout(TimeSpan { Duration: 60 * 10_000_000 }, TimeSpan { Duration: 10 * 10_000_000 })?;
                {
                    let mut active = pending.position.lock().unwrap_or_else(|error| error.into_inner());
                    if pending.canceled.load(Ordering::Acquire) {
                        let _ = position.Cancel();
                        return Ok(());
                    }
                    *active = Some(position.clone());
                }
                let context = Arc::clone(&pending);
                position.SetCompleted(&AsyncOperationCompletedHandler::<Geoposition>::new(move |operation, _status| {
                    let result = (|| -> Result<(f64, f64, String)> {
                        let position = operation.ok_or_else(windows::core::Error::from_win32)?.GetResults()?;
                        let coordinate = position.Coordinate()?.Point()?.Position()?;
                        let city = position
                            .CivicAddress()
                            .and_then(|address| address.City())
                            .ok()
                            .map(|city| city.to_string())
                            .filter(|city| !city.is_empty())
                            .unwrap_or_else(|| "当前位置".into());
                        Ok((coordinate.Latitude, coordinate.Longitude, city))
                    })();
                    context.finish(result.map_err(|error| {
                        crate::pwarn!("weather location: {error}");
                        "定位暂时不可用，请稍后重试或手动选择城市".into()
                    }));
                    Ok(())
                }))?;
                Ok(())
            })();
            if let Err(error) = result {
                crate::pwarn!("weather location: {error}");
                pending.finish(Err("定位暂时不可用，请稍后重试或手动选择城市".into()));
            }
            Ok(())
        }))?;
        Ok(Self { access, request })
    }

    pub fn awaiting_access(&self) -> bool {
        self.request.awaiting_access.load(Ordering::Acquire)
    }
}

impl Drop for LocationService {
    fn drop(&mut self) {
        self.request.canceled.store(true, Ordering::Release);
        let _ = self.access.Cancel();
        let position = self.request.position.lock().unwrap_or_else(|error| error.into_inner()).take();
        if let Some(position) = position {
            let _ = position.Cancel();
        }
    }
}

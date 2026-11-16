//! 蓝牙页面的查询生命周期。关闭页面就停止等待，正在执行的系统读取有界取消。

use crate::events::{AppEvent, EventSink};
use poni_core::bluetooth::{BluetoothDevice, BluetoothSnapshot};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::Duration;

// 蓝牙电量变化慢；页面可见时每30秒读取一次，手动刷新无需等下一轮。
// 页面关闭、锁屏、熄屏或省电时不保留查询线程和定时唤醒。
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct BluetoothState {
    pub devices: Vec<BluetoothDevice>,
    pub loading: bool,
    pub error: Option<String>,
    pub warning: Option<String>,
    /// 上次成功采集的单调秒数，不代表设备电量实际更新的时间。
    pub updated_at: Option<f64>,
    /// 只有页面可见且系统允许读取时为true；false时loading必须为false。
    pub active: bool,
    /// 保留的上次读数：页面暂停、重新打开或本次读取失败。
    pub stale: bool,
    pub scroll: f32,
}

struct Session {
    refresh: SyncSender<()>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct BluetoothMonitor {
    active: bool,
    generation: u64,
    session: Option<Session>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

impl BluetoothMonitor {
    /// `on`由两端入口按实际页面可见性和系统休眠状态决定。
    pub fn set_active(&mut self, on: bool, state: &mut BluetoothState, sink: &EventSink) {
        if self.active == on {
            return;
        }
        self.active = on;
        state.active = on;
        self.generation = self.generation.wrapping_add(1);
        self.stop();
        self.workers.retain(|worker| !worker.is_finished());
        state.loading = false;
        state.stale = state.updated_at.is_some();
        if !on {
            return;
        }

        state.error = None;
        state.loading = true;
        let generation = self.generation;
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let sink = sink.clone();
        let started = std::thread::Builder::new().name("poni-bluetooth-devices".into()).spawn(move || {
            loop {
                if worker_cancel.load(Ordering::Acquire) {
                    break;
                }
                sink.send(AppEvent::BluetoothDevicesStarted(generation));
                let result = crate::services::bluetooth_devices::fetch_devices(&worker_cancel);
                if worker_cancel.load(Ordering::Acquire) {
                    break;
                }
                sink.send(AppEvent::BluetoothDevices { generation, result });
                match rx.recv_timeout(REFRESH_INTERVAL) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        match started {
            Ok(worker) => {
                self.session = Some(Session { refresh: tx, cancel });
                self.workers.push(worker);
            }
            Err(error) => {
                crate::pwarn!("bluetooth devices: couldn't start query thread ({error})");
                state.loading = false;
                state.error = Some("暂时无法读取蓝牙设备，请点击刷新重试。".into());
            }
        }
    }

    pub fn refresh(&mut self, state: &mut BluetoothState, sink: &EventSink) {
        if !self.active || state.loading {
            return;
        }
        if let Some(session) = &self.session {
            match session.refresh.try_send(()) {
                Ok(()) | Err(TrySendError::Full(())) => {
                    state.loading = true;
                    state.error = None;
                    return;
                }
                Err(TrySendError::Disconnected(())) => {}
            }
        }
        // 线程创建失败或意外退出后，刷新仍走完整的真实采集链路。
        self.active = false;
        self.set_active(true, state, sink);
    }

    pub fn started(&self, generation: u64, state: &mut BluetoothState) {
        if self.active && generation == self.generation {
            state.loading = true;
            state.error = None;
        }
    }

    pub fn receive(&self, generation: u64, result: Result<BluetoothSnapshot, String>, state: &mut BluetoothState, now: f64) {
        if !self.active || generation != self.generation {
            return;
        }
        state.loading = false;
        match result {
            Ok(mut snapshot) => {
                snapshot.devices.sort_by_cached_key(|device| (device.name.to_lowercase(), device.id.clone()));
                state.devices = snapshot.devices;
                state.updated_at = Some(now);
                state.error = None;
                state.warning = snapshot.warning;
                state.stale = false;
            }
            Err(error) => {
                state.error = Some(error);
                state.warning = None;
                state.stale = state.updated_at.is_some();
            }
        }
    }

    /// 应用退出前等已取消的系统读取回收资源；关闭页面只取消，不阻塞界面。
    pub fn shutdown(&mut self) {
        self.active = false;
        self.stop();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }

    fn stop(&mut self) {
        if let Some(session) = self.session.take() {
            session.cancel.store(true, Ordering::Release);
            // 唤醒正在等下一轮的线程；系统读取中的线程在有界查询结束后退出。
            let _ = session.refresh.try_send(());
        }
    }
}

impl Drop for BluetoothMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

//! 按需读取系统蓝牙报告、HID属性和附件电源信息，不扫描或连接设备。
//!
//! 数据来源参考Stats的多源方案；字段是否存在取决于macOS和设备。
//! https://github.com/exelban/stats/blob/cb9317e228dae415e06cffbdeed734b46f50592e/Modules/Bluetooth/readers.swift

use super::cf::{self, CFTypeRef};
use poni_core::bluetooth::{BatteryComponent, BluetoothDevice, BluetoothSnapshot, DeviceKind};
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::c_char;
use std::io::{ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 1024 * 1024;
const PROFILE_TIMEOUT: Duration = Duration::from_secs(8);
const ACCESSORY_TIMEOUT: Duration = Duration::from_secs(3);
const CANCELLED: &str = "蓝牙读取已取消";

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFTypeRef;
    fn IOServiceGetMatchingServices(port: u32, matching: CFTypeRef, iterator: *mut u32) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IORegistryEntryCreateCFProperties(
        entry: u32,
        properties: *mut CFTypeRef,
        allocator: CFTypeRef,
        options: u32,
    ) -> i32;
    fn IORegistryEntryGetRegistryEntryID(entry: u32, id: *mut u64) -> i32;
}

extern "C" {
    fn CFDataCreate(allocator: CFTypeRef, bytes: *const u8, length: isize) -> CFTypeRef;
    fn CFPropertyListCreateWithData(
        allocator: CFTypeRef,
        data: CFTypeRef,
        options: usize,
        format: *mut isize,
        error: *mut CFTypeRef,
    ) -> CFTypeRef;
    fn CFGetTypeID(value: CFTypeRef) -> usize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn fcntl(fd: i32, command: i32, ...) -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
}

/// Copy/Create规则返回的CF对象；借用的字典值不构造此类型。
struct OwnedCf(CFTypeRef);

impl Drop for OwnedCf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { cf::CFRelease(self.0) };
        }
    }
}

struct IoObject(u32);

impl Drop for IoObject {
    fn drop(&mut self) {
        unsafe { IOObjectRelease(self.0) };
    }
}

/// 同步采集，仅由页面活动时的后台工作线程调用；失败文案可直接展示给用户。
pub fn fetch_devices(cancel: &AtomicBool) -> Result<BluetoothSnapshot, String> {
    if cancel.load(Ordering::Acquire) {
        return Err(CANCELLED.into());
    }

    let report = run_command(
        "/usr/sbin/system_profiler",
        &["SPBluetoothDataType", "-json"],
        PROFILE_TIMEOUT,
        cancel,
    )
    .and_then(|bytes| parse_report(&bytes));
    if cancel.load(Ordering::Acquire) {
        return Err(CANCELLED.into());
    }
    let hid = fetch_hid(cancel);
    let mut warnings = Vec::new();
    let mut devices = match report {
        Ok(devices) => devices,
        Err(reason) => match &hid {
            Ok(devices) if !devices.is_empty() => {
                warnings.push("系统蓝牙报告暂时不可用，当前仅显示已读取的键鼠".to_string());
                Vec::new()
            }
            _ => {
                return Err(format!(
                    "无法读取蓝牙设备：{reason}。请稍后重试或查看系统蓝牙设置"
                ))
            }
        },
    };
    match hid {
        Ok(hid_devices) => {
            for device in hid_devices {
                if let Some(existing) = devices.iter_mut().find(|existing| existing.id == device.id)
                {
                    existing.battery = device.battery.or(existing.battery);
                    if device.kind != DeviceKind::Other {
                        existing.kind = device.kind;
                    }
                } else {
                    devices.push(device);
                }
            }
        }
        Err(reason) if cancel.load(Ordering::Acquire) => return Err(reason),
        Err(_) => warnings.push("部分键鼠电量暂时无法读取".to_string()),
    }

    // 没有已连接设备就无需再启动附件查询；附件缓存本身不作为连接依据。
    if !devices.is_empty() {
        let accessories = run_command(
            "/usr/bin/pmset",
            &["-g", "accps", "-xml"],
            ACCESSORY_TIMEOUT,
            cancel,
        )
        .and_then(|bytes| parse_accessories(&bytes));
        match accessories {
            Ok(accessories) => apply_accessories(&mut devices, accessories),
            Err(_) if cancel.load(Ordering::Acquire) => return Err(CANCELLED.into()),
            Err(_) => warnings.push("耳机部件电量暂时无法读取".to_string()),
        }
    }
    if cancel.load(Ordering::Acquire) {
        return Err(CANCELLED.into());
    }
    devices.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(BluetoothSnapshot {
        devices,
        warning: (!warnings.is_empty()).then(|| warnings.join("；")),
    })
}

/// 非阻塞读取两条管道，避免大输出堵住子进程；取消、超时或格式失败都会回收进程。
fn run_command(
    path: &str,
    args: &[&str],
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    if cancel.load(Ordering::Acquire) {
        return Err(CANCELLED.into());
    }
    let mut child = Command::new(path)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|error| {
            crate::pwarn!(
                "bluetooth devices: {path} could not start ({})",
                error.kind()
            );
            "无法启动系统查询".to_string()
        })?;
    let pid = child.id() as i32;
    let result = (|| {
        let mut stdout = child.stdout.take().ok_or("无法读取系统查询输出")?;
        let mut stderr = child.stderr.take().ok_or("无法读取系统查询错误状态")?;
        for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
            // Darwin: F_GETFL=3、F_SETFL=4、O_NONBLOCK=0x4。
            let flags = unsafe { fcntl(fd, 3) };
            if flags < 0 || unsafe { fcntl(fd, 4, flags | 0x4) } < 0 {
                return Err("无法读取系统查询输出".to_string());
            }
        }
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err(CANCELLED.into());
            }
            if Instant::now() >= deadline {
                return Err("系统查询超时".to_string());
            }
            read_available(&mut stdout, &mut output)?;
            read_available(&mut stderr, &mut errors)?;
            if let Some(status) = child.try_wait().map_err(|_| "无法读取系统查询状态")? {
                read_available(&mut stdout, &mut output)?;
                read_available(&mut stderr, &mut errors)?;
                if !status.success() {
                    crate::pwarn!(
                        "bluetooth devices: {path} failed ({status}, stderr {} bytes)",
                        errors.len()
                    );
                    return Err("系统查询失败".to_string());
                }
                return Ok(output);
            }
            // 只在系统查询进行时检查取消；20ms避免忙等，页面关闭后及时结束查询。
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    if result.is_err() {
        // 每次查询有独立进程组，连同可能的子进程一起结束，不遗留后台查询。
        unsafe { kill(-pid, 9) };
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

fn read_available(pipe: &mut impl Read, output: &mut Vec<u8>) -> Result<(), String> {
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                if output.len() + count > OUTPUT_LIMIT {
                    return Err("系统查询输出过大".into());
                }
                output.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err("无法读取系统查询输出".into()),
        }
    }
}

fn parse_report(bytes: &[u8]) -> Result<Vec<BluetoothDevice>, String> {
    let report: Value = serde_json::from_slice(bytes).map_err(|_| "系统蓝牙报告格式异常")?;
    let sections = report
        .get("SPBluetoothDataType")
        .and_then(Value::as_array)
        .ok_or("系统蓝牙报告缺少设备信息")?;
    let mut devices: Vec<BluetoothDevice> = Vec::new();
    for (section_index, section) in sections.iter().enumerate() {
        let Some(connected) = section.get("device_connected") else {
            continue;
        };
        let groups = connected.as_array().ok_or("系统蓝牙连接列表格式异常")?;
        for (group_index, group) in groups.iter().enumerate() {
            let entries = group.as_object().ok_or("系统蓝牙设备格式异常")?;
            for (name, fields) in entries {
                if !fields.is_object() {
                    return Err("系统蓝牙设备属性格式异常".into());
                }
                let address = fields
                    .get("device_address")
                    .and_then(Value::as_str)
                    .filter(|address| !address.trim().is_empty());
                let id = address
                    .map(normalize_id)
                    .unwrap_or_else(|| format!("report:{section_index}:{group_index}:{name}"));
                let mut device = BluetoothDevice {
                    id,
                    name: display_name(name),
                    kind: device_kind(
                        fields
                            .get("device_minorType")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    ),
                    battery: fields.get("device_batteryLevelMain").and_then(json_percent),
                    components: Vec::new(),
                };
                for (keys, label) in [
                    (["device_batteryLevelLeft", "Left Battery Level"], "左耳"),
                    (["device_batteryLevelRight", "Right Battery Level"], "右耳"),
                    (["device_batteryLevelCase", "Case Battery Level"], "充电盒"),
                ] {
                    if let Some(percent) = keys
                        .iter()
                        .find_map(|key| fields.get(*key).and_then(json_percent))
                    {
                        set_component(&mut device, label, percent);
                    }
                }
                if !devices.iter().any(|existing| existing.id == device.id) {
                    devices.push(device);
                }
            }
        }
    }
    Ok(devices)
}

fn json_percent(value: &Value) -> Option<u8> {
    let number = if let Some(number) = value.as_i64() {
        number
    } else {
        value
            .as_str()?
            .trim()
            .trim_end_matches('%')
            .trim()
            .parse()
            .ok()?
    };
    percent(number)
}

fn percent(value: i64) -> Option<u8> {
    (0..=100).contains(&value).then_some(value as u8)
}

fn normalize_id(value: &str) -> String {
    value.trim().replace(':', "-").to_lowercase()
}

fn display_name(value: &str) -> String {
    if value.trim().is_empty() {
        "蓝牙设备".into()
    } else {
        value.trim().into()
    }
}

fn device_kind(value: &str) -> DeviceKind {
    let value = value.to_ascii_lowercase();
    if ["headphone", "headset", "earbud", "speaker", "audio"]
        .iter()
        .any(|word| value.contains(word))
    {
        DeviceKind::Audio
    } else if value.contains("keyboard") {
        DeviceKind::Keyboard
    } else if value.contains("mouse") || value.contains("trackpad") {
        DeviceKind::Mouse
    } else if value.contains("gamepad") || value.contains("controller") {
        DeviceKind::Gamepad
    } else {
        DeviceKind::Other
    }
}

fn dictionary_value(dictionary: CFTypeRef, key: &str) -> CFTypeRef {
    let key = cf::key(key);
    unsafe { cf::CFDictionaryGetValue(dictionary, &*key as *const _ as CFTypeRef) }
}

fn dictionary_text(dictionary: CFTypeRef, key: &str) -> Option<String> {
    cf::text(dictionary_value(dictionary, key))
}

fn is_dictionary(value: CFTypeRef) -> bool {
    !value.is_null() && unsafe { CFGetTypeID(value) == CFDictionaryGetTypeID() }
}

fn fetch_hid(cancel: &AtomicBool) -> Result<Vec<BluetoothDevice>, String> {
    let matching = unsafe { IOServiceMatching(c"AppleDeviceManagementHIDEventService".as_ptr()) };
    if matching.is_null() {
        return Err("无法读取键鼠设备信息".into());
    }
    let mut iterator = 0;
    // IOServiceGetMatchingServices消耗matching的引用，无需再次CFRelease。
    let status = unsafe { IOServiceGetMatchingServices(0, matching, &mut iterator) };
    if status != 0 {
        crate::pwarn!("bluetooth devices: HID enumeration failed ({status})");
        return Err("无法读取键鼠设备信息".into());
    }
    let iterator = IoObject(iterator);
    let mut devices = Vec::new();
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(CANCELLED.into());
        }
        let entry = unsafe { IOIteratorNext(iterator.0) };
        if entry == 0 {
            break;
        }
        let entry = IoObject(entry);
        let mut properties = ptr::null();
        let status =
            unsafe { IORegistryEntryCreateCFProperties(entry.0, &mut properties, ptr::null(), 0) };
        let properties = OwnedCf(properties);
        if status != 0 || !is_dictionary(properties.0) {
            crate::pwarn!("bluetooth devices: HID properties unavailable ({status})");
            return Err("无法读取键鼠电量属性".into());
        }
        if cf::dict_bool(properties.0, "BluetoothDevice") != Some(true) {
            continue;
        }
        let address = ["DeviceAddress", "SerialNumber", "BD_ADDR"]
            .iter()
            .find_map(|key| {
                dictionary_text(properties.0, key).filter(|value| !value.trim().is_empty())
            });
        let id = if let Some(address) = address {
            normalize_id(&address)
        } else {
            let mut registry_id = 0;
            if unsafe { IORegistryEntryGetRegistryEntryID(entry.0, &mut registry_id) } != 0 {
                return Err("无法识别键鼠设备".into());
            }
            format!("hid:{registry_id}")
        };
        let name = dictionary_text(properties.0, "Product").unwrap_or_default();
        let usage = cf::dict_i64(properties.0, "PrimaryUsage");
        let kind = match (cf::dict_i64(properties.0, "PrimaryUsagePage"), usage) {
            (Some(1), Some(6)) => DeviceKind::Keyboard,
            (Some(1), Some(1)) | (Some(1), Some(2)) => DeviceKind::Mouse,
            (Some(1), Some(4)) | (Some(1), Some(5)) => DeviceKind::Gamepad,
            _ => device_kind(&name),
        };
        devices.push(BluetoothDevice {
            id,
            name: display_name(&name),
            kind,
            battery: cf::dict_i64(properties.0, "BatteryPercent").and_then(percent),
            components: Vec::new(),
        });
    }
    Ok(devices)
}

struct AccessoryBattery {
    id: String,
    group: Option<String>,
    name: String,
    battery: Option<u8>,
    components: Vec<BatteryComponent>,
}

fn parse_accessories(bytes: &[u8]) -> Result<Vec<AccessoryBattery>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "附件电量编码异常")?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut documents = text.split("<?xml");
    if !documents.next().unwrap_or_default().trim().is_empty() {
        return Err("附件电量数据格式异常".into());
    }
    let mut accessories = Vec::new();
    for document in documents {
        let xml = format!("<?xml{document}");
        let data = OwnedCf(unsafe { CFDataCreate(ptr::null(), xml.as_ptr(), xml.len() as isize) });
        if data.0.is_null() {
            return Err("无法读取附件电量数据".into());
        }
        let mut error = ptr::null();
        let properties = OwnedCf(unsafe {
            CFPropertyListCreateWithData(ptr::null(), data.0, 0, ptr::null_mut(), &mut error)
        });
        let _error = OwnedCf(error);
        if !is_dictionary(properties.0) {
            return Err("附件电量数据格式异常".into());
        }
        if cf::dict_bool(properties.0, "Is Present") == Some(false) {
            continue;
        }
        let mut accessory = AccessoryBattery {
            id: dictionary_text(properties.0, "Accessory Identifier")
                .map(|id| normalize_id(&id))
                .unwrap_or_default(),
            group: dictionary_text(properties.0, "Group Identifier"),
            name: dictionary_text(properties.0, "Name").unwrap_or_default(),
            battery: None,
            components: Vec::new(),
        };
        let part = dictionary_text(properties.0, "Part Identifier").unwrap_or_default();
        if let Some(value) = cf::dict_i64(properties.0, "Current Capacity").and_then(percent) {
            match part_label(&part) {
                Some(label) => accessory.components.push(BatteryComponent {
                    label: label.into(),
                    percent: value,
                }),
                None => accessory.battery = Some(value),
            }
        }
        let parts = dictionary_value(properties.0, "Combined Parts");
        if !parts.is_null() && unsafe { CFGetTypeID(parts) == CFArrayGetTypeID() } {
            for index in 0..unsafe { cf::CFArrayGetCount(parts) } {
                let part = unsafe { cf::CFArrayGetValueAtIndex(parts, index) };
                if !is_dictionary(part) {
                    continue;
                }
                let label =
                    dictionary_text(part, "Part Identifier").and_then(|part| part_label(&part));
                if let (Some(label), Some(value)) = (
                    label,
                    cf::dict_i64(part, "Current Capacity").and_then(percent),
                ) {
                    accessory.components.push(BatteryComponent {
                        label: label.into(),
                        percent: value,
                    });
                }
            }
        }
        accessories.push(accessory);
    }
    if accessories.is_empty() && !text.contains("<?xml") {
        return Err("附件电量数据格式异常".into());
    }
    Ok(accessories)
}

fn part_label(part: &str) -> Option<&'static str> {
    match part {
        "Left" => Some("左耳"),
        "Right" => Some("右耳"),
        "Case" => Some("充电盒"),
        _ => None,
    }
}

fn apply_accessories(devices: &mut [BluetoothDevice], accessories: Vec<AccessoryBattery>) {
    let mut groups = HashMap::new();
    for accessory in &accessories {
        if let (Some(group), Some(index)) = (&accessory.group, accessory_device(devices, accessory))
        {
            groups.insert(group.clone(), index);
        }
    }
    for accessory in accessories {
        let index = accessory_device(devices, &accessory).or_else(|| {
            accessory
                .group
                .as_ref()
                .and_then(|group| groups.get(group).copied())
        });
        let Some(index) = index else { continue };
        let device = &mut devices[index];
        device.battery = accessory.battery.or(device.battery);
        for component in accessory.components {
            set_component(device, &component.label, component.percent);
        }
    }
}

fn accessory_device(devices: &[BluetoothDevice], accessory: &AccessoryBattery) -> Option<usize> {
    if !accessory.id.is_empty() {
        if let Some(index) = devices.iter().position(|device| device.id == accessory.id) {
            return Some(index);
        }
    }
    // 附件的标识有时是UUID；只有名称唯一时才关联，避免同名耳机串电量。
    if accessory.name.trim().is_empty() {
        return None;
    }
    let mut matches = devices
        .iter()
        .enumerate()
        .filter(|(_, device)| device.name == accessory.name.trim());
    let first = matches.next()?.0;
    matches.next().is_none().then_some(first)
}

fn set_component(device: &mut BluetoothDevice, label: &str, percent: u8) {
    if let Some(component) = device
        .components
        .iter_mut()
        .find(|component| component.label == label)
    {
        component.percent = percent;
    } else {
        device.components.push(BatteryComponent {
            label: label.into(),
            percent,
        });
    }
}

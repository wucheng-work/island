//! 蓝牙页面按需读取的Windows快照。系统PnP电量优先，已连接BLE设备的标准服务补全。

use super::bluetooth::{
    braced, gatt_battery_result, paired_only, prop_bool, prop_guid, prop_u32, usable_container,
    wait_result, ComApartment, LE_AUDIO_CATEGORIES, PKEY_COD_MAJOR, PKEY_CONNECTED,
    PKEY_CONTAINER, PKEY_LE_CATEGORY,
};
use poni_core::bluetooth::{BluetoothDevice as Device, BluetoothSnapshot, DeviceKind};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows::core::{Error, Result as WinResult, GUID, HRESULT, HSTRING, PCWSTR};
use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationCollection, DeviceInformationKind};
use windows::Foundation::Collections::IIterable;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT,
    HDEVINFO, MAX_DEVICE_ID_LEN, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_ContainerId, DEVPROPKEY, DEVPROPTYPE, DEVPROP_TYPE_BYTE, DEVPROP_TYPE_GUID,
};
use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, HWND};

const PKEY_COD_MINOR: &str = "System.Devices.Aep.Bluetooth.Cod.Minor";
const PKEY_LE_SUBCATEGORY: &str = "System.Devices.Aep.Bluetooth.Le.Appearance.Subcategory";
const BATTERY_PROPERTY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104EA319_6EE2_4701_BD47_8DDBF425BBE5),
    pid: 2,
};
const ENUM_TIMEOUT: Duration = Duration::from_secs(4);
/// 电量变化缓慢，一次采集共用预算，避免多个不支持GATT的设备逐个耗尽完整超时。
const BATTERY_BUDGET: Duration = Duration::from_secs(10);
const DEVICE_BUDGET: Duration = Duration::from_millis(2500);

struct Endpoint {
    id: String,
    name: String,
    container: Option<GUID>,
    kind: DeviceKind,
}

struct PendingDevice {
    device: Device,
    container: Option<GUID>,
    le_endpoints: Vec<String>,
}

/// 只在后台线程读取配对且连接的设备。不扫描广播、不配对；取消后不再发起新读取。
pub fn fetch_devices(cancel: &AtomicBool) -> Result<BluetoothSnapshot, String> {
    check_cancelled(cancel)?;
    let com = ComApartment::enter_mta();
    if !com.initialized {
        return Err("无法读取蓝牙设备，请关闭页面后重试".into());
    }
    let mut devices: BTreeMap<String, PendingDevice> = BTreeMap::new();
    let mut source_count = 0;
    let mut warnings = Vec::new();
    let mut battery_failed = false;
    for le in [false, true] {
        check_cancelled(cancel)?;
        let endpoints = match enumerate(le, cancel) {
            Ok(endpoints) => endpoints,
            Err(error) => {
                crate::pwarn!("bluetooth devices: {} enumeration failed ({:?})", if le { "LE" } else { "classic" }, error.code());
                continue;
            }
        };
        source_count += 1;
        for endpoint in endpoints {
            let key = endpoint.container.as_ref().map(braced).unwrap_or_else(|| endpoint.id.clone());
            let pending = devices.entry(key.clone()).or_insert_with(|| PendingDevice {
                device: Device { id: key, name: String::new(), kind: DeviceKind::Other, battery: None, components: Vec::new() },
                container: endpoint.container,
                le_endpoints: Vec::new(),
            });
            if pending.device.name.is_empty() && !endpoint.name.is_empty() {
                pending.device.name = endpoint.name;
            }
            if pending.device.kind == DeviceKind::Other {
                pending.device.kind = endpoint.kind;
            }
            if le && !pending.le_endpoints.contains(&endpoint.id) {
                pending.le_endpoints.push(endpoint.id);
            }
        }
    }
    check_cancelled(cancel)?;
    if source_count == 0 {
        return Err("无法读取蓝牙设备，请检查系统蓝牙是否开启后重试".into());
    }
    if source_count == 1 {
        warnings.push("部分蓝牙设备暂时无法读取，请稍后刷新");
    }
    let pnp = if devices.is_empty() {
        HashMap::new()
    } else {
        match fetch_pnp_batteries(cancel) {
            Ok(batteries) => batteries,
            Err(error) => {
                check_cancelled(cancel)?;
                crate::pwarn!("bluetooth devices: PnP battery enumeration failed ({:?})", error.code());
                battery_failed = true;
                HashMap::new()
            }
        }
    };

    let battery_deadline = Instant::now() + BATTERY_BUDGET;
    let mut out = Vec::with_capacity(devices.len());
    for mut pending in devices.into_values() {
        check_cancelled(cancel)?;
        pending.device.battery = pending.container.as_ref().and_then(|container| pnp.get(&braced(container))).copied();
        let deadline = (Instant::now() + DEVICE_BUDGET).min(battery_deadline);
        for id in &pending.le_endpoints {
            if pending.device.battery.is_some() {
                break;
            }
            if Instant::now() >= deadline {
                battery_failed = true;
                break;
            }
            check_cancelled(cancel)?;
            let open_deadline = (Instant::now() + Duration::from_secs(1)).min(deadline);
            let device = match wait_result(open_deadline, Some(cancel), || BluetoothLEDevice::FromIdAsync(&HSTRING::from(id))) {
                Ok(device) => device,
                Err(error) => {
                    crate::pwarn!("bluetooth devices: LE device open failed ({:?})", error.code());
                    battery_failed = true;
                    continue;
                }
            };
            match device.ConnectionStatus() {
                Ok(BluetoothConnectionStatus::Connected) => {
                    // 不沿用HUD的音频过滤：键盘、鼠标也可能提供标准Battery Service。
                    match gatt_battery_result(&device, deadline, Some(cancel)) {
                        Ok(level) => pending.device.battery = level,
                        Err(error) => {
                            crate::pwarn!("bluetooth devices: GATT battery read failed ({:?})", error.code());
                            battery_failed = true;
                        }
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    crate::pwarn!("bluetooth devices: LE connection state read failed ({:?})", error.code());
                    battery_failed = true;
                }
            }
            let _ = device.Close();
        }
        if pending.device.name.is_empty() {
            pending.device.name = "蓝牙设备".into();
        }
        out.push(pending.device);
    }
    check_cancelled(cancel)?;
    if battery_failed {
        warnings.push("部分设备电量暂时无法读取，请稍后刷新");
    }
    out.sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.id.cmp(&right.id)));
    Ok(BluetoothSnapshot { devices: out, warning: (!warnings.is_empty()).then(|| warnings.join("；")) })
}

fn enumerate(le: bool, cancel: &AtomicBool) -> WinResult<Vec<Endpoint>> {
    let selector = if le {
        BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected)?
    } else {
        BluetoothDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected)?
    };
    let selector = HSTRING::from(paired_only(&selector.to_string_lossy()));
    let class_properties = if le { [PKEY_LE_CATEGORY, PKEY_LE_SUBCATEGORY] } else { [PKEY_COD_MAJOR, PKEY_COD_MINOR] };
    let full = [PKEY_CONTAINER, PKEY_CONNECTED, class_properties[0], class_properties[1]];
    let basic = [PKEY_CONTAINER, PKEY_CONNECTED];
    let deadline = Instant::now() + ENUM_TIMEOUT;
    let mut found: Option<DeviceInformationCollection> = None;
    let mut last_error = None;
    // 只对旧系统不支持的分类属性降级，不隐藏权限或蓝牙服务错误。
    for properties in [&full[..], &basic[..], &[][..]] {
        let properties = IIterable::<HSTRING>::try_from(properties.iter().map(|key| HSTRING::from(*key)).collect::<Vec<_>>())?;
        match wait_result(deadline, Some(cancel), || DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(
            &selector, &properties, DeviceInformationKind::AssociationEndpoint,
        )) {
            Ok(devices) => { found = Some(devices); break; }
            Err(error) if error.code() == HRESULT(0x80070057u32 as i32) => last_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    let found = found.ok_or_else(|| last_error.unwrap_or_else(|| Error::new(HRESULT(0x80004005u32 as i32), "Bluetooth enumeration failed")))?;
    let mut out = Vec::new();
    for index in 0..found.Size()? {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let info = found.GetAt(index)?;
        let properties = info.Properties()?;
        if prop_bool(&properties, PKEY_CONNECTED) == Some(false) {
            continue;
        }
        let kind = if le {
            le_kind(prop_u32(&properties, PKEY_LE_CATEGORY), prop_u32(&properties, PKEY_LE_SUBCATEGORY))
        } else {
            classic_kind(prop_u32(&properties, PKEY_COD_MAJOR), prop_u32(&properties, PKEY_COD_MINOR))
        };
        out.push(Endpoint {
            id: info.Id()?.to_string_lossy(),
            name: info.Name()?.to_string_lossy().trim().to_string(),
            container: prop_guid(&properties, PKEY_CONTAINER).filter(usable_container),
            kind,
        });
    }
    Ok(out)
}

struct DeviceInfoSet(HDEVINFO);

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        unsafe { let _ = SetupDiDestroyDeviceInfoList(self.0); }
    }
}

/// SetupAPI只读取Windows维护的本地属性，包含同一蓝牙设备的根节点和服务节点。
fn fetch_pnp_batteries(cancel: &AtomicBool) -> WinResult<HashMap<String, u8>> {
    let set = DeviceInfoSet(unsafe { SetupDiGetClassDevsW(None, PCWSTR::null(), HWND::default(), DIGCF_PRESENT | DIGCF_ALLCLASSES)? });
    let mut out = HashMap::new();
    for index in 0..u32::MAX {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let mut info = SP_DEVINFO_DATA { cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() };
        if let Err(error) = unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut info) } {
            if error.code() == HRESULT::from_win32(ERROR_NO_MORE_ITEMS.0) {
                break;
            }
            return Err(error);
        }
        let mut instance = [0u16; MAX_DEVICE_ID_LEN as usize];
        unsafe { SetupDiGetDeviceInstanceIdW(set.0, &info, Some(&mut instance), None)?; }
        let end = instance.iter().position(|value| *value == 0).unwrap_or(instance.len());
        let instance = String::from_utf16_lossy(&instance[..end]);
        if !instance.starts_with("BTHENUM\\") && !instance.starts_with("BTHLE\\") && !instance.starts_with("BTHLEDEVICE\\") {
            continue;
        }
        let mut property_type = DEVPROPTYPE::default();
        let mut battery = [0u8; 1];
        let mut size = 0;
        let battery_read = unsafe { SetupDiGetDevicePropertyW(set.0, &info, &BATTERY_PROPERTY, &mut property_type, Some(&mut battery), Some(&mut size), 0) };
        if battery_read.is_err() || property_type != DEVPROP_TYPE_BYTE || size != 1 || battery[0] > 100 {
            continue;
        }
        let mut container = [0u8; 16];
        let container_read = unsafe { SetupDiGetDevicePropertyW(set.0, &info, &DEVPKEY_Device_ContainerId, &mut property_type, Some(&mut container), Some(&mut size), 0) };
        if container_read.is_err() || property_type != DEVPROP_TYPE_GUID || size != 16 {
            continue;
        }
        // 属性类型和长度均已校验；GUID按Windows内存布局读取，不要求字节数组对齐。
        let container = unsafe { std::ptr::read_unaligned(container.as_ptr().cast::<GUID>()) };
        if usable_container(&container) {
            out.entry(braced(&container)).or_insert(battery[0]);
        }
    }
    Ok(out)
}

fn classic_kind(major: Option<u32>, minor: Option<u32>) -> DeviceKind {
    match major {
        Some(4) => DeviceKind::Audio,
        Some(5) => match minor {
            Some(value) if value & 0x0F == 1 || value & 0x0F == 2 => DeviceKind::Gamepad,
            Some(value) if value & 0x10 != 0 => DeviceKind::Keyboard,
            Some(value) if value & 0x20 != 0 => DeviceKind::Mouse,
            _ => DeviceKind::Other,
        },
        _ => DeviceKind::Other,
    }
}

fn le_kind(category: Option<u32>, subcategory: Option<u32>) -> DeviceKind {
    match category {
        Some(value) if LE_AUDIO_CATEGORIES.contains(&value) => DeviceKind::Audio,
        Some(0x0F) => match subcategory {
            Some(1) => DeviceKind::Keyboard,
            Some(2) => DeviceKind::Mouse,
            Some(3 | 4) => DeviceKind::Gamepad,
            _ => DeviceKind::Other,
        },
        _ => DeviceKind::Other,
    }
}

fn check_cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("已取消蓝牙设备读取".into());
    }
    Ok(())
}

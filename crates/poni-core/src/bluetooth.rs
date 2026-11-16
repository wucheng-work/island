//! 两端蓝牙设备列表使用的快照，只包含当前连接的设备。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceKind {
    Audio,
    Keyboard,
    Mouse,
    Gamepad,
    #[default]
    Other,
}

/// 耳机左右耳、充电盒等独立部件的电量；单位为百分比，范围0..=100。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatteryComponent {
    pub label: String,
    pub percent: u8,
}

/// 系统读取的一次设备快照。ID只用于本次运行中合并设备，不用于显示或持久化。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    /// `None`表示系统或设备未提供电量，不能按0%显示。
    pub battery: Option<u8>,
    pub components: Vec<BatteryComponent>,
}

/// 一次采集的连接设备列表。补充电量来源失败时保留可用设备，并明确提示。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BluetoothSnapshot {
    pub devices: Vec<BluetoothDevice>,
    pub warning: Option<String>,
}

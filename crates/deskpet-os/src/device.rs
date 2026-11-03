//! 设备信息采集：品牌/机型/系统版本/无线开关状态。
//!
//! 尽力而为采集：单个字段失败不影响其余字段，返回默认值（空串/false）。
//!
//! 注意：禁止直连 IOBluetooth 等隐私框架 API——打包后的 .app 缺少
//! NSBluetoothAlwaysUsageDescription，macOS TCC 会直接终止进程（SIGABRT）。

/// 终端设备信息快照（连接握手时上报）。
#[derive(Debug, Clone, Default)]
pub struct DeviceSnapshot {
    /// 品牌厂商（如 Apple）
    pub device_brand: String,
    /// 机型型号（如 Mac16,8）
    pub device_model: String,
    /// 设备形态（laptop/desktop）
    pub device_type: String,
    /// 操作系统（如 macos）
    pub os_name: String,
    /// 系统版本（如 27.0.1）
    pub os_version: String,
    /// Wi-Fi 开关状态
    pub wifi_enabled: bool,
    /// 蓝牙开关状态（不再采集，恒为 false；避免触发 macOS TCC 终止）
    pub bluetooth_enabled: bool,
}

/// 采集设备信息快照（含少量系统命令调用，同步阻塞，调用方应避免在 async 上下文直接调用）。
pub fn snapshot() -> DeviceSnapshot {
    imp::snapshot()
}

#[cfg(target_os = "macos")]
mod imp {
    use super::DeviceSnapshot;

    pub fn snapshot() -> DeviceSnapshot {
        let model = sysctl("hw.model").unwrap_or_default();
        let device_type = if model.starts_with("MacBook") {
            "laptop"
        } else {
            "desktop"
        };
        DeviceSnapshot {
            device_brand: "Apple".into(),
            device_model: model,
            device_type: device_type.into(),
            os_name: std::env::consts::OS.into(),
            os_version: sysctl("kern.osproductversion").unwrap_or_default(),
            wifi_enabled: wifi_enabled(),
            bluetooth_enabled: false,
        }
    }

    /// sysctl 单键读取（`sysctl -n <key>`）。
    fn sysctl(key: &str) -> Option<String> {
        let out = std::process::Command::new("sysctl").arg("-n").arg(key).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// Wi-Fi 电源状态：先定位 Wi-Fi 网卡（`networksetup -listallhardwareports`），再查其电源状态。
    fn wifi_enabled() -> bool {
        let Ok(ports) = std::process::Command::new("networksetup")
            .arg("-listallhardwareports")
            .output()
        else {
            return false;
        };
        let text = String::from_utf8_lossy(&ports.stdout);
        // 形如 "Hardware Port: Wi-Fi\nDevice: en0\n..."（端口名跨语言不翻译），取该段设备名
        let device = text
            .split("\n\n")
            .find(|blk| blk.contains("Wi-Fi"))
            .and_then(|blk| blk.lines().find_map(|l| l.strip_prefix("Device: ")))
            .map_or("en0", str::trim);
        let Ok(out) = std::process::Command::new("networksetup")
            .args(["-getairportpower", device])
            .output()
        else {
            return false;
        };
        String::from_utf8_lossy(&out.stdout).contains(": On")
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::DeviceSnapshot;

    pub fn snapshot() -> DeviceSnapshot {
        DeviceSnapshot {
            os_name: std::env::consts::OS.into(),
            ..Default::default()
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn snapshot_fields_are_populated() {
        let snap = snapshot();
        assert_eq!(snap.device_brand, "Apple");
        assert!(snap.device_model.starts_with("Mac"));
        assert!(["laptop", "desktop"].contains(&snap.device_type.as_str()));
        assert_eq!(snap.os_name, "macos");
        assert!(!snap.os_version.is_empty());
    }
}

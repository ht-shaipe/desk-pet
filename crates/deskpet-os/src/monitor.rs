//! 系统运行状态采样：CPU、内存、网络速率、磁盘卷空间。
//!
//! 参考 LiteMonitor 的监控项设计，但只保留跨平台可采集的指标
//! （CPU/GPU 温度、功耗等驱动级读数在 macOS 上不可用，不在范围内）。

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sysinfo::{Disks, Networks, System, MINIMUM_CPU_UPDATE_INTERVAL};

/// CPU 采样：总体使用率与每核使用率（0-100）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuSample {
    pub usage: f32,
    pub cores: Vec<f32>,
}

/// 内存采样（字节）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySample {
    pub used: u64,
    pub total: u64,
}

/// 网络采样：上下行速率（字节/秒）与开机以来累计流量（字节），已排除回环接口。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSample {
    pub down_rate: u64,
    pub up_rate: u64,
    pub total_down: u64,
    pub total_up: u64,
}

/// 磁盘卷空间采样（字节）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeSample {
    pub name: String,
    pub mount: String,
    pub total: u64,
    pub available: u64,
}

/// 一次系统采样快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSample {
    pub cpu: CpuSample,
    pub memory: MemorySample,
    pub network: NetworkSample,
    pub volumes: Vec<VolumeSample>,
}

/// 有状态采样器：跨两次 `sample` 保存内部计数，
/// 用于计算 CPU 使用率与网络速率（两者都需要相邻两次采样的差值）。
pub struct Monitor {
    sys: System,
    nets: Networks,
    disks: Disks,
    last_cpu: Option<Instant>,
    last_net: Option<Instant>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

fn is_loopback(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "lo" || lower == "lo0" || lower.starts_with("loopback")
}

fn clamp_percent(value: f32) -> f32 {
    value.clamp(0.0, 100.0)
}

impl Monitor {
    /// sysinfo 0.40 起构造器返回 Result，但仅在系统 API 异常时失败，
    /// 这里保持不可失败签名，出错即 panic（应用启动期即可暴露）。
    pub fn new() -> Self {
        Self {
            sys: System::new().expect("sysinfo System 初始化失败"),
            nets: Networks::new_with_refreshed_list().expect("sysinfo Networks 初始化失败"),
            disks: Disks::new_with_refreshed_list().expect("sysinfo Disks 初始化失败"),
            last_cpu: None,
            last_net: None,
        }
    }

    /// 采样一次。首次调用时 CPU 使用率为 0、网络速率为 0（差值需要两个采样点）。
    pub fn sample(&mut self) -> SystemSample {
        let cpu = self.sample_cpu();
        let memory = self.sample_memory();
        let network = self.sample_network();
        let volumes = self.sample_volumes();
        SystemSample {
            cpu,
            memory,
            network,
            volumes,
        }
    }

    fn sample_cpu(&mut self) -> CpuSample {
        // 相邻两次 refresh_cpu_usage 间隔不得小于 MINIMUM_CPU_UPDATE_INTERVAL，
        // 否则使用率无意义；不足时补齐等待（前端轮询间隔 ≥1s，通常不会触发）
        let since_last = self
            .last_cpu
            .map(|t| t.elapsed())
            .unwrap_or(MINIMUM_CPU_UPDATE_INTERVAL);
        if since_last < MINIMUM_CPU_UPDATE_INTERVAL {
            std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL - since_last);
        }
        self.sys.refresh_cpu_usage();
        self.last_cpu = Some(Instant::now());
        CpuSample {
            usage: clamp_percent(self.sys.global_cpu_usage()),
            cores: self
                .sys
                .cpus()
                .iter()
                .map(|cpu| clamp_percent(cpu.usage()))
                .collect(),
        }
    }

    fn sample_memory(&mut self) -> MemorySample {
        self.sys.refresh_memory();
        MemorySample {
            used: self.sys.used_memory(),
            total: self.sys.total_memory(),
        }
    }

    fn sample_network(&mut self) -> NetworkSample {
        let elapsed = self.last_net.map(|t| t.elapsed());
        self.nets.refresh(true);
        self.last_net = Some(Instant::now());
        let mut sample = NetworkSample::default();
        for (name, data) in self.nets.iter() {
            if is_loopback(&name.to_string_lossy()) {
                continue;
            }
            sample.total_down += data.total_received();
            sample.total_up += data.total_transmitted();
            // sysinfo 只给两次刷新间的字节数，速率需除以实际间隔
            if let Some(dt) = elapsed {
                let secs = dt.as_secs_f64();
                if secs > 0.0 {
                    sample.down_rate += (data.received() as f64 / secs).round() as u64;
                    sample.up_rate += (data.transmitted() as f64 / secs).round() as u64;
                }
            }
        }
        sample
    }

    fn sample_volumes(&mut self) -> Vec<VolumeSample> {
        // 重新扫描挂载点（true=清除已卸载的卷、纳入新挂载的卷）
        self.disks.refresh(true);
        let mut volumes: Vec<VolumeSample> = self
            .disks
            .iter()
            .map(|disk| VolumeSample {
                name: disk.name().to_string_lossy().into_owned(),
                mount: disk.mount_point().display().to_string(),
                total: disk.total_space(),
                available: disk.available_space(),
            })
            .collect();
        volumes.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.mount.cmp(&b.mount)));
        volumes
    }
}

/// 两次采样之间的最小间隔，测试用。
pub const MIN_SAMPLE_INTERVAL: Duration = MINIMUM_CPU_UPDATE_INTERVAL;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_reports_memory_and_volumes() {
        let mut monitor = Monitor::new();
        let sample = monitor.sample();
        assert!(sample.memory.total > 0, "内存总量应大于 0");
        assert!(sample.memory.used <= sample.memory.total, "已用内存不应超过总量");
        assert!(!sample.volumes.is_empty(), "至少应有一个磁盘卷");
        assert!(
            sample
                .volumes
                .iter()
                .all(|v| v.total > 0 && v.available <= v.total),
            "卷空间应满足 0 < available <= total"
        );
    }

    #[test]
    fn second_sample_reports_cpu_within_range() {
        let mut monitor = Monitor::new();
        let _ = monitor.sample();
        std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
        let sample = monitor.sample();
        assert!((0.0..=100.0).contains(&sample.cpu.usage));
        assert!(!sample.cpu.cores.is_empty(), "应能读到核心数");
        assert!(
            sample.cpu.cores.iter().all(|c| (0.0..=100.0).contains(c)),
            "每核使用率应在 0-100"
        );
    }

    #[test]
    fn consecutive_samples_sleep_to_minimum_interval() {
        let mut monitor = Monitor::new();
        let _ = monitor.sample();
        // 上一次 CPU 刷新刚发生，本次采样应自动补齐等待而不产生异常值
        let sample = monitor.sample();
        assert!((0.0..=100.0).contains(&sample.cpu.usage));
    }

    #[test]
    fn network_totals_accumulate() {
        let mut monitor = Monitor::new();
        let first = monitor.sample();
        std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
        let second = monitor.sample();
        assert!(second.network.total_down >= first.network.total_down);
        assert!(second.network.total_up >= first.network.total_up);
    }

    #[test]
    fn loopback_detection() {
        assert!(is_loopback("lo"));
        assert!(is_loopback("lo0"));
        assert!(is_loopback("Loopback Pseudo-Interface 1"));
        assert!(!is_loopback("en0"));
        assert!(!is_loopback("eth0"));
        assert!(!is_loopback("wlan0"));
    }
}

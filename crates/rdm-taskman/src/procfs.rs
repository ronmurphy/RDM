//! Minimal /proc reader: processes, cpu totals, memory, uptime, loadavg,
//! per-core CPU, disk I/O, network I/O, and mount usage.

use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct ProcInfo {
    pub pid: i32,
    pub name: String,
    pub state: char,
    #[allow(dead_code)]
    pub uid: u32,
    pub user: String,
    pub threads: u32,
    pub rss_kb: u64,
    pub cpu_ticks: u64, // utime + stime from /proc/pid/stat
    pub cpu_percent: f32, // populated by sampler
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CpuTotals {
    pub total: u64,
    pub idle: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MemInfo {
    pub mem_total_kb: u64,
    pub mem_avail_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
}

pub fn read_cpu_totals() -> CpuTotals {
    let Ok(s) = fs::read_to_string("/proc/stat") else {
        return CpuTotals::default();
    };
    let Some(line) = s.lines().next() else {
        return CpuTotals::default();
    };
    parse_cpu_line(line)
}

pub fn read_cpu_per_core() -> Vec<CpuTotals> {
    let Ok(s) = fs::read_to_string("/proc/stat") else {
        return Vec::new();
    };
    s.lines()
        .skip(1) // first line is "cpu" aggregate
        .take_while(|l| l.starts_with("cpu"))
        .map(parse_cpu_line)
        .collect()
}

fn parse_cpu_line(line: &str) -> CpuTotals {
    // cpu[N]  user nice system idle iowait irq softirq steal ...
    let parts: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|p| p.parse().ok())
        .collect();
    if parts.len() < 4 {
        return CpuTotals::default();
    }
    let total: u64 = parts.iter().sum();
    let idle = parts[3] + parts.get(4).copied().unwrap_or(0);
    CpuTotals { total, idle }
}

pub fn cpu_percent(prev: CpuTotals, curr: CpuTotals) -> f32 {
    let total_d = curr.total.saturating_sub(prev.total);
    let idle_d = curr.idle.saturating_sub(prev.idle);
    if total_d == 0 {
        0.0
    } else {
        ((1.0 - idle_d as f32 / total_d as f32) * 100.0).clamp(0.0, 100.0)
    }
}

pub fn read_meminfo() -> MemInfo {
    let mut mi = MemInfo::default();
    let Ok(s) = fs::read_to_string("/proc/meminfo") else {
        return mi;
    };
    for line in s.lines() {
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        let Some(val) = it.next() else { continue };
        let Ok(v) = val.parse::<u64>() else { continue };
        match key {
            "MemTotal:" => mi.mem_total_kb = v,
            "MemAvailable:" => mi.mem_avail_kb = v,
            "SwapTotal:" => mi.swap_total_kb = v,
            "SwapFree:" => mi.swap_free_kb = v,
            _ => {}
        }
    }
    mi
}

pub fn read_uptime_secs() -> u64 {
    fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(String::from))
        .and_then(|s| s.parse::<f64>().ok())
        .map(|f| f as u64)
        .unwrap_or(0)
}

pub fn read_loadavg() -> String {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .map(|s| {
            s.split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|| "—".into())
}

pub fn read_hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

pub fn read_kernel() -> String {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

pub fn read_processes(user_cache: &mut HashMap<u32, String>) -> Vec<ProcInfo> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return out;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name_str) = name.to_str() else { continue };
        let Ok(pid) = name_str.parse::<i32>() else { continue };
        if let Some(info) = read_proc(pid, user_cache) {
            out.push(info);
        }
    }
    out
}

fn read_proc(pid: i32, user_cache: &mut HashMap<u32, String>) -> Option<ProcInfo> {
    let status_path = format!("/proc/{pid}/status");
    let status = fs::read_to_string(&status_path).ok()?;

    let mut name = String::new();
    let mut state = '?';
    let mut uid: u32 = 0;
    let mut threads: u32 = 1;
    let mut rss_kb: u64 = 0;
    for line in status.lines() {
        if let Some(v) = line.strip_prefix("Name:") {
            name = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("State:") {
            state = v.trim().chars().next().unwrap_or('?');
        } else if let Some(v) = line.strip_prefix("Uid:") {
            uid = v.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("Threads:") {
            threads = v.trim().parse().unwrap_or(1);
        } else if let Some(v) = line.strip_prefix("VmRSS:") {
            rss_kb = v.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }

    let cpu_ticks = read_pid_cpu_ticks(pid).unwrap_or(0);
    let user = resolve_user(uid, user_cache);

    Some(ProcInfo {
        pid,
        name,
        state,
        uid,
        user,
        threads,
        rss_kb,
        cpu_ticks,
        cpu_percent: 0.0,
    })
}

fn read_pid_cpu_ticks(pid: i32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm may contain spaces and parens; find last ')'
    let close = stat.rfind(')')?;
    let rest = &stat[close + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After comm+state, field indices shift: fields[0] is state, fields[11] = utime, fields[12] = stime
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(utime + stime)
}

fn resolve_user(uid: u32, cache: &mut HashMap<u32, String>) -> String {
    if let Some(s) = cache.get(&uid) {
        return s.clone();
    }
    let name = lookup_user_from_passwd(uid).unwrap_or_else(|| uid.to_string());
    cache.insert(uid, name.clone());
    name
}

fn lookup_user_from_passwd(uid: u32) -> Option<String> {
    let s = fs::read_to_string("/etc/passwd").ok()?;
    for line in s.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 3 {
            if let Ok(u) = parts[2].parse::<u32>() {
                if u == uid {
                    return Some(parts[0].to_string());
                }
            }
        }
    }
    None
}

pub fn state_label(c: char) -> &'static str {
    match c {
        'R' => "Running",
        'S' => "Sleeping",
        'D' => "Disk",
        'Z' => "Zombie",
        'T' => "Stopped",
        't' => "Tracing",
        'I' => "Idle",
        'X' | 'x' => "Dead",
        _ => "?",
    }
}

pub fn format_uptime(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h {mins}m")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
}

pub fn format_kb_as_gb(kb: u64) -> String {
    let gb = kb as f64 / 1024.0 / 1024.0;
    format!("{gb:.2} GB")
}

pub fn format_kb_as_mb(kb: u64) -> String {
    let mb = kb as f64 / 1024.0;
    if mb >= 1024.0 {
        format!("{:.2} GB", mb / 1024.0)
    } else {
        format!("{mb:.1} MB")
    }
}

pub fn clock_ticks_per_sec() -> u64 {
    // SC_CLK_TCK
    let v = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if v > 0 { v as u64 } else { 100 }
}

// Silence unused warning for helpers only called from layout_app.
#[allow(dead_code)]
pub fn proc_exists(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

// ── Disk I/O ───────────────────────────────────────────────────────────────

const DISK_SECTOR_SIZE: u64 = 512;

#[derive(Clone, Debug, Default)]
pub struct DiskIo {
    pub device: String,
    pub read_bytes: u64,
    pub write_bytes: u64,
}

/// Read /proc/diskstats, skipping loop/ram/zram pseudo devices.
pub fn read_disk_io() -> Vec<DiskIo> {
    let Ok(s) = fs::read_to_string("/proc/diskstats") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in s.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 14 {
            continue;
        }
        let device = fields[2].to_string();
        if device.starts_with("loop")
            || device.starts_with("ram")
            || device.starts_with("zram")
            || device.starts_with("sr")
        {
            continue;
        }
        let sectors_read: u64 = fields[5].parse().unwrap_or(0);
        let sectors_written: u64 = fields[9].parse().unwrap_or(0);
        out.push(DiskIo {
            device,
            read_bytes: sectors_read * DISK_SECTOR_SIZE,
            write_bytes: sectors_written * DISK_SECTOR_SIZE,
        });
    }
    out
}

// ── Disk usage (mounts + statvfs) ──────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct MountUsage {
    pub device: String,
    pub mount_point: String,
    pub fs_type: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

/// Read /proc/mounts and statvfs() each real block-device mount.
pub fn read_mount_usage() -> Vec<MountUsage> {
    let Ok(s) = fs::read_to_string("/proc/mounts") else {
        return Vec::new();
    };
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in s.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 {
            continue;
        }
        let device = fields[0];
        let mount_point = fields[1];
        let fs_type = fields[2];

        // Keep only real backing devices: /dev/* — this filters out
        // proc, sysfs, tmpfs, cgroup, overlay, etc.
        if !device.starts_with("/dev/") {
            continue;
        }
        // Some devices appear bind-mounted in multiple places; dedupe by device.
        if !seen.insert(device.to_string()) {
            continue;
        }

        if let Some((total, used, free)) = statvfs_bytes(mount_point) {
            if total == 0 {
                continue;
            }
            out.push(MountUsage {
                device: device.to_string(),
                mount_point: mount_point.to_string(),
                fs_type: fs_type.to_string(),
                total_bytes: total,
                used_bytes: used,
                free_bytes: free,
            });
        }
    }
    out
}

fn statvfs_bytes(path: &str) -> Option<(u64, u64, u64)> {
    let cpath = CString::new(path).ok()?;
    let mut buf: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(cpath.as_ptr(), &mut buf) };
    if rc != 0 {
        return None;
    }
    let frsize = buf.f_frsize as u64;
    let total = buf.f_blocks as u64 * frsize;
    let free = buf.f_bavail as u64 * frsize;
    let used = total.saturating_sub(free);
    Some((total, used, free))
}

// ── Network I/O ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct NetIo {
    pub iface: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// Read /proc/net/dev.
pub fn read_net_io() -> Vec<NetIo> {
    let Ok(s) = fs::read_to_string("/proc/net/dev") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in s.lines().skip(2) {
        // "  iface: rx_bytes rx_packets ... tx_bytes tx_packets ..."
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let iface = name.trim().to_string();
        if iface.is_empty() {
            continue;
        }
        let fields: Vec<u64> = rest
            .split_whitespace()
            .map(|f| f.parse::<u64>().unwrap_or(0))
            .collect();
        if fields.len() < 9 {
            continue;
        }
        out.push(NetIo {
            iface,
            rx_bytes: fields[0],
            tx_bytes: fields[8],
        });
    }
    out
}

// ── Byte-rate formatting ───────────────────────────────────────────────────

pub fn format_bytes(bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K {
        format!("{bytes} B")
    } else if b < K * K {
        format!("{:.1} KB", b / K)
    } else if b < K * K * K {
        format!("{:.1} MB", b / (K * K))
    } else if b < K * K * K * K {
        format!("{:.2} GB", b / (K * K * K))
    } else {
        format!("{:.2} TB", b / (K * K * K * K))
    }
}

pub fn format_rate(bytes_per_sec: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec))
}

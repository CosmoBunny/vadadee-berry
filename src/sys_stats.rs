use std::fs;
use std::time::{Instant, Duration};

#[derive(Debug, Clone)]
pub struct SysStats {
    last_cpu_ticks: (u64, u64), // (idle, total)
    pub cpu_usage: f32,
    pub cpu_temp: f32,
    pub ram_rss_mb: f32,
    pub ram_sys_used_gb: f32,
    pub ram_sys_total_gb: f32,
    pub gpu_usage: f32,
}

impl Default for SysStats {
    fn default() -> Self {
        Self::new()
    }
}

impl SysStats {
    pub fn new() -> Self {
        let mut s = Self {
            last_cpu_ticks: (0, 0),
            cpu_usage: 0.0,
            cpu_temp: 45.0,
            ram_rss_mb: 0.0,
            ram_sys_used_gb: 0.0,
            ram_sys_total_gb: 8.0,
            gpu_usage: 0.0,
        };
        s.update();
        s
    }

    pub fn update(&mut self) {
        #[cfg(target_os = "linux")]
        {
            // Update CPU usage from /proc/stat
            if let Ok(stat) = fs::read_to_string("/proc/stat") {
                if let Some(line) = stat.lines().next() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 5 {
                        let user: u64 = parts[1].parse().unwrap_or(0);
                        let nice: u64 = parts[2].parse().unwrap_or(0);
                        let system: u64 = parts[3].parse().unwrap_or(0);
                        let idle: u64 = parts[4].parse().unwrap_or(0);
                        let iowait: u64 = parts[5].parse().unwrap_or(0);
                        let irq: u64 = parts[6].parse().unwrap_or(0);
                        let softirq: u64 = parts[7].parse().unwrap_or(0);
                        let steal: u64 = parts[8].parse().unwrap_or(0);

                        let idle_ticks = idle + iowait;
                        let total_ticks = user + nice + system + idle_ticks + irq + softirq + steal;

                        let prev_idle = self.last_cpu_ticks.0;
                        let prev_total = self.last_cpu_ticks.1;

                        let diff_idle = idle_ticks.saturating_sub(prev_idle) as f64;
                        let diff_total = total_ticks.saturating_sub(prev_total) as f64;

                        if diff_total > 0.0 {
                            self.cpu_usage = (100.0 * (1.0 - diff_idle / diff_total)) as f32;
                        }
                        self.last_cpu_ticks = (idle_ticks, total_ticks);
                    }
                }
            }

            // Update CPU temperature
            let mut max_temp = 0.0f32;
            if let Ok(entries) = fs::read_dir("/sys/class/thermal") {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.file_name().and_then(|s| s.to_str()).map_or(false, |s| s.starts_with("thermal_zone")) {
                        if let Ok(temp_str) = fs::read_to_string(path.join("temp")) {
                            if let Ok(temp_val) = temp_str.trim().parse::<i32>() {
                                let temp_c = temp_val as f32 / 1000.0;
                                if temp_c > max_temp {
                                    max_temp = temp_c;
                                }
                            }
                        }
                    }
                }
            }
            if max_temp > 0.0 {
                self.cpu_temp = max_temp;
            } else {
                if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if let Ok(hw_entries) = fs::read_dir(&path) {
                            for hw_entry in hw_entries.flatten() {
                                let hw_path = hw_entry.path();
                                if hw_path.file_name().and_then(|s| s.to_str()).map_or(false, |s| s.starts_with("temp") && s.ends_with("_input")) {
                                    if let Ok(temp_str) = fs::read_to_string(hw_path) {
                                        if let Ok(temp_val) = temp_str.trim().parse::<i32>() {
                                            let temp_c = temp_val as f32 / 1000.0;
                                            if temp_c > max_temp {
                                                max_temp = temp_c;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if max_temp > 0.0 {
                    self.cpu_temp = max_temp;
                }
            }

            // Update RAM RSS
            if let Ok(status) = fs::read_to_string("/proc/self/status") {
                for line in status.lines() {
                    if line.starts_with("VmRSS:") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            if let Ok(kb) = parts[1].parse::<f32>() {
                                self.ram_rss_mb = kb / 1024.0;
                            }
                        }
                        break;
                    }
                }
            }

            // Update System RAM
            if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
                let mut total_kb = 0.0f32;
                let mut avail_kb = 0.0f32;
                for line in meminfo.lines() {
                    if line.starts_with("MemTotal:") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            total_kb = parts[1].parse().unwrap_or(0.0);
                        }
                    } else if line.starts_with("MemAvailable:") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            avail_kb = parts[1].parse().unwrap_or(0.0);
                        }
                    }
                }
                if total_kb > 0.0 {
                    self.ram_sys_total_gb = total_kb / (1024.0 * 1024.0);
                    self.ram_sys_used_gb = (total_kb - avail_kb) / (1024.0 * 1024.0);
                }
            }

            // Update GPU usage
            let mut gpu_busy = -1;
            if let Ok(val_str) = fs::read_to_string("/sys/class/drm/card0/device/gpu_busy_percent") {
                if let Ok(val) = val_str.trim().parse::<i32>() {
                    gpu_busy = val;
                }
            }
            if gpu_busy >= 0 {
                self.gpu_usage = gpu_busy as f32;
            } else {
                self.gpu_usage = (self.cpu_usage * 0.6).min(100.0);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JokePlatform {
    /// Rule applies on every platform (no prefix in the .txt file).
    All,
    /// Rule applies only when running on Android.
    Mobile,
    /// Rule applies only on desktop (Linux / Windows / macOS).
    Desktop,
}

#[derive(Debug, Clone)]
pub struct JokeRule {
    pub platform: JokePlatform,
    pub condition: String,
    pub message: String,
}

/// Parse a joke file.
///
/// Condition header format (all case-insensitive):
///   `[CONDITION]`           — applies on all platforms
///   `[MOBILE CONDITION]`    — applies only on Android
///   `[DESKTOP CONDITION]`   — applies only on desktop
///
/// Each non-empty body line under a header is a **separate joke** that can be
/// chosen independently at random — stack as many lines as you like under one
/// condition and the engine will pick one at random each time.
///
/// An empty body (no lines) means "stay silent" when the condition matches.
/// Lines starting with `#` are treated as comments and ignored.
pub fn parse_jokes(content: &str) -> Vec<JokeRule> {
    let mut rules = Vec::new();
    let mut current_platform = JokePlatform::All;
    let mut current_cond: Option<String> = None;
    // Track whether we saw *any* body line for the current header so we can
    // emit a silent (empty-message) sentinel rule when there are none.
    let mut saw_body = false;

    for line in content.lines() {
        let line = line.trim();
        // Skip comment lines
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            // Flush previous header if it had no body lines → silent sentinel
            if let Some(ref cond) = current_cond {
                if !saw_body {
                    rules.push(JokeRule {
                        platform: current_platform.clone(),
                        condition: cond.clone(),
                        message: String::new(),
                    });
                }
            }
            current_cond = None;
            saw_body = false;
            current_platform = JokePlatform::All;

            let inner = line[1..line.len() - 1].trim();
            let upper = inner.to_ascii_uppercase();
            let (platform, rest) = if upper.starts_with("MOBILE ") {
                (JokePlatform::Mobile, inner[7..].trim())
            } else if upper.starts_with("DESKTOP ") {
                (JokePlatform::Desktop, inner[8..].trim())
            } else {
                (JokePlatform::All, inner)
            };
            current_platform = platform;
            current_cond = Some(rest.to_string());
        } else if let Some(ref cond) = current_cond {
            if line.is_empty() {
                // Blank lines between jokes within a block are ignored
                continue;
            }
            // Each non-empty line → its own rule with the same condition
            rules.push(JokeRule {
                platform: current_platform.clone(),
                condition: cond.clone(),
                message: line.to_string(),
            });
            saw_body = true;
        }
    }
    // Flush the last header
    if let Some(ref cond) = current_cond {
        if !saw_body {
            rules.push(JokeRule {
                platform: current_platform.clone(),
                condition: cond.clone(),
                message: String::new(),
            });
        }
    }
    rules
}

pub fn evaluate_condition(cond: &str, cpu_usage: f32, ram_sys_used_gb: f32, sec_per_frame: f32, cpu_temp: f32) -> bool {
    let parts: Vec<&str> = cond.split_whitespace().collect();
    if parts.len() == 1 && parts[0].to_ascii_uppercase() == "DEFAULT" {
        return true;
    }
    if parts.len() != 3 {
        return false;
    }
    let variable = parts[0].to_ascii_uppercase();
    let operator = parts[1];
    let value: f32 = parts[2].parse().unwrap_or(0.0);

    let current_val = match variable.as_str() {
        "CPU" => cpu_usage,
        "RAM" => ram_sys_used_gb,
        "SEC_PER_FRAME" => sec_per_frame,
        "CPU_TEMP" => cpu_temp,
        _ => return false,
    };

    match operator {
        ">" => current_val > value,
        "<" => current_val < value,
        ">=" => current_val >= value,
        "<=" => current_val <= value,
        "==" => current_val == value,
        _ => false,
    }
}

pub fn choose_joke(
    rules: &[JokeRule],
    cpu_usage: f32,
    ram_sys_used_gb: f32,
    sec_per_frame: f32,
    cpu_temp: f32,
    seed: u64,
    is_mobile: bool,
) -> String {
    let mut matching_non_default = Vec::new();
    let mut matching_default = Vec::new();

    for rule in rules {
        // Platform filter
        let platform_ok = match &rule.platform {
            JokePlatform::All => true,
            JokePlatform::Mobile => is_mobile,
            JokePlatform::Desktop => !is_mobile,
        };
        if !platform_ok {
            continue;
        }

        if !evaluate_condition(&rule.condition, cpu_usage, ram_sys_used_gb, sec_per_frame, cpu_temp) {
            continue;
        }

        if rule.condition.to_ascii_uppercase() == "DEFAULT" {
            matching_default.push(rule.message.as_str());
        } else {
            matching_non_default.push(rule.message.as_str());
        }
    }

    // Pick from most-specific matches first, fall back to DEFAULT.
    // Empty message means "stay silent" — return empty string.
    if !matching_non_default.is_empty() {
        let idx = pseudo_random(seed, matching_non_default.len());
        return matching_non_default[idx].to_string();
    }
    if !matching_default.is_empty() {
        let idx = pseudo_random(seed, matching_default.len());
        return matching_default[idx].to_string();
    }
    String::new()
}

fn pseudo_random(seed: u64, max: usize) -> usize {
    if max <= 1 {
        return 0;
    }
    let a: u64 = 6364136223846793005;
    let c: u64 = 1442695040888963407;
    let next_seed = seed.wrapping_mul(a).wrapping_add(c);
    (next_seed as usize) % max
}

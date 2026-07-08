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
/// Condition header format (all case-insensitive, Rust-like range bounds):
///   `[CPU 80..]`            — CPU usage >= 80
///   `[CPU ..2]`             — CPU usage < 2
///   `[CPU_TEMP 80..]`       — CPU temp >= 80
///   `[SEC_PER_FRAME 0.1..=1]` — sec per frame in [0.1, 1] (roughly 1..=10 fps)
///   `[RAM 4..16]`           — RAM in [4, 16)
///   `[DEFAULT]`             — always matches (fallback)
///   `[MOBILE CPU 80..]`     — only on Android
///   `[DESKTOP CPU 80..]`    — only on desktop
///
/// Each non-empty body line under a header is a **separate joke** that can be
/// chosen independently. The engine cycles through matching conditions in a
/// defined order (CPU, RAM, SEC_PER_FRAME, CPU_TEMP, DEFAULT...) then picks
/// within the group.
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
    let cond = cond.trim();
    if cond.eq_ignore_ascii_case("DEFAULT") {
        return true;
    }
    let upper = cond.to_ascii_uppercase();
    let tokens: Vec<&str> = upper.split_whitespace().collect();
    if tokens.len() < 2 {
        return false;
    }
    let variable = tokens[0];
    // Join remaining as range e.g. "80.." or "0.1..=1" or "..40"
    let range_str: String = tokens[1..].join("");

    let current_val = match variable {
        "CPU" => cpu_usage,
        "RAM" => ram_sys_used_gb,
        "SEC_PER_FRAME" => sec_per_frame,
        "CPU_TEMP" => cpu_temp,
        _ => return false,
    };

    evaluate_range(current_val, &range_str)
}

fn evaluate_range(val: f32, r: &str) -> bool {
    if r.is_empty() {
        return true;
    }
    let has_eq = r.contains("..=");
    let sep = if has_eq { "..=" } else { ".." };
    let parts: Vec<&str> = r.split(sep).collect();
    let low_str = if !parts.is_empty() { parts[0] } else { "" };
    let high_str = if parts.len() > 1 { parts[1] } else { "" };

    let low = if !low_str.is_empty() {
        low_str.parse::<f32>().ok()
    } else {
        None
    };
    let high = if !high_str.is_empty() {
        high_str.parse::<f32>().ok()
    } else {
        None
    };

    if let Some(l) = low {
        if val < l {
            return false;
        }
    }
    if let Some(h) = high {
        if has_eq {
            if val > h {
                return false;
            }
        } else {
            if val >= h {
                return false;
            }
        }
    }
    true
}

pub fn choose_joke(
    rules: &[JokeRule],
    cpu_usage: f32,
    ram_sys_used_gb: f32,
    sec_per_frame: f32,
    cpu_temp: f32,
    is_mobile: bool,
    cycle_index: usize,
) -> String {
    // Collect only jokes whose condition is currently true (so low-temp
    // Antarctica jokes won't appear when CPU_TEMP==80, etc.).
    // Then group the *matching* ones by category and cycle sequentially.
    use std::collections::BTreeMap;

    let mut matching_by_cat: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut cat_order: Vec<String> = Vec::new();

    for rule in rules {
        let platform_ok = match &rule.platform {
            JokePlatform::All => true,
            JokePlatform::Mobile => is_mobile,
            JokePlatform::Desktop => !is_mobile,
        };
        if !platform_ok || rule.message.is_empty() {
            continue;
        }
        if !evaluate_condition(&rule.condition, cpu_usage, ram_sys_used_gb, sec_per_frame, cpu_temp) {
            continue;
        }

        let cat = rule.condition.split_whitespace().next()
            .unwrap_or("DEFAULT")
            .to_ascii_uppercase();

        if !matching_by_cat.contains_key(&cat) {
            cat_order.push(cat.clone());
        }
        matching_by_cat.entry(cat).or_default().push(rule.message.as_str());
    }

    if matching_by_cat.is_empty() {
        // Nothing specific matched — fall back to any DEFAULTs (they always match)
        let defaults: Vec<&str> = rules.iter()
            .filter(|r| {
                let p_ok = match &r.platform {
                    JokePlatform::All => true,
                    JokePlatform::Mobile => is_mobile,
                    JokePlatform::Desktop => !is_mobile,
                };
                p_ok && r.condition.eq_ignore_ascii_case("DEFAULT") && !r.message.is_empty()
            })
            .map(|r| r.message.as_str())
            .collect();
        if defaults.is_empty() {
            return String::new();
        }
        let idx = cycle_index % defaults.len();
        return defaults[idx].to_string();
    }

    // Preferred presentation order among whatever is currently relevant
    let preferred = ["CPU", "RAM", "SEC_PER_FRAME", "CPU_TEMP", "DEFAULT"];
    let mut ordered: Vec<String> = Vec::new();
    for p in &preferred {
        if matching_by_cat.contains_key(*p) && !ordered.contains(&p.to_string()) {
            ordered.push(p.to_string());
        }
    }
    for c in &cat_order {
        if !ordered.contains(c) {
            ordered.push(c.clone());
        }
    }

    if ordered.is_empty() {
        return String::new();
    }

    let cat_idx = cycle_index % ordered.len();
    let cat = &ordered[cat_idx];
    let msgs = &matching_by_cat[cat];

    let j_idx = (cycle_index / ordered.len().max(1)) % msgs.len();
    msgs[j_idx].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joke_cycling_sequence_demo() {
        let content = std::fs::read_to_string("jokes_export.txt")
            .unwrap_or_else(|_| {
                // embedded fallback subset for CI
                r#"[CPU 80..]
CPU working hard.
[CPU ..2]
CPU sleeping.
[CPU_TEMP 80..]
CPU toasty.
[RAM 16..]
RAM full.
[RAM ..4]
RAM low.
[SEC_PER_FRAME 0.1..=1]
Mediocre fps range.
[DEFAULT]
Default joke here.
"#.to_string()
            });
        let rules = parse_jokes(&content);

        println!("\n=== Joke sequence (desktop, cycle 0..12) ===");
        // Use values that trigger several conditions (high CPU/temp, medium RAM, ~2-5 fps)
        for i in 0..12 {
            let joke = choose_joke(&rules, 85.0, 12.0, 0.3, 82.0, false, i);
            println!("  [{}] -> {}", i, joke);
        }

        println!("\n=== Joke sequence (mobile) ===");
        for i in 0..6 {
            let joke = choose_joke(&rules, 75.0, 4.0, 0.5, 60.0, true, i);
            println!("  [{}] -> {}", i, joke);
        }
    }

    #[test]
    fn evaluate_range_bounds_work() {
        // exact 80 should match high range, not low
        assert!(evaluate_condition("CPU_TEMP 80..", 0., 0., 0., 80.));
        assert!(!evaluate_condition("CPU_TEMP ..40", 0., 0., 0., 80.));
        // fps 1..=10 range (sec 0.1..=1)
        assert!(evaluate_condition("SEC_PER_FRAME 0.1..=1", 0., 0., 0.5, 0.));
        assert!(!evaluate_condition("SEC_PER_FRAME 0.1..=1", 0., 0., 2.0, 0.));
        assert!(evaluate_condition("CPU 80..", 85., 0., 0., 0.));
        assert!(evaluate_condition("CPU ..2", 1., 0., 0., 0.));
    }
}


use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use sysinfo::{Process, System};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppDetail {
    pub name: String,
    pub exe_path: String,
    #[serde(default)]
    pub cmdline: String,
    pub process_count: usize,
    pub ram_mb: u64,
    pub ram_pct: f32,
    pub cpu_pct: f32,
}

struct AppAcc {
    exe_path: String,
    cmdline: String,
    process_count: usize,
    ram_mb: u64,
    cpu_pct: f32,
}

pub fn is_interpreter(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = lower.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-');
    matches!(
        base,
        "python"
            | "pypy"
            | "node"
            | "nodejs"
            | "deno"
            | "bun"
            | "ruby"
            | "perl"
            | "bash"
            | "sh"
            | "zsh"
            | "dash"
            | "fish"
            | "lua"
            | "luajit"
            | "electron"
    )
}

pub fn extract_script_name(cmd: &[String]) -> Option<String> {
    if cmd.len() <= 1 {
        return None;
    }
    let mut iter = cmd.iter().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "-m" {
            if let Some(module) = iter.next() {
                return Some(module.clone());
            }
        } else if arg.starts_with('-') {
            continue;
        } else {
            let file_name = Path::new(arg)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| arg.clone());
            if !file_name.is_empty() {
                return Some(file_name);
            }
        }
    }
    None
}

pub fn resolve_process_identity(p: &Process) -> (String, String, String) {
    let raw_exe_path = p
        .exe()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();

    let exe_file_name = p
        .exe()
        .and_then(|e| e.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_default();

    let proc_name = p.name().to_string();
    let cmd = p.cmd();
    let cmdline = cmd.join(" ");

    let exe_clean = if exe_file_name.is_empty() {
        proc_name.clone()
    } else {
        exe_file_name
    };

    let resolved_name = if is_interpreter(&exe_clean) {
        if !proc_name.is_empty() && !is_interpreter(&proc_name) {
            proc_name
        } else if let Some(script) = extract_script_name(cmd) {
            script
        } else {
            exe_clean
        }
    } else {
        exe_clean
    };

    (resolved_name, raw_exe_path, cmdline)
}

pub fn get_proc_status_info(pid: u32) -> Option<(u32, u32, u64)> {
    let status_path = format!("/proc/{pid}/status");
    let content = fs::read_to_string(status_path).ok()?;
    let mut proc_pid = None;
    let mut proc_tgid = None;
    let mut rss_kb = 0u64;

    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("Pid:") {
            proc_pid = rest.split_whitespace().next().and_then(|v| v.parse().ok());
        } else if let Some(rest) = line.strip_prefix("Tgid:") {
            proc_tgid = rest.split_whitespace().next().and_then(|v| v.parse().ok());
        } else if let Some(rest) = line.strip_prefix("VmRSS:") {
            rss_kb = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or_default();
        }
    }

    match (proc_pid, proc_tgid) {
        (Some(pid_val), Some(tgid_val)) => Some((pid_val, tgid_val, rss_kb)),
        _ => None,
    }
}

pub fn collect_app_details(sys: &System, total_mem: u64) -> (Vec<AppDetail>, Vec<String>) {
    let mut app_map: BTreeMap<String, AppAcc> = BTreeMap::new();
    let mut seen_tgids = HashSet::new();

    for (pid, p) in sys.processes() {
        if p.cmd().is_empty() {
            continue;
        }

        let Some((proc_pid, proc_tgid, rss_kb)) = get_proc_status_info(pid.as_u32()) else {
            continue;
        };

        if proc_pid != proc_tgid || seen_tgids.contains(&proc_tgid) {
            continue;
        }
        seen_tgids.insert(proc_tgid);

        let (name, raw_exe_path, cmdline) = resolve_process_identity(p);
        let mem_mb = rss_kb / 1024;
        let proc_cpu = p.cpu_usage();

        let entry = app_map.entry(name).or_insert(AppAcc {
            exe_path: raw_exe_path.clone(),
            cmdline: cmdline.clone(),
            process_count: 0,
            ram_mb: 0,
            cpu_pct: 0.0,
        });

        entry.process_count += 1;
        entry.ram_mb += mem_mb;
        entry.cpu_pct += proc_cpu;
        if entry.exe_path.is_empty() && !raw_exe_path.is_empty() {
            entry.exe_path = raw_exe_path;
        }
        if entry.cmdline.is_empty() && !cmdline.is_empty() {
            entry.cmdline = cmdline;
        }
    }

    let mut sorted_apps: Vec<(String, AppAcc)> = app_map.into_iter().collect();
    sorted_apps.sort_by_key(|(_, acc)| std::cmp::Reverse(acc.ram_mb));

    let cpu_count = sys.cpus().len().max(1) as f32;
    let app_details: Vec<AppDetail> = sorted_apps
        .into_iter()
        .map(|(name, acc)| {
            let app_ram_pct = if total_mem > 0 {
                (acc.ram_mb as f32 / total_mem as f32) * 100.0
            } else {
                0.0
            };
            AppDetail {
                name: name.clone(),
                exe_path: if acc.exe_path.is_empty() {
                    name
                } else {
                    acc.exe_path
                },
                cmdline: acc.cmdline,
                process_count: acc.process_count,
                ram_mb: acc.ram_mb,
                ram_pct: app_ram_pct,
                cpu_pct: (acc.cpu_pct / cpu_count).clamp(0.0, 100.0),
            }
        })
        .collect();

    let top_apps: Vec<String> = app_details
        .iter()
        .take(4)
        .map(|app| format!("{} ({} MB)", app.name, app.ram_mb))
        .collect();

    (app_details, top_apps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_interpreter() {
        assert!(is_interpreter("python"));
        assert!(is_interpreter("python3"));
        assert!(is_interpreter("python3.14"));
        assert!(is_interpreter("node"));
        assert!(is_interpreter("bash"));
        assert!(!is_interpreter("udiskie"));
        assert!(!is_interpreter("uwsm"));
        assert!(!is_interpreter("firefox"));
    }

    #[test]
    fn test_extract_script_name() {
        let cmd = vec![
            "/usr/bin/python".to_string(),
            "/usr/bin/udiskie".to_string(),
            "--automount".to_string(),
        ];
        assert_eq!(extract_script_name(&cmd), Some("udiskie".to_string()));

        let mod_cmd = vec![
            "python3".to_string(),
            "-m".to_string(),
            "http.server".to_string(),
        ];
        assert_eq!(
            extract_script_name(&mod_cmd),
            Some("http.server".to_string())
        );

        let flag_cmd = vec![
            "python3".to_string(),
            "-u".to_string(),
            "/path/to/worker.py".to_string(),
        ];
        assert_eq!(
            extract_script_name(&flag_cmd),
            Some("worker.py".to_string())
        );
    }

    #[test]
    fn test_collect_app_details_identifies_python_tools() {
        use sysinfo::ProcessRefreshKind;
        let mut sys = System::new_all();
        sys.refresh_processes_specifics(ProcessRefreshKind::everything());
        let (apps, _) = collect_app_details(&sys, 16000);
        // If udiskie or uwsm are running on this host, verify they are identified by tool name
        for app in &apps {
            if app.name == "udiskie" {
                assert!(app.cmdline.contains("udiskie"));
            }
            if app.name == "uwsm" {
                assert!(app.cmdline.contains("uwsm"));
            }
        }
    }
}

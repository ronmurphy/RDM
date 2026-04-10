//! systemd service listing via `systemctl` shell-out.
//!
//! We parse `systemctl list-units --type=service --all --no-legend --plain`
//! rather than pulling in a D-Bus dependency. Each line is:
//!     UNIT LOAD ACTIVE SUB DESCRIPTION...
//! where DESCRIPTION may contain spaces.

use std::process::Command;

#[derive(Clone, Debug, Default)]
pub struct ServiceUnit {
    pub name: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
}

pub fn list_services() -> Vec<ServiceUnit> {
    let out = match Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--all",
            "--no-legend",
            "--plain",
            "--no-pager",
        ])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        Ok(o) => {
            log::warn!(
                "systemctl list-units failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            return Vec::new();
        }
        Err(e) => {
            log::warn!("systemctl not available: {e}");
            return Vec::new();
        }
    };

    let text = String::from_utf8_lossy(&out);
    let mut units = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_start();
        if line.is_empty() {
            continue;
        }
        // Some entries are prefixed with "● " for failed units — strip it.
        let line = line.strip_prefix("● ").unwrap_or(line).trim_start();

        let mut parts = line.splitn(5, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_string();
        // splitn with whitespace yields empty strings between runs of spaces,
        // so walk manually instead.
        let rest = line[name.len()..].trim_start();
        let (load, rest) = take_word(rest);
        let (active, rest) = take_word(rest);
        let (sub, rest) = take_word(rest);
        let description = rest.trim().to_string();

        if name.is_empty() {
            continue;
        }
        units.push(ServiceUnit {
            name,
            load,
            active,
            sub,
            description,
        });
    }
    units
}

fn take_word(s: &str) -> (String, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (s[..i].to_string(), &s[i..]),
        None => (s.to_string(), ""),
    }
}

pub fn run_action(action: &str, unit: &str) -> Result<(), String> {
    // Privileged operations: use pkexec when available.
    let use_pkexec = Command::new("pkexec").arg("--version").output().is_ok();
    let (program, args): (&str, Vec<&str>) = if use_pkexec {
        ("pkexec", vec!["systemctl", action, unit])
    } else {
        ("systemctl", vec![action, unit])
    };
    let output = Command::new(program)
        .args(&args)
        .output()
        .map_err(|e| format!("failed to spawn {program}: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("systemctl {action} {unit} failed: {stderr}"));
    }
    Ok(())
}

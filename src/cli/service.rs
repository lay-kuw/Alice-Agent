//! Start/stop the server: launchd (macOS), systemd user unit (Linux), or a
//! detached child process with a PID file when no service is registered.

use super::client::{CliResult, Client};
use crate::config::Paths;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const LAUNCHD_LABEL: &str = "local.alice.agent";

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

pub fn launchd_plist() -> PathBuf {
    home().join("Library/LaunchAgents").join(format!("{LAUNCHD_LABEL}.plist"))
}

pub fn systemd_unit() -> PathBuf {
    home().join(".config/systemd/user/alice.service")
}

#[derive(PartialEq, Debug)]
pub enum Manager {
    Launchd,
    Systemd,
    None,
}

pub fn manager() -> Manager {
    if cfg!(target_os = "macos") && launchd_plist().exists() {
        Manager::Launchd
    } else if cfg!(target_os = "linux") && systemd_unit().exists() {
        Manager::Systemd
    } else {
        Manager::None
    }
}

fn uid() -> String {
    Command::new("id").arg("-u").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

fn run(cmd: &mut Command) -> CliResult<()> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Write and enable the service definition for this OS.
pub fn register(paths: &Paths) -> CliResult<String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let log = paths.log_file();
    paths.ensure().map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") {
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key><array><string>{}</string><string>serve</string></array>
  <key>EnvironmentVariables</key><dict><key>ALICE_HOME</key><string>{}</string></dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>StandardOutPath</key><string>{}</string>
  <key>StandardErrorPath</key><string>{}</string>
  <key>ProcessType</key><string>Background</string>
</dict>
</plist>
"#,
            xml_escape(&exe.display().to_string()),
            xml_escape(&paths.home.display().to_string()),
            xml_escape(&log.display().to_string()),
            xml_escape(&log.display().to_string()),
        );
        let p = launchd_plist();
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&p, plist).map_err(|e| e.to_string())?;
        let domain = format!("gui/{}", uid());
        let _ = run(Command::new("launchctl").args(["bootout", &format!("{domain}/{LAUNCHD_LABEL}")]));
        run(Command::new("launchctl").args(["bootstrap", &domain]).arg(&p))?;
        Ok(format!("registered launchd agent {}", p.display()))
    } else if cfg!(target_os = "linux") {
        let unit = format!(
            "[Unit]\nDescription=Alice Agent\nAfter=network-online.target\n\n[Service]\nExecStart={} serve\nEnvironment=ALICE_HOME={}\nRestart=on-failure\nRestartSec=3\nStandardOutput=append:{}\nStandardError=append:{}\nNoNewPrivileges=true\n\n[Install]\nWantedBy=default.target\n",
            exe.display(),
            paths.home.display(),
            log.display(),
            log.display()
        );
        let p = systemd_unit();
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&p, unit).map_err(|e| e.to_string())?;
        run(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
        run(Command::new("systemctl").args(["--user", "enable", "--now", "alice.service"]))?;
        Ok(format!(
            "registered systemd user unit {} (for start at boot without login: `loginctl enable-linger $USER`)",
            p.display()
        ))
    } else {
        Err("service registration is supported on macOS and Linux".into())
    }
}

pub fn unregister() -> CliResult<()> {
    match manager() {
        Manager::Launchd => {
            let _ = run(Command::new("launchctl").args(["bootout", &format!("gui/{}/{LAUNCHD_LABEL}", uid())]));
            std::fs::remove_file(launchd_plist()).map_err(|e| e.to_string())
        }
        Manager::Systemd => {
            let _ = run(Command::new("systemctl").args(["--user", "disable", "--now", "alice.service"]));
            std::fs::remove_file(systemd_unit()).map_err(|e| e.to_string())
        }
        Manager::None => Ok(()),
    }
}

fn read_pid(paths: &Paths) -> Option<u32> {
    std::fs::read_to_string(paths.pid_file()).ok()?.trim().parse().ok()
}

fn pid_alive(pid: u32) -> bool {
    Command::new("kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

async fn wait_up(client: &Client, up: bool, timeout: Duration) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < timeout {
        if client.is_up().await == up {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

pub async fn start(paths: &Paths) -> CliResult<()> {
    let client = Client::from_env(paths)?;
    if client.is_up().await {
        println!("Alice is already running at {}", client.base);
        return Ok(());
    }
    match manager() {
        Manager::Launchd => {
            let target = format!("gui/{}/{LAUNCHD_LABEL}", uid());
            if run(Command::new("launchctl").args(["kickstart", &target])).is_err() {
                run(Command::new("launchctl").args(["bootstrap", &format!("gui/{}", uid())]).arg(launchd_plist()))?;
            }
        }
        Manager::Systemd => run(Command::new("systemctl").args(["--user", "start", "alice.service"]))?,
        Manager::None => {
            paths.ensure().map_err(|e| e.to_string())?;
            let log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(paths.log_file())
                .map_err(|e| e.to_string())?;
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let mut cmd = Command::new(exe);
            cmd.arg("serve")
                .env("ALICE_HOME", &paths.home)
                .stdin(Stdio::null())
                .stdout(log.try_clone().map_err(|e| e.to_string())?)
                .stderr(log);
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                cmd.process_group(0);
            }
            let child = cmd.spawn().map_err(|e| e.to_string())?;
            std::fs::write(paths.pid_file(), child.id().to_string()).map_err(|e| e.to_string())?;
        }
    }
    if wait_up(&client, true, Duration::from_secs(8)).await {
        println!("Alice started at {}", client.base);
        Ok(())
    } else {
        Err(format!("server did not come up; see `alice log` ({})", paths.log_file().display()))
    }
}

pub async fn stop(paths: &Paths) -> CliResult<()> {
    let client = Client::from_env(paths)?;
    match manager() {
        Manager::Launchd => {
            let _ = run(Command::new("launchctl").args(["kill", "TERM", &format!("gui/{}/{LAUNCHD_LABEL}", uid())]));
        }
        Manager::Systemd => run(Command::new("systemctl").args(["--user", "stop", "alice.service"]))?,
        Manager::None => {
            let Some(pid) = read_pid(paths) else {
                if client.is_up().await {
                    return Err("server is running but was not started by `alice start` (no PID file)".into());
                }
                println!("Alice is not running");
                return Ok(());
            };
            if pid_alive(pid) {
                let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
            }
            let _ = std::fs::remove_file(paths.pid_file());
        }
    }
    if wait_up(&client, false, Duration::from_secs(8)).await {
        println!("Alice stopped");
        Ok(())
    } else {
        Err("server is still running".into())
    }
}

pub fn log(paths: &Paths, lines: usize, follow: bool) -> CliResult<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let path = paths.log_file();
    let mut f = std::fs::File::open(&path).map_err(|_| format!("no log file yet ({})", path.display()))?;
    let mut s = String::new();
    f.read_to_string(&mut s).map_err(|e| e.to_string())?;
    let all: Vec<&str> = s.lines().collect();
    for l in &all[all.len().saturating_sub(lines)..] {
        println!("{l}");
    }
    if !follow {
        return Ok(());
    }
    let mut pos = f.stream_position().map_err(|e| e.to_string())?;
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if len < pos {
            pos = 0; // rotated / truncated
        }
        if len > pos {
            f.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            pos += buf.len() as u64;
            let _ = std::io::stdout().write_all(&buf);
            let _ = std::io::stdout().flush();
        }
    }
}

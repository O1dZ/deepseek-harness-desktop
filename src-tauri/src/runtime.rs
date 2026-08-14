use crate::{
    local_log::LocalLog,
    settings::{existing_directory, AppPaths, DesktopSettings},
};
use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
#[cfg(not(feature = "full-runtime"))]
use semver::Version;
use serde::Serialize;
use std::{
    collections::VecDeque,
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Weak,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use url::Url;

pub const DSH_VERSION: &str = "0.1.0-rc.6";
const READY_PREFIX: &str = "dsh web: http://127.0.0.1:";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellSnapshot {
    pub edition: String,
    pub status: String,
    pub detail: String,
    pub workspace: Option<String>,
    pub runtime_source: Option<String>,
    pub runtime_url: Option<String>,
    pub node_version: Option<String>,
    pub port: Option<u16>,
    pub log_path: String,
    pub app_version: String,
    pub dsh_version: String,
}

struct RuntimeProcess {
    generation: u64,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    _master: Box<dyn MasterPty + Send>,
    #[cfg(windows)]
    _job: Option<win32job::Job>,
}

struct SupervisorData {
    snapshot: ShellSnapshot,
    process: Option<RuntimeProcess>,
    crashes: VecDeque<Instant>,
}

pub struct RuntimeSupervisor {
    paths: AppPaths,
    log: LocalLog,
    data: Mutex<SupervisorData>,
    app: Mutex<Option<AppHandle>>,
    generation: AtomicU64,
    stopping: AtomicBool,
}

struct Invocation {
    program: OsString,
    args: Vec<OsString>,
    source: String,
    node_version: String,
}

impl RuntimeSupervisor {
    pub fn new(paths: AppPaths, log: LocalLog) -> Arc<Self> {
        let snapshot = ShellSnapshot {
            edition: edition_name().to_string(),
            status: "checking".into(),
            detail: "正在检查桌面环境…".into(),
            workspace: None,
            runtime_source: None,
            runtime_url: None,
            node_version: None,
            port: None,
            log_path: log.path().display().to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            dsh_version: DSH_VERSION.to_string(),
        };
        Arc::new(Self {
            paths,
            log,
            data: Mutex::new(SupervisorData {
                snapshot,
                process: None,
                crashes: VecDeque::new(),
            }),
            app: Mutex::new(None),
            generation: AtomicU64::new(0),
            stopping: AtomicBool::new(false),
        })
    }

    pub fn attach_app(&self, app: AppHandle) {
        *self.app.lock() = Some(app);
    }

    pub fn snapshot(&self) -> ShellSnapshot {
        self.data.lock().snapshot.clone()
    }

    pub fn log(&self) -> &LocalLog {
        &self.log
    }
    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn mark_needs_workspace(&self) {
        let mut data = self.data.lock();
        data.snapshot.status = "needsWorkspace".into();
        data.snapshot.detail = "首次启动需要选择 Workspace。".into();
    }

    pub fn start_saved(self: &Arc<Self>, manual: bool) -> Result<()> {
        let settings = DesktopSettings::load(&self.paths)?;
        let Some(workspace) = settings.workspace.clone() else {
            self.mark_needs_workspace();
            return Ok(());
        };
        self.start(workspace, settings, manual)
    }

    pub fn start(
        self: &Arc<Self>,
        workspace: PathBuf,
        mut settings: DesktopSettings,
        manual: bool,
    ) -> Result<()> {
        if !existing_directory(&workspace) {
            bail!("Workspace 不存在：{}", workspace.display());
        }
        if self.data.lock().process.is_some() {
            return Ok(());
        }

        self.stopping.store(false, Ordering::SeqCst);
        if manual {
            self.data.lock().crashes.clear();
        }

        let port = reserve_stable_port(settings.port)?;
        if settings.port != Some(port) {
            settings.port = Some(port);
            settings.save(&self.paths)?;
        }

        self.set_status(
            if manual { "starting" } else { "restarting" },
            format!(
                "正在 {}:{} 启动 Harness Runtime…",
                Ipv4Addr::LOCALHOST,
                port
            ),
        );
        {
            let mut data = self.data.lock();
            data.snapshot.workspace = Some(workspace.display().to_string());
            data.snapshot.port = Some(port);
            data.snapshot.runtime_url = None;
        }
        navigate_local_shell(self.app.lock().as_ref());

        let mut invocation = match resolve_invocation(&self.paths, &settings) {
            Ok(value) => value,
            Err(error) => {
                self.fail("error", format!("{error:#}"));
                return Err(error);
            }
        };
        inject_port(&mut invocation.args, port);
        self.log.write(
            "desktop",
            &format!("Starting {} in {}", invocation.source, workspace.display()),
        );

        fs::create_dir_all(self.paths.harness_home())?;
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 32,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut command = CommandBuilder::new(invocation.program);
        command.args(invocation.args);
        command.cwd(&workspace);
        command.env("DSH_HOME", self.paths.harness_home());
        command.env("NO_COLOR", "1");

        let child = pair
            .slave
            .spawn_command(command)
            .context("无法创建 Harness Runtime 进程")?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        #[cfg(windows)]
        let job = attach_kill_on_close_job(child.as_ref(), &self.log);

        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let child = Arc::new(Mutex::new(child));
        let writer = Arc::new(Mutex::new(writer));
        {
            let mut data = self.data.lock();
            data.snapshot.runtime_source = Some(invocation.source);
            data.snapshot.node_version = Some(invocation.node_version);
            data.process = Some(RuntimeProcess {
                generation,
                child: child.clone(),
                writer,
                _master: pair.master,
                #[cfg(windows)]
                _job: job,
            });
        }

        spawn_output_reader(Arc::downgrade(self), generation, reader);
        spawn_exit_monitor(Arc::downgrade(self), generation, child);
        Ok(())
    }

    pub fn restart(self: &Arc<Self>) -> Result<()> {
        self.shutdown(Duration::from_secs(5));
        self.start_saved(true)
    }

    pub fn shutdown(&self, grace: Duration) {
        self.stopping.store(true, Ordering::SeqCst);
        let handles = {
            let data = self.data.lock();
            data.process
                .as_ref()
                .map(|process| (process.child.clone(), process.writer.clone()))
        };

        if let Some((child, writer)) = handles {
            self.log
                .write("desktop", "Requesting graceful shutdown with Ctrl+C");
            {
                let mut writer = writer.lock();
                let _ = writer.write_all(&[3]);
                let _ = writer.flush();
            }
            let deadline = Instant::now() + grace;
            while Instant::now() < deadline {
                if child.lock().try_wait().ok().flatten().is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            if child.lock().try_wait().ok().flatten().is_none() {
                self.log.write(
                    "desktop",
                    "Graceful shutdown timed out; terminating runtime job",
                );
                let _ = child.lock().kill();
            }
        }

        self.data.lock().process = None;
        self.set_status("stopped", "Harness Runtime 已停止。".into());
    }

    fn ready(&self, generation: u64, url: &str) {
        if !self.is_current(generation) {
            return;
        }
        let Some(port) = parse_ready_url(url) else {
            self.log.write("desktop", "Ignored invalid readiness URL");
            return;
        };
        {
            let mut data = self.data.lock();
            data.snapshot.status = "ready".into();
            data.snapshot.detail = "Harness Runtime 已就绪。".into();
            data.snapshot.runtime_url = Some(url.to_string());
            data.snapshot.port = Some(port);
        }
        self.log
            .write("desktop", &format!("Runtime ready at {url}"));
        if let Some(app) = self.app.lock().as_ref() {
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(url) = Url::parse(url) {
                    let _ = window.navigate(url);
                }
            }
        }
    }

    fn exited(self: &Arc<Self>, generation: u64, exit_code: u32) {
        let should_restart = {
            let mut data = self.data.lock();
            if data.process.as_ref().map(|p| p.generation) != Some(generation) {
                return;
            }
            data.process = None;
            if self.stopping.load(Ordering::SeqCst) {
                data.snapshot.status = "stopped".into();
                data.snapshot.detail = "Harness Runtime 已停止。".into();
                false
            } else {
                let now = Instant::now();
                while data
                    .crashes
                    .front()
                    .map(|time| now.duration_since(*time) > Duration::from_secs(60))
                    .unwrap_or(false)
                {
                    data.crashes.pop_front();
                }
                data.crashes.push_back(now);
                data.snapshot.runtime_url = None;
                data.crashes.len() == 1
            }
        };

        self.log
            .write("desktop", &format!("Runtime exited with code {exit_code}"));
        navigate_local_shell(self.app.lock().as_ref());
        if should_restart {
            self.set_status(
                "restarting",
                "Runtime 意外退出，正在进行一次自动恢复…".into(),
            );
            let weak = Arc::downgrade(self);
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(1));
                if let Some(supervisor) = weak.upgrade() {
                    if let Err(error) = supervisor.start_saved(false) {
                        supervisor.fail("crashed", format!("自动恢复失败：{error:#}"));
                    }
                }
            });
        } else if !self.stopping.load(Ordering::SeqCst) {
            self.fail(
                "crashed",
                format!("Runtime 在 60 秒内再次退出（代码 {exit_code}），已停止自动重试。"),
            );
        }
    }

    fn is_current(&self, generation: u64) -> bool {
        self.data.lock().process.as_ref().map(|p| p.generation) == Some(generation)
    }

    fn set_status(&self, status: &str, detail: String) {
        let mut data = self.data.lock();
        data.snapshot.status = status.into();
        data.snapshot.detail = detail;
    }

    fn fail(&self, status: &str, detail: String) {
        self.log.write("desktop", &detail);
        self.set_status(status, detail);
        navigate_local_shell(self.app.lock().as_ref());
    }
}

fn spawn_output_reader(
    supervisor: Weak<RuntimeSupervisor>,
    generation: u64,
    reader: Box<dyn std::io::Read + Send>,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) => break,
                Ok(_) => {
                    let clean = String::from_utf8_lossy(&strip_ansi_escapes::strip(&buffer))
                        .trim()
                        .to_string();
                    if clean.is_empty() {
                        continue;
                    }
                    let Some(supervisor) = supervisor.upgrade() else {
                        break;
                    };
                    supervisor.log.write("runtime", &clean);
                    if let Some(start) = clean.find(READY_PREFIX) {
                        let url = clean[start + "dsh web: ".len()..]
                            .split_whitespace()
                            .next()
                            .unwrap_or_default();
                        supervisor.ready(generation, url);
                    }
                }
                Err(error) => {
                    if let Some(supervisor) = supervisor.upgrade() {
                        supervisor
                            .log
                            .write("desktop", &format!("Runtime output reader failed: {error}"));
                    }
                    break;
                }
            }
        }
    });
}

fn spawn_exit_monitor(
    supervisor: Weak<RuntimeSupervisor>,
    generation: u64,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
) {
    thread::spawn(move || loop {
        match child.lock().try_wait() {
            Ok(Some(status)) => {
                if let Some(supervisor) = supervisor.upgrade() {
                    supervisor.exited(generation, status.exit_code());
                }
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(250)),
            Err(error) => {
                if let Some(supervisor) = supervisor.upgrade() {
                    supervisor.fail("crashed", format!("无法监视 Runtime：{error}"));
                }
                break;
            }
        }
    });
}

fn resolve_invocation(_paths: &AppPaths, _settings: &DesktopSettings) -> Result<Invocation> {
    #[cfg(feature = "full-runtime")]
    {
        let root = _paths.resource_dir.join("resources").join("full-runtime");
        let node = root.join("node.exe");
        let dsh = root
            .join("app")
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh")
            .join("lib")
            .join("bin.js");
        if !node.is_file() || !dsh.is_file() {
            bail!(
                "Full Runtime 资源不完整。请重新安装 Full Edition。\nNode: {}\nDSH: {}",
                node.display(),
                dsh.display()
            );
        }
        let node_version = read_node_version(&node)?;
        return Ok(Invocation {
            program: node.into_os_string(),
            args: dsh_args(dsh.into_os_string()),
            source: format!("Bundled dsh {DSH_VERSION}"),
            node_version,
        });
    }

    #[cfg(not(feature = "full-runtime"))]
    {
        let settings = _settings;
        let node = settings
            .custom_node
            .clone()
            .or_else(|| find_on_path("node.exe"))
            .ok_or_else(|| {
                anyhow!(
                    "Lite Edition 需要 Node.js ^22.19.0 或 >=24.0.0。\n请安装兼容版本后重新检测。"
                )
            })?;
        let node_version = read_node_version(&node)?;
        validate_node_version(&node_version)?;

        if let Some(custom) = settings.custom_dsh.clone() {
            if !custom.exists() {
                bail!("自定义 dsh 路径不存在：{}", custom.display());
            }
            if custom
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case("js"))
                .unwrap_or(false)
            {
                return Ok(Invocation {
                    program: node.into_os_string(),
                    args: dsh_args(custom.into_os_string()),
                    source: "Custom dsh JavaScript entry".into(),
                    node_version,
                });
            }
        }

        if let Some((entry, version)) = find_global_dsh(settings)? {
            if version != DSH_VERSION && !settings.allow_unverified_runtime {
                bail!("检测到 dsh {version}，但此桌面版本仅验证了 {DSH_VERSION}。\n可安装匹配版本，或在桌面设置中允许未验证版本。")
            }
            return Ok(Invocation {
                program: node.into_os_string(),
                args: dsh_args(entry.into_os_string()),
                source: format!("Global dsh {version}"),
                node_version,
            });
        }

        let npx = find_on_path("npx.cmd")
            .ok_or_else(|| anyhow!("没有找到 npx.cmd。请确认 npm 已加入 PATH。"))?;
        let command_line = format!(
            "\"{}\" --yes @deepseek-ai/dsh@{} web --host 127.0.0.1 --port {{PORT}}",
            npx.display(),
            DSH_VERSION
        );
        Ok(Invocation {
            program: OsString::from("cmd.exe"),
            args: vec![
                OsString::from("/d"),
                OsString::from("/s"),
                OsString::from("/c"),
                OsString::from(command_line),
            ],
            source: format!("npx dsh {DSH_VERSION}"),
            node_version,
        })
    }
}

fn dsh_args(entry: OsString) -> Vec<OsString> {
    vec![
        entry,
        OsString::from("web"),
        OsString::from("--host"),
        OsString::from("127.0.0.1"),
        OsString::from("--port"),
        OsString::from("{PORT}"),
    ]
}

#[cfg(not(feature = "full-runtime"))]
fn find_global_dsh(settings: &DesktopSettings) -> Result<Option<(PathBuf, String)>> {
    if let Some(custom) = &settings.custom_dsh {
        let manifest = custom
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .map(|p| p.join("package.json"));
        if let Some(manifest) = manifest.filter(|path| path.is_file()) {
            return Ok(Some((custom.clone(), read_package_version(&manifest)?)));
        }
    }

    let npm = match find_on_path("npm.cmd") {
        Some(path) => path,
        None => return Ok(None),
    };
    let output = Command::new(npm).args(["root", "-g"]).output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let root = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let package = root.join("@deepseek-ai").join("dsh");
    let entry = package.join("lib").join("bin.js");
    let manifest = package.join("package.json");
    if entry.is_file() && manifest.is_file() {
        Ok(Some((entry, read_package_version(&manifest)?)))
    } else {
        Ok(None)
    }
}

#[cfg(not(feature = "full-runtime"))]
fn read_package_version(path: &Path) -> Result<String> {
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    value
        .get("version")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("package.json 缺少 version"))
}

fn read_node_version(node: &Path) -> Result<String> {
    let output = Command::new(node)
        .arg("--version")
        .output()
        .with_context(|| format!("无法运行 Node.js：{}", node.display()))?;
    if !output.status.success() {
        bail!("Node.js 版本检查失败：{}", node.display());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_start_matches('v')
        .to_string())
}

#[cfg(not(feature = "full-runtime"))]
fn validate_node_version(raw: &str) -> Result<()> {
    let version = Version::parse(raw).with_context(|| format!("无法解析 Node.js 版本：{raw}"))?;
    let compatible = (version.major == 22 && version.minor >= 19) || version.major >= 24;
    if compatible {
        Ok(())
    } else {
        bail!("Node.js {version} 不受支持；Lite Edition 需要 ^22.19.0 或 >=24.0.0。")
    }
}

#[cfg(not(feature = "full-runtime"))]
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn reserve_stable_port(saved: Option<u16>) -> Result<u16> {
    if let Some(port) = saved.filter(|port| *port > 0) {
        if TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok() {
            return Ok(port);
        }
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
}

fn parse_ready_url(raw: &str) -> Option<u16> {
    let url = Url::parse(raw).ok()?;
    if url.scheme() != "http" || url.host_str()? != "127.0.0.1" || url.path() != "/" {
        return None;
    }
    url.port()
}

fn navigate_local_shell(app: Option<&AppHandle>) {
    let Some(app) = app else {
        return;
    };
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Ok(url) = Url::parse("http://tauri.localhost/index.html") {
        let _ = window.navigate(url);
    }
}

#[cfg(windows)]
fn attach_kill_on_close_job(child: &dyn Child, log: &LocalLog) -> Option<win32job::Job> {
    let result = (|| -> Result<win32job::Job> {
        let handle = child
            .as_raw_handle()
            .ok_or_else(|| anyhow!("Runtime process handle unavailable"))?
            as isize;
        let job = win32job::Job::create()?;
        let mut limits = job.query_extended_limit_info()?;
        limits.limit_kill_on_job_close();
        job.set_extended_limit_info(&limits)?;
        job.assign_process(handle)?;
        Ok(job)
    })();
    match result {
        Ok(job) => Some(job),
        Err(error) => {
            log.write(
                "desktop",
                &format!("Job Object setup failed; shutdown fallback remains active: {error:#}"),
            );
            None
        }
    }
}

pub fn edition_name() -> &'static str {
    if cfg!(feature = "full-runtime") {
        "Full"
    } else {
        "Lite"
    }
}

pub fn inject_port(args: &mut [OsString], port: u16) {
    for argument in args {
        if argument == "{PORT}" {
            *argument = OsString::from(port.to_string());
        } else if let Some(value) = argument.to_str() {
            if value.contains("{PORT}") {
                *argument = OsString::from(value.replace("{PORT}", &port.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(feature = "full-runtime"))]
    use super::validate_node_version;
    use super::{inject_port, parse_ready_url};
    use std::ffi::OsString;

    #[test]
    fn accepts_only_loopback_readiness_url() {
        assert_eq!(parse_ready_url("http://127.0.0.1:3080/"), Some(3080));
        assert_eq!(parse_ready_url("http://localhost:3080/"), None);
        assert_eq!(parse_ready_url("https://127.0.0.1:3080/"), None);
    }

    #[test]
    #[cfg(not(feature = "full-runtime"))]
    fn checks_supported_node_lines() {
        assert!(validate_node_version("22.19.0").is_ok());
        assert!(validate_node_version("24.0.0").is_ok());
        assert!(validate_node_version("22.18.0").is_err());
        assert!(validate_node_version("23.9.0").is_err());
    }

    #[test]
    fn replaces_port_placeholder() {
        let mut args = vec![OsString::from("--port"), OsString::from("{PORT}")];
        inject_port(&mut args, 43123);
        assert_eq!(args[1], "43123");
    }
}

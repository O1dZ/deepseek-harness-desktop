use crate::{
    local_log::LocalLog,
    settings::{existing_directory, AppPaths, DesktopSettings},
};
use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
#[cfg(not(feature = "full-runtime"))]
use semver::Version;
use serde::Serialize;
#[cfg(windows)]
use std::os::windows::{io::AsRawHandle, process::CommandExt};
use std::{
    collections::VecDeque,
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Read},
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
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
const FULL_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const LITE_STARTUP_TIMEOUT: Duration = Duration::from_secs(300);
#[cfg(not(feature = "full-runtime"))]
const LITE_INSTALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
#[cfg(not(feature = "full-runtime"))]
const LITE_RUNTIME_PACKAGE: &str = include_str!("../../runtime/full/package.json");
#[cfg(not(feature = "full-runtime"))]
const LITE_RUNTIME_LOCK: &str = include_str!("../../runtime/full/package-lock.json");

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
    kind: ProcessKind,
    child: Arc<Mutex<Child>>,
    #[cfg(windows)]
    _job: Option<win32job::Job>,
}

#[derive(Debug, Clone)]
enum ProcessKind {
    Runtime,
    #[cfg(not(feature = "full-runtime"))]
    Install(LiteInstallPlan),
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

enum RuntimeResolution {
    Ready(Invocation),
    #[cfg(not(feature = "full-runtime"))]
    Install(LiteInstallPlan),
}

#[cfg(not(feature = "full-runtime"))]
#[derive(Debug, Clone)]
struct LiteInstallPlan {
    node: PathBuf,
    npm_cli: PathBuf,
    node_version: String,
    staging_dir: PathBuf,
    final_dir: PathBuf,
    cache_dir: PathBuf,
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

        {
            let mut data = self.data.lock();
            data.snapshot.workspace = Some(workspace.display().to_string());
            data.snapshot.port = Some(port);
            data.snapshot.runtime_url = None;
        }
        navigate_local_shell(self.app.lock().as_ref());

        let resolution = match resolve_runtime(&self.paths, &settings) {
            Ok(value) => value,
            Err(error) => {
                self.fail("error", format!("{error:#}"));
                return Err(error);
            }
        };
        let result = match resolution {
            RuntimeResolution::Ready(mut invocation) => {
                self.set_status(
                    if manual { "starting" } else { "restarting" },
                    format!(
                        "正在 {}:{} 启动 Harness Runtime…",
                        Ipv4Addr::LOCALHOST,
                        port
                    ),
                );
                inject_port(&mut invocation.args, port);
                self.launch_runtime(workspace, invocation)
            }
            #[cfg(not(feature = "full-runtime"))]
            RuntimeResolution::Install(plan) => self.launch_lite_install(plan),
        };
        if let Err(error) = &result {
            self.fail("error", format!("{error:#}"));
        }
        result
    }

    fn launch_runtime(self: &Arc<Self>, workspace: PathBuf, invocation: Invocation) -> Result<()> {
        self.log.write(
            "desktop",
            &format!("Starting {} in {}", invocation.source, workspace.display()),
        );

        fs::create_dir_all(self.paths.harness_home())?;
        let mut command = Command::new(invocation.program);
        command
            .args(invocation.args)
            .current_dir(&workspace)
            .env("DSH_HOME", self.paths.harness_home())
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let mut child = command.spawn().context("无法创建 Harness Runtime 进程")?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("无法读取 Harness Runtime 标准输出"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("无法读取 Harness Runtime 错误输出"))?;

        #[cfg(windows)]
        let job = attach_kill_on_close_job(&child, &self.log);

        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let child = Arc::new(Mutex::new(child));
        {
            let mut data = self.data.lock();
            data.snapshot.runtime_source = Some(invocation.source);
            data.snapshot.node_version = Some(invocation.node_version);
            data.process = Some(RuntimeProcess {
                generation,
                kind: ProcessKind::Runtime,
                child: child.clone(),
                #[cfg(windows)]
                _job: job,
            });
        }

        spawn_output_reader(Arc::downgrade(self), generation, stdout, "runtime", true);
        spawn_output_reader(
            Arc::downgrade(self),
            generation,
            stderr,
            "runtime-stderr",
            true,
        );
        spawn_exit_monitor(Arc::downgrade(self), generation, child);
        spawn_startup_watchdog(Arc::downgrade(self), generation);
        Ok(())
    }

    #[cfg(not(feature = "full-runtime"))]
    fn launch_lite_install(self: &Arc<Self>, plan: LiteInstallPlan) -> Result<()> {
        prepare_lite_install_directory(&plan)?;
        self.set_status(
            "installing",
            format!(
                "首次使用正在自动安装固定 Harness Runtime {}。下载和安装可能需要数分钟…",
                DSH_VERSION
            ),
        );
        {
            let mut data = self.data.lock();
            data.snapshot.runtime_source = Some(format!("Managed dsh {DSH_VERSION}"));
            data.snapshot.node_version = Some(plan.node_version.clone());
        }
        navigate_local_shell(self.app.lock().as_ref());
        self.log.write(
            "desktop",
            &format!(
                "Installing managed dsh {} into {}",
                DSH_VERSION,
                plan.final_dir.display()
            ),
        );

        let mut command = Command::new(&plan.node);
        command
            .args(npm_ci_args(&plan))
            .current_dir(&plan.staging_dir)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let mut child = command.spawn().context("无法启动 Lite Runtime 自动安装")?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("无法读取 Runtime 安装标准输出"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("无法读取 Runtime 安装错误输出"))?;
        #[cfg(windows)]
        let job = attach_kill_on_close_job(&child, &self.log);

        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let child = Arc::new(Mutex::new(child));
        {
            let mut data = self.data.lock();
            data.process = Some(RuntimeProcess {
                generation,
                kind: ProcessKind::Install(plan),
                child: child.clone(),
                #[cfg(windows)]
                _job: job,
            });
        }
        spawn_output_reader(
            Arc::downgrade(self),
            generation,
            stdout,
            "runtime-install",
            false,
        );
        spawn_output_reader(
            Arc::downgrade(self),
            generation,
            stderr,
            "runtime-install-stderr",
            false,
        );
        spawn_exit_monitor(Arc::downgrade(self), generation, child);
        spawn_install_watchdog(Arc::downgrade(self), generation);
        Ok(())
    }

    pub fn restart(self: &Arc<Self>) -> Result<()> {
        self.shutdown(Duration::from_secs(5));
        self.start_saved(true)
    }

    pub fn shutdown(&self, grace: Duration) {
        self.stopping.store(true, Ordering::SeqCst);
        let child = {
            let data = self.data.lock();
            data.process.as_ref().map(|process| process.child.clone())
        };

        if let Some(child) = child {
            self.log
                .write("desktop", "Stopping supervised process tree");
            let _ = child.lock().kill();
            let deadline = Instant::now() + grace;
            while Instant::now() < deadline {
                if child.lock().try_wait().ok().flatten().is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            if child.lock().try_wait().ok().flatten().is_none() {
                self.log
                    .write("desktop", "Runtime process did not exit promptly");
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
        let _kind = {
            let mut data = self.data.lock();
            let Some(process) = data
                .process
                .as_ref()
                .filter(|process| process.generation == generation)
            else {
                return;
            };
            let kind = process.kind.clone();
            data.process = None;
            kind
        };

        #[cfg(not(feature = "full-runtime"))]
        if let ProcessKind::Install(plan) = _kind {
            if self.stopping.load(Ordering::SeqCst) {
                self.set_status("stopped", "Runtime 安装已停止。".into());
                return;
            }
            self.log.write(
                "desktop",
                &format!("Runtime installer exited with code {exit_code}"),
            );
            if exit_code != 0 {
                self.fail(
                    "error",
                    format!(
                        "固定 Harness Runtime {} 自动安装失败（代码 {}）。请检查网络和日志后重试。",
                        DSH_VERSION, exit_code
                    ),
                );
                return;
            }
            if let Err(error) = finalize_lite_install(&plan) {
                self.fail("error", format!("Runtime 安装校验失败：{error:#}"));
                return;
            }
            self.log.write(
                "desktop",
                &format!("Managed dsh {} installed successfully", DSH_VERSION),
            );
            self.set_status("starting", "固定 Runtime 已安装，正在启动 Harness…".into());
            if let Err(error) = self.start_saved(false) {
                self.fail("error", format!("安装后启动失败：{error:#}"));
            }
            return;
        }

        let should_restart = {
            let mut data = self.data.lock();
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
        self.data
            .lock()
            .process
            .as_ref()
            .map(|process| {
                process.generation == generation && matches!(&process.kind, ProcessKind::Runtime)
            })
            .unwrap_or(false)
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

fn spawn_output_reader<R>(
    supervisor: Weak<RuntimeSupervisor>,
    generation: u64,
    reader: R,
    source: &'static str,
    parse_readiness: bool,
) where
    R: Read + Send + 'static,
{
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
                    supervisor.log.write(source, &clean);
                    if parse_readiness {
                        if let Some(start) = clean.find(READY_PREFIX) {
                            let url = clean[start + "dsh web: ".len()..]
                                .split_whitespace()
                                .next()
                                .unwrap_or_default();
                            supervisor.ready(generation, url);
                        }
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
    child: Arc<Mutex<Child>>,
) {
    thread::spawn(move || loop {
        match child.lock().try_wait() {
            Ok(Some(status)) => {
                if let Some(supervisor) = supervisor.upgrade() {
                    supervisor.exited(generation, status.code().unwrap_or(1) as u32);
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

fn spawn_startup_watchdog(supervisor: Weak<RuntimeSupervisor>, generation: u64) {
    thread::spawn(move || {
        let timeout = startup_timeout();
        thread::sleep(timeout);
        let Some(supervisor) = supervisor.upgrade() else {
            return;
        };
        let still_starting = {
            let data = supervisor.data.lock();
            data.process.as_ref().map(|process| {
                process.generation == generation && matches!(&process.kind, ProcessKind::Runtime)
            }) == Some(true)
                && data.snapshot.runtime_url.is_none()
        };
        if !still_starting {
            return;
        }

        supervisor.log.write(
            "desktop",
            &format!(
                "Runtime did not report readiness within {} seconds",
                timeout.as_secs()
            ),
        );
        supervisor.shutdown(Duration::from_secs(5));
        supervisor.fail(
            "error",
            format!(
                "Harness Runtime 在 {} 秒内未报告就绪。已停止该进程；请检查日志或桌面设置。",
                timeout.as_secs()
            ),
        );
    });
}

#[cfg(not(feature = "full-runtime"))]
fn spawn_install_watchdog(supervisor: Weak<RuntimeSupervisor>, generation: u64) {
    thread::spawn(move || {
        thread::sleep(LITE_INSTALL_TIMEOUT);
        let Some(supervisor) = supervisor.upgrade() else {
            return;
        };
        let still_installing = supervisor
            .data
            .lock()
            .process
            .as_ref()
            .map(|process| {
                process.generation == generation && matches!(&process.kind, ProcessKind::Install(_))
            })
            .unwrap_or(false);
        if !still_installing {
            return;
        }
        supervisor
            .log
            .write("desktop", "Runtime installation exceeded 30 minutes");
        supervisor.shutdown(Duration::from_secs(5));
        supervisor.fail(
            "error",
            "固定 Runtime 在 30 分钟内未安装完成。已停止安装；请检查网络和日志后重试。".into(),
        );
    });
}

fn startup_timeout() -> Duration {
    if cfg!(feature = "full-runtime") {
        FULL_STARTUP_TIMEOUT
    } else {
        LITE_STARTUP_TIMEOUT
    }
}

fn resolve_runtime(_paths: &AppPaths, _settings: &DesktopSettings) -> Result<RuntimeResolution> {
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
        return Ok(RuntimeResolution::Ready(Invocation {
            program: node.into_os_string(),
            args: dsh_args(dsh.into_os_string()),
            source: format!("Bundled dsh {DSH_VERSION}"),
            node_version,
        }));
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
                return Ok(RuntimeResolution::Ready(Invocation {
                    program: node.into_os_string(),
                    args: dsh_args(custom.into_os_string()),
                    source: "Custom dsh JavaScript entry".into(),
                    node_version,
                }));
            }
        }

        if let Some((entry, version)) = find_global_dsh(settings)? {
            if version == DSH_VERSION || settings.allow_unverified_runtime {
                return Ok(RuntimeResolution::Ready(Invocation {
                    program: node.into_os_string(),
                    args: dsh_args(entry.into_os_string()),
                    source: format!("Global dsh {version}"),
                    node_version,
                }));
            }
        }

        let final_dir = lite_runtime_version_dir(_paths);
        if let Some((entry, version)) = managed_dsh_at(&final_dir)? {
            return Ok(RuntimeResolution::Ready(Invocation {
                program: node.into_os_string(),
                args: dsh_args(entry.into_os_string()),
                source: format!("Managed dsh {version}"),
                node_version,
            }));
        }

        let npm = find_on_path("npm.cmd")
            .ok_or_else(|| anyhow!("没有找到 npm.cmd。Lite Edition 首次准备 Runtime 需要 npm。"))?;
        let npm_cli = find_npm_cli(&node, &npm).ok_or_else(|| {
            anyhow!(
                "没有找到 npm-cli.js。请修复 Node.js/npm 安装。\nNode: {}\nNPM: {}",
                node.display(),
                npm.display()
            )
        })?;
        let runtime_dir = _paths.runtime_dir();
        Ok(RuntimeResolution::Install(LiteInstallPlan {
            node,
            npm_cli,
            node_version,
            staging_dir: runtime_dir.join(format!("{DSH_VERSION}.installing")),
            final_dir,
            cache_dir: runtime_dir.join("npm-cache"),
        }))
    }
}

#[cfg(not(feature = "full-runtime"))]
fn find_npm_cli(node: &Path, npm: &Path) -> Option<PathBuf> {
    [npm.parent(), node.parent()]
        .into_iter()
        .flatten()
        .map(|directory| {
            directory
                .join("node_modules")
                .join("npm")
                .join("bin")
                .join("npm-cli.js")
        })
        .find(|candidate| candidate.is_file())
}

#[cfg(not(feature = "full-runtime"))]
fn npm_ci_args(plan: &LiteInstallPlan) -> Vec<OsString> {
    vec![
        plan.npm_cli.clone().into_os_string(),
        OsString::from("ci"),
        OsString::from("--omit=dev"),
        OsString::from("--no-audit"),
        OsString::from("--no-fund"),
        OsString::from("--offline=false"),
        OsString::from("--prefer-online"),
        OsString::from("--loglevel=notice"),
        OsString::from("--cache"),
        plan.cache_dir.clone().into_os_string(),
    ]
}

#[cfg(not(feature = "full-runtime"))]
fn lite_runtime_version_dir(paths: &AppPaths) -> PathBuf {
    paths.runtime_dir().join(DSH_VERSION)
}

#[cfg(not(feature = "full-runtime"))]
fn managed_dsh_at(root: &Path) -> Result<Option<(PathBuf, String)>> {
    let package = root.join("node_modules").join("@deepseek-ai").join("dsh");
    let entry = package.join("lib").join("bin.js");
    let manifest = package.join("package.json");
    if !entry.is_file() || !manifest.is_file() {
        return Ok(None);
    }
    let version = read_package_version(&manifest)?;
    if version == DSH_VERSION {
        Ok(Some((entry, version)))
    } else {
        Ok(None)
    }
}

#[cfg(not(feature = "full-runtime"))]
fn prepare_lite_install_directory(plan: &LiteInstallPlan) -> Result<()> {
    let runtime_root = plan
        .final_dir
        .parent()
        .ok_or_else(|| anyhow!("Runtime 目录无效"))?;
    fs::create_dir_all(runtime_root)?;
    if plan.staging_dir.exists() {
        fs::remove_dir_all(&plan.staging_dir).context("清理上次未完成的 Runtime 安装失败")?;
    }
    fs::create_dir_all(&plan.staging_dir)?;
    fs::write(plan.staging_dir.join("package.json"), LITE_RUNTIME_PACKAGE)?;
    fs::write(
        plan.staging_dir.join("package-lock.json"),
        LITE_RUNTIME_LOCK,
    )?;
    Ok(())
}

#[cfg(not(feature = "full-runtime"))]
fn finalize_lite_install(plan: &LiteInstallPlan) -> Result<()> {
    managed_dsh_at(&plan.staging_dir)?
        .ok_or_else(|| anyhow!("安装结果缺少 dsh {} 入口", DSH_VERSION))?;
    if plan.final_dir.exists() {
        fs::remove_dir_all(&plan.final_dir).context("清理无效 Runtime 目录失败")?;
    }
    fs::rename(&plan.staging_dir, &plan.final_dir).context("启用新 Runtime 失败")?;
    let _ = fs::remove_dir_all(&plan.cache_dir);
    Ok(())
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
fn attach_kill_on_close_job(child: &Child, log: &LocalLog) -> Option<win32job::Job> {
    let result = (|| -> Result<win32job::Job> {
        let handle = child.as_raw_handle() as isize;
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
    use super::{inject_port, parse_ready_url};
    #[cfg(not(feature = "full-runtime"))]
    use super::{npm_ci_args, validate_node_version, LiteInstallPlan};
    use std::ffi::OsString;
    #[cfg(not(feature = "full-runtime"))]
    use std::path::PathBuf;

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
    #[cfg(not(feature = "full-runtime"))]
    fn installs_locked_runtime_with_npm_cli_directly() {
        let plan = LiteInstallPlan {
            node: PathBuf::from(r"C:\Program Files\nodejs\node.exe"),
            npm_cli: PathBuf::from(r"C:\Program Files\nodejs\node_modules\npm\bin\npm-cli.js"),
            node_version: "24.0.0".into(),
            staging_dir: PathBuf::from(r"C:\app\runtime\installing"),
            final_dir: PathBuf::from(r"C:\app\runtime\0.1.0-rc.6"),
            cache_dir: PathBuf::from(r"C:\app\runtime\npm-cache"),
        };
        let args = npm_ci_args(&plan);
        assert_eq!(
            args,
            vec![
                OsString::from(r"C:\Program Files\nodejs\node_modules\npm\bin\npm-cli.js"),
                OsString::from("ci"),
                OsString::from("--omit=dev"),
                OsString::from("--no-audit"),
                OsString::from("--no-fund"),
                OsString::from("--offline=false"),
                OsString::from("--prefer-online"),
                OsString::from("--loglevel=notice"),
                OsString::from("--cache"),
                OsString::from(r"C:\app\runtime\npm-cache"),
            ]
        );
    }

    #[test]
    fn replaces_port_placeholder() {
        let mut args = vec![OsString::from("--port"), OsString::from("{PORT}")];
        inject_port(&mut args, 43123);
        assert_eq!(args[1], "43123");
    }
}

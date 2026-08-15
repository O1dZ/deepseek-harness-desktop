use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    #[cfg_attr(not(feature = "full-runtime"), allow(dead_code))]
    pub resource_dir: PathBuf,
}

impl AppPaths {
    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    pub fn harness_home(&self) -> PathBuf {
        self.data_dir.join("harness-home")
    }

    #[cfg_attr(feature = "full-runtime", allow(dead_code))]
    pub fn runtime_dir(&self) -> PathBuf {
        self.data_dir.join("runtime")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DesktopSettings {
    pub workspace: Option<PathBuf>,
    pub port: Option<u16>,
    pub custom_node: Option<PathBuf>,
    pub custom_dsh: Option<PathBuf>,
    pub allow_unverified_runtime: bool,
    pub launch_at_login: bool,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            workspace: None,
            port: None,
            custom_node: None,
            custom_dsh: None,
            allow_unverified_runtime: false,
            launch_at_login: false,
        }
    }
}

impl DesktopSettings {
    pub fn load(paths: &AppPaths) -> Result<Self> {
        let path = paths.settings_file();
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = fs::read(&path).with_context(|| format!("读取设置失败：{}", path.display()))?;
        serde_json::from_slice(&bytes).context("设置文件格式无效")
    }

    pub fn save(&self, paths: &AppPaths) -> Result<()> {
        fs::create_dir_all(&paths.config_dir)?;
        let path = paths.settings_file();
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        if path.exists() {
            fs::remove_file(&path)?;
        }
        fs::rename(temp, path)?;
        Ok(())
    }
}

pub fn configure_autostart(enabled: bool) -> Result<()> {
    let executable = std::env::current_exe().context("无法确定应用程序路径")?;
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let name = "DeepSeek Harness Desktop";

    let status = if enabled {
        let value = format!("\"{}\" --hidden", executable.display());
        Command::new("reg.exe")
            .args(["ADD", key, "/v", name, "/t", "REG_SZ", "/d", &value, "/f"])
            .status()?
    } else {
        Command::new("reg.exe")
            .args(["DELETE", key, "/v", name, "/f"])
            .status()?
    };

    if status.success() || !enabled {
        Ok(())
    } else {
        anyhow::bail!("Windows 登录启动设置失败")
    }
}

pub fn existing_directory(path: &Path) -> bool {
    path.is_dir()
}

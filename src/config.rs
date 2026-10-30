//! 本地配置：`~/.config/rgc-agent/config.toml`，权限 0600。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 平台地址，例如 https://rgc-dev.tensorplay.cn
    pub server: String,
    /// 配对后下发的长期 token。
    pub token: String,
    /// 平台侧的 agent 记录 id。
    pub agent_id: String,
}

pub fn path() -> Result<PathBuf> {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            let home = std::env::var("HOME").context("HOME is not set")?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join("rgc-agent").join("config.toml"))
}

pub fn load() -> Result<Config> {
    let path = path()?;
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("读取配置失败：{}（先运行 rgc-agent pair）", path.display()))?;
    let mut cfg: Config = toml::from_str(&raw)?;

    if let Ok(v) = std::env::var("RGC_AGENT_SERVER") {
        if !v.is_empty() {
            cfg.server = v;
        }
    }
    if let Ok(v) = std::env::var("RGC_AGENT_TOKEN") {
        if !v.is_empty() {
            cfg.token = v;
        }
    }
    Ok(cfg)
}

pub fn save(cfg: &Config) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let path = path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = toml::to_string_pretty(cfg)?;
    std::fs::write(&path, body)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(path)
}

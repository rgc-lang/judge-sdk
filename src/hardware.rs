//! GPU 指纹。
//!
//! 榜单按「同硬件」分桶，所以指纹要能区分性能真正不同的机器：
//! 同名但 TGP 不同的卡（笔记本 4090 vs 台式 4090）必须落到不同桶。

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hardware {
    pub gpu: String,
    pub count: u32,
    pub vram_mb: u32,
    pub power_w: f64,
    pub clock_max_mhz: u32,
    pub driver: String,
    /// 例如 "8.6"
    pub compute_cap: String,
}

impl Hardware {
    /// 榜单分桶键：型号 + 卡数 + 显存 + 功耗。
    pub fn bucket(&self) -> String {
        format!(
            "{}|{}x|{}G|{:.0}W",
            self.gpu,
            self.count,
            self.vram_mb / 1024,
            self.power_w
        )
    }

    /// nvcc 的 `-arch` 参数，例如 "sm_86"。
    pub fn arch(&self) -> String {
        format!("sm_{}", self.compute_cap.replace('.', ""))
    }
}

/// 读 nvidia-smi 得到指纹。同一台机器上多张卡取第一张的型号，卡数单独记。
pub fn detect() -> Result<Hardware> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,power.limit,clocks.max.sm,driver_version,compute_cap",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .context("无法执行 nvidia-smi（确认已安装 NVIDIA 驱动）")?;

    if !out.status.success() {
        bail!("nvidia-smi 退出码 {:?}", out.status.code());
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let first = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .context("nvidia-smi 没有返回任何 GPU")?;

    let fields: Vec<&str> = first.split(',').map(str::trim).collect();
    if fields.len() < 6 {
        bail!("nvidia-smi 输出格式异常：{first}");
    }

    Ok(Hardware {
        gpu: fields[0].to_string(),
        count: text.lines().filter(|l| !l.trim().is_empty()).count() as u32,
        vram_mb: fields[1].parse().unwrap_or(0),
        power_w: fields[2].parse().unwrap_or(0.0),
        clock_max_mhz: fields[3].parse().unwrap_or(0),
        driver: fields[4].to_string(),
        compute_cap: fields[5].to_string(),
    })
}

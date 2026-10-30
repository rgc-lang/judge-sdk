//! 编译：把 harness 模板 + 用户代码 + 参考实现拼成一份源码，交给 nvcc。
//!
//! `-arch` 由本机 GPU 决定（agent 侧编译的原因就在这），其余参数固定，
//! 保证同一道题在不同机器上编出来的东西可比。

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::exec;
use crate::hardware::Hardware;
use crate::job::Job;

pub const HARNESS_CUDA: &str = include_str!("../harness/cuda.cu");

pub struct CompileOutcome {
    pub ok: bool,
    pub ms: u64,
    pub output: String,
    pub exe: Option<PathBuf>,
}

pub fn compile(job: &Job, dir: &Path, hw: &Hardware) -> Result<CompileOutcome> {
    match job.language.as_str() {
        "cuda" => compile_cuda(job, dir, hw),
        other => bail!("暂不支持的语言：{other}（当前只有 cuda）"),
    }
}

fn compile_cuda(job: &Job, dir: &Path, hw: &Hardware) -> Result<CompileOutcome> {
    if job.reference_code.trim().is_empty() {
        bail!("题目缺少 reference_code，无法产生期望值");
    }

    let source = HARNESS_CUDA
        .replace("{{USER_CODE}}", &job.code)
        .replace("{{REFERENCE_CODE}}", &job.reference_code);
    let src_path = dir.join("main.cu");
    std::fs::write(&src_path, source).context("写入 main.cu 失败")?;

    let exe = dir.join("prog");
    let nvcc = std::env::var("NVCC").unwrap_or_else(|_| "nvcc".to_string());
    let mut cmd = Command::new(&nvcc);
    cmd.arg("-O3")
        .arg("-std=c++17")
        .arg(format!("-arch={}", hw.arch()))
        .arg("-o")
        .arg(&exe)
        .arg(&src_path);

    let outcome = exec::run_with_timeout(
        &mut cmd,
        Duration::from_secs(180),
        &dir.join("compile.out"),
        &dir.join("compile.err"),
    )?;

    let mut output = outcome.stdout;
    output.push_str(&outcome.stderr);

    let ok = !outcome.timed_out && outcome.exit_code == Some(0);
    Ok(CompileOutcome {
        ok,
        ms: outcome.elapsed_ms,
        output,
        exe: if ok { Some(exe) } else { None },
    })
}

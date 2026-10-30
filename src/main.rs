//! RGC OJ 用户侧评测 agent。
//!
//! 定位：只跑**自己的**提交，结果只用于练习与排行，不做防作弊。
//! 因此 agent 的设计目标是「跑得准、跑得稳」，而不是「防得住」。

mod api;
mod compile;
mod config;
mod exec;
mod hardware;
mod job;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tracing::{info, warn};

const USAGE: &str = "\
rgc-agent — RGC OJ 用户侧评测 agent

用法:
  rgc-agent pair --server <URL> --code <配对码>   用配对码换取长期 token
  rgc-agent run [--once]                          常驻：拉取自己的提交并评测
  rgc-agent local <job.json>                      离线跑一个任务文件（不联网）
  rgc-agent hardware                              打印本机 GPU 指纹与分桶键

环境变量:
  RGC_AGENT_SERVER / RGC_AGENT_TOKEN   覆盖配置文件
  NVCC                                 nvcc 路径（默认 nvcc）
  RGC_AGENT_KEEP=1                     保留临时目录，便于排查
";

#[tokio::main]
async fn main() -> Result<()> {
    // 日志走 stderr，stdout 只留给数据（local 模式输出 JSON）。
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("pair") => cmd_pair(&args[1..]).await,
        Some("run") => cmd_run(&args[1..]).await,
        Some("local") => cmd_local(&args[1..]).await,
        Some("hardware") => cmd_hardware(),
        _ => {
            print!("{USAGE}");
            Ok(())
        }
    }
}

// ------------------------------------------------------------------ 子命令

async fn cmd_pair(args: &[String]) -> Result<()> {
    let server = flag(args, "--server").context("缺少 --server")?;
    let code = flag(args, "--code").context("缺少 --code")?;

    let hw = hardware::detect()?;
    info!(gpu = %hw.gpu, bucket = %hw.bucket(), "本机指纹");

    let paired = api::Client::pair(server, code, &hw).await?;
    let cfg = config::Config {
        server: server.trim_end_matches('/').to_string(),
        token: paired.token,
        agent_id: paired.agent_id,
    };
    let path = config::save(&cfg)?;

    println!("配对成功，配置已写入 {}", path.display());
    println!("接下来运行：rgc-agent run");
    Ok(())
}

async fn cmd_run(args: &[String]) -> Result<()> {
    let once = args.iter().any(|a| a == "--once");
    let cfg = config::load()?;
    let hw = hardware::detect()?;
    let client = api::Client::new(&cfg.server, &cfg.token)?;

    info!(
        server = %cfg.server,
        gpu = %hw.gpu,
        bucket = %hw.bucket(),
        arch = %hw.arch(),
        "agent 上线"
    );

    loop {
        match client.next_job(25).await {
            Ok(Some(j)) => {
                info!(job = j.id, slug = %j.problem_slug, lang = %j.language, "领到任务");
                let job_id = j.id;
                let hw2 = hw.clone();
                let report = match tokio::task::spawn_blocking(move || process_blocking(&j, &hw2)).await {
                    Ok(Ok(r)) => r,
                    Ok(Err(e)) => {
                        warn!(job = job_id, error = %e, "评测过程出错");
                        continue;
                    }
                    Err(e) => {
                        warn!(job = job_id, error = %e, "评测任务 panic");
                        continue;
                    }
                };
                info!(job = job_id, status = %report.status, "评测完成");
                if let Err(e) = client.report(job_id, &report).await {
                    warn!(job = job_id, error = %e, "上报失败");
                }
                if once {
                    return Ok(());
                }
            }
            Ok(None) => {
                if once {
                    info!("没有待评测任务");
                    return Ok(());
                }
                let _ = client.heartbeat(&hw).await;
            }
            Err(e) => {
                warn!(error = %e, "拉取任务失败，5 秒后重试");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn cmd_local(args: &[String]) -> Result<()> {
    let path = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .context("用法：rgc-agent local <job.json>")?;

    let raw = std::fs::read_to_string(path).with_context(|| format!("读取 {path} 失败"))?;
    let job: job::Job = serde_json::from_str(&raw).context("任务文件格式不匹配")?;
    let hw = hardware::detect()?;

    let report = tokio::task::spawn_blocking(move || process_blocking(&job, &hw)).await??;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn cmd_hardware() -> Result<()> {
    let hw = hardware::detect()?;
    println!("{}", serde_json::to_string_pretty(&hw)?);
    println!("分桶键: {}", hw.bucket());
    println!("编译架构: {}", hw.arch());
    Ok(())
}

// ------------------------------------------------------------------ 评测主流程

fn process_blocking(job: &job::Job, hw: &hardware::Hardware) -> Result<job::Report> {
    let ws = workspace()?;
    let dir = ws.path();
    info!(job = job.id, dir = %dir.display(), "工作目录");

    let compiled = compile::compile(job, dir, hw)?;
    if !compiled.ok {
        return Ok(job::Report {
            status: "CompilationError".to_string(),
            compile_ok: false,
            compile_ms: compiled.ms,
            compile_output: truncate(&compiled.output, 8192),
            max_run_us: 0,
            cases: Vec::new(),
            hardware: hw.clone(),
        });
    }

    let exe = compiled.exe.context("编译成功但缺少可执行文件")?;
    let cases_path = dir.join("cases.txt");
    std::fs::write(&cases_path, job.cases_text()).context("写入用例文件失败")?;

    let mut cmd = Command::new(&exe);
    cmd.arg(&cases_path);
    let run = exec::run_with_timeout(
        &mut cmd,
        job.run_timeout(),
        &dir.join("run.out"),
        &dir.join("run.err"),
    )?;

    let mut cases: Vec<job::CaseResult> = if run.timed_out {
        job.judge
            .cases
            .iter()
            .map(|c| job::CaseResult {
                ordinal: c.ordinal,
                status: "TimeLimitExceeded".to_string(),
                err: "运行超时".to_string(),
                ..Default::default()
            })
            .collect()
    } else {
        let mut parsed = job::parse_case_lines(&run.stdout);
        for spec in &job.judge.cases {
            if !parsed.iter().any(|c| c.ordinal == spec.ordinal) {
                parsed.push(job::CaseResult {
                    ordinal: spec.ordinal,
                    status: "RuntimeError".to_string(),
                    err: format!(
                        "测试点未产出结果（{}）{}",
                        exit_desc(&run),
                        tail(&run.stderr, 400)
                    ),
                    ..Default::default()
                });
            }
        }
        parsed.sort_by_key(|c| c.ordinal);
        parsed
    };

    for c in &mut cases {
        if c.status.is_empty() {
            c.status = if !c.err.is_empty() {
                "RuntimeError"
            } else if c.ok {
                "Accepted"
            } else {
                "WrongAnswer"
            }
            .to_string();
        }
    }

    let max_run_us = (cases
        .iter()
        .map(|c| c.p50_ms)
        .fold(0.0_f64, f64::max)
        * 1000.0)
        .round() as u64;

    Ok(job::Report {
        status: job::final_status(&cases).to_string(),
        compile_ok: true,
        compile_ms: compiled.ms,
        compile_output: truncate(&compiled.output, 2048),
        max_run_us,
        cases,
        hardware: hw.clone(),
    })
}

// ------------------------------------------------------------------ 工具

enum Workspace {
    Temp(tempfile::TempDir),
    Kept(PathBuf),
}

impl Workspace {
    fn path(&self) -> &Path {
        match self {
            Workspace::Temp(t) => t.path(),
            Workspace::Kept(p) => p,
        }
    }
}

fn workspace() -> Result<Workspace> {
    let dir = tempfile::Builder::new().prefix("rgc-agent-").tempdir()?;
    if std::env::var("RGC_AGENT_KEEP").ok().as_deref() == Some("1") {
        let path = dir.keep();
        info!(dir = %path.display(), "已保留临时目录");
        return Ok(Workspace::Kept(path));
    }
    Ok(Workspace::Temp(dir))
}

fn exit_desc(run: &exec::RunOutcome) -> String {
    match (run.exit_code, run.signal) {
        (_, Some(sig)) => format!("被信号 {sig} 终止"),
        (Some(code), _) => format!("退出码 {code}"),
        _ => "未知退出状态".to_string(),
    }
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…（已截断）", &s[..end])
}

fn tail(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= max {
        return trimmed.to_string();
    }
    let mut start = trimmed.len() - max;
    while start < trimmed.len() && !trimmed.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &trimmed[start..])
}

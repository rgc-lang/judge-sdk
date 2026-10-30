//! 进程执行：进程组隔离 + 超时整组终止 + 输出落文件（避免管道写满死锁）。

use anyhow::{Context, Result};
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct RunOutcome {
    pub timed_out: bool,
    pub exit_code: Option<i32>,
    /// 被信号杀死时的信号值（例如 11 = SIGSEGV）。
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u64,
}

/// 输出最多读回 256 KiB，防止用户代码刷屏把内存吃光。
const MAX_CAPTURE: usize = 256 * 1024;

/// 跑一个命令，超时则 kill 整个进程组。
pub fn run_with_timeout(
    cmd: &mut Command,
    timeout: Duration,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<RunOutcome> {
    let out = File::create(stdout_path).context("创建 stdout 文件失败")?;
    let err = File::create(stderr_path).context("创建 stderr 文件失败")?;

    cmd.stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .stdin(Stdio::null())
        .process_group(0);

    let started = Instant::now();
    let mut child = cmd.spawn().context("启动子进程失败")?;
    let pid = child.id() as i32;

    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            // 负 pid = 整个进程组，连子进程的子进程一起收掉。
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    use std::os::unix::process::ExitStatusExt;

    Ok(RunOutcome {
        timed_out,
        exit_code: status.and_then(|s| s.code()),
        signal: status.and_then(|s| s.signal()),
        stdout: read_capped(stdout_path),
        stderr: read_capped(stderr_path),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

fn read_capped(path: &Path) -> String {
    match std::fs::read(path) {
        Ok(bytes) => {
            let slice = if bytes.len() > MAX_CAPTURE {
                &bytes[..MAX_CAPTURE]
            } else {
                &bytes[..]
            };
            String::from_utf8_lossy(slice).into_owned()
        }
        Err(_) => String::new(),
    }
}

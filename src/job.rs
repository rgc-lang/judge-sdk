//! 任务与结果的线上协议。

use serde::{Deserialize, Serialize};

use crate::hardware::Hardware;

#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    pub id: i64,
    #[serde(default)]
    pub problem_slug: String,
    /// cuda | triton | rgc（当前只实现 cuda）
    #[serde(default = "default_language")]
    pub language: String,
    pub code: String,
    /// CPU 参考实现，与用户代码同一套签名，用于产生期望值。
    #[serde(default)]
    pub reference_code: String,
    #[serde(default = "default_time_limit")]
    pub time_limit_ms: u64,
    #[serde(default)]
    pub judge: JudgeSpec,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct JudgeSpec {
    #[serde(default = "default_rtol")]
    pub rtol: f64,
    #[serde(default = "default_atol")]
    pub atol: f64,
    #[serde(default = "default_warmup")]
    pub warmup: u32,
    #[serde(default = "default_repeat")]
    pub repeat: u32,
    #[serde(default)]
    pub cases: Vec<CaseSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CaseSpec {
    pub ordinal: i64,
    #[serde(default)]
    pub a_len: i32,
    #[serde(default)]
    pub b_len: i32,
    #[serde(default)]
    pub out_len: i32,
    #[serde(default = "one")]
    pub m: i32,
    #[serde(default = "one")]
    pub n: i32,
    #[serde(default = "one")]
    pub k: i32,
    #[serde(default = "default_seed")]
    pub seed: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaseResult {
    pub ordinal: i64,
    #[serde(default)]
    pub p50_ms: f64,
    #[serde(default)]
    pub p90_ms: f64,
    #[serde(default)]
    pub max_abs_err: f64,
    #[serde(default)]
    pub max_rel_err: f64,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub err: String,
    /// 由 agent 填充：Accepted / WrongAnswer / TimeLimitExceeded / RuntimeError
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub status: String,
    pub compile_ok: bool,
    pub compile_ms: u64,
    pub compile_output: String,
    /// 最慢测试点的 p50，单位微秒——GPU 算子普遍在亚毫秒级，毫秒精度不够。
    pub max_run_us: u64,
    pub cases: Vec<CaseResult>,
    pub hardware: Hardware,
}

impl Job {
    /// 渲染 harness 读取的用例文件。格式与 `harness/cuda.cu` 里的 sscanf 一一对应。
    pub fn cases_text(&self) -> String {
        let mut out = String::from("# ordinal a_len b_len out_len m n k seed warmup repeat rtol atol
");
        for c in &self.judge.cases {
            out.push_str(&format!(
                "{} {} {} {} {} {} {} {} {} {} {} {}\n",
                c.ordinal,
                c.a_len,
                c.b_len,
                c.out_len,
                c.m,
                c.n,
                c.k,
                c.seed,
                self.judge.warmup,
                self.judge.repeat,
                self.judge.rtol,
                self.judge.atol,
            ));
        }
        out
    }

    /// 单次运行的超时上限：给足 warmup + repeat 的余量，再兜一个下限。
    pub fn run_timeout(&self) -> std::time::Duration {
        // 调试用：直接钉死超时，避免为了验证 TLE 干等一分钟。
        if let Ok(v) = std::env::var("RGC_AGENT_RUN_TIMEOUT_MS") {
            if let Ok(ms) = v.parse::<u64>() {
                return std::time::Duration::from_millis(ms.max(100));
            }
        }
        let per_case = self
            .judge
            .warmup
            .saturating_add(self.judge.repeat)
            .max(1) as u64
            * self.time_limit_ms.max(50)
            * 2;
        let total = per_case.saturating_mul(self.judge.cases.len().max(1) as u64);
        std::time::Duration::from_millis(total.clamp(60_000, 600_000))
    }
}

/// 解析 harness 输出里的 `##CASE## {json}` 行。
pub fn parse_case_lines(stdout: &str) -> Vec<CaseResult> {
    stdout
        .lines()
        .filter_map(|line| line.trim().strip_prefix("##CASE## "))
        .filter_map(|json| serde_json::from_str::<CaseResult>(json).ok())
        .collect()
}

/// 汇总判定。空结果视为系统错误，避免「什么都没跑」被判 AC。
pub fn final_status(cases: &[CaseResult]) -> &'static str {
    if cases.is_empty() {
        return "SystemError";
    }
    if cases.iter().any(|c| c.status == "TimeLimitExceeded") {
        return "TimeLimitExceeded";
    }
    if cases.iter().any(|c| !c.err.is_empty()) {
        return "RuntimeError";
    }
    if cases.iter().all(|c| c.ok) {
        "Accepted"
    } else {
        "WrongAnswer"
    }
}

impl Default for JudgeSpec {
    fn default() -> Self {
        Self {
            rtol: default_rtol(),
            atol: default_atol(),
            warmup: default_warmup(),
            repeat: default_repeat(),
            cases: Vec::new(),
        }
    }
}

fn default_language() -> String {
    "cuda".to_string()
}
fn default_time_limit() -> u64 {
    1000
}
fn default_rtol() -> f64 {
    1e-5
}
fn default_atol() -> f64 {
    1e-6
}
fn default_warmup() -> u32 {
    10
}
fn default_repeat() -> u32 {
    50
}
fn default_seed() -> u64 {
    42
}
fn one() -> i32 {
    1
}

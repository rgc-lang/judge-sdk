//! 平台 API 客户端。协议见 `web/backend` 的 `/api/v1/agent/*`。

use anyhow::{bail, Context, Result};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

use crate::hardware::Hardware;
use crate::job::{Job, Report};

pub struct Client {
    http: reqwest::Client,
    base: String,
    token: String,
}

#[derive(Debug, Deserialize)]
pub struct PairResponse {
    pub token: String,
    #[serde(default)]
    pub agent_id: String,
}

#[derive(Serialize)]
struct PairRequest<'a> {
    code: &'a str,
    hardware: &'a Hardware,
}

fn build_http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("rgc-agent/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(120))
        .build()
        .context("构建 HTTP 客户端失败")
}

fn base_of(server: &str) -> String {
    server.trim_end_matches('/').to_string()
}

impl Client {
    pub fn new(server: &str, token: &str) -> Result<Self> {
        Ok(Self {
            http: build_http()?,
            base: base_of(server),
            token: token.to_string(),
        })
    }

    /// 一次性配对码换长期 token。
    pub async fn pair(server: &str, code: &str, hw: &Hardware) -> Result<PairResponse> {
        let url = format!("{}/api/v1/agent/pair", base_of(server));
        let resp = build_http()?
            .post(url)
            .json(&PairRequest { code, hardware: hw })
            .send()
            .await
            .context("配对请求失败")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("配对失败 HTTP {status}: {body}");
        }
        Ok(resp.json().await.context("解析配对响应失败")?)
    }

    /// 长轮询领取自己的待评测提交。204 表示这段时间内没有任务。
    pub async fn next_job(&self, wait_secs: u64) -> Result<Option<Job>> {
        let url = format!("{}/api/v1/agent/jobs/next?wait={}", self.base, wait_secs);
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("拉取任务失败")?;

        match resp.status() {
            StatusCode::NO_CONTENT => Ok(None),
            s if s.is_success() => {
                let value: Value = resp.json().await.context("解析任务响应失败")?;
                let job = value.get("job").cloned().unwrap_or(value);
                if job.is_null() {
                    return Ok(None);
                }
                Ok(Some(serde_json::from_value(job).context("任务格式不匹配")?))
            }
            s => {
                let body = resp.text().await.unwrap_or_default();
                bail!("拉取任务失败 HTTP {s}: {body}")
            }
        }
    }

    pub async fn report(&self, job_id: i64, report: &Report) -> Result<()> {
        let url = format!("{}/api/v1/agent/jobs/{}/result", self.base, job_id);
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(report)
            .send()
            .await
            .context("上报结果失败")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("上报结果失败 HTTP {status}: {body}");
        }
        Ok(())
    }

    pub async fn heartbeat(&self, hw: &Hardware) -> Result<()> {
        let url = format!("{}/api/v1/agent/heartbeat", self.base);
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(hw)
            .send()
            .await
            .context("心跳失败")?;
        if !resp.status().is_success() {
            bail!("心跳失败 HTTP {}", resp.status());
        }
        Ok(())
    }
}

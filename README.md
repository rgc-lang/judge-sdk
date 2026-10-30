# rgc-agent

RGC OJ 的用户侧评测 agent：用户在自己的 GPU 机器上装一个二进制，拉取**自己的**提交，
本地编译、本地运行、回传结果。

定位是**练习工具**，不是正式比赛裁判：

- 只跑自己的提交，不跨用户派发
- 结果只用于练习与排行，不做防作弊
- 榜单按「同硬件」分桶比较，绝对耗时才有意义

## 安装

一行装到 `~/.local/bin`（静态链接，不挑发行版）：

```bash
curl -fsSL https://github.com/rgc-lang/judge-sdk/releases/latest/download/install.sh | bash
```

支持 **Linux x86_64 / aarch64**（脚本按 `uname -m` 自动选包）。
macOS 没有 CUDA，Windows 需要单独移植，两者都会直接报错退出。

或者直接下压缩包：

```bash
curl -LO https://github.com/rgc-lang/judge-sdk/releases/latest/download/rgc-agent-linux-x86_64.tar.gz
tar -xzf rgc-agent-linux-x86_64.tar.gz
```

发布走 GitHub Releases（CDN 直出）：打 `v*` tag 触发 `.github/workflows/release.yml`，
矩阵构建 `x86_64-unknown-linux-musl` 与 `aarch64-unknown-linux-musl`（cross 交叉编译），
产物连同 `install.sh` 一起上传。

## 构建

```bash
cargo build --release
# 产物：target/release/rgc-agent
```

依赖：Linux + NVIDIA 驱动（`libcuda`）、CUDA 工具链（`nvcc`）。

## 用法

```bash
# 1. 用平台给的一次性配对码换长期 token（写入 ~/.config/rgc-agent/config.toml，权限 0600）
rgc-agent pair --server https://rgc-dev.tensorplay.cn --code ABCD-1234

# 2. 常驻：长轮询拉取自己的待评测提交
rgc-agent run

# 3. 离线跑一个任务文件（不联网，用于开发与排查）
rgc-agent local examples/saxpy.job.json

# 4. 打印本机 GPU 指纹与榜单分桶键
rgc-agent hardware
```

环境变量：`RGC_AGENT_SERVER` / `RGC_AGENT_TOKEN` 覆盖配置，`NVCC` 指定 nvcc 路径，
`RGC_AGENT_KEEP=1` 保留临时目录，`RGC_AGENT_RUN_TIMEOUT_MS` 钉死运行超时（调试用）。

## 与平台的协议

### 配对

```
POST /api/v1/agent/pair
{ "code": "ABCD-1234", "hardware": { ... } }
→ { "token": "rgc_...", "agent_id": "..." }
```

### 领任务（长轮询）

```
GET /api/v1/agent/jobs/next?wait=25
Authorization: Bearer rgc_...
→ 200 { "job": { ... } }   或   204 No Content
```

服务端必须只返回 `user_id = 当前 agent 的 user` 且 `status='Pending'` 的提交。

### 上报

```
POST /api/v1/agent/jobs/{id}/result
Authorization: Bearer rgc_...
{ "status": "Accepted", "compile_ok": true, "compile_ms": 1695,
  "compile_output": "", "max_run_us": 241, "cases": [ ... ], "hardware": { ... } }
```

`status` ∈ `Accepted / WrongAnswer / TimeLimitExceeded / RuntimeError / CompilationError / SystemError`。

### 心跳

```
POST /api/v1/agent/heartbeat
Authorization: Bearer rgc_...
{ "gpu": "...", "count": 1, ... }
```

## 任务格式

```jsonc
{
  "id": 1,
  "problem_slug": "saxpy",
  "language": "cuda",
  "time_limit_ms": 1000,
  "code": "用户提交的源码",
  "reference_code": "CPU 参考实现，签名与 solution 相同",
  "judge": {
    "rtol": 1e-5, "atol": 1e-6,
    "warmup": 10, "repeat": 50,
    "cases": [
      { "ordinal": 1, "a_len": 1048576, "b_len": 1048576, "out_len": 1048576,
        "m": 1048576, "n": 1, "k": 1, "seed": 42 }
    ]
  }
}
```

用户只写两个东西：

```cuda
void solution(const RgcArgs* args);   // 必写，跑在 GPU 上
```

参考实现由题目提供，跑在 CPU 上产生期望值：

```c
void reference(const RgcArgs* args);
```

`RgcArgs` 定义在 `harness/cuda.cu`：

```c
typedef struct {
    const float* a; const float* b; float* out;
    int a_len, b_len, out_len;
    int m, n, k;      // 2D 语义；1D 时 k = 1
} RgcArgs;
```

### 为什么参考实现也在本地跑

算子题的输出是几 MB 的数组，下发期望输出没有意义——用户拿到也抄不了，真要抄得先算对。
本地比对省掉一条几 MB 的上行链路，也省掉服务端存参考输出的存储。

## 计时协议

- 用 CUDA events 只圈内核执行，不含进程启动、内存分配、H2D/D2H 拷贝
- 先 warmup（默认 10 次）再计时，排除 PTX JIT 与频率爬升
- 每用例重复 `repeat` 次（默认 50），上报 p50 与 p90，榜单用 p50
- `-arch` 由本机 GPU 决定（agent 侧编译的原因），其余编译参数固定

## harness 协议

agent 把用例渲染成一行一条：

```
# ordinal a_len b_len out_len m n k seed warmup repeat rtol atol
1 1048576 1048576 1048576 1048576 1 1 42 10 50 1e-5 1e-6
```

程序每个用例输出一行：

```
##CASE## {"ordinal":1,"p50_ms":0.0184,"p90_ms":0.0193,"max_abs_err":0,"max_rel_err":0,"ok":true,"err":""}
```

判定：`|got - exp| <= atol + rtol * |exp|`，NaN 直接判错。

## 目录

```
src/main.rs      子命令与评测主流程
src/api.rs       平台 API 客户端
src/compile.rs   模板拼装 + nvcc 调用
src/exec.rs      进程组隔离、超时整组终止、输出落文件
src/hardware.rs  GPU 指纹与分桶键
src/job.rs       任务/结果类型与判定汇总
harness/cuda.cu  CUDA harness 模板（编译期嵌入二进制）
```

## 已实现 / 未实现

- [x] 配对、长轮询领任务、上报、心跳
- [x] CUDA 编译与运行、容差判定、AC/WA/TLE/RE/CE 全路径
- [x] 硬件指纹与分桶键
- [ ] Triton（需要 Python 环境，计时语义要单独处理 JIT）
- [ ] 服务端 `/api/v1/agent/*` 端点（当前只有 `local` 模式可跑）
- [ ] 榜单聚合与前端

// RGC OJ CUDA harness —— 由 agent 在编译前注入用户实现与参考实现。
//
// 协议：
//   argv[1] = 用例文件，每行 "ordinal a_len b_len out_len m n k seed warmup repeat"
//   每个用例输出一行 "##CASE## {json}"，agent 解析后回传平台。
//
// 计时用 CUDA events，只圈内核执行，不含进程启动、分配与拷贝。
// 参考实现在 CPU 上跑，用来产生期望值——算子题的输出是几 MB 的数组，
// 下发期望输出没有意义（拿到也抄不了），本地比对更省带宽。

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cmath>
#include <cuda_runtime.h>
#include <vector>
#include <algorithm>

typedef struct {
    const float* a;
    const float* b;
    float* out;
    int a_len;
    int b_len;
    int out_len;
    int m;
    int n;
    int k;
} RgcArgs;

/* ==================== 用户实现 ==================== */
{{USER_CODE}}

/* ==================== 参考实现 ==================== */
{{REFERENCE_CODE}}

static void fail(const char* msg) {
    fprintf(stderr, "harness: %s\n", msg);
    exit(3);
}

static unsigned long long g_state = 88172645463325252ULL;
static float next_f32(void) {
    g_state ^= g_state << 13;
    g_state ^= g_state >> 7;
    g_state ^= g_state << 17;
    return (float)((double)(g_state & 0xFFFFFFu) / (double)0xFFFFFFu) * 2.0f - 1.0f;
}

static void emit(int ordinal, double p50, double p90, double abs_err, double rel_err,
                 int ok, const char* err) {
    printf("##CASE## {\"ordinal\":%d,\"p50_ms\":%.6f,\"p90_ms\":%.6f,"
           "\"max_abs_err\":%.9g,\"max_rel_err\":%.9g,\"ok\":%s,\"err\":\"%s\"}\n",
           ordinal, p50, p90, abs_err, rel_err, ok ? "true" : "false", err);
    fflush(stdout);
}

int main(int argc, char** argv) {
    if (argc < 2) fail("usage: prog <cases.txt>");

    FILE* f = fopen(argv[1], "r");
    if (!f) fail("cannot open cases file");

    char line[1024];
    while (fgets(line, sizeof(line), f)) {
        if (line[0] == '#' || line[0] == '\n' || line[0] == '\r') continue;

        int ordinal = 0, a_len = 0, b_len = 0, out_len = 0;
        int m = 0, n = 0, k = 0, warmup = 0, repeat = 0;
        unsigned long long seed = 0;
        double rtol = 1e-5, atol = 1e-6;
        int parsed = sscanf(line, "%d %d %d %d %d %d %d %llu %d %d %lf %lf",
                            &ordinal, &a_len, &b_len, &out_len, &m, &n, &k,
                            &seed, &warmup, &repeat, &rtol, &atol);
        if (parsed != 12) fail("bad case line");
        if (repeat < 1) repeat = 1;
        if (warmup < 0) warmup = 0;

        g_state = seed ? seed : 88172645463325252ULL;

        std::vector<float> ha((size_t)(a_len > 0 ? a_len : 1));
        std::vector<float> hb((size_t)(b_len > 0 ? b_len : 1));
        std::vector<float> hout((size_t)(out_len > 0 ? out_len : 1));
        std::vector<float> hexp((size_t)(out_len > 0 ? out_len : 1), 0.0f);
        for (int i = 0; i < a_len; i++) ha[i] = next_f32();
        for (int i = 0; i < b_len; i++) hb[i] = next_f32();
        for (int i = 0; i < out_len; i++) hout[i] = 0.0f;

        RgcArgs ref_args;
        ref_args.a = ha.data();
        ref_args.b = hb.data();
        ref_args.out = hexp.data();
        ref_args.a_len = a_len;
        ref_args.b_len = b_len;
        ref_args.out_len = out_len;
        ref_args.m = m;
        ref_args.n = n;
        ref_args.k = k;
        reference(&ref_args);

        float *da = NULL, *db = NULL, *dout = NULL;
        cudaError_t e = cudaSuccess;
        if (a_len > 0) e = cudaMalloc((void**)&da, sizeof(float) * (size_t)a_len);
        if (e == cudaSuccess && b_len > 0) e = cudaMalloc((void**)&db, sizeof(float) * (size_t)b_len);
        if (e == cudaSuccess && out_len > 0) e = cudaMalloc((void**)&dout, sizeof(float) * (size_t)out_len);
        if (e != cudaSuccess) {
            emit(ordinal, 0, 0, 0, 0, 0, cudaGetErrorString(e));
            continue;
        }

        if (a_len > 0) cudaMemcpy(da, ha.data(), sizeof(float) * (size_t)a_len, cudaMemcpyHostToDevice);
        if (b_len > 0) cudaMemcpy(db, hb.data(), sizeof(float) * (size_t)b_len, cudaMemcpyHostToDevice);
        if (out_len > 0) cudaMemset(dout, 0, sizeof(float) * (size_t)out_len);

        RgcArgs args;
        args.a = da;
        args.b = db;
        args.out = dout;
        args.a_len = a_len;
        args.b_len = b_len;
        args.out_len = out_len;
        args.m = m;
        args.n = n;
        args.k = k;

        for (int i = 0; i < warmup; i++) solution(&args);
        e = cudaGetLastError();
        cudaDeviceSynchronize();
        if (e != cudaSuccess) {
            emit(ordinal, 0, 0, 0, 0, 0, cudaGetErrorString(e));
            cudaFree(da); cudaFree(db); cudaFree(dout);
            continue;
        }

        std::vector<double> times;
        times.reserve((size_t)repeat);
        const char* run_err = "";
        for (int i = 0; i < repeat; i++) {
            cudaEvent_t start, stop;
            cudaEventCreate(&start);
            cudaEventCreate(&stop);
            cudaEventRecord(start);
            solution(&args);
            cudaEventRecord(stop);
            cudaEventSynchronize(stop);
            float ms = 0.0f;
            cudaEventElapsedTime(&ms, start, stop);
            cudaEventDestroy(start);
            cudaEventDestroy(stop);

            e = cudaGetLastError();
            if (e != cudaSuccess) { run_err = cudaGetErrorString(e); break; }
            times.push_back((double)ms);
        }

        if (!run_err[0] && !times.empty()) {
            cudaMemcpy(hout.data(), dout, sizeof(float) * (size_t)out_len, cudaMemcpyDeviceToHost);
            cudaDeviceSynchronize();
            e = cudaGetLastError();
            if (e != cudaSuccess) run_err = cudaGetErrorString(e);
        }

        if (run_err[0] || times.empty()) {
            emit(ordinal, 0, 0, 0, 0, 0, run_err);
            cudaFree(da); cudaFree(db); cudaFree(dout);
            continue;
        }

        std::sort(times.begin(), times.end());
        double p50 = times[(size_t)((double)(times.size() - 1) * 0.50)];
        double p90 = times[(size_t)((double)(times.size() - 1) * 0.90)];

        double max_abs = 0.0, max_rel = 0.0;
        int nan_seen = 0, within = 1;
        for (int i = 0; i < out_len; i++) {
            double got = (double)hout[i];
            double exp = (double)hexp[i];
            if (std::isnan(got) || std::isnan(exp)) { nan_seen = 1; continue; }
            double d = std::fabs(got - exp);
            if (d > max_abs) max_abs = d;
            double denom = std::fabs(exp) > 1e-12 ? std::fabs(exp) : 1e-12;
            double r = d / denom;
            if (r > max_rel) max_rel = r;
            // 容差判定：|got - exp| <= atol + rtol * |exp|
            if (d > atol + rtol * std::fabs(exp)) within = 0;
        }

        emit(ordinal, p50, p90, max_abs, max_rel, (nan_seen || !within) ? 0 : 1,
             nan_seen ? "output contains NaN" : "");

        cudaFree(da);
        cudaFree(db);
        cudaFree(dout);
    }

    fclose(f);
    return 0;
}

# 用 Rust 重写打标服务：可行性与收益评估

> 评估日期：2026-11-14 · 基线 `git HEAD d0db2a8`
> 范围：**只针对打标（tagger）这一个环节**，不含美学模型。
> 全量推理服务的迁移评估见 [`docs/RUST_INFERENCE_FEASIBILITY.md`](./RUST_INFERENCE_FEASIBILITY.md)，本文是其中「打标」一行在实测之后的展开。

---

## 0. 结论速览

### 0.1 一句话结论

> **技术上完全可行，而且已经在本机跑通了** —— ONNX 推理、CUDA 加速、数值一致性三项全部实测通过。
> **但收益远小于预期**：真机上打标任务 98% 以上的耗时是 SauceNAO 网络往返，本地推理只占 1–2%；
> 单看「Rust 比 Python 快」只有约 5%。**建议不把它当作性能优化项，也不要现在动手。**

### 0.2 四项判据

| 判据 | 结论 | 证据强度 |
|---|---|---|
| 能否加载 cl_tagger 模型并推理 | **能**。ort 2.0.0-rc.10 直读 model.onnx + model.onnx.data，108139 标签词表正常 | 已验证 |
| GPU 能否用 | **能**。CUDA EP 生效，纯推理 76.8 ms/张 | 已验证 |
| 数值是否一致 | **一致**。同一张图 Python 47 标签 / Rust 47 标签，交集 47，Jaccard 1.0000 | 已验证 |
| 值不值得做 | **不值得（按当前数据）**。见 §0.4 与 §5 | 已验证 |

### 0.3 与原可行性报告的差异（重要更正）

原报告 `RUST_INFERENCE_FEASIBILITY.md` 的 **R3 风险**写的是：

> ort 的 CUDA 要求（CUDA 13.2 / cuDNN 9.23 在 PATH）比现状（torch wheel 自带 CUDA）更重，会让「一键装 GPU」体验倒退

**这条经实测需要分版本讨论，原文把两个不同 ORT 版本的要求混在了一起：**

| ort crate 版本 | 预编译 ORT | Windows CUDA 要求 | 能否复用 torch 现役 CUDA DLL |
|---|---|---|---|
| **rc.10**（2025-06-01，本文实测用） | **1.22.0** | **CUDA 12.x + cuDNN 9** | **能，torch cu128 的 DLL 直接可用** |
| rc.11 – rc.15（2026-01-07 起） | 1.31.0 | CUDA 13 | 不能，需另外找 CUDA 13 的 DLL |

依据：`ort-sys-2.0.0-rc.10/build.rs:8` 的 `ONNXRUNTIME_VERSION = "1.22.0"`；rc.15 的 `build/download/dist.tsv` 里 Windows 只剩 cuda13 feature set（`x86_64-pc-windows-msvc+cuda13,tensorrt,nvrtx,directml`）。

**这条更正的实际意义**：在本机这种「Python 美学仍在、torch 的 4.2 GB 已经装着」的场景下，Rust 打标可以**直接借用 torch/lib 里的 CUDA DLL，零额外体积**。所谓「部署门槛倒退」并不成立。

### 0.4 那为什么不建议做

因为**收益不在语言上**。真机上打标任务的实测耗时（jobs 表）：

| 任务 | 张数 | 耗时 | 折算 | 本地推理占比 |
|---|---|---|---|---|
| job #152 | 38 | 290 s | **7.6 s/张** | 126 ms / 7600 ms = **1.7%** |
| job #153 | 29 | 387 s | **13.3 s/张** | **0.9%** |

即使把本地推理优化到 0 ms，这两个任务也只能快 1–2%。真正的瓶颈是 SauceNAO 的网络往返与限流，那是另一个议题（多出口、key 池、任务并发）。

---

## 1. 可行性验证：已经跑通的部分

### 1.1 试验程序

```
scripts/ort_trial/
  Cargo.toml        ort = "=2.0.0-rc.10", features = ["cuda","download-binaries","copy-dylibs"]
                    + ndarray 0.16 + image 0.25 + serde_json 1
  src/main.rs       端到端：解码 -> 预处理 -> 推理 -> sigmoid -> 阈值 -> 降序，含分段计时
  src/bin/conc.rs   并发吞吐（多线程并行解码/预处理 + 单 Session 互斥包 run）
  src/bin/bench2.rs 可配 CUDA_DLL_DIR 的基准，用于 DLL 剪枝
  src/bin/dlldir.rs 验证 SetDefaultDllDirectories + AddDllDirectory
  src/bin/ver.rs    版本自检
```

首次 `cargo build --release` 耗时 **51.03 s**（含 ort-sys 下载预编译 ORT）。

### 1.2 推理链路（与 Python 逐行等价）

| 步骤 | Python（`tagger_model.py`） | Rust（`ort_trial/src/main.rs`） |
|---|---|---|
| 读图 | `Image.open(path).convert("RGB")` | `image::open(path)?.to_rgb8()` |
| resize | `AutoProcessor`（SiglipImageProcessor，resample=BILINEAR，384x384） | `imageops::resize(&rgb, 384, 384, FilterType::Triangle)` |
| 归一化 | mean/std 均 0.5（processor 配置） | `(p as f32 / 255.0 - 0.5) / 0.5` |
| 布局 | NCHW | 手工填 `d[c*nn + i]` |
| 推理 | `session.run(["logits"], inputs)` | `sess.run(ort::inputs![tensor])` |
| 激活 | `1/(1+exp(-logits))` | `1.0/(1.0+(-x).exp())` |
| 过滤 | `prob >= threshold`，降序 | 同 |

模型签名实测（onnxruntime 读 session 得到）：1 输入 `pixel_values[N,3,384,384] float32`，1 输出 `logits[N,108139] float32`；`model_metadata.json` 为 `{"vision_encoder_repo":"google/siglip2-so400m-patch14-384","is_naflex":false,"num_tags":108139}`。

**`is_naflex: false` 是关键** —— naFlex 路径需要 `pixel_attention_mask` / `spatial_shapes` / `max_num_patches` 三个额外输入，非 naFlex 只需一个 `pixel_values`，手写预处理因此成立。

### 1.3 数值对拍（同一张图）

测试图：`test/f5e16ea662d6f62e874d19b8eb5af275.png`

| 项 | 结果 |
|---|---|
| Python 标签数 | 47 |
| Rust 标签数 | 47 |
| 交集 | **47** |
| **Jaccard** | **1.0000** |
| 仅 Python 有 / 仅 Rust 有 | 0 / 0 |
| 置信度最大差 | **0.016855**（`highres`） |
| 平均差 | 0.001490 |
| 前 5 大差 | `highres` 0.016855 / `cleavage` 0.0049 / `sensitive` 0.003496 / `smile` 0.003259 / `pantyhose` 0.002667 |

差异来源是 resize 实现不同（PIL 的 u8 整数双线性 vs image crate 的 f32 双线性），不是逻辑错误。40 图批量对拍实测 `prob_maxdiff 0.013272`、**阈值 0.5 处翻转 8 / 2487**。

> **resize 内核选错会静默改变标签**：实测翻转数 BILINEAR 2 处 / BICUBIC 41 处 / LANCZOS 55 处（每 2487 个标签判定）。Rust 侧必须用 `FilterType::Triangle`（对应双线性），换成 `CatmullRom` 或 `Lanczos3` 会悄悄漂移。

### 1.4 GPU 实测

| 项 | 结果 |
|---|---|
| EP 选择 | `CUDAExecutionProvider`（用 `.error_on_failure()` 强制失败即报错，不静默回落 CPU） |
| session 构建 | 2.22–2.43 s |
| 纯 `session.run` | **76.8 ms/张** |
| 端到端（解码+预处理+推理+后处理） | **125.2–130 ms/张** |

**CPU 不可接受**：同程序走 CPU EP 是 **5733–6552 ms/张**（约 50 倍慢）。打标必须走 GPU，CPU 兜底等于不可用 —— 这一点与现状一致（Python 侧 CPU 也一样慢）。
---

## 2. R3 更正：ORT 的 CUDA 依赖到底是什么

### 2.1 依赖链实测

对 `onnxruntime_providers_cuda.dll` 做 `objdump -p`，直接依赖里非系统的部分只有 6 个：

```
cublas64_12.dll
cublasLt64_12.dll
cudart64_12.dll
cudnn64_9.dll
cufft64_11.dll
onnxruntime_providers_shared.dll
```

需要的是 **CUDA 12.x + cuDNN 9**，与 torch 2.11.0+cu128 自带的那套完全同源。原报告写的「CUDA 13.2 / cuDNN 9.23」是 ort crate README 对 rc.15 的官方要求，不是 ORT 1.22 / 1.23 的实际 DLL 依赖。

### 2.2 ORT 不需要 torch

四组 Python 侧探针（`scripts/probe_ort_*.py`）：

| 实验 | 结果 |
|---|---|
| 裸 `import onnxruntime`（不 import torch） | **失败**：`Error loading "...onnxruntime_providers_cuda.dll" which depends on "cublasLt64_12.dll" which is missing. (Error 126)`，回落 `[CPUExecutionProvider]` |
| 只把 `torch/lib` 写进 `os.environ["PATH"]` | **仍然失败**，同一报错 |
| `_h = os.add_dll_directory(TL)` **持有句柄** + PATH 前置 | **成功**，`[CUDAExecutionProvider, CPUExecutionProvider]`，`torch imported? False` |
| 把 DLL 复制到独立目录，只 `add_dll_directory` 指向它 | **成功**，`torch imported? False` |

结论：**ORT 完全不需要 torch，只需要能把 CUDA 12 / cuDNN 9 的 DLL 找得到。**

### 2.3 DLL 怎么给（三种方式，只有两种可行）

| 方式 | Rust | Python | 说明 |
|---|---|---|---|
| 放在 exe 同目录 | ✅ 可用（隐式搜索路径） | ✅ | 最简单，也是建议的交付形态 |
| `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS \| LOAD_LIBRARY_SEARCH_USER_DIRS)` + `AddDllDirectory(宽字符路径)` | ✅ 可用，实测 153.4 / 163.8 ms/张 | ✅（`os.add_dll_directory` 且必须持有返回句柄） | 便于把 1.66 GB 收进子目录 |
| 只设 `PATH` 环境变量 | ❌ **无效** | ❌ **无效** | Windows 的 DLL 搜索顺序里 PATH 优先级低于 exe 目录，且受 SetDefaultDllDirectories 影响 |

> **Python 侧那个「必须持有句柄」的坑很隐蔽**：`os.add_dll_directory(TL)` 的返回值一旦被 GC（写成临时表达式），DLL 目录立刻失效，随后静默回落 CPU。本轮的 `probe_py_inproc.py` 第一次跑就踩了这个坑，得出「推理 5918 ms/张」的假数据。

> **`onnxruntime_providers_cuda.dll` 自身必须留在 exe 同目录**，移到子目录后会报 `which is missing. Error 126`。只有它依赖的 CUDA/cuDNN 可以外置。

### 2.4 最小 DLL 集合（用「真实推理成功」当判据）

判据演进过程值得记录，因为前几轮结论都是错的：

| 轮次 | 判据 | 结论 |
|---|---|---|
| 1–3 | `InferenceSession(providers=[CUDAExecutionProvider])` 能否构造 | ❌ **误判**「12 个 DLL 必需」——构造只验证 DLL 加载，不验证 cuDNN 子库齐全 |
| 6 | **真实推理跑出 47 标签** | ✅ 正确判据 |

只保留「构造能过」的 6 个 DLL 会**真实推理失败**，报 `CUDNN failure 3009: CUDNN_STATUS_NOT_SUPPORTED_SUBLIBRARY_UNAVAILABLE` 与 `Cannot load symbol cudnnCreateFilterDescriptor`，失败节点在 `/vision_encoder/embeddings/patch_embedding/Conv`。

**最终必需集合 = 10 个 DLL / 1660.4 MB**：

| DLL | 字节 | MB |
|---|---|---|
| cublasLt64_12.dll | 674,667,520 | 643.41 |
| cublas64_12.dll | 113,716,224 | 108.45 |
| cudart64_12.dll | 573,952 | 0.55 |
| cufft64_11.dll | 276,121,600 | 263.33 |
| cudnn_engines_precompiled64_9.dll | 481,015,408 | 458.73 |
| cudnn_ops64_9.dll | 105,604,208 | 100.71 |
| cudnn_heuristic64_9.dll | 58,874,992 | 56.15 |
| cudnn_engines_runtime_compiled64_9.dll | 27,759,216 | 26.47 |
| cudnn_graph64_9.dll | 2,493,040 | 2.38 |
| cudnn64_9.dll | 264,304 | 0.25 |
| **合计** | **1,741,090,464** | **1660.4** |

**可删**（实测删掉后真实推理照跑）：`cudnn_adv64_9.dll`（256.6 MB）、`cudnn_cnn64_9.dll`（2.9 MB）、`DirectML.dll`（17.7 MB）、`onnxruntime_providers_tensorrt.dll`（0.6 MB）。

### 2.5 零额外体积的前提：复用 torch/lib

`runtime/python/.venv/Lib/site-packages/torch/lib` 下已逐个核实这 10 个 DLL 全部存在（体积略大，torch 打包的是完整版）。**只要 Python 美学还在，Rust 打标的 CUDA 依赖就是免费的。**

---

## 3. 收益量化

### 3.1 体积

三个口径，结论完全不同：

| 场景 | 现状 | 迁移后 | 净变化 |
|---|---|---|---|
| **A. 共存**（Python 美学保留，复用 torch/lib 的 CUDA DLL） | venv 4956 MB，其中 onnxruntime 包 **391 MB** | 删 onnxruntime 391 MB；新增 exe 22.2 MB（ORT core 静态链接）+ `onnxruntime_providers_cuda.dll` 180.7 MB + `providers_shared` 0.01 MB | **−188 MB** |
| **B. 只迁打标、用户机器没装 torch** | 需装 onnxruntime-gpu 391 MB | 自带 10 DLL 1660.4 MB + 203 MB | **+1,472 MB（更差）** |
| **C. 彻底删 Python**（美学也迁走） | venv **4956 MB** | exe + ORT + 10 DLL ≈ **1853.9 MB** | **−3,102 MB** |

**三个关键事实**：

1. 场景 A 是真实可选的方案，省 188 MB。
2. 场景 C 的 3 GB 收益**不能单独算给打标** —— 它是「美学也迁走」的合并收益，而美学依赖 torch/transformers（`aesthetic_model.py:99` / `:120` 与 `tagger_model.py:134`），那是另一项工作。
3. **只要美学还在 Python，torch 的 4228 MB 就一个字节都删不掉。** 打标自己只占 venv 里的 391 MB。

### 3.2 吞吐

这是最容易算错的一项，必须分三个口径。

#### 口径一：单路串行（当前后端管线的真实形态）

| 实现 | 耗时 | 折算 |
|---|---|---|
| Python HTTP 串行（30 次实测） | 357.5 ms/张 | 2.80 张/秒 |
| Python HTTP 串行（20 次实测，另一时点） | 183 ms/张 | 5.46 张/秒 |
| **Rust 端到端串行（20 次）** | **125.2–130 ms/张** | **7.7–8.0 张/秒** |

> 两组 Python 数据差约 2 倍，是机器状态与预热差异（该机器同时跑过 cargo build、Clash 代理等后台负载）。两个都给，不取平均。

#### 口径二：进程内分段（同图、同机、连续测）

| 阶段 | Python（in-process，细粒度锁） | Rust | 差异 |
|---|---|---|---|
| 解码 | 63.5 ms | **35.2 ms** | Rust 快 1.8x |
| 预处理 | 14.2 ms | 14.1 ms | 持平 |
| **推理（GPU）** | **76.3 ms** | **76.8 ms** | **持平** |
| 后处理 | 4.2 ms | **0.3 ms** | Rust 快 14x |
| **合计** | **158.2 ms** | **126.4 ms** | Rust 快 1.25x |

**推理环节两者完全一致**（76.3 vs 76.8 ms），因为都是同一块 GPU 上的同一份 ONNX。差距全在解码（PIL vs image crate）与后处理（Python 逐标签循环建 dict vs Rust 向量化）。

#### 口径三：并发（真正决定吞吐的口径）

| 实现 | 1 线程 | 2 线程 | 4 线程 | 8 线程 |
|---|---|---|---|---|
| Rust（单 Session + 互斥包 run，多线程并行解码/预处理） | 262.4 ms（3.81/s） | **87.3 ms（11.46/s）** | 87.7 ms（11.41/s） | 88.8 ms（11.26/s） |
| Python（`_infer_lock` 细化到只包 `session.run`） | 155.7 ms（6.42/s） | **91.7 ms（10.90/s）** | 91.9 ms（10.88/s） | — |
| Python HTTP 接口（当前交付形态） | 194.5 ms（5.14/s） | 174.0 ms（5.75/s） | 175.2 ms（5.71/s） | — |

**三个结论**：

1. 并发到 2 路以后，**Rust 与 Python 的吞吐基本持平（11.46 vs 10.90 张/秒，差 5%）**。语言层面的性能优势在这个任务上几乎不存在 —— 瓶颈是单卡 GPU 串行推理，不是 CPU。
2. **当前 Python 慢的真正原因是两把锁**：`main.py:47` 的 `_infer_lock` 把整张图的解码+预处理+推理全包在一个全局锁里；同时后端 `pipeline.rs:265` 的 `for image_id in &ids` 是串行循环，根本没有并发调用。
3. **HTTP 层本身吃掉约一半吞吐**：Python HTTP 2 并发 5.75 张/秒 vs in-process 2 线程 10.90 张/秒。这一半正是「把打标进程内嵌进后端」能拿回的部分。

### 3.3 启动与可靠性（这部分收益是确定的）

| 项 | 现状 | Rust 内嵌后 |
|---|---|---|
| 首启依赖安装 | `ensure_infer_venv()` 跑 `py -3 -m venv` + `pip install` + 双镜像回退，**数分钟，且是最脆弱的环节** | 无 |
| Python 进程管理 | `start_infer()` / `infer_port_uses_system_python()` 判断 8001 上是不是系统 python 并 kill 重拉 | 无 |
| 端口 | 额外占 8001 | 无（进程内） |
| `Connection: close` 补丁 | 为绕开 reqwest keep-alive 死锁而在中间件强制关连接 | 无 |
| 进程崩溃隔离 | 推理服务独立进程，崩了不影响后端 | **反向变差**：内嵌后打标 panic 会带走整个后端（需 `catch_unwind` 兜底） |

### 3.4 真实任务耗时的收益（最诚实的口径）

| 任务 | 张数 | 总耗时 | 折算 | 其中本地推理 | 推理占比 |
|---|---|---|---|---|---|
| #152 tag | 38 | 290 s | 7.6 s/张 | 38 x 0.126 s = 4.8 s | **1.7%** |
| #153 tag | 29 | 387 s | 13.3 s/张 | 29 x 0.126 s = 3.7 s | **0.9%** |
| #155 sauce | 77 | 1216 s | 15.8 s/张 | — | — |
| #160 sauce | 22 | 183 s | 8.3 s/张 | — | — |

**把本地推理优化到 0 ms，任务总耗时也只改善 1–2%。** `tag_one`（`pipeline.rs:302`）的策略是「先 SauceNAO 溯源，未命中才走本地」；有溯源命中的图完全不走本地推理，而 SauceNAO 单次往返 8–16 s。

**这是本文最重要的结论：打标慢是因为网络，不是因为 Python。**

### 3.5 当前真实负载（决定紧迫性）

从 `runtime 数据目录 app.db` 实测（活跃 271 张）：

| 项 | 数量 |
|---|---|
| 已有自动标签 | 235 张（86.7%） |
| 有溯源链接 | 115 张 |
| `no_auto_sauce=1` | 127 张 |
| 有 AI 元数据 | 61 张 |
| **尚未打标** | **36 张**（其中 24 张 `no_auto_sauce=1`，走纯本地路径） |

一次性补齐只需 36 x 0.126 s ≈ **4.5 秒**的本地推理。日常增量导入时每张图一次，用户根本感知不到 126 ms 与 357 ms 的差别。

---

## 4. 成本与风险

### 4.1 工作量

| 项 | 估算 |
|---|---|
| 新增 `backend/crates/tagger/src/local_infer.rs`（ort 封装、模型加载、预处理、推理、词表） | 250–350 行 |
| 词表解析（`model_vocabulary.json` 108139 条，兼容 `idx_to_tag` / `tag_to_idx` 两种键名） | 40 行 |
| DLL 定位（同目录 + `AddDllDirectory` 兜底） | 60 行 |
| `tagger/Cargo.toml` 加 ort 依赖 + workspace 依赖 | 5 行 |
| 切换开关（本地内嵌 vs HTTP 远程，保留回退） | 80 行 |
| wd14 分支的等价实现与验证（本机无 wd14 模型，**无法实测**） | 60 行 + 未知风险 |
| 单测 / 对拍基准 | 100 行 |
| **合计** | **约 600–700 行新增，2–3 个工作日** |

### 4.2 风险清单

| 等级 | 风险 | 说明 |
|---|---|---|
| 中 | **ort 版本被锁死在 rc.10** | 想复用 torch 的 CUDA 12 DLL 就不能升到 rc.11+（rc.11+ 只发 ORT 1.31 + CUDA 13）。rc.10 发布于 2025-06-01。要么长期锁版本，要么将来被迫重找一整套 CUDA 13 的 DLL |
| 中 | **DLL 定位脆弱** | PATH 无效、providers_cuda 必须同目录、Python 侧要持句柄 —— 这条链上任何一步写错都是「静默回落 CPU」，表现为「能用但慢 50 倍」，极难排查 |
| 中 | **wd14 无法实测** | 本机只有 cl_tagger 模型。wd14 的预处理（mean/std 是 CLIP 常量、448x448 BILINEAR）虽是纯手写可复刻，但没有真模型验证过 |
| 低 | 内嵌后崩溃隔离消失 | 建议 `catch_unwind` + 失败自动降级到 HTTP 远程模式 |
| 低 | 体积核算易错 | ort 的 `copy-dylibs` 在 workspace 里只在 binary crate 生效，且它生成的是**符号链接**（168–190 字节），不是真文件。打包时要按真实文件算，且 MSI 需重新验证是否跟随软链 |
| 低 | resize 内核写错会静默改标签 | BICUBIC 会翻转 41/2487、LANCZOS 55/2487。必须用 `FilterType::Triangle` 并有对拍测试守护 |

### 4.3 已排除的顾虑

- ❌「Rust 没有 ONNX 推理能力」—— ort 可用，且 **ORT core 静态链接进 exe**（`ort_trial.exe` 22.2 MB，导入表里**没有** `onnxruntime.dll`）。
- ❌「必须自装 CUDA Toolkit」—— 10 个 DLL 同目录即可，复用 torch/lib 时零体积成本。
- ❌「数值会漂移」—— Jaccard 1.0000，最大置信差 0.0169，远小于任何有意义的阈值变化。
- ❌「预处理依赖 transformers 无法复刻」—— `is_naflex: false`，只需一个 `pixel_values`，手写成立。

---

## 5. 建议

### 5.1 不做什么

**不建议现在做 Rust 重写。** 理由是收益与成本严重不匹配：

- 体积省 188 MB（共存场景），而磁盘上还有 4.2 GB 的 torch 删不掉；
- 吞吐提升在并发形态下只有 5%（11.46 vs 10.90 张/秒）；
- 真实任务耗时改善不到 2%（因为 98% 的时间在等 SauceNAO）；
- 代价是锁死 ort rc.10、引入一条脆弱的 DLL 加载链、增加 600–700 行无测试覆盖的新代码。

### 5.2 做什么（按性价比排序）

| 优先级 | 动作 | 预期收益 | 成本 |
|---|---|---|---|
| **P0** | **后端打标管线并发化**：`pipeline.rs:265` 的 `for` 循环改成有界并发（比如 4 路），每路独立 `tag_one`。注意 db 是单连接 `parking_lot::Mutex`，写进度要合并后批量刷 | 当前串行 357 ms/张 → 并发后逼近 175 ms，**理论上限 2x** | 小（约 80 行，不换语言） |
| **P0** | **Python 侧锁细化**：`main.py:47` 的 `_infer_lock` 只包 `session.run`，解码与预处理放到锁外 | in-process 1 线程 155.7 → HTTP 路径同步受益；2 线程可达 **10.90 张/秒**（实测） | 小（约 30 行） |
| P1 | **打标任务与溯源任务解耦**：当前 `tag_one` 先溯源再本地，溯源 8–16 s/张 完全掩盖了本地推理。可先本地打标、溯源作为后台补充 | 用户看到标签的时间从 8–16 s 降到 0.2 s | 中（需改任务编排语义） |
| P2 | **加 golden 基准集**（200–500 张 + 期望标签 JSON） | 为将来任何推理改动提供回归护栏 | 小 |
| P3 | Rust 重写打标 | 见 §3 | 中 |

**如果将来一定要做 Rust 迁移，前置条件是**：先完成 P0 两项，让 Python 侧的并发吞吐摸到 10.9 张/秒的天花板；届时若还嫌不够（比如要 20+ 张/秒），那才是 Rust 内嵌 + 多 Session + 批处理的场合 —— 而那时的收益来源是**批处理**，不是语言。

---

## 附录 A：实测数据索引

### 环境

- GPU：NVIDIA GeForce RTX 5070 Laptop GPU，驱动 595.76，8151 MiB
- Python：`runtime/python/.venv`，`onnxruntime-gpu 1.23.2`，`torch 2.11.0+cu128`（`torch.version.cuda = 12.8`）
- Rust：`ort 2.0.0-rc.10` → 预编译 ORT **1.22.0**（`ort-sys-2.0.0-rc.10/build.rs:8`）
- ORT 二进制缓存：`%LOCALAPPDATA%/ort.pyke.io/dfbin/x86_64-pc-windows-msvc/743380B97.../onnxruntime/lib/`（`onnxruntime_providers_cuda.dll` 180,731,904 B、`providers_shared` 10,752 B；其中的 `onnxruntime.lib` 320.3 MB 是导入库，**不参与交付**）

### 脚本清单（均在 `scripts/`，已被 .gitignore 排除）

| 脚本 | 用途 |
|---|---|
| `ort_trial/` | Rust 试验 crate（端到端 / 并发 / DLL 剪枝 / 目录加载） |
| `probe_ort_no_torch.py` | 裸 import 复现 ORT 缺 DLL 失败 |
| `probe_ort_dllpath.py` | 验证只设 PATH 无效 |
| `probe_ort_dllpath2.py` | 验证 `add_dll_directory` 持句柄有效 |
| `probe_ort_minimal_cuda.py` | 12 DLL 独立目录成功 |
| `probe_dll_sizes.py` | 量各 DLL 体积 |
| `probe_py_inproc.py` | Python 进程内分段计时（解码/预处理/推理/后处理） |
| `probe_py_http_conc.py` | Python HTTP 接口并发吞吐 |
| `probe_py_finelock.py` | Python 细粒度锁吞吐 |
| `probe_cuda_prune.py` ~ `probe_cuda_prune6.py` | Python 侧 DLL 剪枝（前几轮判据错误，见 §2.4） |
| `probe_cuda_rust_prune.py` / `probe_cuda_rust_prune2.py` | Rust 侧真实推理剪枝 |
| `dump_py_tags.py` / `compare_tags.py` | 标签对拍 |
| `cudatrial/` | 12 DLL 副本（1.9 GB，可删） |

### 最小可运行交付集（实测）

`bench2.exe`(22.2 MB) + `onnxruntime_providers_cuda.dll`(180.7 MB) + `onnxruntime_providers_shared.dll`(10.7 KB) + 10 个 CUDA DLL(1660.4 MB) = **1853.93 MB**，实测输出 `20 张 2.59s -> 129.6 ms/张, 47.0 标签/张`。

---

## 附录 B：与主报告的关系

本文是对 `docs/RUST_INFERENCE_FEASIBILITY.md` 的**局部更正与细化**。主报告以下内容应以本文为准：

1. **§4.1 表格**「ort CUDA EP 用户需自备 CUDA 13.2 + cuDNN 9.23」—— 只在 ort rc.11+ 成立；rc.10 只需 CUDA 12 + cuDNN 9，可复用 torch 的 DLL。
2. **§4.3**「ort 的 CUDA EP 是否能在本机这套 CUDA/cuDNN 上跑通，仍未验证」—— **已验证通过**，76.8 ms/张。
3. **§8 R3**「ort 的 CUDA 要求比现状更重」—— **需分版本讨论**；在「Python 美学仍在」的前提下，复用 torch/lib 零额外体积，R3 不成立。
4. **§6.5**「不提升吞吐」—— 这条**仍然正确，且本轮给出了量化证据**：并发形态下 Rust 仅快 5%，真实任务级改善不到 2%。

仍然成立的：R5（ort 非 stable）、R7–R13（美学侧风险）、§6.1 美学的体积收益。本文不涉及美学。


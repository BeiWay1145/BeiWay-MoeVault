# 用 Rust 取代 Python 推理服务：可行性评估报告

> 状态：**已完稿**（task-2 交付物）
> 评估范围：`python/server/` 全部推理逻辑（1010 行）→ Rust 原生实现的可行性
> 评估立场：**严格**。本报告用于决策，乐观结论必须有证据支撑，风险必须明说。
> 标记约定：**[已验证]** = 本机实测 / 源码逐行核对 / 上游源码确认；**[推测]** = 基于架构常识的估计，未在本机跑通；**[未知]** = 当前条件无法核实

---

## 0. 结论速览

### 0.1 总结论：**部分迁移**（先迁美学；打标的预处理障碍已解除，改为「可迁但排后」）

| 环节 | 迁移可行性 | 关键依据 | 建议 |
|---|---|---|---|
| 美学 · qalign（SIGLIP2 回归） | **高** | safetensors 键与 candle `siglip::VisionModel` 逐条匹配 8/8 [已验证]；预处理只有 resize+归一化 [已验证] | **建议迁** |
| 美学 · anime（ViT-1024/48 层） | **高** | 权重键与 candle `vit::Model` 完全对应（含 classifier 同级）[已验证]；预处理需复现「RGBA 白底合成」[已验证] | **建议迁** |
| **打标 · cl_tagger（SIGLIP2 ONNX）—— 现役主力** | **中高**（初稿判「低」，已两次更正） | 模型已定位在 `E:\Game\AI\cl_tagger\models`（2.2GB）；ONNX 签名实测**只有 1 个输入** `pixel_values [N,3,384,384]`，`model_metadata.json` 明写 `is_naflex: false`，**不存在三输入分支** [已验证]；手写预处理 vs transformers **40 图对拍 prob_maxdiff 0.0133、0.5 阈值翻转 8/2487** [已验证] | **可迁，但排在美学之后**。迁移前必须先做 200–500 张 golden 基准 |
| 打标 · wd14（兼容分支） | **中高** | 预处理是纯手写 numpy 逻辑，无 transformers 依赖 [已验证] | **仅在确认仍在使用时才谈迁移** |
| HTTP 契约 | **中** | 6 个端点、6 个请求模型、字段级响应结构已逐条抄录 [已验证] | 第一期原样复刻 |

### 0.2 为什么不是「推荐全面迁移」

初稿给了三条硬理由，**其中前两条已被本机实测推翻**，只有第三条仍然成立。下面逐条交代，原文保留以便追溯「报告当初是怎么错的」：

1. ~~**打标侧无法验证——模型文件不在仓库里。**~~ → **已推翻。** 模型确实不在仓库，但在本机 `E:\Game\AI\cl_tagger\models`（2.2GB：`model.onnx` 791KB + `model.onnx.data` 2.11GB + `model_vocabulary.json` 14.6MB + metadata）。
   报告初稿只做了全仓 `find`，没有追问「模型是否可能在仓库之外」。**教训：把「仓库里没有」当成「不存在」，是一个错误推断。**
   现该目录已复制到 `runtime/models/tagger/`，打标服务正常运行，GPU 实测 **183 ms/张**（20 次串行 3.659 s）。
2. ~~**cl_tagger 的预处理不可离线复现。**~~ → **已推翻。** 两个子断言都不成立：
   - 「可能有 naflex 三输入」→ `model_metadata.json` 明写 `"is_naflex": false`；用 onnxruntime 读出的实际签名**只有 1 个输入** `pixel_values`，shape `[batch_size, 3, 384, 384]`，输出 `logits` `[N, 108139]`。三输入分支在本机部署里根本不存在。
   - 「预处理不可手写」→ 手写 `resize(384,384,BILINEAR) → /255 → (x-0.5)/0.5 → CHW` 与 transformers 路径**40 图严格对拍**：prob_maxdiff **0.0133**、0.5 阈值翻转 **8 / 2487**（全部落在 0.5±0.005 的模型自身边界抖动区）。等价于 u8 量化级差异，不影响任何实际判定。
3. **ort 的 CUDA 部署门槛比现状更高，不是更低。**（**这一条仍然成立，也是真正剩下的障碍**） ort 官方文档原文：`ort` provides binaries for **CUDA ≥ 13.2** and targets **cuDNN ≥ 9.23**. Make sure compatible versions of CUDA and cuDNN are installed and available on the **PATH** [已验证，ort.pyke.io/perf/execution-providers]。而现状的 `torch+cu126` wheel **自带 CUDA 运行时 DLL**，用户不需要装 CUDA Toolkit。迁移后反而要让用户装 CUDA 13.2 + cuDNN 9.23 —— 这是**部署复杂度上升**，不是下降。

### 0.3 现役打标模型已确认是 cl_tagger（本报告初稿判错，此处更正）

> **结论：现役主力是 cl_tagger，wd14 只是保留下来的兼容分支。**
>
> 本报告初稿把 wd14 当成默认迁移对象、并把它列为「决定性未知量」，这是错的。更正依据：
>
> 1. **代码路径**：`python/server/models/tagger_model.py` 的 `_load_cl_tagger` / `_preprocess_cl_tagger` 是主路径，`_load_wd14` / `_preprocess_wd14` 是兼容分支；`python/server/config.py:25` 把 `MODEL_KIND_CL_TAGGER` 列在首位。
> 2. **项目文档**：`README.md:22`「本地 cl_tagger 模型回退」、`README.md:87`「承载 cl_tagger 打标 + Q-Align 美学评分」、`README.md:172-183` 的模型探测顺序、`docs/PLAN.md:25/69/102` 均以 cl_tagger 为准。
> 3. **运行时实况**（`GET http://127.0.0.1:8001/health` + `settings` 表）：`tagger_model_name = "cl-tagger (SIGLIP2 ONNX)"`、`tagger_model_dir = <runtime>/models/tagger`。
>
> 因此打标迁移的**唯一现实对象**是 cl_tagger。
>
> 判定种类的方法仍然有效：`selected_tags.csv + model.onnx` = wd14；`model_vocabulary.json + model.onnx` = cl_tagger（`tagger_model.py` 的 `detect_kind` [已验证]）。
>
> **关于「本机无任何打标模型文件」——这句话是错的，已作废。**
> 模型在 `E:\Game\AI\cl_tagger\models`，本报告已据此完成完整数值验证（见 §0.2 第 2 条与 §5.3）。
>
> ## 0.3.1 更正：cl_tagger 的迁移结论从「不迁」翻转为「可迁，但排后」
>
> 推翻的两个前提见 §0.2 第 1、2 条。本机实测得到的确切事实：
>
> | 项目 | 实测值 | 证明方式 |
> |---|---|---|
> | ONNX 输入 | 仅 `pixel_values` `[N,3,384,384]` float32 | onnxruntime 读 session get_inputs() |
> | ONNX 输出 | `logits` `[N,108139]` float32 | 同上 |
> | naflex | `false` | `model_metadata.json` 原文 |
> | processor | `SiglipImageProcessor`（slow 版）、`image_mean/std = [0.5,0.5,0.5]`、`resample = 2`（BILINEAR）、size 384 | `preprocessor_config.json` |
> | 手写预处理复现 | prob_maxdiff `0.0133`；0.5 阈值翻转 `8/2487` | 40 图逐图对拍（`scripts/probe_cltagger_parity.py`） |
> | resize 内核敏感性 | BILINEAR 翻转 2 处 / BICUBIC 41 处 / LANCZOS 55 处 | 6 图 × 3 内核 |
> | GPU 吞吐 | 183 ms/张（RTX 5070 Laptop，CUDAExecutionProvider） | 预热后 20 次串行 |
>
> **必须用 BILINEAR**（对应 `resample: 2`）。换成 BICUBIC/LANCZOS 会让 prob 偏差放大到 0.4–0.6，标签大幅翻转——这是迁移时最容易踩的坑。
>
> 剩余的真障碍只有一条：**`ort` 的 CUDA 部署要求（CUDA ≥13.2 + cuDNN ≥9.23 在 PATH）比 torch wheel 自带运行时更重**，会让「一键装 GPU」体验倒退。这是部署取舍问题，不是算法可行性问题。
>
> 因此结论改为：**打标在技术上可迁，优先级排在美学之后**；是否迁取决于产品是否接受 GPU 部署退化，或是否愿意走 DirectML 兜底。
>
> 另注：aesthetic 的 qalign 用的是同一族 `SiglipImageProcessor`（同样 mean/std 0.5），**两者的预处理可以共用一份 Rust 代码**，这进一步降低了打标迁移的边际成本。

---

## 1. 现状事实基线

### 1.1 Python 推理服务代码结构 [已验证]

| 文件 | 行数 | 职责 |
|---|---|---|
| `python/server/main.py` | 335 | FastAPI 应用、6 个推理端点 + `/health` + `/devices`、全局单例与串行锁 |
| `python/server/models/tagger_model.py` | 318 | 打标：cl_tagger / wd14 双探测器，ONNX Runtime 会话 |
| `python/server/models/aesthetic_model.py` | 215 | 美学：qalign（transformers SigLIP）/ anime（ViT）双后端 |
| `python/server/config.py` | 140 | 路径探测、阈值、模型名、环境变量覆盖 |
| **合计** | **1010** | |

关键全局事实：
- 打标与美学**共用一个 `_infer_lock`**，全串行（注释写明：两者共用显存）。
- `import` 时执行 `torch.set_num_threads(1)`；anime 后端另有一处 `torch.set_num_threads(1)`（注释：多线程首次加载死锁）。
- middleware 强制 `Connection: close`（注释：修 reqwest keep-alive 调 `/infer/aesthetic` 时 uvicorn 挂起死锁）。**这是现状的一个真实缺陷，迁移会顺带消除它。**

### 1.2 模型文件实况 [已验证，`python` 直读 safetensors 头]

```
models/aesthetic/model.safetensors      374,108,532 B   210 张量 全 F32  metadata={format:pt}
models/aesthetic_anime/model.safetensors 2,182,620,068 B 776 张量 全 F16  metadata=None
models/ 合计 ≈ 2.4 GB
```

- `models/aesthetic`：`SiglipForImageClassification`，`problem_type=regression`，`id2label={0:LABEL_0}`。权重顶层只有 `classifier`（`weight[1,768]` + `bias[1]`）与 `vision_model`（12 层 768/3072/12头，patch16，`position_embedding[1024,768]`，head 是 `MultiheadAttentionPoolingHead` = `probe[1,1,768]` + `in_proj_weight[2304,768]` + `mlp` + `layernorm`）。**无 cls_token**（SigLIP 靠 MAP head 池化）。
- `models/aesthetic_anime`：`ViTForImageClassification`，`hidden_size=1536`、`intermediate_size=4192`、`num_hidden_layers=48`、`num_attention_heads=16`、`patch_size=64`、`image_size=1024`、`qkv_bias=true`、`hidden_act=gelu`、`layer_norm_eps=1e-12`、`id2label={0:hq,1:lq}`。权重顶层 `classifier`（`[2,1536]`）与 `vit`，`position_embeddings[1,257,1536]`（1 CLS + 16×16 patch）。
- **打标模型与词表：存在，但不在仓库里。** 初稿判「不存在」是错的。实际位置 `E:\Game\AI\cl_tagger\models`（2.2GB）：

  ```
  model.onnx                 791,773 B
  model.onnx.data      2,211,645,300 B   ← 权重外置
  model_vocabulary.json   14,594,140 B   ← 108139 个标签
  model_metadata.json            112 B   ← {"vision_encoder_repo":"google/siglip2-so400m-patch14-384","is_naflex":false,"num_tags":108139}
  model_ood_ref.npz        4,949,435 B
  model_tag_metrics.npz   54,015,616 B
  ```

  `config.py` 里的 `LEGACY_TAGGER_DIR` 常量写的是 `D:/Game/AI/cl_tagger/models`，而实际模型在 **E:** 盘 —— 这就是自动探测「全部候选未命中」的直接原因。
  现已复制到 `<install>\runtime\models\tagger\`，`python/server/config.py` 的 `_find_tagger_dir()` 能自动识别（实测 `TAGGER_MODEL_KIND = cl_tagger`）。

### 1.3 当前运行时开销

| 项 | 数值 | 置信度 |
|---|---|---|
| `torch` PyPI win cp312 wheel | 下载 120.9 MB / **解压 461.7 MB**（13,110 文件），**CPU-only** | [已验证] |
| `torch+cu126` win cp312 wheel | 2,496,082,434 B ≈ **2.32 GB** 下载，解压后约 6–7 GB | 下载体积已验证；解压体积 [推测] |
| `onnxruntime-gpu` 1.32 win cp312 | 下载 150.5 MB / 解压 226.4 MB | [已验证] |
| `transformers` 5.23.0 | 纯 Python，sdist 10.1 MB，解压约 100 MB 级 | 体积 [已验证]，解压 [推测] |
| `numpy` 2.5.4 / `pillow` 13.0.0 | 12.1 MB / 6.9 MB | [已验证] |
| CPU-only venv 合计 | **约 0.85–1.0 GB** | [推测]（分项已验证） |
| GPU venv 合计 | **约 7–9 GB** | [推测]（与 Lead 转述「10GB」同量级） |
| 现状 MSI 产物 | `BeiWay-MoeVault_0.1.0_x64_en-US.msi` = 10,481,664 B ≈ **10.0 MB**，不含 venv 与模型 | [已验证] |

**一个容易被忽略的事实 [已验证]**：`python/requirements.txt` 只写 `torch>=2.1`，**没有指定 `+cu` 索引**。所以默认安装路径拿到的是 **CPU-only torch（461.7 MB）**，美学推理默认吃不到 GPU；GPU 版要用户手动触发 `infer_install_gpu`。

### 1.4 Rust 侧现状 [已验证]

- `backend/` 是独立 cargo workspace，7 个成员 crate，**零 ML 推理依赖**。
- 推理在 Rust 里的全部接触面只有 4 处 HTTP 调用：
  - `backend/crates/tagger/src/pipeline.rs:144` `POST /infer/tags`（`infer_tags(path, threshold)`，导入流程里溯源失败的回退路径，调用点 `pipeline.rs:835` `apply_local_tags`）
  - `backend/crates/tagger/src/aesthetic.rs:166` `POST /infer/aesthetic`（`run_aesthetic_pipeline` → `score_one`）
  - `tagger/src/aesthetic.rs:136` / `pipeline.rs:104` 两个 `/config` 端点
  - `backend/crates/api/src/routes/settings.rs:64-65` `/api/v1/devices`、`/api/v1/infer/health` → `proxy_json` 透传
- `src-tauri/src/lib.rs`：`INFER_URL=http://127.0.0.1:8001`，6 个 command（`infer_start/stop/status/install_deps/install_gpu/gpu_status`），用**裸 TCP** `GET /health` 探活。`ensure_infer_venv()` 用 `py -3` 建 venv 并 pip install。
- `frontend/src/api/infer.ts`：唯一前端耦合点，含 `InferHealth` 类型与 `summarizeHealth`（`running/degraded/stopped`）。

> 结论：**Rust 侧的耦合面很窄**（4 个 HTTP 调用 + 6 个 Tauri command + 1 个前端文件），这降低了迁移的改动风险。真正的风险全在**模型侧**。

---

## 2. 逐段拆解：推理服务到底做了什么

### 2.1 打标（`tagger_model.py`）[已验证]

**加载**：懒加载 + `threading.Lock`；`load_from_dir()` 支持运行时切目录切种类。种类由目录标志文件判定。

- `_load_cl_tagger`：读 `model.onnx` / `model_vocabulary.json` / `model_metadata.json`（缺任一即 `FileNotFoundError`）→ `meta.get("is_naflex", False)` → `processor_repo = meta.get("vision_encoder_repo", "google/siglip2-so400m-patch14-384")` → `AutoProcessor.from_pretrained(..., local_files_only=True)`（失败再联网，**会下载**）→ 词表 → `ort.InferenceSession(model.onnx, providers=...)`。
- `_load_wd14`：读 `model.onnx` + `selected_tags.csv`，跳过表头，`row[0].strip()` 为空即 `ValueError`。

**预处理**：

- `_preprocess_cl_tagger`：完全依赖 `transformers.AutoProcessor`。
  - 非 naflex：只返回 `{pixel_values}`。（触发条件：`autoprocessor` 的 `tokenizer` 是否存在等，**运行时行为未核实** → [未知]）
  - naflex：`processor(images=image, return_tensors="np", max_num_patches=256)`，返回**三个**输入 `pixel_values` / `pixel_attention_mask` / `spatial_shapes`。
- `_preprocess_wd14`（纯手写，无 transformers —— **这是好消息**）：
  ```python
  mean=[0.48145466,0.4578275,0.40821073]; std=[0.26862954,0.26130258,0.27577711]
  image.resize((448,448), Image.BILINEAR)   # PIL，u8 域
  np.asarray(float32)/255.0 → (arr-mean)/std → transpose(2,0,1)[None]   # NCHW
  ```

**推理与后处理**：
```python
outputs = session.run(["logits"], inputs)     # 输出节点名固定为 "logits"
logits  = outputs[0][0]                        # 取 batch 第 0 行
probs   = 1.0/(1.0+np.exp(-logits))            # sigmoid，多标签
for idx, p in enumerate(probs):
    if p >= threshold: result[_idx_to_tag.get(int(idx), f"class_{idx}")] = float(p)
return dict(sorted(result.items(), key=lambda kv: kv[1], reverse=True))  # 置信度降序
```
阈值：wd14 默认 `0.35`，cl_tagger 默认 `0.5`（显式传参不覆盖）。

**迁移评估（已按实测重写）**：cl_tagger 是现役主力，但初稿对它列的两个障碍**都已不成立**：

| 初稿障碍 | 实测结论 |
|---|---|
| 依赖 `transformers.AutoProcessor`，Rust 无现成实现 | 该 processor 就是 `SiglipImageProcessor`（mean/std 0.5、resample 2/BILINEAR、384×384），手写 4 行即可：`resize → /255 → (x-0.5)/0.5 → CHW`。40 图对拍 prob_maxdiff 0.0133 |
| 可能有 naflex 三输入，Rust 无实现 | `is_naflex: false`，ONNX 实际签名只有 1 个输入 `pixel_values`。**不存在此分支** |
| 缺模型文件，无法数值验证 | 模型在 `E:\Game\AI\cl_tagger\models`，已完成对拍与 GPU 吞吐实测 |

唯一剩下的问题在推理运行时而非预处理：**`ort` 的 CUDA EP 要求系统级 CUDA ≥13.2 + cuDNN ≥9.23 在 PATH**，比 torch wheel 自带运行时重（见 §4.1）。

wd14 分支仍是**可以逐行翻译**的备选（resize + CLIP 归一化 + NCHW + sigmoid + 阈值 + 降序），但它已不是主路径。

### 2.2 美学（`aesthetic_model.py`）[已验证]

**qalign 后端**（`_load_qalign` / `_score_qalign`）：
```python
image = Image.open(p).convert("RGB")
inputs = self._processor(images=image, return_tensors="pt")   # SiglipImageProcessorFast
if cuda: inputs = {k: v.to(target) for k, v in inputs.items()}
with torch.no_grad(): out = self._model(**inputs)
raw = float(out.logits.reshape(-1)[0])                        # 标量回归输出
score = 1.0+4.0*sigmoid(raw) if AESTHETIC_SIGMOID else raw     # 默认 AESTHETIC_SIGMOID=0
score = max(1.0, min(5.0, score))                              # clamp 到 AESTHETIC_RANGE
return {"score": round(score,4), "raw": round(raw,6), "range":[1.0,5.0], "model":..., "kind":"qalign", ...}
```
预处理（`models/aesthetic/preprocessor_config.json` 全文）[已验证]：`size=512×512`、`resample=2(BILINEAR)`、`rescale=1/255`、`image_mean=[0.5,0.5,0.5]`、`image_std=[0.5,0.5,0.5]`、`crop_size=null`、`default_to_square=false`、`do_center_crop=null`。
→ 实际等效：**resize 到 512×512（不等比，直接压扁）→ /255 → (x-0.5)/0.5 → NCHW**。

**anime 后端**（`_load_anime` / `_score_anime`）：
```python
image = Image.open(p).convert("RGBA")                          # 先 RGBA
bg = Image.new("RGBA", image.size, (255,255,255,255))
image = Image.alpha_composite(bg, image).convert("RGB")        # 白底合成（官方 Space 预处理，必须复现）
inputs = self._processor(images=image, return_tensors="pt")    # ViTImageProcessor(use_fast=True)
probs  = softmax(out.logits, dim=-1)
score01 = probs[0,0].item()                                    # 第 0 类 = hq
score  = 1.0 + score01*4.0
```
预处理（`models/aesthetic_anime/preprocessor_config.json` 全文）[已验证]：`size=1024×1024`、`resample=2`、`rescale=1/255`、`mean=[0.65463551,0.60715182,0.61108185]`、`std=[0.32903292,0.32726001,0.31909652]`。

**陷阱（必须写进实现）**：Python 的 `round()` 是**银行家舍入**（round-half-to-even），Rust 的 `f64::round()` 是**远离零**。`round(2.5)=2` vs `2.5.round()=3`。要 1:1 复刻 `round(score,4)` / `round(raw,6)` 必须自己写 half-to-even 实现。

### 2.3 HTTP 契约（`main.py`）[已验证]

6 个推理端点 + 2 个状态端点，响应结构必须 1:1 复刻（否则 `backend/crates/tagger` 的 `resp.json::<AestheticResp>()` 与前端 `InferHealth` 会解析失败）。

| 端点 | 响应要点 |
|---|---|
| `GET /health` | `{status:"ok", models:{tagger:{state,error,kind}, aesthetic:{state,error}}, paths:{...}}`；`state ∈ ok/failed/not_loaded` |
| `GET /devices` | `{devices:[{id,name,kind}], capabilities:{tagger_cuda,tagger_providers,aesthetic_cuda}}`；CPU 名走 `winreg` 读 `HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0\\ProcessorNameString` |
| `POST /infer/tags` | `{tags:[{name,confidence}], model, device, providers, elapsed_ms}`；失败 500 `"打标失败: {e}"` |
| `POST /infer/tags_batch` | `{results:[{path,ok:true,tags,elapsed_ms} | {path,ok:false,error}]}`；空数组 422 `"paths 不能为空"` |
| `POST /infer/aesthetic` | `score()` 的 dict + `elapsed_ms`；失败 500 `"美学评分失败: {e}"` |
| `POST /infer/aesthetic_batch` | `{results:[{path,ok:true,<score dict>} | {path,ok:false,error}]}` |
| `POST /infer/{tagger,aesthetic}/config` | `{ok:true, kind, device}` |

其它语义：
- `device`：`None`/缺省 = 不干预当前设备；`"auto"` / `"cuda:0"` / `"cpu"` 显式指定。
- `threshold`：`Field(ge=0.0, le=1.0)`，越界 422。
- batch 端点：单张失败不中断，逐张记 `ok:false`。
- 全部推理在单一 `_infer_lock` 内串行（打标与美学互斥）。

---

## 3. 技术路线评估

先明确「取代」的两种形态，后面的评估都按形态区分：

- **形态 ①（保守）**：保留独立 sidecar 进程与 HTTP 契约，只把实现从 Python 换成 Rust。改动面最小，风险最低，收益 = 去掉 venv。
- **形态 ②（彻底）**：推理直接内嵌进 `moevault-app`，删掉 8001 端口、`infer_*` 六个 Tauri command 与裸 TCP 探活。收益最大，但要动 Tauri 生命周期、后端启动顺序、前端状态机。

**建议**：第一期走 ①，验证数值一致后再评估 ②。理由：①已经能拿到 80% 的收益（venv 消失），却把风险压在单个进程内。

### 3.1 方案 A：`ort`（ONNX Runtime 绑定）

**版本与成熟度** [已验证，crates.io API]：`ort 2.0.0-rc.15`（2026-10-19 发布，累计下载 26.08M），封装 ONNX Runtime **1.31**，edition 2024、`rust-version=1.92`。**仍是 rc 版本，不是 stable。**

**能力**：EP feature 齐全（`cuda`/`tensorrt`/`directml`/`openvino`/`rocm`/`coreml`/`qnn`…）；默认 feature 含 `download-binaries`（pyke CDN 预编译 ORT）、`copy-dylibs`（Windows 把 CUDA 等 DLL 复制到 target 目录，仅 binary crate 生效）、`api-31`。另支持 `load-dynamic`（运行期 dlopen）与 `alternative-backend`（`ort-candle` / `ort-tract`）。

**优势**：
- 打标是 **ONNX 模型**，`ort` 是唯一「不改模型、直接跑」的路线。换成 candle 必须先把 ONNX 转成 safetensors/GGUF 并逐层对齐，工程量远超收益。
- 官方 Fallback 语义与现状 Python 侧 `_resolve_providers()` **完全同构**：注册失败静默回落 CPU；用 `.error_on_failure()` 可改成硬失败（Python 侧是「先查 `get_available_providers()` 再决定」，Rust 侧用 `is_available()` 等价）。
- 有 `ep::CUDA::default()` 与 `with_execution_providers`，多 EP 顺序注册即自动逐算子回落。

**劣势（这是关键）**：
- **CUDA 部署门槛上升**：官方原文要求 **CUDA ≥ 13.2、cuDNN ≥ 9.23，且在 PATH 上** [已验证]。而 torch `+cu126` wheel 自带 CUDA runtime，用户不必装 Toolkit。**迁移后「一键装 GPU」这个已经做好的体验会退化。**
- 预编译二进制只覆盖 `sm_75 / 8.x / 9.x / 12.x`；不在列表内的算力需自行编译 ONNX Runtime [已验证]。
- WebGPU EP 官方明示 **experimental，可能产生错误结果或崩溃** [已验证] —— 不能作为兜底。
- 仍是 rc：`2.0.0-rc.15` 意味着 API 仍可能破坏性变动，不适合作为「交付给用户」的稳定底座。

**结论**：`ort` 是打标环节的唯一现实选择。打标主力 cl_tagger 的 preprocessing 障碍已解除（§0.3.1），**所以打标在技术上可迁**；是否迁取决于能否接受上面那条 CUDA 部署代价。

### 3.2 方案 B：`candle`（纯 Rust 张量框架）

**版本** [已验证]：`candle-core` / `candle-transformers` **0.11.1**（2026-10-31），下载量 11.76M / 5.26M。

**关键发现——两个美学模型都有现成实现，且权重键严格匹配：**

**(a) qalign → `candle_transformers::models::siglip`**

`VisionModel::new(cfg: &VisionConfig, use_head: bool, vb: VarBuilder)`，`VisionConfig{hidden_size, intermediate_size, num_hidden_layers, num_attention_heads, num_channels, image_size, patch_size, hidden_act, layer_norm_eps}`。

源码结构：`VisionTransformer{embeddings, encoder, post_layernorm, head: Option<MultiheadAttentionPoolingHead>}`，全部走 `vb.pp(...)` 取权重；MAP head 用 `vb.get((1,1,h), "probe")` + `MultiheadAttention(vb.pp("attention"))`（内部 `in_proj_weight` `.chunk(3,0)` 拆 q/k/v）。

**验证结果 [已验证]**：把 `models/aesthetic/model.safetensors` 的键去掉 `vision_model.` 前缀后，与 candle `VarBuilder` 路径**逐条匹配 8/8**：
`embeddings.patch_embedding.weight`、`embeddings.position_embedding.weight`、`post_layernorm.weight`、`encoder.layers.0.layer_norm1.weight`、`head.probe`、`head.attention.in_proj_weight`、`head.mlp.fc1.weight`、`head.layernorm.weight`。
→ **candle SigLIP 视觉塔可零改造加载该 safetensors**，只差外面套一层 `classifier`（`weight[1,768]` + `bias[1]`）并取输出。

**(b) anime → `candle_transformers::models::vit`**

`Model::new(cfg: &Config, num_labels: usize, vb: VarBuilder) -> Result<Model>`，`forward(&self, xs: &Tensor) -> Result<Tensor>`（**直接返回 logits，正好够用**）。`Config` 字段含 `qkv_bias`、`image_size`、`patch_size`、`num_channels` —— 原生支持 1024/64/qkv_bias=true。
源码：`Model::new` 在 `vb.pp("vit")` 下建 `embeddings`/`encoder`/`layernorm`，`classifier` 走 `linear(hidden_size, num_labels, vb.pp("classifier"))`（**与权重布局 `vit.*` + `classifier.*` 同级完全一致**）。`forward` = embeddings → encoder → 取 CLS `.i((..,0,..))` → layernorm → classifier。
→ **aesthetic_anime 可原样加载，零改造。**

**两个已知限制**：
- `vit` 的 `interpolate_pos_encoding` 是 `todo!()`。但本模型固定 1024×1024 输入，走不插值分支，**无影响** [已验证源码路径]。
- candle **没有 SigLIP2 NaFlex** 实现 → 但本机 cl_tagger 实测 `is_naflex: false`，不走该分支，所以这条对本项目**不构成限制**。打标仍建议用 `ort`（ONNX 原生），而非转 candle。

**CPU 上的 F16 性能**：`candle-core` 依赖 `gemm 0.19`（default features 含 `f16`），`cpu_backend/mod.rs:1455-1457` 显式允许 `F16 | F32 | F64` 走 matmul；`conv2d.rs` 的 `impl Map2` 是泛型 `T: WithDType`，F16 卷积也可跑 [已验证源码]。**但「能跑」不等于「够快」——anime 模型 2.18GB F16、48 层 1536 hidden，CPU 单张耗时仍未实测 [未知]。** 本机现已具备 GPU 与 torch 环境，缺的是 candle 版实现本身，所以这项要等到第 1–2 步落地后才能测。

**candle 的 CUDA**：`features = ["cuda"]`（依赖 `cudarc` + `candle-kernels`）。注意 candle 的 CUDA 是**自己编译 kernel**（首次编译很慢），与 ort 用预编译 DLL 的路线不同。这是 [已验证的 feature 定义 / 编译耗时未知]。

### 3.3 方案 C：混合（`ort` 打标 + `candle` 美学）—— **推荐**

理由：
1. 打标是 ONNX 产物，天然属于 `ort`；强转 candle 需要模型格式转换 + 逐层对齐，属于自找工程量。
2. 美学有原生 HF 权重（safetensors），candle 已有**严格匹配的现成实现**，是收益/风险比最好的部分。
3. 两套运行时共存于同一 binary 是可行的（`ort` 是 FFI，`candle` 是纯 Rust；各自管理显存/内存）。代价是二进制体积与两份 CUDA 栈（**这点必须实测**：`ort` 用系统 CUDA，`candle` 用 `cudarc` 动态加载 CUDA，两者的 CUDA 版本要求可能互相冲突 → [未知]，需要在目标机上验证）。

**若第一期只做美学（推荐的第一步）**：则只需 candle，**完全不需要 ort**，风险面最小。

---

## 4. GPU 支持与部署复杂度

### 4.1 四条路径的对比

| | 现状（Python） | ort CUDA EP | candle CUDA | CPU-only |
|---|---|---|---|---|
| 用户需自备 | 无（torch wheel 自带 CUDA 运行时） | **CUDA ≥13.2 + cuDNN ≥9.23 且在 PATH** | CUDA toolkit + nvcc（要编译 kernel） | 无 |
| 安装体积 | 2.32GB wheel / 解压 6–7GB [推测] | ORT CUDA DLL（数百 MB）[未知] | 二进制内置 kernel，随 binary 增大 [未知] | +几十 MB |
| 首次可用时间 | pip 装完即可 | 装完 CUDA 即可 | **首次需编译 kernel**（分钟级）[推测] | 立即 |
| 一键化难度 | 已实现（`infer_install_gpu`） | **高**（装 CUDA Toolkit 无法静默 pip 化） | 中（需 MSVC + CUDA） | 低 |

**这是本报告最重要的部署结论**：现在能「一键装 GPU」，是因为 PyTorch 把 CUDA 运行时**打包在 wheel 里**。改用 `ort` 后，ONNX Runtime 的 CUDA EP 是**在运行期 dlopen 系统的 `cudnn64_9.dll` / CUDA runtime**，官方明确要求装在 PATH 上。**这会让「非技术用户一键用上 GPU」这个产品体验直接倒退。**

### 4.2 缓解方案（必须写进方案，否则不建议做）

1. **双轨交付**：Rust 版默认 CPU；检测到用户已装 CUDA/cuDNN 才启用 CUDA EP。现状的 `infer_install_gpu` 保留但语义改为「检测 + 提示」，而不是安装。
2. **DirectML EP 兜底**：Windows 上 `directml` feature 不需要 CUDA，可覆盖 AMD/Intel 核显，且是 Windows 原生 DX12 路径。**但本项目的目标机型（如 Intel Core Ultra 9 285H 核显）在 DirectML 上的实际吞吐未测 [未知]**。
3. **candle CPU 的 SIMD 加速**：`candle` 走 gemm crate（rayon + f16），CPU 侧性能需实测。

### 4.3 本机环境的现实（已随机器重启而改变）

初稿写作时本机无可用 GPU，因此把「GPU 只能文档核实」写进了结论。**用户重启并启用独显后，这一条已不成立**，实测如下：

| 项 | 实测值 |
|---|---|
| 显卡 | NVIDIA GeForce RTX 5070 Laptop GPU，驱动 595.76，显存 8151 MiB [已验证] |
| 推理 EP | `["CUDAExecutionProvider","CPUExecutionProvider"]`（onnxruntime-gpu 1.23.2）[已验证] |
| GPU 占用 | 打标并发期间 `utilization.gpu` 达 68%，显存 3224 MiB → 确认真的走 GPU [已验证] |
| 打标吞吐 | 预热后 20 次串行 POST `/infer/tags` 共 3.659 s ≈ **183 ms/张**（含 HTTP 往返）[已验证] |
| venv 位置 | `D:\Program Files\BeiWay-MoeVault\runtime\python\.venv`（torch 2.11.0+cu128）[已验证] |

→ **GPU 路径现在是可实测的了**。但 `ort` 的 CUDA EP 是否能在本机这套 CUDA/cuDNN 上跑通，仍未验证——它要求的是系统级 CUDA 13.2 + cuDNN 9.23，与 torch 自带的 cu128 不是一回事。

---

## 5. 数值一致性与验证方案

### 5.1 差异来源清单（按影响从大到小）

| 来源 | 影响 | 可控性 |
|---|---|---|
| **resize 实现差异**：PIL `BILINEAR` 在 u8 域用整数运算；Rust `image::imageops::resize` + `FilterType::Triangle` 在 f32 域 | 每像素 ±1 LSB → 归一化后 ≈0.004 → 经 12/48 层放大后 logit 可能有 1e-2 量级偏差 | 可量化；必要时用 f32 直接缩放并接受差异，或自写与 PIL 同构的 u8 双线性 |
| **F16 权重（anime）**：PyTorch 内部会把 F16 升到 F32 做部分运算；candle 需明确 `VarBuilder::from_mmaped_safetensors(..., DType::F16)` 还是 F32 | 数值与显存占用都受影响 | 可实测：两种 dtype 各跑一次对比 |
| **`gelu` 变体**：candle `Activation::Gelu` → `gelu_erf()`，与 PyTorch `F.gelu`（erf 版）一致 [已验证]；但 `gelu_pytorch_tanh` 是另一种 | 若配置被判成 tanh 版本会系统性偏差 | 已确认本模型 `hidden_act=gelu` → erf 版，一致 |
| **softmax / sigmoid 实现**：candle vs numpy | 1e-7 量级 | 可忽略 |
| **`round()` 语义**：Python 银行家舍入 vs Rust 远离零 | `score` 第 4 位小数可能差 1；`raw` 第 6 位可能差 1 | **必须自写 half-to-even**，否则返回 JSON 会有可见差异 |
| **颜色通道顺序 / alpha 合成**：anime 必须「RGBA → 白底 composite → RGB」 | 不做的话半透明 PNG 分数会显著偏移 | 必须逐行复现 |
| **JPEG 解码器差异**：PIL `libjpeg-turbo` vs `image` crate 的 jpeg 解码器（纯 Rust） | IDCT 舍入不同 → ±1 LSB | 可量化；若超阈值需换 `zune-jpeg`/libjpeg 绑定 |

### 5.2 验证方法（可执行的三层门）

**第 0 层：基准数据集**（**当前不可能，这是最大障碍**）
`backend/data/app.db` 的 `images` 表 **0 行**、`settings` 表 **0 行** [已验证] → **没有任何既有美学分/标签可作回归基准**。必须先造基准集：
- 挑 200–500 张覆盖极端情况的图：纯色/灰阶、超长宽比、极小图（<64px）、带 alpha 的 PNG、CMYK/16-bit、损坏 JPEG。
- 用**现在能跑通的 Python 服务**跑一遍，落 `{path, score, raw, tags[]}` 为 golden JSON。**本轮已确认本机就能做**：推理服务在 `D:\Program Files\BeiWay-MoeVault\runtime\python\.venv` 下正常跑（torch 2.11.0+cu128、CUDAExecutionProvider 生效），不再需要另找目标机。

**第 1 层：单模型逐层比对**（Rust 与 Python 各导出中间张量）
- qalign：比对 `patch_embedding` 输出、`encoder.layers.11` 输出、`post_layernorm` 输出、`head` 输出、最终 `logits`。
- anime：比对 `vit.embeddings` 输出、`encoder.layer.47` 输出、`vit.layernorm` 输出、`classifier` logits。
- 验收线建议：`max_abs_diff(logits) < 1e-3`（F32 路径）/ `< 1e-2`（F16 路径）。**超线就不许切换，不要用「肉眼看分数差不多」当验收。**

**第 2 层：端到端分数门**
- 对基准集逐张比 `score`（4 位小数）与 `raw`（6 位小数）。
- 因为 `round(score,4)` 本身放大了量化，建议直接比**未 round 的 float64 值**，阈值 `|Δ| < 5e-4`；再单独统计 `round` 后不一致的图片数（应只有极少数临界样本）。
- 打标侧另加：标签集合 Jaccard ≥ 0.98 + 置信度 `|Δ| < 1e-3`（阈值附近的标签翻转要单独记录）。

**第 3 层：接口契约门**
- 用同一批请求打两套服务，逐字段 diff JSON（含字段名、类型、顺序、`elapsed_ms` 之外的数值）。
- 检查 `/health` 的 `state` 三态、`/devices` 的 `capabilities`、422 与 500 的错误文案。

### 5.3 已验证与仍无法验证的部分

**已从小节标题里的「无法验证」移出（本轮实测完成）：**

| 项 | 结果 | 方法 |
|---|---|---|
| 打标 ONNX 输入输出签名 | 1 输入 `pixel_values[N,3,384,384]` / 1 输出 `logits[N,108139]` | onnxruntime 读 session |
| 是否 naflex | 否（`is_naflex: false`） | metadata + 输入签名双重确认 |
| 手写预处理与 transformers 等价性 | prob_maxdiff 0.0133、阈值翻转 8/2487 | 40 图逐图对拍 |
| resize 内核选择 | 必须 BILINEAR；BICUBIC/LANCZOS 会翻转 41/55 处 | 6 图 × 3 内核 |
| GPU 是否真的参与推理 | 是，68% 利用率 / 3224 MiB | nvidia-smi 采样 + 并发压测 |
| 打标吞吐 | 183 ms/张 | 预热后 20 次串行 |

**仍然无法验证的（不含已在别处解决的）：**

- **candle CPU 上 anime 模型的实际延迟**：本机 torch 齐全但 candle 版尚未实现，无对照。
- **`ort` CUDA EP 在本机的可用性**：现役是 torch 的 cu128 运行时，onnxruntime-gpu 的 CUDA EP 走的是它自己那份；`ort` 要求系统级 CUDA 13.2 + cuDNN 9.23，本机未装，未验证。
- **candle 与 ort 两套 CUDA 栈能否共存**：未验证。
- **`ort` CUDA DLL 的实际体积**：未测。
- **美学两个模型迁移后的数值一致性**：仅完成了权重键匹配核对，未做端到端 logits 对拍。

**方法论教训（写下来避免重犯）**：初稿把「仓库 `find` 没找到模型」直接写成「本机不存在模型」，并据此把整个打标环节判为「不迁」。一个正确做法是**先问用户模型装在哪**——模型文件动辄 2GB，不放进版本库是常态，全仓搜索本来就不可能命中。

---

## 6. 收益量化

### 6.1 体积（分项已验证，合计为推测）

| 项 | 现状 | 迁移后（形态①，含打标 ort + 美学 candle） | 迁移后（形态①，仅美学 candle） |
|---|---|---|---|
| Python venv（CPU-only） | 0.85–1.0 GB [推测] | 0 | 0 |
| Python venv（CUDA torch） | 7–9 GB [推测] | 0 | 0 |
| Rust 二进制增量 | — | candle+safetensors ≈ +8–15 MB；ort 预编译 ORT ≈ +15–25 MB（CPU）[推测] | ≈ +8–15 MB [推测] |
| CUDA 运行时归属 | 由 wheel 自带 | **转嫁给用户系统 CUDA 13.2 + cuDNN 9.23** | 同左 |
| MSI 包体 | 10.0 MB [已验证] | 估 35–50 MB [推测] | 估 20–28 MB [推测] |
| 模型文件 | 2.4 GB（外置，不进包） | 不变 | 不变 |

**净效果**：用户机器上「少装 0.85–9 GB 的 venv」，换来「MSI 变大 10–40 MB」+「GPU 用户要自己装 CUDA Toolkit」。**净收益对 CPU 用户非常明显；对 GPU 用户的体验是负向的。**

### 6.2 启动时间

| 阶段 | 现状 | 迁移后 | 置信度 |
|---|---|---|---|
| 进程启动 | `python -m uvicorn` + `import torch` + `import transformers` + `import onnxruntime` ≈ **3–8 s** | Rust 进程启动 < 100 ms | [推测]（未实测） |
| 模型加载（懒加载，首次请求触发） | qalign 374MB F32 + anime 2.18GB F16，包加载 + 设备搬运 ≈ **1–3 s** | 量级相近（safetensors mmap 可能更快） | [推测] |
| `/health` 首次响应 | 受上两项影响，Tauri 探活需重试 | 明显更快 | [推测] |

### 6.3 内存

| 项 | 现状 | 迁移后 | 置信度 |
|---|---|---|---|
| 解释器 + 框架基线 RSS | 300–500 MB（torch 自身占用大） | 数十 MB | [推测] |
| 模型常驻 | qalign 374 MB + anime 2.18 GB（+ 计算中间张量） | 基本相同（candle 可用 mmap 降低峰值） | [推测] |
| **净节省** | — | **约 300–500 MB** | [推测] |

### 6.4 用户可感知的其它收益（这部分是确定的）

- **没有 pip 安装失败**：现状 `ensure_infer_venv()` 要跑 `py -3 -m venv` + `pip install` + 两个镜像源回退，是首启最脆弱的环节。迁移后消失 [已验证代码路径]。
- **没有 `Connection: close` 补丁**：现状为了绕开 reqwest keep-alive 死锁而在 middleware 里强制关连接，是性能与正确性的双重妥协。Rust 内嵌后自然消失 [已验证代码]。
- **没有「venv 被系统 python 污染」问题**：`start_infer()` 现在要判断 8001 上的服务是不是系统 python 并 kill 重拉，这类逻辑全部消失 [已验证代码]。
- **没有 `torch.set_num_threads(1)` 这类 Python 特有的死锁规避** [已验证代码]。

### 6.5 收益里【不要】算进去的

- **不减少模型体积**：2.4 GB 模型完全不变，且 `aesthetic_anime` 的 2.18 GB 是用户磁盘占用主体。
- **不提升吞吐**：现状是全串行 (`_infer_lock`)，迁移后如果在 Rust 侧也保持串行（**建议保持**，因为显存仍会冲突），吞吐不会有数量级提升。真正的提速手段是 batching 与异步流水，那是独立议题，不该被算进「语言迁移」的收益里。

---

## 7. 需要自研的代码量估算

按「形态①（保留 HTTP sidecar）+ 只迁美学」估算，单位：行（不含测试）：

| 模块 | 内容 | 估算行数 | 风险 |
|---|---|---|---|
| 图像预处理 | decode（jpeg/png/webp/bmp/gif）、RGB 转换、alpha 白底合成、resize（可配 filter）、归一化、HWC→CHW | 200–300 | 低（但数值对齐需反复调） |
| qalign 推理 | `VarBuilder` 加载 safetensors（键前缀重命名 `vision_model.`→``）、`siglip::VisionModel::new(cfg, use_head=true, vb)`、`linear` classifier、取标量 | 120–180 | 低（键已 8/8 匹配） |
| anime 推理 | `vit::Model::new(cfg, 2, vb)`、F16 处理、softmax 取 `[0,0]` | 80–120 | 低（键完全匹配） |
| 打分语义 | `AESTHETIC_SIGMOID` 分支、`clamp [1,5]`、**half-to-even 舍入**、返回 dict 字段名对齐 | 60–100 | 中（舍入必须自己写） |
| 模型注册表与生命周期 | 懒加载、`RwLock`/`Mutex`、设备切换、`load_error` 三态、热切目录 | 150–250 | 中（要复刻 Python 的语义，包括「加载失败不抛，记 error」） |
| HTTP 契约 | 6 端点 + 6 请求模型 + 错误文案 + 串行锁 | 250–400 | 低（照抄即可） |
| **小计（仅美学）** | | **约 850–1,350** | |
| 打标（现役 cl_tagger，**可迁**） | `ort` 会话 + EP 选择 + SigLIP2 processor 手写复现（已对拍验证） | **250–450**（初稿估 400–700，naflex 分支不存在后下调） | **中** |
| 打标（备查：若回退 wd14） | `ort` 会话 + EP 选择 + wd14 预处理 + CSV 词表 + sigmoid/阈值/降序 | 200–300 | 中 |
| 设备枚举 `/devices` | GPU 枚举（需 `nvml`/`wgpu` 或调 `nvidia-smi`）、CPU 名（`winreg` crate 读同名键） | 80–150 | 中 |

**配套改动（不属于新代码但必须计入工作量）**：
- `src-tauri/src/lib.rs`：`infer_*` 六个 command 改造或删除（`start_infer`/`ensure_infer_venv`/`infer_deps_missing`/`infer_install_gpu` 全部失去意义），约 150–250 行**删除** + 新增「sidecar 路径解析」约 50 行。注意 `tauri.conf.json` 的 `bundle.externalBin`/`bundle.resources` 要同步改。
- `frontend/src/api/infer.ts`：`infer_deps_missing`、`inferInstallDeps`、`inferInstallGpu`、`inferGpuStatus` 对应的 UI 需要删除或改写，约 100–200 行改动 + 设置页 UI。
- `python/` 目录整体处理（保留作参考/降级路径，还是删除？**建议保留一个 tag 后删除**）。

**测试/基准另计**：基准集跑批脚本 + 逐层比对工具 + 契约 diff 工具，约 300–600 行。

**总计**：约 **1,200–1,900 行新代码 + 300–600 行测试**（只迁美学、保留 HTTP）；若含完整打标，约 **1,700–2,600 行**。

---

## 8. 风险清单

按「会不会让项目翻车」排序。

### 🔴 阻断级（不解决就不能启动迁移）

| # | 风险 | 证据 | 应对 |
|---|---|---|---|
| R1 | ~~打标模型文件不存在~~ → **已消除**。模型在 `E:\Game\AI\cl_tagger\models`，签名与 naflex 均已实测 | onnxruntime 读 session + metadata | 已解决；模型已复制到 `runtime/models/tagger\` |
| R2 | **无 golden 基准**：`app.db` 的 `images` 表 0 行 | `backend/data/app.db` 实测 | 必须在目标机上用现存 Python 服务造基准集，再谈迁移 |
| R3 | **ort 的 CUDA 要求（≥13.2 / cuDNN ≥9.23 在 PATH）比现状（torch wheel 自带 CUDA）更重** | ort 官方文档原文 | 要么接受 GPU 体验退化，要么走 DirectML EP，要么**不迁 GPU 路径**（保留 Python 作为 GPU 侧可选增强） |
| R4 | ~~本机无法做任何 GPU 实验~~ → **已消除**。用户启用独显后 RTX 5070 Laptop 可用，打标实测 183 ms/张 | `nvidia-smi` + 并发压测 | 已解决；剩余风险是 `ort` 的 CUDA EP 与本机 torch cu128 不是同一套运行时，未验证 |

### 🟠 高风险（会显著增加工期或返工）

| # | 风险 | 说明 |
|---|---|---|
| R5 | `ort` 仍是 **2.0.0-rc.15**，非 stable | 作为交付底座不稳；升级 rc 可能有破坏性变更。建议锁定精确版本并写迁移隔离层 |
| R6 | candle 与 ort **两套 CUDA 栈共存** | candle 用 `cudarc` 自己编译 kernel，ort 用系统 ORT CUDA DLL，两者对 CUDA 版本要求可能冲突 [未知]。必须在目标机验证，混合方案的核心不确定点 |
| R7 | **anime 模型 CPU 性能未知** | 2.18GB F16、48 层 1536 hidden、1024×1024 输入。candle 在 CPU 上能不能跑到可接受延迟**完全没测**。若 CPU 慢到不可用，而 GPU 又因 R3 变难，则 anime 后端的迁移**净亏** |
| R8 | **resize/JPEG 解码数值漂移** | PIL（u8 整数 + libjpeg-turbo）vs Rust（f32 + 纯 Rust jpeg）。可能需自写与 PIL 同构的 resize，或换 libjpeg 绑定 |
| R9 | **resize 内核选错会静默改变标签**（原写「processor 无法离线复现」，已不成立） | 实测 BILINEAR 翻转 2 处、BICUBIC 41 处、LANCZOS 55 处。Rust `image` crate 没有与 PIL BILINEAR 完全同构的实现，需自行验证或自写 u8 双线性 |
| R10 | **舍入语义** | Python `round` 是 half-to-even，Rust 默认不是。写错会让 `score`/`raw` 在 JSON 层面对不上 |

### 🟡 中风险（可控但别忘）

| # | 风险 |
|---|---|
| R11 | 并发语义改变：现状单 `_infer_lock` 全串行，Rust 侧若不小心并行化会撞显存 |
| R12 | 热切换模型目录（`load_from_dir` / `set_device`）的语义要复刻，含「加载失败只记 error 不崩」 |
| R13 | `/devices` 的 GPU 枚举与 CPU 友好名（`winreg`）要重写；`capabilities` 字段被前端直接依赖 |
| R14 | Tauri `externalBin` / `bundle.resources` 改动会导致打包链路变化，MSI 需重新验证（含 `windows/main.wxs` 自定义模板） |
| R15 | 二进制体积：`ort` 默认 `copy-dylibs` 只在 binary crate 生效；若 sidecar 是多 crate workspace，DLL 可能不会被复制到运行目录 → 运行期「找不到 onnxruntime.dll」 |
| R16 | 首次 `cargo build` 含 candle CUDA kernel 编译，CI/开发机耗时会明显变长 [推测] |
| R17 | 模型精度：anime 是 F16 原生，若 candle 强制 F32 会**显存翻倍**（2.18→4.36 GB），可能 OOM |
| R18 | 无 CUDA 的机器上，anime 的 2.18GB 模型加载本身就可能让低内存机器卡死（现状同样存在，非新增风险） |

### 已排除的风险

- ❌「Rust 没有 SigLIP 实现」——**错**，candle 有，且键 8/8 匹配。
- ❌「Rust 没有 ViT 图像分类实现」——**错**，candle 有，`Model::new` 直接带 `classifier`。
- ❌「必须先把 safetensors 转成别的格式」——**错**，candle 直读 safetensors。

---

## 9. 结论与建议

### 9.1 结论

**部分迁移。不推荐一次性全面迁移。**

- ✅ **美学（qalign + anime）**：**推荐迁移**。这是唯一「证据充分、收益确定、风险可控」的部分：两个模型的权重键与 candle 现成实现**严格匹配**（qalign 8/8、anime 完全对应），预处理逻辑简单且已在配置里写死，不需要 transformers。
- ✅/⏸️ **打标（cl_tagger / SigLIP2，现役主力）**：**可迁，排在美学之后**。初稿判「不迁」的两个前提已被实测推翻（§0.3.1）：naflex 不存在、processor 可手写复现（40 图 prob_maxdiff 0.0133）。剩下的唯一代价是 `ort` 的 CUDA 部署要求更重。
- ⏸️ **打标（wd14，兼容分支）**：**备查**。预处理是纯手写逻辑、技术上可 1:1 复现，但它是退路不是主路径，迁它没有收益。
- ❌ **GPU 一键安装**：**迁移会让这个体验倒退**。ort 要求系统级 CUDA 13.2 + cuDNN 9.23，而 torch wheel 自带运行时。这一项必须提前和产品预期对齐。

### 9.2 建议的执行顺序（每步都有独立的验收点）

**第 0 步（前置，不做完不要动代码）**
1. ~~确认现役打标模型种类与文件位置~~ → **已完成：cl_tagger，模型在 `E:\Game\AI\cl_tagger\models`（见 §0.3 / §0.3.1）**。
2. ~~确认是否 naflex~~ → **已完成：不是（`is_naflex: false`）**。
3. ~~在有 GPU 的目标机上跑基准~~ → **GPU 已可用**。但 **200–500 张 golden 基准 JSON 仍未产出**，这是下一轮必须先做的事（现在打标服务跑得起来，可以出了）。

**第 1 步：离线验证 candle 能加载两个模型（1–2 天，纯本地，无风险）**
- 写一个最小示例：`models/aesthetic` 走 `siglip::VisionModel::new(cfg, true, vb)` + classifier；`models/aesthetic_anime` 走 `vit::Model::new(cfg, 2, vb)`。
- 验收：能出 logits，不报键缺失。**这一步不需要 GPU，本机就能做。**

**第 2 步：数值对齐（3–5 天）**
- 引入图像预处理模块，对基准集比对 `raw`/`score`。
- 验收：`|Δlogits| < 1e-3`（F32）/ `< 1e-2`（F16）；端到端 `|Δscore| < 5e-4`。
- 不达标就停在这里重新审视，不要「先上线再说」。

**第 3 步：美学 sidecar（形态①，5–8 天）**
- Rust 写独立进程，1:1 复刻 6 个端点；Tauri 侧把 sidecar 从 `python -m uvicorn` 换成新 exe。
- 验收：`frontend/src/api/infer.ts` 的 `InferHealth` 解析不变；后端 `AestheticResp` 解析不变；设置页「运行/降级/停止」三态正常。

**第 4 步（美学通过后再做）：打标**
- 先产出 golden 基准（第 0 步第 3 条），再按 `ort` 路线实现：ONNX session + `pixel_values` 单输入 + sigmoid + 阈值 + 降序。
- **预处理必须 BILINEAR**（对齐 PIL `resample: 2`），mean/std = 0.5。这条是硬约束，选错内核会让 41–55 个标签翻转。
- 数值验收线：prob_maxdiff `< 0.02`（对拍基准已达到 0.0133），0.5 阈值翻转 `< 15/2487`。
- 卡点：先验证 `ort` CUDA EP 能否在本机跑通；跑不通就只用 CPU EP，或保留 Python 作为 GPU 侧可选增强。
- wd14 分支仅在需要回退时再做。

**第 5 步（可选）：形态②（内嵌进主进程，删掉 8001）**
- 只有在第 3 步稳定运行一段时间后再考虑。

### 9.3 一句话给决策者

> 美学那部分（374MB + 2.18GB 两个模型）可以放心迁到 Rust，candle 已经有严丝合缝的现成实现，能顺带干掉 0.85–9 GB 的 venv。
> **打标那部分同样可以迁**——初稿说「预处理依赖 transformers、模型不在手边」的两条理由，经本机实测都不成立（模型在 E: 盘 2.2GB，`is_naflex: false`，手写预处理 40 图对拍 prob_maxdiff 0.0133）。建议排在美学之后做。
> 唯一要有心理准备的是部署侧：**迁到 Rust 会让「一键装 GPU」这个已经做好的体验倒退**，因为 ONNX Runtime 不像 PyTorch 那样把 CUDA 运行时打包进 wheel，它要求用户系统里已有 CUDA 13.2 + cuDNN 9.23。这是取舍，不是技术障碍。

---

## 附录 A：证据索引

本报告的全部「[已验证]」结论，来源如下（均在本机实际执行过）：

**源码逐行核对**
- `python/server/models/tagger_model.py`（318 行，全文）
- `python/server/models/aesthetic_model.py`（215 行，全文）
- `python/server/main.py`（335 行，全文，含请求模型与全部端点响应体）
- `python/server/config.py`（140 行，全文）
- `python/requirements.txt`、`python/run_server.bat`
- `backend/crates/tagger/src/{pipeline.rs,aesthetic.rs,lib.rs}`
- `backend/crates/api/src/routes/{settings.rs,aesthetic.rs}`
- `src-tauri/src/lib.rs`、`src-tauri/tauri.conf.json`
- `frontend/src/api/infer.ts`

**模型文件实测**
- `python -c` 直读 safetensors header（`models/aesthetic`：210 张量 / 全 F32 / 374,108,532 B；`models/aesthetic_anime`：776 张量 / 全 F16 / 2,182,620,068 B）
- `models/aesthetic/config.json`、`preprocessor_config.json` 全文
- `models/aesthetic_anime/config.json`、`preprocessor_config.json` 全文

**上游源码核对（GitHub raw 逐行）**
- `huggingface/candle`：`candle-transformers/src/models/siglip.rs`（25027 B）、`candle-transformers/src/models/vit.rs`（12535 B）、`candle-core/src/cpu_backend/mod.rs`（3415 行，MatMul 的 dtype 白名单在 1455-1457 行）、`candle-core/src/cpu_backend/conv2d.rs`（419 行）、`candle-core/src/cpu_backend/utils.rs`、`candle-core/Cargo.toml`
- `pykeio/ort`：`README.md`、`ort.pyke.io/perf/execution-providers`（CUDA ≥13.2 / cuDNN ≥9.23 / sm_75-12.x / WebGPU experimental）

**crates.io API 实测**
- `ort` 2.0.0-rc.15、`candle-core`/`candle-transformers` 0.11.1、`tract-onnx` 0.23.10、`burn` 0.23.0-pre.2、`burn-import` 0.22.0、`half` 2.7.1、`image` 0.25.10、`safetensors` 0.8.0

**wheel 体积实测（curl HEAD + PyPI JSON）**
- `torch` 2.15.0 win cp312：120.9 MB / 解压 461.7 MB / 13,110 文件
- `torch-2.5.1+cu124`：2,510,680,879 B；`torch-2.6.0+cu126`：2,496,082,434 B
- `onnxruntime-gpu` 1.32.0 win cp312：150.5 MB / 226.4 MB
- `numpy` 2.5.4：12.1 MB；`pillow` 13.0.0：6.9 MB；`transformers` 5.23.0 sdist：10.1 MB

**环境实测**
- `cargo/rustc` 1.97.1 可用；**系统** Python 3.12.11 无 torch/onnxruntime/numpy（但项目自带的 `runtime\python\.venv` 里全都有，见 §4.3）
- `nvidia-smi` 驱动不可用（**初稿写作时**；第二轮更正时已可用，见 §4.3）
- `backend/data/app.db`：`images` 0 行、`settings` 0 行
- `BeiWay-MoeVault_0.1.0_x64_en-US.msi` = 10,481,664 B

**本轮新增实测（第二轮更正依据）**
- onnxruntime 读 cl_tagger 模型签名：1 输入 `pixel_values[N,3,384,384] float32`，1 输出 `logits[N,108139] float32`，`custom_metadata_map` 为空
- model_metadata.json 原文：`{"vision_encoder_repo":"google/siglip2-so400m-patch14-384","is_naflex":false,"num_tags":108139}`
- preprocessor_config.json（hf_cache/hub/models--google--siglip2-so400m-patch14-384）: `SiglipImageProcessor`、mean/std 0.5、resample 2、size 384、rescale 1/255
- 手写预处理 vs transformers：单图 px_maxdiff 0.007843（纯 u8 取整误差）、prob_maxdiff 0.00114；40 图 prob_maxdiff 0.013272、阈值翻转 8/2487
- 6 图 x 3 resize 内核：BILINEAR 翻转 2 / BICUBIC 41 / LANCZOS 55
- GPU：RTX 5070 Laptop、驱动 595.76、8151 MiB；打标并发 68% 利用率 / 3224 MiB；20 次串行 3.659 s
- nvidia-smi、推理服务 /health、/infer/tagger/config 端到端跑通

**仍然未能验证（明确标注）**
- `ort` CUDA EP 在本机的实际可用性（要求系统级 CUDA 13.2 + cuDNN 9.23，本机未装）
- candle CPU 上 anime 模型的实际延迟
- `ort` CUDA DLL 的实际体积与 candle+ort 两套 CUDA 栈能否共存
- 200-500 张 golden 基准 JSON（打标服务现已可用，应尽快产出）

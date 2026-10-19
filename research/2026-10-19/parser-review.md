# 官网原文与解析结果审查

[研究索引](README.md) · [实施计划](../../docs/metadata-collection-plan.md) · [机器可读核查记录](parser-review-observations.json)

审查日期：2026-10-19。
代码与数据基线：`847f9ef`，负载按当前单人 CLI 维护目录判断。
本次审查发现两处证据归属校验缺口，以及六类实际内容遗漏。
下文八项均为 P2；这些发现尚未修复。

## 实际核查范围

在隔离目录中用当前 release 二进制重新请求全部 20 个厂商、56 个官方页面。
没有复制 HTTP 缓存凭据或条件请求收据，因此本轮获取的是新的上游响应。
51 页成功解析，5 页继续失败；逐页保存请求 URL、最终 URL、检查时间、原始响应 SHA-256 与结果。
成功结果中的 284 条结构化声明，与生产目录的值、单位、精度、作用域和条件一致。
结合官网正文逐项审查，未确认这些既有声明出现新的数值、单位或型号归属错误。
全部 9 个官方目录也已核对，当前保存的 96 条候选与对应响应相符，但存在下文所列漏项。
新采集副本通过 `validate --evidence`，这说明字节与解析结果可回放，不能替代本次对字段含义和完整性的人工核对。

另外匿名读取 9 个官方 HF 仓库 API、2 份固定提交的模板文件，以及 OpenRouter、Vercel 公共目录。
6 个首先抽查的 HF 记录，其已保存事实均与实时同路径原值一致。
其他样本用于核对独立模板、嵌套量化和专家精度。
生产代码、配置和数据没有修改；复现中的错误绑定仅存在于隔离样本。

## 应先修复的校验问题

### 1. P2：其他记录的字段仍能通过证据校验

位置：[registry.rs](../../src/registry.rs#L224)、[identity.rs](../../src/adapters/identity.rs#L41)。
身份重放检查记录的 ID 和名称，事实检查只验证指定 JSON Pointer 上确实存在该值。
它没有验证事实名称、指针和单位是否属于当前记录的对应字段。

用已有 [OpenRouter 目录](https://openrouter.ai/api/v1/models)快照构造两条记录的最小样本：

| 项目 | 原值 | 错误替换值 |
| --- | --- | --- |
| 记录 | `aion-labs/aion-2.0` | 保持不变 |
| 记录位置 | `/data/420` | 保持不变 |
| `context_window` | `131072` | `262144` |
| 字段位置 | `/data/420/context_length` | 另一型号的 `/data/78/context_length` |

`validate --evidence` 仍成功，`query aion-2.0 --min-context 200000` 也错误命中该记录。
当前 release 二进制已独立复现；没有发现正式目录已经发生这种错配。
若解析、迁移或手工编辑引入串记录错误，现有证据检查无法阻止错误值进入筛选结果。

最小修复是利用已有归一化结果，逐键比较已存事实的 `pointer`、`value`、`unit`，允许旧事实集合是当前映射的子集。
单纯检查指针位于记录子树还不够，同一模型内的不同字段也可能被错误交换。
回归应验证上述真实形状的错配被拒绝，同时保留合法历史字段子集。

### 2. P2：固定型号解析器允许配置绑定到另一个模型

位置：[asia.rs](../../src/documents/asia.rs#L9)、[details_asia.rs](../../src/documents/details_asia.rs#L12)、[mod.rs](../../src/documents/mod.rs#L223)。
旧解析器按 `source.id` 选择固定型号规则，再直接将声明归给 `source.records`。
配置校验仅确认该记录存在且类型是模型，没有核对解析器目标与绑定型号。
较新的 `expanded_asia` 已有明确绑定检查，旧路径没有同等保护。

复现保留 [Qwen3-8B 官方模型卡](https://huggingface.co/Qwen/Qwen3-8B/raw/main/README.md)和 `qwen-card` 标识，只将绑定记录改为库中已有的 DeepSeek-V3.2。
解析成功返回 15 条声明、零警告，将 `parameter_count=8200000000` 和 `license=apache-2.0` 等 Qwen 字段归给 DeepSeek。
这属于配置变更时才会触发的校验缺口，现有生产绑定未发现错误。
一旦发生，原文回放仍会成功，字段选择也可能把这些值当作目标模型的官方声明。

最小修复是把固定解析器的厂商、文档身份和明确型号绑定检查收拢到公共入口，同时覆盖基础字段和详情提取路径。
复用已有绑定规则，并用上述错误绑定作为回归输入。

## 官网原文已经包含、当前结果却遗漏的内容

### 3. P2：GPT-4.1 的弃用状态没有被提取

位置：[details_west.rs](../../src/documents/details_west.rs#L260)。
[GPT-4.1 模型页](https://developers.openai.com/api/docs/models/gpt-4.1)的当前 HTML 在标题和 `gpt-4.1-2025-04-14` 版本旁标注 `Deprecated`。
徽标与对应型号处于同一父节点，祖先没有隐藏属性。
当前解析只保存 12 条能力与版本声明；`inspect` 中的领域视图和字段选择没有生命周期信息。
旧缓存也有该徽标，因此这是已有提取规则的遗漏。

[官方弃用公告](https://developers.openai.com/api/docs/deprecations)进一步确认：2026-10-14 宣布该快照弃用，计划于 2027-01-14 从 API 移除。
这两个日期只用于本次交叉核对，未从模型页徽标推断，也未写入生产数据。
模型页的 [Markdown 版本](https://developers.openai.com/api/docs/models/gpt-4.1.md)省略该徽标，HTML 仍有；直接改用 Markdown 抓取会丢掉这项信息。

最小修复是在已确认型号的标题或版本区域提取明确的弃用状态，保存 OpenAI API 作用域与证据。
如需停用日期，应另外配置弃用公告并按精确快照匹配。
遗漏会使消费者在查看已支持能力和版本时，看不到作者已经发布的生命周期提醒；弃用也不能直接解释为已经无法调用。

### 4. P2：39 份 HF 响应中的独立模板没有进入事实

位置：[public.rs](../../src/adapters/public.rs#L49)。
151 份已有 HF 缓存中，39 份含 `/config/chat_template_jinja`，当前适配器全部遗漏。
其中 30 份 tokenizer 摘要没有模板，4 份旧模板与独立模板不同，另 5 份相同。

[Qwen3-Reranker-4B 官方 API](https://huggingface.co/api/models/Qwen/Qwen3-Reranker-4B)包含 741 字符的独立排序模板，处理 query/document 角色。
当前事实只保留 tokenizer 摘要中的 4,116 字符通用对话模板。
按 API 提交下载的[独立模板文件](https://huggingface.co/Qwen/Qwen3-Reranker-4B/resolve/22e683669bc0f0bd69640a1354a6d0aebcfeede5/chat_template.jinja)与独立 API 字符串逐字节相同。
[Transformers 官方加载实现](https://github.com/huggingface/transformers/blob/main/src/transformers/tokenization_utils_base.py)在存在独立模板时优先读取它。
这里只确认元数据遗漏，没有声称本项目已经执行了错误模板。

[NVIDIA Nemotron Super API](https://huggingface.co/api/models/nvidia/NVIDIA-Nemotron-3-Super-120B-A12B-BF16)也包含 10,771 字符的独立模板，当前 tokenizer 摘要没有该正文。
最小修复是保存独立模板事实及其原始指针，保留旧摘要值，然后离线重归一化。
两个模板同时存在且不同的回归样本，可以防止后续把旧摘要当成完整模板依据。

### 5. P2：嵌套量化配置和专家精度被遗漏

位置：[public.rs](../../src/adapters/public.rs#L46)。
当前只映射顶层 `dtype`、`torch_dtype` 和 `quantization_config`，没有保留已有响应中的以下信息。

| 官方 API | 原始路径与内容 | 当前结果 |
| --- | --- | --- |
| [Kimi K3](https://huggingface.co/api/models/moonshotai/Kimi-K3) | `/config/text_config/quantization_config` 中含 compressed-tensors、mxfp4-pack-quantized、4-bit 权重及忽略模块规则 | 整个文本组件量化配置缺失 |
| [Kimi K2.7 Code](https://huggingface.co/api/models/moonshotai/Kimi-K2.7-Code) | 同路径中含 pack-quantized、4-bit 权重 | 已有缓存中的配置同样缺失 |
| [DeepSeek V4 Pro 0813](https://huggingface.co/api/models/deepseek-ai/DeepSeek-V4-Pro-0813) | `/config/expert_dtype=fp4`，另有顶层 `quant_method=fp8` | 只保存顶层量化对象，专家精度缺失 |

这是原文信息缺失，不代表现有顶层量化事实本身错误，也不能据此给整个模型强行指定单一精度。
最小修复是独立保存 `expert_dtype` 和完整 `text_config`，或保存明确文本组件范围的量化对象，保留原始路径及忽略规则。
已有缓存中的两份嵌套配置和一份专家精度可离线补齐。
否则部署评估只能看到不完整的制品描述。

### 6. P2：同一官网响应中至少九个型号入口被目录筛选排除

位置：[directories.rs](../../src/documents/directories.rs#L22)、[路径规则](../../src/documents/directories.rs#L87)。
这些链接都已出现在同一提取结果的 `links` 中，却未进入 `catalog_entries`。
它们不涉及翻页、JavaScript 或 500 条数量上限。

| 目录 | 漏项 | 原因 |
| --- | --- | --- |
| [Mistral Research](https://mistral.ai/research/) | Ministral 3、Voxtral TTS、Voxtral Mini Transcribe Realtime、Timestral、Shieldstral | 只接受 docs.mistral.ai，排除同型号卡上的官方 HF 仓库和集合；九张带链接卡只留下四张 |
| [Google Models](https://ai.google.dev/gemini-api/docs/models) | Gemini Robotics ER 2 | 型号表行指向 robotics-overview，被 models/ 路径前缀排除 |
| [Z.AI Index](https://docs.z.ai/llms.txt) | GLM-OCR、GLM-Image、GLM-ASR-2512 | 只选列表项的第一个链接，同时遗漏 image/audio 型号路径 |

最小修复是保持型号卡、型号表行或目录列表的结构约束，接受已核实的官方型号 URL，并保留去重。
Z.AI 需要读取列表项内的所有链接，再应用型号路径过滤。
九条完整 URL、快照和精确定位见核查记录中的 `directory_omissions`。
这些遗漏会使后续候选发现和人工扩展看不到官网已经提供的型号入口。

### 7. P2：正文范围选错，四个成功页面漏掉官方链接

位置：[page.rs](../../src/documents/page.rs#L308)。
非目录页面优先选择 `main article`，但 HF 公开模型卡的真正正文位于 `.model-card-content`，`article` 是侧栏 Collection/Paper 卡片。
另一个触发方式是 NVIDIA 已下发的正文位于流式容器中，而 `main` 本身为空。
字段解析仍能成功，链接提取却只搜索错误或空的范围。

| 官网样本 | 当前结果 | 正文已经包含的漏项示例 |
| --- | --- | --- |
| [FLUX.2-dev](https://huggingface.co/black-forest-labs/FLUX.2-dev) | 仅保留一条侧栏 Collection 链接 | BFL 发布博客、官方 flux2 GitHub 仓库 |
| [SD 3.5 Large](https://huggingface.co/stabilityai/stable-diffusion-3.5-large) | `links=[]` | Community License 与许可页面 |
| [Stable Audio Open](https://huggingface.co/stabilityai/stable-audio-open-1.0) | `links=[]` | 官方许可页面与仓库 LICENSE.md |
| [Nemotron Super modelcard](https://build.nvidia.com/nvidia/nemotron-3-super-120b-a12b/modelcard) | 22 条声明，`links=[]` | 官方 BF16 下载仓库 |

目标链接已经存在于原始响应 DOM，无需执行 JavaScript。
最小修复是让已支持的 HF 页面使用真正的模型卡容器，拒绝空正文范围，并让 NVIDIA 链接提取复用已经识别的型号正文。
应以“侧栏有 article”和“空 main 外有已下发正文”两个真实结构补回归。
否则消费者无法从成功页面的结构化结果追溯作者提供的许可、发布和实现来源。

### 8. P2：官网发布者提供的域名和代码组织被白名单拒绝

位置：[mod.rs](../../src/documents/mod.rs#L95)、[组织映射](../../src/documents/mod.rs#L130)。
多个托管平台共用同一组作者名称，但官方 HF 组织名与 GitHub 组织名并不总是一致。
以下 URL 均由本轮实际读取的官方模型卡直接链接，当前 `is_official_url` 返回 false。

| 发布者原文 | 被拒绝的官方地址 |
| --- | --- |
| [Qwen3-8B 模型卡](https://huggingface.co/Qwen/Qwen3-8B/raw/main/README.md) | `qwen.readthedocs.io` 的部署与使用文档 |
| [MiniMax M3 模型卡](https://huggingface.co/MiniMaxAI/MiniMax-M3/raw/main/README.md) | `github.com/MiniMax-AI/MiniMax-M3` 与 `MiniMax-AI/MSA` |
| [ERNIE 模型卡](https://huggingface.co/baidu/ERNIE-4.5-21B-A3B-PT/raw/main/README.md) | `github.com/PaddlePaddle/ERNIE` |
| [SD 3.5 模型卡](https://huggingface.co/stabilityai/stable-diffusion-3.5-large)、[音频模型卡](https://huggingface.co/stabilityai/stable-audio-open-1.0) | `github.com/Stability-AI/sd3.5` 与 `Stability-AI/stable-audio-tools` |

即使修复第 7 项的正文范围，这些链接仍会被过滤；把它们配置为来源也会被拒绝。
最小修复是按平台维护已核实的域名和组织或仓库路径，保持明确范围，不向其他平台推断同名组织。
这样才能保留作者给出的代码与文档证据链。

## 可继续改善，但本次不列为新缺陷

- PDF 选摘遵守已声明的六段预算，但 Claude 4 system card 的 124 页中，正文从第 7 页开始，当前选摘全部落在封面、说明、摘要和目录。
  可改为选择明确正文章节，继续保留页码与截断警告。
- Kimi K3 模型卡已保存文本、图像、视频能力的明确段落，却没有 `input_modalities` 声明；这是可直接补齐的字段覆盖项。
- OpenAI 的端点与工具清单、AWS 的语言与带跨区条件的区域信息也可继续提取，保留其服务作用域和条件。
- `documents check` 比较双方历史声明时不携带目录 `observation`，虽然符合当前历史比较定义，单独消费此输出仍不易辨认目录最近是否失败或缺席。
  可增加该状态，而不改变历史事实本身。

## 本次未误判为解析缺陷的情况

Kimi 博客、Kimi 目录、OpenAI GPT-4.1 博客本次实际请求仍返回 HTTP 403。
Qwen 动态博客入口仍是应用壳，xAI 历史型号页已不再满足目标型号身份检查。
五页都如实记录失败，没有把不合格内容当成正常更新。

OpenRouter 和 Vercel 的实时目录出现价格、输出上限等动态变化；这些变化需要正常同步，不能仅凭旧值不同就判断解析器出错。
OpenRouter 记录总数还包含历史保留条目，不能直接与一次实时返回数量比较并推断解析失败。
同样，当前 PDF 预算、目录不翻页、字段未知值以及未执行真实推理都属于已声明的范围。

建议先修复第 1、2 项证据归属检查，再补第 3、4、5 项生命周期、模板和制品信息，最后处理第 6、7、8 项目录与链接完整性。
本次没有执行认证目录或模型推理，也没有以现有自动测试通过替代官网原文核对。

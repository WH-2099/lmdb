# 模型元信息采集扩充与实现审查

[返回本轮研究](README.md) · [研究索引](../README.md) · [路线与进度](../../project/roadmap.md)

核查日期：2026-10-19。
代码与数据基线：`fde0614`。
本轮围绕字段、信息来源与采集实现提出建议，包含官方资料搜索、匿名只读接口抽样、现有快照盘点和本地代码复现。
建议先修复证据与更新状态的缺口，再补现有快照中已经存在的字段，然后增加逐提供方目录和官方价格来源。
下述建议尚未作为实现决定；认证接口仅核对官方契约。

## 现有基础与扩充方向

当前有 14 个来源：models.dev、Hugging Face、ModelScope、OpenRouter、Vercel、official，以及默认关闭的 Artificial Analysis、OpenAI、Anthropic、Gemini、DeepSeek、Cohere、fal、Replicate。
其中 6 个属于公开来源；HF 和 ModelScope 采集精选仓库，official 采集精选页面，覆盖范围见 [来源配置](../../config/sources.yaml)。
fal、Replicate 已有适配器，xAI、Mistral、NVIDIA 已有部分官方页面；新增其结构化目录属于深化采集。

现有 [领域投影](../../src/domain.rs) 覆盖任务、模态、token 限制、参数、能力、谱系和价格。
许可证、语言、日期、架构、版本、生命周期等已有不少原始 facts；官网提取还保存总参数、激活参数、原生上下文与 YaRN 条件等 claims。
改进需要明确区分三种工作：补原始字段映射、让已有信息可按类型查询、访问新的描述资源。

| 优先顺序 | 工作 | 可获得的增量 |
| --- | --- | --- |
| 第一批 | 身份证据校验、页面退化检测、版本跟踪、最近观察状态 | 防止错误归属、空内容覆盖和历史记录被误读为当前声明 |
| 第二批 | 回放已有 HF 快照，补映射与必要的类型投影 | 无需新增网络来源即可补配置、许可证名称、数据集和模板信息 |
| 第三批 | 网关逐提供方接口、AWS/Azure 价格、选定仓库文件清单 | 获得服务实际限制、计价条件与可部署制品信息 |
| 第四批 | xAI/Mistral 原生目录、HF 评测引用、Arena 数据 | 补原生能力与有归因、版本和方法范围的评测证据 |

## 一、值得扩充的字段

### 先利用已经保存的原始响应

按当前 HF 目录中 151 条记录实际引用的快照盘点，151 份响应均可回放。
以下计数表示字段存在，不能解释为相应内容完整或已经验证可运行。

| 原始字段 | 存在份数 / 151 | 当前处理 |
| --- | ---: | --- |
| `config.tokenizer_config` | 101 | 原始快照已有，未映射为 fact |
| `transformersInfo` | 97 | 原始快照已有，未映射 |
| `cardData.license_name` | 34 | license、license_link 已映射，自定义名称未映射 |
| `config.auto_map` | 30 | 原始快照已有，未映射 |
| `cardData.datasets` | 20 | 原始快照已有，未映射 |
| `config.quantization_config` | 9 | 原始快照已有，未映射 |
| `cardData.model-index` | 8 | 旧评测 payload 已保存，未映射 |
| `config.num_experts_per_tok` | 8 | 原始快照已有，未映射 |
| `cardData.new_version` | 2 | 原始快照已有，未映射 |
| `cardData.language` | 82 | 已映射 languages fact，领域投影未提供 |
| `siblings` | 151 | 原始文件名已有；当前请求主要未带文件大小与 hash |

代码落点为 [HF 适配器](../../src/adapters/public.rs) 的 `huggingface`。
字段语义见 [HF model card 规范](https://huggingface.co/docs/hub/en/model-cards) 与 [metadata 规范](https://github.com/huggingface/hub-docs/blob/main/modelcard.md)。

### 字段组与语义边界

| 字段组 | 建议补充 | 实施边界 |
| --- | --- | --- |
| 许可证 | 原始标识、自定义名称、链接；相对文件链接的 repo/revision | 已有 license/link 继续使用；开放权重不能推出商业使用许可 |
| 量化与权重 | 方法、格式、bits、group/block size、activation scheme、排除模块 | 发布权重配置与服务使用的量化分别记录；dtype 不能替代量化方法 |
| Tokenizer 与模板 | tokenizer/processor 摘要、模板名称、路径、revision、hash | 模板存在不证明 tools/reasoning 已通过行为验证 |
| 运行入口 | library、auto_map、transformersInfo、配置声明的版本 | 保存声明；保存时的库版本不等于最低兼容版本 |
| 文件清单 | 路径、格式、字节数、Git OID、LFS SHA256、repo commit | 读取 manifest 和小型配置即可；权重文件 hash 与响应 hash 分开 |
| 架构与专家 | 层数、hidden size、attention/KV heads、专家总数、每 token 专家数 | 已有 total/active 参数声明继续使用；不能按专家比例推算激活参数 |
| 版本与生命周期 | 官方别名目标、successor、发布日期精度、状态及生效日期 | 仓库创建、模型发布、服务退役和退出本地采集范围各有含义 |
| 语言与训练出处 | languages 类型投影、发布者列出的 datasets、训练阶段 | 标签不构成完整训练语料清单；训练、微调、评测数据分别归因 |
| 上下文与条件 | native/configured/extended/API 限制的统一查询及条件 | 复用已有条件 claims，保留单位与适用范围 |
| 服务与价格 | provider、region、service tier、计价数量、缓存类型、阈值、有效期、SKU | 保留精确十进制原值和来源单位；账号可用性独立于公共目录 |
| 评测引用 | 被测型号/版本、dataset/task/revision、metric、方法、发布日期、归因与验证状态 | 先保存引用原值；需要排序比较时再建立独立结果集合 |

专业任务已有不少条件声明：现有 assertions 覆盖 embedding 维度、输入条数、截断、rerank 文档 token 限制，音视频提取器也保存采样率、时长和分辨率。
可补的是 embedding 输出表示（float/int8/binary 等）、input_type 用途，以及按模型版本和输入形态限定的图像大小；依据 [Cohere Embed](https://docs.cohere.com/reference/embed)。
[Cohere Rerank](https://docs.cohere.com/reference/rerank) 的 1,000 篇文档为性能建议，不能录成硬上限。

[Qwen FP8 官方配置](https://huggingface.co/Qwen/Qwen3-235B-A22B-FP8/blob/main/config.json) 同时给出 `torch_dtype=bfloat16`、`quant_method=fp8`、动态 activation 和 block size，说明单一“精度”字符串会丢信息。
[Transformers 模板文档](https://huggingface.co/docs/transformers/en/chat_templating_writing#storing-and-loading-chat-templates) 已支持 `chat_template.jinja`、`additional_chat_templates/<name>.jinja`，多模态模板还可能属于 processor。
仅增加 `tokenizer_config.chat_template` 仍会漏掉独立模板文件。

[HF 官方客户端](https://github.com/huggingface/huggingface_hub/blob/main/src/huggingface_hub/hf_api.py) 将 `files_metadata=True` 编码为 `blobs=true`。
匿名读取 `Qwen/Qwen3-235B-A22B?blobs=true` 得到 38,066 字节 JSON，其中一个权重分片明确给出 3,993,004,560 bytes 与 LFS SHA256；本次没有下载权重。
该接口的 `expand` 与 `files_metadata` 不可混用，摘要与文件清单应分请求。

新增配置文件还涉及证据归属：[Fact](../../src/model.rs) 仅保存 pointer，snapshot 在 `Record.origin`。
来自另一份 `config.json` 的值必须关联该文件自己的 snapshot 和 revision，不能继续指向目录 API 的 JSON Pointer。
可以沿用已有快照与引用机制，为文件描述保存独立记录，再做投影；无需先设计覆盖所有制品的复杂体系。

## 二、值得扩充的信息来源

### 优先深化现有来源

**OpenRouter 与 Vercel 的逐提供方接口。**
当前适配器采集聚合型号目录；两个平台另有 `/models/{creator}/{model}/endpoints`，可以提供 provider、限制、价格、参数支持、量化或缓存能力，以及带窗口的延迟、吞吐与可用率。
[OpenRouter 契约](https://openrouter.ai/docs/api/api-reference/endpoints/list-all-endpoints-for-a-model) 与 [Vercel 契约](https://vercel.com/docs/ai-gateway/sdks-and-apis/rest-api#get-model-endpoints)。

本次匿名读取 `openai/gpt-4.1` 均为 HTTP 200，OpenRouter 返回 3 个端点，Vercel 返回 2 个。
两边 Azure 端点的 `max_completion_tokens` 分别为 **942,818** 与 **32,768**。
这只是两个来源的当次声明，未通过推理请求验证；应并存保留并产生待核查差异，不能选择较大值作为基础模型能力。
OpenRouter 文档将 Authorization 标为 required，与本次匿名成功不同，接入时必须保留这个访问契约差异；Vercel 文档明确该接口无需认证。
Vercel 的延迟为一小时窗口 TTFT 毫秒、吞吐为 tokens/s，不能去掉窗口后写成模型永久速度。
该页价格描述明确 per-token，但不足以单独确认每个价格字段的币种，仍需独立单位证据。

**HF 评测引用。**
10 月 15 日研究已介绍 `.eval_results/*.yaml`；本轮确认了可读取的 `expand=evalResults` 与 dataset leaderboard 接口。[官方读取指南](https://huggingface.co/docs/hub/en/leaderboard-data-guide)、[格式与验证规则](https://huggingface.co/docs/hub/en/eval-results)。
`Qwen/Qwen3.5-397B-A17B` 样本返回 21 条结果，全部 `verified=false` 且带 PR；同次检查的 main 文件清单没有对应 `.eval_results`。
因此既不能只枚举 main 后声称完整覆盖，也不能把模型仓库页面展示的社区结果当作厂商声明。
保存 filename、PR、source.user、verified、dataset/task；metadata revision 与实际被测权重版本分别记录。

**Artificial Analysis 的版本与结果范围。**
现有适配器有意省略评测分数；当前 [OpenAPI](https://artificialanalysis.ai/api/v2/openapi) 的免费目录已有部分 indices，是否导入属于范围选择。
若导入，应同时保存响应根 `intelligence_index_version`；它只表达 major.minor，不能用 [方法页](https://artificialanalysis.ai/methodology/intelligence-benchmarking) 当前 patch 版本补写历史结果。
免费与 Pro 路由可接受的查询参数不同；免费结果的具体测量条件尚未核实，缺失的 prompt length、并发与测量条件应保持未知。

### 新结构化来源的取舍

表中“优先”表示增量与接入成本的判断，不表示已启用采集。

| 来源 | 值得补充的信息 | 本轮验证与接入条件 | 建议 |
| --- | --- | --- | --- |
| [AWS Bedrock 价格文件](https://docs.aws.amazon.com/awsaccountbilling/latest/aboutv2/using-the-aws-price-list-bulk-api-fetching-price-list-files-manually.html) | 区域、SKU、价格版本/有效期、服务等级、token/缓存类型、阶梯价 | 匿名成功；区域索引有 36 个区域，大阪样本 71 个 SKU | 优先接选定区域的固定版本文件 |
| [Azure Retail Prices](https://learn.microsoft.com/en-us/rest/api/cost-management/retail-prices/azure-retail-prices) | 区域、product/SKU/meter、单位、币种、有效期 | 匿名首页 1,000 条且有下一页；样本服务分类为 Foundry Models | 优先接明确筛选和完整分页，型号绑定需证据 |
| [xAI Models API](https://docs.x.ai/developers/rest-api-reference/inference/models) | alias/version/fingerprint、模态、reasoning effort、长上下文价、图像矩阵价 | 契约已读；需 Bearer key；通用列表与各模态详细列表有互补字段 | 优先作为显式启用的原生目录 |
| [Mistral Models API](https://docs.mistral.ai/api/endpoint/models) | aliases、能力、上下文、微调 root、archived | 需 Bearer key；目录属于用户及区域可见范围 | 下一批；公开基础型号与私有微调分别处理 |
| [Arena 官方数据集](https://huggingface.co/datasets/lmarena-ai/leaderboard-dataset) | 偏好结果、类别、置信区间、票数、发布日期与历史分区 | 匿名 metadata/单行均成功；数据集声明 CC-BY-4.0 | 新评测来源中优先接选定 latest 分区 |
| [Google PublisherModel](https://docs.cloud.google.com/gemini-enterprise-agent-platform/reference/rest/v1/publishers.models) | framework、launchStage、versionState、预测 schema、部署动作 | 需 Cloud OAuth、project 与地域范围 | 有云部署查询需求时接；补充现有 Gemini API |
| [Together Models](https://docs.together.ai/reference/models) | 托管价格、上下文、模型类型、许可证标识与模型链接、dedicated 范围 | 需 Bearer key | 需要覆盖其服务时接 |
| [Fireworks Models](https://docs.fireworks.ai/api-reference/list-models) | HF 引用、PEFT/base、能力、serverless 退役、默认采样与部署引用 | 需 Bearer key、分页及按需详情 | 控制面目录比普通兼容接口更有价值 |
| [ElevenLabs Models](https://elevenlabs.io/docs/api-reference/models/list) | 语言、字符限制、音色控制/转换、并发组、成本乘数 | 文档标 key 可选；匿名两次 404 workspace_not_found | 先作为认证候选，不能标为验证通过的公开源 |
| [NVIDIA 精选 feed](https://assets.ngc.nvidia.com/products/api-catalog/featured-models.json) | 精选 ID 与限制 | 匿名成功，只有 4 条；未确认稳定接口承诺 | 限精选范围，不能代表完整 NIM/NGC 目录 |

AWS 的 [ListFoundationModels](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_ListFoundationModels.html) 与 Azure 的 [Models List](https://learn.microsoft.com/en-us/rest/api/aiservices/accountmanagement/models/list?view=rest-aiservices-accountmanagement-2024-10-01) 可再补生命周期、部署与定制能力，但需要云控制面认证。
现有 `RequestSpec` 的固定 URL/静态凭据头不足以表示 AWS SigV4、OAuth、订阅与地域参数；先接匿名价格文件能以较小改动获得有效增量。
价格存在不能证明某账号有权调用，也不能证明所有区域都可用。

三个容易造成错误的数据细节：

1. Azure 旧筛选 `serviceName='Cognitive Services'` 加 Azure OpenAI 条件本次返回空；较宽的 OpenAI 产品筛选成功返回 Foundry Models 条目，不能把旧分类为空解释为下架。
2. xAI 文本价单位为美分/一亿 tokens，换为 USD/百万 tokens 需除以 10,000；特定长上下文字段的零值表示沿用普通价，图像矩阵又使用不同尺度。
3. AWS 价格样本包含字符串 `0.0009625000 USD / 1K tokens`、priority、Cache Read Input Tokens；这些条件必须保留，不能合成无条件的模型输入价。

[GitHub Models 官方页面](https://docs.github.com/en/github-models) 明确整个产品已于 2026-07-30 退役，旧教程的 catalog API 应从候选中排除。
更多聚合目录可用于发现差异，但转录同一上游的信息不能计为独立证据。

### 评测信息的最小范围

Arena 的普通偏好榜与 Agent Arena 使用不同方法；应保存方法引用、类别和发布日期，不能统一命名为 Elo。[官方数据集卡](https://huggingface.co/datasets/lmarena-ai/leaderboard-dataset)。
本次单行样本模型名含 `(High)`，不能仅凭名称把它绑定到厂商 API 的确切推理设置。
HF 的上游 verified、本地 accepted 审查、提交 PR 与厂商发布是不同状态。
跨来源比较至少需要被测主体、任务/数据集版本、metric 名称与方向/单位、方法或 harness、effort/budget/tools、运行及发表时间、样本数和不确定性。
来源没提供的字段保持未知；当前 repo SHA 不等于评测实际权重 SHA。

[SWE-bench 提交规范](https://github.com/SWE-bench/experiments/blob/main/checklist.md) 把 agent、model、attempts、reasoning_effort 分列；[MLPerf](https://github.com/mlcommons/inference_results_v6.1) 还依赖硬件、精度与场景；[AILuminate](https://mlcommons.org/ailuminate/safety-faq/) 的被测系统可含护栏和系统提示。
这些适合作为后续系统评测来源，不应直接写成模型固有 coding/safety/速度字段。
LiveBench、EvalPlus 本轮用于核对方法版本和指标语义；尚未证实的最新性与数据范围不支持将其立刻作为实时主目录。

## 三、采集实现的具体改进

现有实现已有共享 HTTP client、单次请求超时、字节上限、ETag/Last-Modified/304、429/5xx 重试、Retry-After、同源分页、原子写入、缺席记录保留和原始证据归档。
本轮建议集中在以下可定位的缺口。

| 顺序 | 问题与代码依据 | 影响 | 建议与有效验证 |
| --- | --- | --- | --- |
| 1 | `registry.rs:161–215` 核对 facts 与原文，但不核对 upstream_id/name 的原文来源 | 身份与名称同时写错仍可通过 `validate --evidence`，已独立复现 | 先核对已支持来源的身份与 name fallback；再考虑带 normalizer 版本的全记录回放；用错误 ID/名称及合法 fallback 验证 |
| 2 | `documents/mod.rs:270–275,538–540` 将结构缺失作为 warning，所有 Ok 都覆盖旧内容 | HTTP 200 应用壳会替换有效提取，已复现 | 按来源声明的页面身份和结构判失败，复用现有保留旧内容的 Err 分支；验证正常→壳页→恢复 |
| 3 | 10 个 HF README 固定历史 SHA，部分解析器也硬编码完整 URL；`http.rs:271` 未填 upstream_revision | 同步成功仍可能一直读取旧模型卡 | 配置 tracked ref 或 pinned ref；每轮解析一次 SHA，用同一 SHA 取 README/config；身份校验绑定 repo/path，验证 ref 更新与旧证据回放 |
| 4 | `documents/resolve.rs:100–103` 仅按非 null 判断 catalog fact 可用；普通 query 未携带观察状态 | 最近缺席的旧记录与仍被返回的记录参与相同选择 | 扩展 SyncManifest，补采集范围、完成状态与失败观察，再派生返回/完整列表缺席/未请求/请求失败；在 query/resolve 导出及筛选 |
| 5 | `sync.rs:168–186` 对独立仓库详情使用 `try_collect` | 一个失效 seed 会阻断整个来源刷新 | 逐条保存结果并标 partial；完整分页目录仍需全部页成功才声明完整；验证 3 个 seed 中一个失败 |
| 6 | `http.rs:183–186,253–254,471–495` 未纳入暂时网络错误与 HF reset 头 | 短暂断流即失败，配额未恢复前迅速耗尽重试 | 在现有流程增加有界 GET 重试与 retry_at/deferred；需要总时限时让 deadline 覆盖请求和等待 |
| 7 | 文档只支持 UTF-8 HTML/Markdown，所有跳转关闭；已保存 Claude system card 307 | 官方 CDN/PDF 型资料无法进入现有提取流程 | 先确认稳定官方入口；文档路径增加逐跳主机校验，PDF 独立保存原文与页码证据 |
| 8 | CI 普通 validate 不恢复原始证据；`.cache` 不入 Git | 新 clone 无法自动核对原文 hash 与字段指针 | 使用已有 archive/restore，为数据提交关联持久证据包，在 CI 跑 `validate --evidence` |

代码链接：[registry](../../src/registry.rs)、[documents](../../src/documents/mod.rs)、[resolve](../../src/documents/resolve.rs)、[sync](../../src/sync.rs)、[HTTP](../../src/http.rs)、[CI](../../.github/workflows/check.yml)。
上述行号以 `fde0614` 为准。

### 两个本地复现

**身份证据。**
原文是 `{"id":"example/real-model","name":"Real Model","author":"example"}`。
测试目录保存正确原文 SHA、origin 和 `/author` 的 publisher fact，但将 `upstream_id` 改为 `example/invented-model`、`id` 改为对应标准前缀、name 改为 `Invented Model`。
已有 release 二进制执行 `validate --evidence` 返回 0，输出 `Validated 1 records, 0 identities and 0 assertions with cached evidence`。
此复现使用 `/tmp` 独立目录；它证明校验覆盖不完整，不能据此推断现有正式数据已经出现同类错误。
若采用适配器回放，需要保存版本或明确重归一化迁移；新增一个字段映射不应使历史目录被误判为证据失真。

**提取退化。**
已配置的 `openai-gpt41-model` 当前有 12 条 claims、1 个 passage。
向提取器传入 `<html><head><title>Please wait</title></head><body><div id="app"></div></body></html>`，得到 `Ok`、0 claims、0 passages 与两条 warning。
同步代码接受所有 Ok 结果，因此会替换旧内容；本轮只运行提取器并检查该调用路径，没有改写正式目录。
判失败应依据预期页面身份或结构，不能把所有零字段页面统一拒绝，因为部分目录和发布页允许不同产出。

### 版本、状态与协议依据

[HF revision 下载](https://huggingface.co/docs/huggingface_hub/en/guides/download#from-specific-version) 与 [refs/snapshots 缓存](https://huggingface.co/docs/huggingface_hub/en/guides/manage-cache#refs) 支持“跟踪可变引用、按提交固定本轮证据”的实现方式。
`config/official-pages.yaml` 的 10 个固定 README 是第 3 项的范围；部分官方文档 URL 跟踪 main，新增多文件采集时同样应固定到本轮解析的 SHA。
静态研究资料仍可显式保留 pinned 模式。

[HF 限流文档](https://huggingface.co/docs/hub/en/rate-limits#http-headers) 给出 RateLimit 剩余配额与重置秒数。
当前 30 秒重试预算只扣等待，单次请求另有 30 秒超时，不能称为整个 fetch 的 30 秒截止时间。
请求失败、退出精选清单、完整目录缺席与明确退役必须区分；HTTP [404](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.5.5) 和 [410](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.5.11) 也各有语义。

[官方 Claude 4 system card PDF](https://www-cdn.anthropic.com/4263b940cabb546aa0e3283f35b686f4f3b2ff47/claude-opus-4-and-claude-sonnet-4-system-card.pdf) 是格式缺口的具体候选。
本轮核对了已有 307 错误与官方 PDF 入口，未重新验证旧入口当前每一跳的 Location。

## 实施划分与验收

1. **证据与状态修复。** 完成身份、页面退化、revision 和观察状态；验收使用已描述的错误归属、壳页、ref 更新、缺席/恢复样例。
2. **现有快照回放。** 增加 HF 映射与需要的查询投影；每个新值必须能沿 snapshot/pointer 回到当前缓存原文，明确执行数据迁移。
3. **结构化采集扩充。** 先网关端点、文件 manifest、AWS/Azure 价格；保留来源单位、选择范围、分页完成状态和版本，不按名称强行合并型号。
4. **原生目录与评测。** xAI/Mistral 按既有显式启用模式接入；HF/Arena 先保存引用、归因与结果范围，再按查询需求扩展实体。

一次采集成功只描述当次访问结果；公开可读不等于响应可任意再分发，代码、模型、目录、评测数据的许可证分别记录。
本轮未使用真实凭据、调用模型推理或执行评测；需要认证的目录访问与账号范围继续待验证。
匿名观察与当前快照统计的摘要见 [机器可读核查记录](metadata-observations.json)。

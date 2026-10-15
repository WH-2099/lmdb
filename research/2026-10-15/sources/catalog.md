# 模型元数据来源目录

核查日期：2026-10-15。
范围是模型发现、作者声明、接口约束、谱系与平台目录，不把排行榜完整性等同于模型覆盖率。
本轮核查了 26 组来源，部分来源包含多个相关端点。
访问样本和文档冲突的可复核记录见[访问采样记录](../evidence/source-probes.md)。

## 这一轮得到的结论

1. 单一目录无法覆盖“所有模型”，应把它作为长期发现目标，先选跨任务的代表样本检验信息能否被准确记录。
2. 官方模型列表正在提供更多结构化能力字段，但各家字段深度差异很大；官方说明、请求 schema 和模型卡仍然必要。
3. 模型能力、某协议的请求规则、某平台的实际部署限制需要保留各自适用范围。
4. 对图像、视频、音频与 3D，fal 和 Replicate 的输入输出 schema 比通用聊天模型目录更有价值。
5. OpenRouter 当前文档的默认模型筛选仅包含文本输出；跨模态采集要显式使用 `output_modalities=all` 并处理分页。
6. Artificial Analysis 的旧参考页、新概览与实时 OpenAPI 存在差异，不能只保存一个来源首页再假定接口长期稳定。
7. 公开访问、软件开源、模型权重许可、目录数据再分发是不同问题；目前多数目录的批量再分发条件仍待确认。

第 2、3 点有直接例子：DeepSeek 的模型列表已经描述不同协议下更新 system prompt 的方式；Jina 官方说明区分模型架构上下文与托管 API 上限。
它们说明需要按字段保存证据与适用条件，不能只记录 `supports_image: true` 或一个通用上下文数值。
[DeepSeek Models](https://api-docs.deepseek.com/api/list-models/)、[Jina Embeddings](https://jina.ai/embeddings/)。

## 阅读约定

`P0` 表示适合第一批概念验证；`P1` 表示补齐类别或约束；`P2` 表示平台扩展阶段。
优先级不代表已经授权大量抓取或承诺建立适配器。
`docs_read` 表示已读官方正文或源码，`api_sampled` 表示实际请求了所述元数据 API，`unverified` 表示仍需核实。
没有单独说明时，本轮未验证账号鉴权、目录专用限流和批量再分发许可。
推理 API 的 RPM、TPM 或并发额度不能直接套用到文档抓取和模型列表请求。

## 来源总览

| 编号 | 来源 | 优先级 | 首要价值 | 本轮证据 |
| --- | --- | --- | --- | --- |
| S01 | OpenAI 官方模型页与 Models API | P0 | 官方 ID、版本、能力、生命周期 | docs_read |
| S02 | Gemini Models API 与官方模型说明 | P0 | token 限制、方法、参数范围 | docs_read |
| S03 | Anthropic Models API | P0 | 能力、生命周期、游标分页 | docs_read |
| S04 | Hugging Face Hub、模型卡、配置 | P0 | 开放模型、谱系、任务与仓库版本 | docs_read + api_sampled |
| S05 | ModelScope | P0 | 中国模型社区、中文任务与作者元数据 | 源码已读 + api_sampled；目录文档待补 |
| S06 | OpenRouter Models | P0 | 规范化模型目录、跨源 ID | docs_read + api_sampled |
| S07 | OpenRouter Providers 与 Endpoints | P2 | 开发者与服务商关系、部署差异 | docs_read + api_sampled |
| S08 | Artificial Analysis 模型与媒体 API | P1 | 稳定 ID、模型补充与版本化测量来源 | docs_read + OpenAPI sampled；业务 API 未鉴权 |
| S09 | Artificial Analysis Providers / 原 Hosts 线索 | P2 | 服务商数据和性能测量 | docs_read；Commercial 访问未验证 |
| S10 | Replicate | P0 | 模型版本与输入输出 OpenAPI schema | docs_read |
| S11 | fal | P0 | 图像、视频、音频、3D 目录及 schema | docs_read |
| S12 | Cohere | P0 | embedding、rerank 等结构化目录 | docs_read |
| S13 | Voyage AI | P1 | 检索模型、维度、截断与批次规则 | docs_read |
| S14 | Jina AI | P1 | 多模态 embedding 与部署限制 | docs_read |
| S15 | Amazon Bedrock | P2 | 托管模型能力、生命周期与部署类型 | docs_read |
| S16 | Google Model Garden | P2 | 发布者、版本、框架、部署动作 | docs_read |
| S17 | ElevenLabs | P1 | 音频能力、语言、字符上限 | docs_read |
| S18 | Runway | P1 | 视频、图像、音频模型目录 | docs_read |
| S19 | Stability AI | P1 | 图像、音频、3D 的参数约束 | docs_read |
| S20 | NVIDIA NIM | P2 | 视觉/语言部署 profile、GPU 与精度 | docs_read |
| S21 | DeepSeek | P0 | 官方模型与按协议声明的行为 | docs_read |
| S22 | 阿里云百炼 / Model Studio | P1 | Qwen、Wan、音频、检索及其他厂商目录 | docs_read；目录 API 待补 |
| S23 | SAM 2 作者仓库 | P1 | 图像/视频分割、提示类型、状态与配置配对 | docs_read |
| S24 | Chronos-2 作者仓库与模型卡 | P1 | 时序预测、协变量、历史与预测步数 | docs_read |
| S25 | OpenVLA 作者仓库与模型卡 | P1 | 机器人动作、反归一化、具身限制与许可冲突 | docs_read |
| S26 | Biohub ESMC 作者仓库与模型卡 | P1 | 蛋白质表示、任务头与运行条件 | docs_read |

S23–S26 的详细证据见[专业模型补充](specialized-models.md)。
这四组仅查阅作者文档，没有运行模型或枚举完整模型家族。

## 官方模型与开放模型社区

### S01 · OpenAI

已读 [GPT-5.4 模型页](https://developers.openai.com/api/docs/models/gpt-5.4)和 [List models](https://developers.openai.com/api/reference/resources/models/methods/list)。
模型页提供快照、上下文、最大输出、知识截止、输入输出模态、端点、工具与价格条件；Models API 返回 `id`、`created`、`owned_by` 等基础信息，当前文档还包含可空的 `shutdown_date`。
列表 API 不能替代完整模型页；文档示例值不能直接导入为生产记录。
API 示例使用 Bearer key，本轮未调用，分页和目录额度未验证。
建议从官方页面保存事实和引用，另记录别名指向的快照；再分发条款未确认。

### S02 · Google Gemini

已读 [Models API](https://ai.google.dev/api/models)，可取得 `name`、`baseModelId`、`version`、`inputTokenLimit`、`outputTokenLimit`、`supportedGenerationMethods`、thinking 与采样参数范围。
列表使用 `pageSize`、`pageToken`、`nextPageToken`，默认每页 50，最多 1000；翻页时其他查询参数必须保持一致。
文档示例使用 API key；本轮没有调用带凭据 API，也没有验证账号、区域和配额差异。
列表描述未提供“每种角色允许哪些内容”的完整规则，需要继续读取对应接口和模型说明。
页面标明正文通常适用 CC BY 4.0、代码示例 Apache 2.0；这不能自动证明 API 返回数据享有相同再分发许可。

### S03 · Anthropic

已读 [List Models](https://platform.claude.com/docs/en/api/models/list)。
当前文档列出 `capabilities`、`max_input_tokens`、`max_tokens`、`line`、`lifecycle`、`deprecated_at`、`retires_at`，覆盖图片、PDF、thinking、structured outputs 等能力。
`line` 可空，文档明确不应从 ID 猜测；`retires_at` 是计划时间，实际退役状态以 `lifecycle` 为准。
默认列表包含 active 和 deprecated，查询历史记录需显式包含 retired。
分页为 `after_id` / `before_id`，默认 20，上限 1000，并返回 `has_more`、首尾 ID。
文档示例使用 API key 和版本头；本轮未调用 API。
Anysearch 首次提取被导向区域限制页，随后通过另一文档读取工具取得正文；这不表示业务 API 在当前环境可用。
目录额度及再分发条件待确认。

### S04 · Hugging Face

已读 [Hub API](https://huggingface.co/docs/hub/en/api)、[Model Cards](https://huggingface.co/docs/hub/en/model-cards)和[限流说明](https://huggingface.co/docs/hub/en/rate-limits)。
模型卡为 `README.md` 加 YAML 元数据，支持任务、语言、库、许可证、数据集、`base_model`、`base_model_relation`、`new_version` 等；base model 可以有多个。
Hub 会根据配置推导部分任务和标签，因此作者填写、平台推导与本项目推导应保留区别。
实际匿名请求了模型列表和 `Qwen/Qwen3-8B` 详情：列表返回 SHA、任务、库、时间、文件名等，详情额外返回 `cardData`、部分 `config` 与 safetensors 参数统计。
列表实际提供 `Link: ... rel="next"` 游标，不能用单页 `limit` 当作完整集合。
实际限流头为匿名 API 配额 `500 / 300s`；官方说明匿名额度可能随平台健康变化，应以响应头为准。
当前官方 API 索引指向 [OpenAPI](https://huggingface.co/.well-known/openapi.json)，但本次下载的规范未列出所用的两个 GET 列表/详情路径，说明规范也可能不完整。
配置文件、模型卡应固定到仓库 revision，读取 JSON/YAML 即可，不需要加载模型或执行远端仓库代码。
模型仓库声明的 license 不自动覆盖整个 Hub 目录、所有 README 文本或本项目的大规模再分发。

### S05 · ModelScope

实际匿名请求 [Qwen3-8B 元数据](https://modelscope.cn/api/v1/models/Qwen/Qwen3-8B)，HTTP 与业务 `Code` 都为 200。
响应包含 `Tasks`、`Architectures`、`BaseModel`、`BaseModelRelation`、`License`、`ReadMeContent`、revision、更新时间、推理支持标记等。
同一个模型可用于与 HF 交叉核对 ID、谱系和许可证，但两站最后更新时间是仓库事件，不应自动当作模型发布日期。
详情响应混合模型内容和大量平台管理字段，未来导入应明确选择字段，避免无意义地搬运整个响应。
已读[官方 SDK 当前源码](https://github.com/modelscope/modelscope/blob/master/modelscope/hub/api.py)，当前 `HubApi` 已委托到 `modelscope_hub`；旧教程的内部接口不能视为稳定契约。
官方文档首页提取失败，全局列表、分页、token 要求、限流和批量再分发许可仍为 `unverified`。
本次仅验证了公开单模型详情，不代表可以无登录枚举全部仓库。

### S21 · DeepSeek

已读 [Lists Models](https://api-docs.deepseek.com/api/list-models/)。
当前文档除了 ID 和 owner，还提供名称、总上下文、最大输出、输入输出模态、effort，以及 `api_capabilities`。
其中 `anthropic_messages.system_prompt_update` 明确区分 `leading-only` 与 `in-history`，后者表示历史中最新的 system 消息替换此前 system prompt。
这是一类可直接采集的协议行为声明，也说明模型级 `supports_system_prompt` 布尔值表达力不足。
本轮只读文档，未用示例模型值生成记录，未调用 API；分页、鉴权细则、目录限流及再分发仍待核实。

### S22 · 阿里云百炼 / Model Studio

已读[官方模型总览](https://www.alibabacloud.com/help/en/model-studio/models)，覆盖文本、图像/视频理解与生成、世界模型、TTS、ASR、实时语音、embedding、rerank 和决策模型。
该页是推荐入口，完整目录转到 Model Plaza；不能把推荐表格当作完整可枚举模型集合。
Model Studio 同时提供 Qwen 与其他厂商模型，作者和托管平台要分别记录。
页面链接到不同区域的控制台，后续需核对地域、模型 ID 和版本别名。
找到了官方 List models 文档入口，但正文提取失败，因此本轮不确认其 API 契约、分页和访问条件。
公开文档适合提供类别覆盖和官方能力补充，批量复制与再分发条件尚未确认。

## 聚合目录与商业数据来源

### S06 · OpenRouter Models

已读 [Models API](https://openrouter.ai/docs/api/api-reference/models/list-all-models-and-their-properties)，并实际请求 `output_modalities=all&limit=2`。
返回 `id`、`canonical_slug`、`hugging_face_id`、输入输出模态、上下文、价格、`supported_parameters`、`top_provider`、知识截止和到期时间等。
文档默认 `output_modalities=text`，支持 all；分页使用 offset/limit，单页最多 1000，本次返回 `total_count` 和 `links.next`。
本次匿名访问成功，而文档授权栏要求 Bearer key，采集实现应把二者记为不同证据，不保证匿名方式长期可用。
`top_provider` 描述服务端选择的提供方属性；不能直接当作底层模型唯一的能力边界。
价格的单位需要逐字段保留，不能混淆每 token、每次请求、图片、秒或每百万 token 的筛选条件。
模型目录的批量再分发许可未确认；不能把 OpenRouter 其他数据集的许可推广到 models API。

### S07 · OpenRouter Providers / Endpoints

已读 [Providers](https://openrouter.ai/docs/api/api-reference/providers/list-providers)和[官方 SDK Endpoints 文档](https://openrouter.ai/docs/client-sdks/python/sdks/endpoints/README.md)，两类 API 均做了匿名采样。
Providers 返回名称、slug、隐私政策、服务条款、状态页、总部和数据中心信息。
单模型 Endpoints 实际返回提供方名称、quantization、context、最大输入/输出、参数、tool choice、native tools、缓存与观测性能等。
本次 GPT-5.4 详情含 7 个端点，这是特定时刻的服务目录，不代表 7 个独立基础模型。
平台数据中心列表也不能证明每个模型、每次请求均部署在所有列出的地点。
首轮模型元数据可只保留关联来源，完整的端点状态、价格和性能历史留给平台阶段。
文档示例带 key，匿名成功不等于访问契约；专用限流、再分发与数据保留条件待核实。

### S08 · Artificial Analysis 模型与媒体

已读用户提供的[原 API Reference](https://artificialanalysis.ai/api-reference)、[Data API docs](https://artificialanalysis.ai/data-api/docs)，并实际下载[当前 OpenAPI](https://artificialanalysis.ai/api/v2/openapi)。
旧页描述 `/api/v2/data/llms/models`、免费 1000 次/天；新概览描述 Free 100 次/24 小时、Pro 500，实际 OpenAPI 进一步区分 `/language/models/free` 与 Pro+ 的 `/language/models`。
本轮没有带 key 调用，因此没有验证旧路径是否仍可用，也不能把不同文档直接解释为一次已完成的迁移。
OpenAPI 规定免费模型响应排除上下文、参数量、模态、许可等 Pro 字段；免费服务不能满足本项目全部基础模型字段需求。
当前规范覆盖图像生成/编辑、视频及视频音频、TTS、语音转换、ASR、器乐和带人声音乐；对跨模态模型发现有价值。
列表含分页信息，样例每页 200，实际以当前契约与响应为准；鉴权为 `x-api-key`。
官方要求所有层级署名，且明确要求就再分发权限联系团队；本项目不应在权利未确认时镜像整个数据集。
模型与 creator 的稳定 ID 可作为跨源标识，排行榜分数应保留方法版本和测量条件，不宜混入未经限定的模型能力声明。

### S09 · Artificial Analysis Hosts / Providers

用户给出的 `getLLMHosts` 锚点没有在本次旧页正文中提供相应契约。
当前 [OpenAPI](https://artificialanalysis.ai/api/v2/openapi)包含 `/api/v2/language/providers`、单 provider、性能历史和原始测量接口；provider 列表明确要求 Commercial。
新[订阅说明](https://artificialanalysis.ai/data-api/docs)同样把 provider 层列为 Commercial。
这部分可以作为后续平台来源候选，本轮没有验证付费访问、历史范围或任何商业协议。
先保留来源线索和合作依赖，避免让首版模型注册表依赖未经确认的商业输入。

## 多模态、检索与音频专项来源

### S10 · Replicate

已读 [HTTP API](https://replicate.com/docs/reference/http#models.get)。
模型列表和单模型对象提供 owner/name、说明、GitHub/paper/license URL、最新版本；版本包含 `openapi_schema`，可读取输入输出参数与限制。
文档要求 Bearer token，列表分页，并支持按模型创建时间或最新版本时间排序。
本轮读取范围未完整覆盖所有历史版本接口，因此只确认最新版本 schema 的文档契约。
搜索 API 中的 `generated_description` 是 AI 生成字段，应与模型作者说明区分。
平台模型版本未必等于作者的基础模型版本；模型许可证链接也不等于平台目录再分发许可。
API 调用、额度、目录完整度及再分发条件待核实。

### S11 · fal

已读 [Model search API](https://fal.ai/docs/platform-apis/v1/models)与[分类目录](https://fal.ai/docs/model-api-reference)。
覆盖图像、视频、音频、视觉理解和 3D；目录包含 `endpoint_id`、名称、类别、描述、状态、标签、日期和模型链接。
`expand=openapi-3.0` 可返回输入输出 schema，适合发现分辨率、时长、格式、参考输入等参数约束。
使用 `Authorization: Key …`，分页为 cursor/next_cursor/has_more，按类别、状态、ID 筛选；ID 查询一次支持 1–50 个，limit 上限随查询及展开内容变化。
翻页需保持查询条件一致，不应把 endpoint ID 自动当作独立模型 ID。
本轮没有调用 API；目录额度、许可与批量再分发待核实。

### S12 · Cohere

已读 [List Models](https://docs.cohere.com/reference/list-models)。
`GET /v1/models` 提供名称、deprecated、兼容端点、finetuned、context_length、tokenizer URL、features 和 sampling defaults，端点类别包含 embed/rerank/classify。
使用 Bearer key；page_size 默认 20、范围 1–1000，page_token/next_page_token 分页。
文档说明 special tokens 或 preamble 会占用上下文，因此名义长度不总等于用户文本可用长度。
适合作为非聊天模型的第一批结构化样本；请求批次、向量维度和重排约束还应读取对应 API。
本轮未调用，目录额度和再分发待核实。

### S13 · Voyage AI

已读[当前模型总览](https://www.mongodb.com/docs/voyageai/models/)和 [Text Embeddings](https://docs.voyageai.com/docs/embeddings)。
覆盖文本、多模态、上下文分块 embedding 与 rerank，文档给出 ID、上下文、维度、旧版本和任务描述。
请求规则包括 input_type、truncation、output_dimension、output_dtype、条数与总 token 限制；文本 embedding 文档规定一次最多 1000 条，合计 token 上限按模型变化。
`truncation=true` 截断超限输入，false 则报错，是需要记录的接口行为特例。
本轮只读公开文档，未确认统一模型枚举 API；不要把页面中旧模型与推荐模型混成同一可用状态。
鉴权、列表接口、抓取额度和再分发待核实。

### S14 · Jina AI

已读 [Embedding API 官方页面](https://jina.ai/embeddings/)。
信息包括文本/图片/音频/视频/PDF 输入、模型尺寸、向量维度、上下文、任务 adapter、late chunking，以及 dense 或 token 级输出。
官方 FAQ 区分模型架构支持的上下文与托管 API 接受的上下文，建议分别记录来源和适用对象。
公开页面可供提取，调用示例使用 Bearer key；本轮没有确认模型枚举 API，也没有运行推理。
目录与文档的抓取额度、模型说明复用和再分发条件待核实。

### S17 · ElevenLabs

已读 [List models](https://elevenlabs.io/docs/api-reference/models/list)。
`GET /v1/models` 返回 ID、语言、TTS/voice conversion/微调/style/speaker boost 能力、alpha 准入、单次文本上限、费用倍率和并发组。
文档把旧的按订阅划分字符限制字段标为 deprecated，应优先使用 `maximum_text_length_per_request`。
该端点页没有列分页或明确鉴权要求，示例不带鉴权；本轮未调用，不能据此承诺匿名或完整目录可用。
音频产品可能有不同模型列表和能力边界，需结合对应 API 验证。
再分发许可未确认。

### S18 · Runway

已读 [Available AI Models](https://docs.dev.runwayml.com/guides/models)与[API 使用说明](https://docs.dev.runwayml.com/guides/using-the-api)。
目录列出 API ID、显示名、Video/Image/Audio/Real-time/Upscale & HDR 类别和能力链接，适合继续提取时长、分辨率、参考素材与输出音频约束。
生成 API 使用 Bearer key 与 `X-Runway-Version`，本轮没有找到并确认公共 catalog REST API。
页面含其他厂商模型，应区分原作者、Runway 提供的端点和上游模型版本。
当前证据来自公开 HTML，列表完整度、抓取额度和再分发待核实。

### S19 · Stability AI

已读 [API Reference](https://platform.stability.ai/docs/api-reference)，涉及图像生成/编辑/放大、Stable Audio、3D 与旧 v1 engines。
参数级事实包括 model 枚举、输出格式、尺寸、范围、计费单位和条件必填；例如 Ultra 接口提供 `image` 时要求 `strength`。
文档说明 v2beta 为主要服务方向，v1 不再增加新功能，因此旧 engines 列表不能当作现代产品全集。
实际 API 使用 key；文档列出的 150 次/10 秒及 429 后暂停规则属于该 API 说明，不能推定为文档抓取额度。
本轮未调用，也未确认统一现代模型目录 API；再分发待核实。

## 托管与部署目录

### S15 · Amazon Bedrock

已读 [ListFoundationModels](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_ListFoundationModels.html)。
字段包含 model ID/name/ARN、provider、输入输出模态、streaming、customization/inference types 和生命周期。
所读接口未列分页参数，支持按 provider、输出模态、定制方式和推理方式过滤，并描述 403/429。
输出模态筛选枚举仅列 TEXT/IMAGE/EMBEDDING，不能从一个接口推断全部 Bedrock 产品覆盖。
本轮未核实账号权限、区域差异、真实响应和授权签名配置。
适合后续托管关系交叉验证；目录再分发条件未确认。

### S16 · Google Model Garden

已读 [publishers.models.list](https://docs.cloud.google.com/gemini-enterprise-agent-platform/reference/rest/v1beta1/publishers.models/list)与 [PublisherModel](https://docs.cloud.google.com/gemini-enterprise-agent-platform/reference/rest/v1beta1/publishers.models)。
当前文档路径来自旧 Vertex 路径重定向；资源含 name、versionId、openSourceCategory、parent、frameworks、launchStage、versionState、predictSchemata 与部署动作。
列表有 pageSize/pageToken/nextPageToken，以及 listAllVersions、view、filter 等。
本轮未验证账号、鉴权、区域和实际可见集合；`v1beta1` 契约及文档路径应作为来源版本的一部分保存。
页面通常标记 CC BY 4.0 正文许可，但 API 数据再分发仍需单独确认。

### S20 · NVIDIA NIM

已读 [LLM 支持矩阵](https://docs.nvidia.com/nim/large-language-models/latest/reference/support-matrix.html)与[实例 API](https://docs.nvidia.com/nim/large-language-models/latest/reference/api-reference.html)。
矩阵包含 LLM/VLM、GPU、tensor parallelism、精度、LoRA、profile ID 和优化目标。
实例的 `/v1/models`、`/v1/metadata`、`/v1/manifest`、`/v1/license` 可描述当前模型与 profile，它们不是全站模型目录。
`NIM_SERVED_MODEL_NAME` 可改变服务名称，不能据服务名称反推唯一基础模型。
后续保存矩阵时需要固定版本，不能只引用会更新的 latest。
本轮没有访问实例或确认托管目录认证；部署支持、模型许可和目录再分发各自待核实。

## 按任务检查覆盖与缺口

| 任务或对象 | 首选来源组合 | 仍需补充的信息 |
| --- | --- | --- |
| 文本、推理、代码模型 | 官方模型页 + HF/ModelScope + OpenRouter | 别名漂移、协议差异、不同 reasoning 配置 |
| 图像/视频理解、OCR、文档理解 | 官方说明 + HF 任务 + fal schema | MIME、页数、帧采样、分辨率、角色允许内容 |
| 图像生成、编辑、放大 | fal + Replicate + Stability + 官方模型卡 | mask、参考图、条件必填、尺寸组合、模型与工作流区别 |
| 视频生成、编辑、实时视频 | fal + Runway + Model Studio + 作者说明 | 时长、帧率、首尾帧、音轨、不同端点的输入组合 |
| ASR、TTS、语音转换、音乐 | ElevenLabs + fal + Model Studio + AA 发现 | 语言、说话人、时长、字符单位、采样率、流式能力 |
| embedding、rerank、分类 | Cohere + Voyage + Jina + HF | 输出维度/类型、归一化、query/document 区别、截断、批次 |
| 3D、mesh、重建、点云 | fal + Replicate + Stability + 作者仓库 | 输出格式、坐标/单位、纹理、拓扑和多视角要求 |
| 检测、分割、深度、姿态等视觉模型 | HF + ModelScope + SAM 2 作者仓库（S23） | 已读分割样本；检测、深度与姿态尚待专项样本，仍需类别表、预处理、输出形状 |
| 世界模型、机器人、动作策略 | OpenVLA 作者资料（S25）+ HF/ModelScope；Model Studio 提供部分发现入口 | 已读动作与具身限制；世界模型、更多机器人动作空间和传感器待扩展 |
| 时序预测 | Chronos-2 作者仓库与模型卡（S24）+ HF | 已读历史步数、预测步数与协变量；不规则时间、其他模型家族待扩展 |
| 科学/生物模型 | Biohub ESMC 作者仓库与模型卡（S26）+ HF | 已读蛋白质表示样本；结构、分子与其他科学任务待扩展 |
| 表格、图模型 | HF/ModelScope 发现与作者文献 | 本轮未建立专项样本，缺统一模型目录 |
| 量化、LoRA、merge、转换文件 | HF 谱系 + ModelScope + 作者仓库 | 变体与原模型的关系、转换工具、精度和配置差异 |

“所有模型”若包括每个训练 checkpoint、微调、量化与个人实验仓库，集合会持续膨胀，完整性无法只靠几个 API 保证。
建议将“可持续发现”和“精选且经过审查的记录”设成不同覆盖指标，避免为了扩大数量把低置信度记录自动升为正式数据。
这只是本轮范围建议，具体收录规则仍需项目决定。

## 来源冲突和时效的处理建议

- 同名、同 slug、同发布者字符串不足以认定同一模型；优先保存上游稳定 ID、官方仓库与明确的版本映射。
- 研发组织声明用于模型本体，平台 API 用于它实际提供的接口；第三方测量用于其测量条件下的观察。
- “未返回字段”“返回 null”“明确不支持”要分别记录；列表默认过滤造成的缺席也不等于退役。
- 价格、可用区域、模型别名、端点状态变化更快，应与发布信息和模型架构信息分开设定复核频率。
- schema 或 API 文档只说明允许表达的请求结构，不证明每个模型都支持每个组合；模型特例仍需要模型页或实测证据。
- 对同一官方来源的冲突，也保留每个页面/规范的 URL、抓取时间和响应证据；AA 本轮差异是实际案例。
- 对模型卡和配置，记录仓库 SHA；对 docs/latest、可变网页和商业目录，记录观察时间及内容摘要哈希。

## 优先验证顺序

1. 用 OpenAI/Gemini/Anthropic/DeepSeek 各少量模型，比较官方声明和聚合目录是否能对齐。
2. 用同一组开放模型在 HF 与 ModelScope 的记录验证身份、谱系、任务、license 和 revision 的差异。
3. 用 fal/Replicate 的图像、视频、3D 模型与 Cohere/Voyage 的检索模型验证参数、单位与条件能否完整表达。
4. 用 ElevenLabs/Jina/Stability 的少量样本验证字符长度、架构与部署差异、条件必填等规则。
5. 确认 AA 接口套餐与再分发条件后，再决定它是否成为自动输入。
6. 在平台阶段验证 OpenRouter、Bedrock、Model Garden、NIM 的实际部署范围、地区和可用性。

本轮未验证付费账号、推理行为、全量分页或大规模再分发许可。
文档和公开元数据样本足以支持下一轮选型讨论，尚不足以承诺完整覆盖或直接开放发布所有上游数据。

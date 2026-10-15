# 模型能力、条件限制与证据语义

[返回本轮调研](../README.md) · [来源目录](README.md) · [元数据架构](../architecture/metadata.md) · [采集流程](../architecture/pipeline.md)

核查日期：2026-10-15。
研究问题：如何覆盖各类模型的关键元信息，并保留角色、模态、参数组合、平台和版本带来的特例。
本报告是一轮设计输入，所有 YAML 都是讨论示意，字段尚未定案。
文档事实均为 `reported`：已核查官方正文，未进行实际推理调用。

## 1. 对范围的建议

“覆盖所有模型”适合作为长期收录方向，第一阶段应先证明同一套记录方式能容纳不同任务。
模型数量没有可穷尽的固定边界，模型副本、量化版本、微调版本、服务别名和滚动更新也不能直接相加为有意义的覆盖率。
建议按任务、来源、服务接口和限制类型检查覆盖面，再逐步扩大条目数量。
以下分类是研究用的检查表，允许一个模型属于多个任务。

| 任务 | 应核查的元信息 | 容易误判的地方 |
|---|---|---|
| 文本生成、对话、代码 | 输入与输出 token、角色、工具、结构化输出、推理模式、停用参数 | 支持工具和支持 JSON，不代表二者可同时使用 |
| 图像理解、OCR、文档理解 | 图片 MIME、大小、数量、像素、文档页数、角色位置、分辨率处理 | 图片理解与图片生成是不同能力；OCR 可能属于独立接口 |
| 图像生成、编辑 | 输出尺寸、宽高比、参考图数量、编辑 mask、输出格式 | 同一产品品牌可能调用不同模型；参考图限制不等于理解接口限制 |
| 视频理解 | 总时长、帧率、分辨率、音轨、视频数量、URL 类型 | “支持 3 小时”可能只在低分辨率、特定上下文预算下成立 |
| 视频生成、编辑 | 时长档位、分辨率、帧率、首尾帧、参考图、音轨输出、异步任务 | 生成视频的任务执行方式与对话 streaming 不同 |
| 语音识别、音频理解 | 编解码、采样率、声道、时长、文件大小、语言、时间戳、说话人区分 | 离线转写、实时转写和音频问答应分别记录 |
| 语音合成、音乐、语音对话 | 音频输出格式、声音选择、参考音频、双向传输、会话时间 | 分块返回、实时输入和实时对话是三个不同特征 |
| Embedding | 输入任务、输入模态、维度集合、数值类型、归一化、批量、截断策略 | “最大输入数”与“最大图片数”不是同一计数对象 |
| Rerank | query、document、候选数、返回数、上下文、分块与截断 | `top_n` 不代表最多能输入多少篇文档 |
| 分类、检测、分割、深度、3D、机器人等 | 输入输出结构、标签或坐标语义、几何单位、时序约束 | 应先登记任务与来源，未研究的任务专属字段保留未知 |

这张表是建议的调查范围，不表示本轮已经核验每类任务中的全部限制。
本轮实证主要覆盖生成式对话、图片与视频理解、语音、embedding 和 rerank。

## 2. Gemini 的 system prompt 图片说法

### 2.1 当前能确认的结论

目前没有找到足够的一手证据，证明“某些 Gemini 型号允许在 system prompt 中放图片，而另一些不允许”这一具体差异。
已核查的两个 `generateContent` 接口均把系统指令限定为文本。
该说法应保留为待核查问题，不能据此创建一组型号能力记录。

| 入口与范围 | 官方证据 | 可记录结论 |
|---|---|---|
| Gemini Developer API，`v1beta/models/*:generateContent`，字段 `systemInstruction` | API reference 明确写 “Currently, text only.”。[字段定义](https://ai.google.dev/api/generate-content#request-body) | 在该接口和字段内，image part 为文档声明的不支持；没有列出型号例外 |
| 原 Vertex AI 文档入口对应的 Google Cloud Gemini API，`v1/{model}:generateContent` | REST reference 规定 `systemInstruction.parts` 仅用文本，且列出 publisher model 与 tuned endpoint 两种路径。[v1 接口定义](https://docs.cloud.google.com/gemini-enterprise-agent-platform/reference/rest/v1/projects.locations.endpoints/generateContent) | 同样是接口字段限制；不能扩张为所有 Google 产品的所有 system 消息 |
| Google Cloud Gemini Live，`BidiGenerateContentSetup.systemInstruction` | Live reference 的该字段也要求 parts 仅文本。[Live 字段定义](https://docs.cloud.google.com/gemini-enterprise-agent-platform/reference/models/multimodal-live) | 这是 Live setup 的限制，应与普通生成请求分开 |

上述 Cloud 旧 URL 本轮访问时重定向到 `gemini-enterprise-agent-platform` 文档路径。
采集时应同时保存请求 URL、最终 URL、页内章节和访问日期，避免重定向后误以为仍在读取原接口文档。
本轮成功读取了新的 v1 接口页；尝试旧版 publisher 路径的 v1/v1beta1 文档时重定向到了总索引，未把索引页当作版本规则的证据。

### 2.2 需要拆开的四个层面

1. **位置**：`systemInstruction` 和普通 `contents` 的图像 part 是不同位置。
2. **模型**：多模态输入能力不能推出每种角色和每个字段都接受图像。
3. **API**：Developer API、Cloud API、Live API、Interactions API 和兼容接口分别核查。
4. **SDK**：客户端可构造某种对象，不足以证明服务器接受并执行其语义。

Google 官方 Python SDK 说明默认使用 beta 接口，并允许通过 `http_options.api_version` 选择版本。
该 SDK 还会把简写 `contents` 合并为 user 或 model 内容，所以仅保存 Python 调用示例不足以确定最终角色和请求内容。[官方 SDK 的 API 选择与 contents 转换说明](https://github.com/googleapis/python-genai#api-selection)
本轮阅读的是 SDK 当前 README，未固定提交或逐版检查类型定义，因此此处仅用于识别核查维度。

后续若找到允许 system 图片的正面证据，至少需要确切模型 ID、平台、endpoint、API 版本、请求字段路径与原始来源。
若只有某 SDK 对象能成功创建，应记录为客户端行为；若有真实响应，再另记服务器观察结果。
图片 URL 作为普通文本出现，也不能作为 image part 被系统指令接受的证据。

## 3. 已找到的真实条件限制

以下条目来自五个组织的官方文档：Google、Anthropic、Cohere、AWS、Mistral。
表中的“低／中／高”表示持续采集与解释难度，不表示文档事实已经经过运行验证。
本轮没有核查列出型号在各地区的可用性，文档中出现旧型号也不代表现在仍可新建调用。

### 3.1 角色、工具、参数与 token 预算

| 编号 | 范围 | 文档声明与应保留的条件 | 采集难度与证据 |
|---|---|---|---|
| C01 | Gemini Developer API `v1beta`，`generateContent.systemInstruction` | 仅文本；不能从模型支持 image 输入推出 system 图片支持 | 低；字段说明清楚。[API reference](https://ai.google.dev/api/generate-content#request-body) |
| C02 | AWS Bedrock Converse 消息结构 | 图片与文档只允许在 `role=user` 的消息中；含 document 时还要包含 text block | 低；属于托管接口规则，不能直接写成所有底层模型的本体能力。[Message](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Message.html) |
| C03 | Claude `/v1/messages` 顶层 `system` 参数 | 类型为字符串或 `TextBlockParam[]`，不含 image block；该结论只针对顶层字段 | 中；同页的 system role 叙述存在演进问题，见第 4 节。[Messages API](https://platform.claude.com/docs/en/api/messages/create) |
| C04 | Claude 支持 manual thinking 的型号，`thinking.type=enabled` | `budget_tokens` 至少 1024，通常须小于 `max_tokens`；interleaved thinking 有例外，预算可跨多个 thinking block 超过 `max_tokens` | 中；需要模式、型号、interleaved 条件；该预算还不能用于 `max_tokens=0` 的缓存预热。[预算规则](https://platform.claude.com/docs/en/build-with-claude/extended-thinking#budget-rules-and-tuning) |
| C05 | Claude manual thinking 与工具选择 | manual 模式只支持 `tool_choice.type=auto/none`；`any/tool` 强制工具调用会报错；adaptive 模式和新型号另有规则 | 中；不能归纳成所有 thinking 模型都拒绝 forced tools。[Thinking with tool use](https://platform.claude.com/docs/en/build-with-claude/thinking#thinking-with-tool-use) |
| C06 | Claude 支持 structured outputs 的型号及 Messages 请求 | JSON 输出可与 `strict:true` 的工具结合；JSON 输出与 citations 不兼容并返回 400；直接输出格式与工具参数格式是两项约束 | 中；需保存功能组合与失败方式。[Feature compatibility](https://platform.claude.com/docs/en/build-with-claude/structured-outputs#feature-compatibility) |
| C07 | Gemini 2.5 Pro / 2.5 Flash，Generate Content thinking 配置 | Pro 不允许关闭 thinking；Flash 可用 `thinkingBudget=0` 关闭；2.5 系列使用 `thinkingBudget` 而非 `thinkingLevel` | 中；文档同时包含其他世代规则，应按明确型号拆分，不能从版本数字自动继承。[Thinking budgets](https://ai.google.dev/gemini-api/docs/generate-content/thinking#thinking-budgets) |

`max_output_tokens` 是否含思考 token、上下文是否共享输入与输出、默认值是否等于硬上限，都应有独立定义。
“参数被接受”“参数被忽略”“参数影响输出”“参数被拒绝”也应能分别记录。
例如 Claude 文档说明部分不支持 interleaved thinking 的型号会接受相关 beta header 并忽略它，因此 HTTP 成功不能单独证明能力存在。[Interleaved thinking](https://platform.claude.com/docs/en/build-with-claude/extended-thinking#interleaved-thinking)

### 3.2 图片、视频、语音与传输方式

| 编号 | 范围 | 文档声明与应保留的条件 | 采集难度与证据 |
|---|---|---|---|
| C08 | AWS Bedrock Converse，消息 content | 最多 20 张图片，单图不超过 3.75 MB，宽高各不超过 8000 px；最多 5 个文档，单文档不超过 4.5 MB | 低；数量、文件大小、像素和计数对象都要保留，底层模型仍可能有更小限制。[Message](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Message.html) |
| C09 | Gemini Developer API Live，音频格式 | 音频数据为 little-endian、16-bit PCM；输出固定 24 kHz；输入原生 16 kHz，但服务允许重采样其他采样率 | 中；应记录原生值与可接受值的区别。[Audio formats](https://ai.google.dev/gemini-api/docs/live-api/capabilities#audio-formats) |
| C10 | Gemini Developer API Live，会话 | native audio 模型响应模态为 AUDIO，文本结果通过音频转写获得；音频会话 15 分钟、音频加视频 2 分钟，存在会话管理延长方式 | 中；这是无延长机制时的会话约束，不能写成模型只能理解 2 分钟视频。[Limitations](https://ai.google.dev/gemini-api/docs/live-api/capabilities#limitations) |
| C11 | Gemini Developer API，视频理解文档中的静态处理 | 1M 上下文的视频处理时长受媒体分辨率影响：低分辨率约可达 3 小时，高分辨率约 1 小时；默认静态抽帧为 1 FPS | 中高；时长是条件容量，其他提示也占用上下文；不要写成无条件最大视频时长。[Technical details](https://ai.google.dev/gemini-api/docs/video-understanding#technical-details-about-videos) |
| C12 | Google Cloud Gemini `generateContent`，HTTP `fileUri` | HTTP 文件须可公开读取；每请求最多 1 个视频、1 个音频、10 张图片；音频、视频与文档文件不超过 15 MB；VPC Service Controls 启用时不支持该媒体 URL 方式 | 中；限制与传入方式及平台配置绑定，不能套用到所有内联上传或 Cloud Storage 文件。[FileData](https://docs.cloud.google.com/gemini-enterprise-agent-platform/reference/models/inference#filedata) |
| C13 | Mistral Voxtral Realtime 转写 | 实时转写与 `diarize` 参数不兼容 | 低；需与离线转写分开，不要把流式返回离线任务结果当作实时录音输入。[Audio overview](https://docs.mistral.ai/studio/audio/overview) |

### 3.3 Embedding 与 rerank

| 编号 | 范围 | 文档声明与应保留的条件 | 采集难度与证据 |
|---|---|---|---|
| C14 | Cohere `embed-v4.0`，Embed API v2 | 输出维度集合为 256、512、1024、1536，默认 1536；支持文本、图片和文本图片混合输入 | 低；维度是离散集合，不能写成 256–1536 任意整数。[Embed 模型表](https://docs.cohere.com/docs/cohere-embed) |
| C15 | Cohere Embed API v2，`inputs` 与 `images` | 每请求最多 96 个 input；一个 input 可包含多个内容组件；v3.x 的 `images` 每请求最多 1 图、5 MB，v4.0 及更新模型该字段的所有图片总大小最多 20 MB | 中；input、组件、图片、请求总大小分别计数；文档未在这里明确 base64 膨胀是否计入大小。[Embed API](https://docs.cohere.com/reference/embed) |
| C16 | Cohere Embed API v2，长输入 | `truncate` 默认 `END`；`START` 丢弃开头，`END` 丢弃末尾，`NONE` 超限返回错误 | 低；上下文长度需配合超限行为，否则调用者会误以为完整输入被处理。[Embed API](https://docs.cohere.com/reference/embed) |
| C17 | Cohere Rerank API v2 | `max_tokens_per_doc` 默认 4096，长文档按该值截断；`top_n` 控制返回结果数，省略时返回全部排序结果；最多 1000 篇是性能建议而非文档明确的硬上限 | 低至中；需区分默认、硬限制和建议，也需与模型级 context 分开。[Rerank API](https://docs.cohere.com/v2/reference/rerank) |

本轮未核验 embedding 的各类数值编码、归一化行为，也未核验上述服务全部 MIME 类型组合。
这些字段应继续研究，不能用空数组表示“不支持”。

## 4. 官方材料也会冲突

以下是需要人工复核的具体问题，不能用“来源来自官网”自动消除矛盾。

| 问题 | 已读取的证据 | 处理建议 |
|---|---|---|
| Gemini structured output 与工具的型号范围 | [Structured outputs](https://ai.google.dev/gemini-api/docs/structured-output#structured-outputs-with-tools) 正文说 preview 组合仅适用于 Gemini 3 系列，同节当前示例却使用 `gemini-4-argon`，调用 `/v1beta/interactions` | 保存正文限制与示例分别作为声明；对于 Gemini 4 的具体组合保持 `unknown`，标注 `conflicted`，不要从示例直接认定运行支持 |
| Claude system role | [Messages API](https://platform.claude.com/docs/en/api/messages/create) 叙述仍有不支持 system role 的说明，但枚举与 [中途 system 消息](https://platform.claude.com/docs/en/build-with-claude/mid-conversation-system-messages) 专页已反映新能力 | 顶层 system 与消息 role 分开；专页对带 `clear_at: next_user_message` 的 system 消息明确仅文本；其他内容类型继续核查 |
| Mistral 离线转写时长 | [Known limitations](https://docs.mistral.ai/resources/known-limitations#audio-transcription) 通用页写 60 分钟，[Audio overview](https://docs.mistral.ai/studio/audio/overview) 对 Voxtral Mini Transcribe 2 写每请求 3 小时 | 型号范围可能解释差异，但在确认前不选一个数覆盖另一个；记录通用页与具体型号声明的范围 |
| Cohere Rerank query 长度与分块 | [最佳实践](https://docs.cohere.com/docs/reranking-best-practices#queries) 顶部通用表写 query 2048 token，后文给 v4 query 16384、context 32768；[v2 API](https://docs.cohere.com/v2/reference/rerank) 另有默认 4096 token/document 截断 | 保留各自声明；分块与端点截断的先后关系、`max_chunks_per_doc` 在 v2 的适用性仍待核验 |

模型卡、API 类型、教程与代码示例各自回答的问题不同。
来源优先级需要结合具体字段的作用域：接口字段限制通常以该接口定义为先，模型容量则需型号文档。
若两个来源针对同一作用域直接冲突，应保留冲突，交由审查处理。
较新的网页抓取时间只证明“何时看到”，不能证明该条规则何时生效。

## 5. 最小可维护的条件声明

### 5.1 四种状态与证据类别

| 状态 | 建议语义 |
|---|---|
| `supported` | 在明确作用域内，有支持证据 |
| `unsupported` | 在明确作用域内，有明确拒绝、类型排除或不支持说明 |
| `unknown` | 缺证据、作用域缺失、版本未核查，或冲突暂未裁定 |
| `conditional` | 已知支持取决于列出的条件；条件满足后的结果仍应能明确表达 |

`conditional` 描述已知条件，不能拿来代替“不知道”。
条件未提供或未被检查时，解析结果应保留未知。
`conflicted` 更适合作为审查状态附在声明集合上，不混进模型能力的真假判断。
没有记录与记录为 `unsupported` 必须不同；`null` 也不能未经定义就表示不支持。

证据类别至少分开 `reported` 与 `observed`。
`reported` 表示来源声称某行为，可以进一步注明 official reference、官方示例、模型卡或第三方目录。
本轮引用均来自官方文档，对应[架构讨论](../architecture/metadata.md)中的 `documented`；`reported` 是这里暂用的更宽泛名称，最终词汇待统一。
`observed` 表示一次可追溯的实际请求观察，需要请求形态、响应、时间、平台、模型 ID、API/SDK 版本、关键 header、地区或资格条件。
一次成功只能支持该次条件下的行为；一次失败还应排除鉴权、配额、地区与输入损坏等原因。
HTTP 200、字段被忽略、功能实际生效与输出质量达标也不应被混为同一观察。

### 5.2 每条声明最少回答六个问题

1. **针对谁**：模型 ID，或明确标注为接口层的规则。
2. **在哪条路径**：作者与托管方、平台、endpoint、API 版本、字段位置，必要时加 SDK 版本。
3. **什么条件**：角色、模态、参数值、功能组合、上传方式、地区、账户资格、beta header。
4. **什么结果**：支持、拒绝、忽略、截断、数值边界、默认值或建议，以及计量单位和计数对象。
5. **根据什么**：来源 URL、章节或字段定位、短摘录、抓取日期；实现来源采用固定提交。
6. **还缺什么**：未确认条件、来源冲突与下一次核查动作。

建议先用少量可读的条件项覆盖本报告中的真实案例。
数值规则至少区分离散集合、上下界、参数间关系、默认值与推荐值。
遇到表达不下的案例，再依据实际需求扩充；本轮不设计通用表达式语言或规则执行引擎。
稳定事实与服务变化宜关联存储：模型具备图片理解能力，某平台通过某接口限制图片上传数量，两者能同时成立。

## 6. YAML 讨论示意

下面四段只演示信息表达能力，不是最终 schema、可执行规则或已发布数据。
示例里的 `illustrative`、`scope`、`when` 等名称均可在后续设计中调整。
示例不包含虚构的成功请求，全部 `observed` 列表为空。

### 6.1 接口字段的明确限制

```yaml
illustrative: true
subject: system_instruction_image_part
scope:
  provider: google
  api: gemini-developer
  api_version: v1beta
  operation: models.generateContent
  field: systemInstruction.parts
  model_scope: endpoint_contract
status: unsupported
evidence:
  reported:
    - url: https://ai.google.dev/api/generate-content
      locator: Request body / systemInstruction
      excerpt: "Currently, text only."
      checked_on: '2026-10-15'
  observed: []
```

这里的 `endpoint_contract` 是范围说明，避免生成一条未经型号逐项核查的模型列表。
其含义也不延伸到 Interactions 或兼容接口。

### 6.2 离散维度与默认值

```yaml
illustrative: true
subject: embedding_output_dimension
scope:
  provider: cohere
  api_version: v2
  operation: embed
  model: embed-v4.0
status: conditional
rule:
  parameter: output_dimension
  allowed_values: [256, 512, 1024, 1536]
  default: 1536
  unit: dimensions
  when_allowed: supported
  otherwise: unknown
evidence:
  reported:
    - url: https://docs.cohere.com/docs/cohere-embed
      locator: Model details / embed-v4.0
      checked_on: '2026-10-15'
  observed: []
```

官方给出的可选集合可以记录；集合以外具体返回何种错误仍须以接口说明或观察补充。

### 6.3 参数关系与例外条件

```yaml
illustrative: true
subject: manual_thinking_budget_relation
scope:
  provider: anthropic
  operation: messages.create
  model_scope: models_supporting_manual_thinking
when:
  thinking.type: enabled
  interleaved_thinking: false
status: conditional
requirements:
  - parameter: thinking.budget_tokens
    minimum: 1024
    unit: tokens
  - less_than:
      left: thinking.budget_tokens
      right: max_tokens
evidence:
  reported:
    - url: https://platform.claude.com/docs/en/build-with-claude/extended-thinking
      locator: Budget rules and tuning
      checked_on: '2026-10-15'
  observed: []
```

`models_supporting_manual_thinking` 在这里是讨论占位，正式记录需要引用已审定的明确型号集合。
该示例展示参数关系的需要，不允许据此自动匹配未来型号。

### 6.4 同页冲突保持未裁定

```yaml
illustrative: true
subject: structured_output_with_tools
scope:
  provider: google
  api: interactions
  api_version: v1beta
  model: gemini-4-argon
status: unknown
review_state: conflicted
reported:
  - claim: "正文称仅 Gemini 3 系列提供该组合。"
    url: https://ai.google.dev/gemini-api/docs/structured-output
    locator: Structured outputs with tools / preview note
  - claim: "同节示例使用 gemini-4-argon，并组合 JSON 与工具。"
    url: https://ai.google.dev/gemini-api/docs/structured-output
    locator: Structured outputs with tools / REST example
checked_on: '2026-10-15'
observed: []
next_check: "核查明确型号文档或官方修订，确认组合的实际适用范围。"
```

## 7. 持续采集的工作方式

建议把一次采集的产物先看作来源声明，再形成经过审查的规范化结论。
来源内容进入更新流程后，先比较 URL 重定向、章节、字段、单位和适用型号，再判断是数值变化、范围变化还是页面迁移。
HTML/OpenAPI 类型适合自动提取候选声明，跨页例外和文档冲突需要保留人工审查入口。
抽取模型可用于提示可能遗漏的限制，但它生成的总结须回到可定位的原文。

| 信息来源形态 | 采集难度 | 最容易漏掉的内容 |
|---|---|---|
| API 字段类型、枚举与默认值 | 低 | 泛化类型背后的字段专属限制；客户端类型比服务端宽松 |
| 型号表、上下文和维度表 | 低至中 | 表头作用域、停用状态、默认值与最大值区别 |
| 指南中的兼容性说明 | 中 | beta 条件、不同平台、manual/adaptive 模式 |
| 媒体指南 | 中 | 编码、采样率、文件传递方式、每图与每请求大小 |
| 文档跨页冲突、重定向与滚动型号 | 高 | 旧示例被部分替换、页内范围不一致、接口迁移 |
| 实际行为探针 | 高 | 成本、账户条件、输入样本、地区、重试、字段被忽略 |

第一批人工复核可以直接采用 C01、C02、C04、C06、C09、C14、C17 作为不同规则形态的讨论样本。
下一阶段优先补齐图像生成、视频生成、独立 OCR、音乐与专用视觉模型的官方限制，以及同一模型在多个服务商上的差异。
正式采集前还需明确来源更新频率、内容保留许可与是否允许付费探针，这些由总体流程设计决定。

本轮未执行付费 API、运行探针或编写采集代码。
主要风险是官方页面会更新且存在内部冲突，本文描述的是 2026-10-15 读取到的文档声明，不能当作所有请求条件下的运行保证。

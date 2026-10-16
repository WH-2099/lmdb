# 人工审查的条件声明

[返回项目](../README.md) · [首批数据](../data/assertions/core.yaml) · [专业模型](specialized-models.md) · [规则查询](rule-queries.md) · [类型与校验](../src/assertion.rs)

`data/assertions/` 保存已阅读官方材料后整理的条件声明。
`core.yaml` 保留首批 25 条声明与 18 个来源；`specialized.yaml` 新增 4 个专业模型的 9 条声明。
截至 2026-10-16，共 34 条声明，其中 32 条已接受，2 条保留来源冲突。
所有证据均为 `reported`，没有进行模型推理调用。
这些记录补充自动导入的目录数据；它们的来源和作用域分别保留。

## 文件结构

每个文件是一个 `AssertionSet`，包含 `schema_version: 1`、来源映射 `sources` 和声明数组 `assertions`。
所有字段名均由 Rust 类型定义，结构中的未知字段会被拒绝。
YAML 使用 JSON 兼容值，字符串显式加引号，日期采用字符串形式的 `YYYY-MM-DD`。

| 对象 | 字段 | 含义 |
|---|---|---|
| `DocumentSource` | `url`、可选 `title`、`checked_on` | HTTPS 原始来源、标题、最近阅读该来源的日期 |
| `Assertion` | `id`、`scope`、`property`、`value` | 本文件内唯一的声明 ID、主体范围、属性路径、原生 JSON 值 |
| `Assertion` | `status`、`conditions`、`review` | 支持状态、适用条件、审查结果 |
| `Assertion` | `evidence`、可选 `unit`、可选 `notes` | 非空证据列表、计量单位、必要的范围说明 |
| `Scope` | `kind`、`id` | `model` 或 `interface`，以及外部命名空间中的明确标识 |
| `Condition` | `path`、`op`、`value` | 需要核对的条件位置、`eq` 或 `in`、条件值 |
| `Evidence` | `source`、`locator`、`checked_on`、`kind` | 来源键、章节或字段定位、本条声明的核查日期、`reported` |

`scope.id` 不要求出现在自动导入的 `Record` 中。
例如 `cohere/embed-v4.0` 描述模型，`cohere/embed.v2` 描述服务接口。
本库为接口使用的说明性 ID 并不意味着上游存在同名 HTTP 路径。
这些 ID 用于准确引用当前声明的主体，不能根据名称相似、大小写或标点自动合并不同主体。
跨平台对应关系需要独立证据。
[`data/identities/core.yaml`](../data/identities/core.yaml) 把已核查的模型记录与模型作用域、服务记录与接口作用域显式绑定，`rules --record` 只沿这些直接绑定选择声明。
服务的模型引用和派生关系不会带入其他作用域的规则；没有绑定的记录返回空的规则集合。

`property` 是此库的属性路径，并不自动等于请求字段路径或 JSON Pointer。
属性名中的 `default`、`max`、`required` 等词有不同意义，应连同作用域、单位和注释阅读。
`value` 直接保存数字、字符串、布尔、null、数组或对象；不会把 JSON 编码进字符串。
`false` 和空数组会保留原值，不能在导出时被当作缺失数据删掉。

## 状态与条件

| `status` | 含义 |
|---|---|
| `supported` | 所述能力在明确范围内可用，或该范围有已说明的可用规格 |
| `unsupported` | 有明确不支持、拒绝或类型排除的证据 |
| `unknown` | 证据不足或尚未裁定；`value` 必须为 null |
| `not_applicable` | 该属性不属于所述操作的语义；通常使用 null，不能由文档未提及推定 |

支持状态与审查结果分开。
`review: accepted` 表示接受这条带来源和范围的声明，不表示已运行验证。
`review: conflicted` 必须同时使用 `status: unknown` 与 `value: null`，保留冲突定位供后续复核。
首批没有证据足够明确的 `not_applicable` 条目，类型保留了这个状态，避免后续将其混成“不支持”。

条件列表中的各项共同限定声明的适用范围。
`eq` 表示与给出的 JSON 值相等，`in` 表示属于给出的非空数组。
比较保留 JSON 类型，数字 `0` 与字符串 `"0"` 不相等。
条件缺失或无法确认时，消费方不能直接应用这条声明。
条件列表为空只表示在已写明的作用域内没有附加条件，不表示可以扩展到其他接口或型号。
`rules --context` 接收以条件路径为字面键的 JSON 对象，例如 `{"artifact.checkpoint":"MedSAM2_US_Heart.pt"}`。
条件求值返回 `matched`、`not_matched` 或 `undetermined`，缺少的路径在结果中列明。
同一作用域与属性的候选一起求值，未知条件保持 `unknown`，明确矛盾保持 `conflicted`，全部不匹配则返回 `no_applicable_rule`。
这些结果都不能简化为 `unsupported`；只有明确声明的不支持状态才表达不支持。
指定接口作用域后，`context.interface` 与它矛盾会报错，避免套用另一接口的上下文。
研究报告中讨论过的 `conditional` 在当前格式中通过 `conditions` 与具体 `status` 组合表达。

## 属性的范围

| 属性或样本 | 必须保留的解释 |
|---|---|
| `input.system_instruction.image` | Gemini `systemInstruction.parts` 的图像内容，不包括普通 `contents` 中的图片，也不包括文本形式的图片 URL |
| `input.top_level_system.content_types` | Claude 请求顶层 `system`，不约束中途的 `messages[].role=system` |
| `output.citations` + JSON 条件 | 在 Claude 的 JSON 输出功能范围内核查组合兼容性，不宣称所有型号都支持 JSON 输出 |
| `input.image.limits` | AWS Bedrock 的消息内容规则；MB 保留来源原单位，没有假定它等于 MiB |
| `input.inputs.max_items` | Cohere 请求的 `inputs` 数组，不是独立 `images` 数组或每条 input 内的组件数 |
| `embedding.output_dimensions` | 离散的可选维度集合，不是上下界之间的任意整数 |
| `input.truncate.default` | 默认参数值，不是模型容量 |
| `input.document.max_tokens.default` | Rerank 端点的单篇文档默认截断长度，不是模型总上下文长度 |
| `forecast.context.max_steps`、`forecast.prediction.max_steps` | 时间序列的历史步与预测步，不是语言 token |
| `output.action.dimensions`、`output.action.unnormalization_scope` | OpenVLA 动作结构及机器人/数据集统计量，不表示已经验证实际机器人执行 |
| ESMC `input.context.max_tokens` | 蛋白质 tokenizer 的 token，不能换算为自然语言词数或直接当作氨基酸数 |
| ESMC `output.attentions` | 带具体运行实现和 attention 配置的限制，不是所有实现都不能返回 attention |
| `classification.head.pretrained` | ESMC 的 token/sequence 分类头，不是已有的 masked language model 头 |
| `workflow.inference_state.required` | SAM2 视频预测工作流维护状态，不表示 `init_state` 需要一个预先存在的状态 |
| FAST `artifact.role` | `action_tokenizer` 表示动作序列 tokenizer；`robotics` 任务归类本身不能证明机器人控制策略能力 |
| GR00T `input.representations`、`output.action.representation` | 输入表示和连续动作向量来自作者模型卡；动作维度随机器人本体变化，不补写固定维度或 SDK 必填字段 |
| ESMFold2 蛋白输入与结构输出 | 保存氨基酸序列、可选 MSA 和结构输出；可选 MSA 不扩展到 ESMFold2-Fast |
| MedSAM2 `segmentation.specialization` | 心脏超声、MRI 肝脏病灶和 CT 病灶分别受 `artifact.checkpoint` 条件约束 |
| Mistral 离线转写时长冲突 | `voxtral-mini-latest` 是按核查日期记录的滚动别名，不能当作不可变模型版本 |

本轮的两条冲突分别是 Gemini structured output 与工具的型号范围，以及 Mistral 离线转写时长。
具体官方 URL、章节和原始说法保存在声明的证据与注释中。
专业样本的命令、固定修订来源与作用域见[专业模型](specialized-models.md)，条件求值示例见[规则查询](rule-queries.md)。
更多研究上下文见[限制报告](../research/2026-10-15/sources/constraints.md)和[专业模型报告](../research/2026-10-15/sources/specialized-models.md)。

## 校验边界与维护

`assertion::validate` 校验版本、唯一且非空的声明 ID、非空作用域和属性、来源引用、HTTPS URL、真实日历日期、状态与 null 的关系，以及 `in` 的非空数组要求。
布尔值不能带计量单位；数值、数组等规格值保持原类型。
校验不会联网，也不能证明来源正文、模型身份、规则含义或能力本身正确。
来源页和某条声明的核查日期可以不同，应分别记录实际阅读和核查时间。

更新一条声明时，需要重新核对主体、接口、条件、数值口径和页内定位，再更新该条证据日期。
来源网页更新不应自动覆盖旧规则，也不能用最近抓取日期替代规则实际生效日期。
确认有实际运行证据后，再设计 `observed` 所需的请求、响应和环境信息；当前格式只接收 `reported`。

当前数据仍可能受到动态文档迁移、滚动别名变化和上游说明不完整的影响。
调用方需要同时阅读条件、审查状态和证据，不能把 `accepted` 当作每个账号、地区与版本都可运行的保证。

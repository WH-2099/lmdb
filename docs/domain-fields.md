# 通用领域字段

[项目首页](../README.md) · [数据模型与证据](data-model.md) · [条件声明](assertions.md)

[`src/domain.rs`](../src/domain.rs) 将记录中已导入的事实投影为 `DomainMetadata`。
`inspect` 的 `profile` 与 `export` 的 `profiles` 使用这组类型，原始 `facts` 继续保留。
投影只读取当前记录，不从模型名称、关联记录或人工声明补全能力。
已识别字段类型不符时，投影返回错误；注册表校验也执行这项检查。

## 值与证据

每个派生值使用 `Derived<T>` 保存 `value`、`fact` 和 `pointer`。
`fact` 是当前记录的事实键，`pointer` 是指向该记录所引用快照的完整 JSON Pointer。
复合字段中的价格和任务标签使用更具体的子路径，例如 `/pricing/prompt` 或 `/tasks/1/Name`。
实际指针沿用来源位置，不能根据统一后的字段名重新拼出。

可选字段缺失表示未知，序列化时省略。
`0`、`false` 和空数组都是已提供的值，分别保留为 `0`、`false` 和空数组。
来源事实中的顶层 `null` 在适配阶段省略；手工填入不符合类型的值会校验失败。
`family: null`、`canonical: null` 和 `basis: null` 分别表示尚未归类的任务、模态和计价单位。
来源标签与价格金额仍可存在，不能因归类未知而丢弃。

| 字段组 | 类型与含义 |
| --- | --- |
| `tasks` | 可选的来源任务分类，保存 `source`、`taxonomy` 和带证据的 `labels` |
| `languages` | 来源声明的语言标签，接受字符串或字符串数组，保留原始词表 |
| `licensing.labels`、`licensing.name`、`licensing.link` | 来源许可证标签、自定义名称及链接或文件路径 |
| `access.gated`、`access.private` | 来源访问状态；`gated` 保留布尔值或来源标签，`private` 为布尔值 |
| `lifecycle` | Anthropic `lifecycle` 或 models.dev `status` 的原始状态标签 |
| `modalities.input`、`modalities.output` | 可选标签数组，每项保存 `source_label` 和可选标准模态 `canonical` |
| `limits.context_window` | 来源报告的上下文窗口，非负整数，单位为 token |
| `limits.max_input_tokens`、`limits.max_output_tokens` | 来源明确报告的输入、输出 token 上限 |
| `limits.top_provider_context_window` | OpenRouter 的 `top_provider.context_length`，独立于模型级上下文窗口 |
| `parameters.safetensors_total` | safetensors 元数据报告的总参数量，非负整数，单位为参数 |
| `parameters.by_dtype` | 来源报告的各数据类型参数量，保留 `BF16` 等原始键 |
| `capabilities` | 可选的能力布尔值及 `supported_parameters` 字符串数组 |
| `lineage.base_models`、`lineage.base_model_relation` | 来源报告的基础模型名称列表及关系标签 |
| `lineage.new_version` | 发布者在 HF 模型卡中声明的后续版本引用 |
| `lineage.revision`、`lineage.version`、`lineage.base_model_id` | 来源声明的仓库修订、版本字符串与基础型号标识 |
| `pricing` | 可选价格表，包含基础费率、原始条件及未归类字段 |

token 上限必须同时具有 `unit: tokens`，总参数量必须具有 `unit: parameters`。
不以总参数量推导混合专家模型的激活参数量，也不从名称中的 `B` 或数据类型计数推导模型规模。
基础模型字符串被包装成单项数组；ModelScope 的空关系字符串视为未提供。
这些谱系字段保留来源声明，不自动继承基础模型的能力或许可证。

许可证与语言的单个字符串被包装成单项数组，原始形状可由 `fact` 回放。
许可证标签保留 `other` 等来源词表，自定义名称和链接各自保存证据。
HF 的相对许可路径属于当前记录的仓库和 `lineage.revision` 所指修订；来源未提供修订时，路径的修订也未知。
许可证标签与开放权重状态各自表达来源声明，商业使用条款需要查阅对应许可文本。
语言标签描述发布者列出的语言，不能证明实际质量或完整语言覆盖。

HF 的 `gated` 可能为 `false`、`auto`、`manual` 或未来增加的标签，投影原样保留。
`private: false` 与门禁状态分别记录；二者均不表达当前账号下载权限。
`lifecycle` 保留未知状态标签，且不从本地采集范围或目录缺席推导状态。
发布日期、弃用日期和退役日期继续分别保留原始事实；数字日期的单位需要来源契约支持。
版本引用保持来源原文；后续版本引用不等同于别名、等价模型或自动迁移指令。

`capabilities` 包含 `reasoning`、`thinking`、`tool_call`、`structured_output`、`open_weights`、`attachment`、`temperature_control` 与 `supported_parameters`。
各布尔值独立保留，`reasoning` 和 `thinking` 不互相推导。
`temperature_control` 仅接收 models.dev 的布尔字段 `temperature`，其他来源中的采样温度数值不属于这个能力字段。
这些字段表达来源报告的值，不代表经过推理调用验证。

## Hugging Face 复合元数据

以下字段直接读取已保存的模型目录响应，所有复合对象与数组完整保存在事实中。
每项事实的 JSON Pointer 指向当前记录引用的同一份响应。

| 事实键 | 来源路径 | 含义 |
| --- | --- | --- |
| `license_name` | `/cardData/license_name` | 自定义许可证名称 |
| `datasets` | `/cardData/datasets` | 发布者填写的数据集标签 |
| `new_version` | `/cardData/new_version` | 后续版本引用 |
| `torch_dtype`、`dtype` | `/config/torch_dtype`、`/config/dtype` | 配置声明的数据类型，分别保留 |
| `quantization_config` | `/config/quantization_config` | 完整量化配置，包括方法、块大小和排除模块等来源字段 |
| `tokenizer_config` | `/config/tokenizer_config` | 目录响应中的 tokenizer 配置摘要 |
| `auto_map` | `/config/auto_map` | 来源声明的自定义代码入口 |
| `transformers_version` | `/config/transformers_version` | 配置中保存的库版本 |
| `transformers_info` | `/transformersInfo` | Hub 返回的 Transformers 运行信息 |
| `num_experts`、`num_experts_per_tok` | `/config/num_experts`、`/config/num_experts_per_tok` | 配置声明的专家数与每 token 专家数 |
| `siblings` | `/siblings` | 文件清单，保留已返回的路径、大小、Git OID 与 LFS 元数据 |
| `model_index` | `/cardData/model-index` | 模型卡中的旧评测 payload |
| `eval_results` | `/evalResults` | Hub 返回的评测引用，包含已有的贡献者、PR 和验证状态 |

缺失或顶层 `null` 省略，空对象与空数组原样保留。
`torch_dtype: bfloat16` 和 `quantization_config.quant_method: fp8` 可以同时存在，投影不将其折叠为单个精度标签。
配置中的库版本表达保存配置时的声明，不能确定最低兼容版本；专家数也不用于推算激活参数量。
模板可能位于 tokenizer 摘要、独立 `chat_template.jinja` 或 `additional_chat_templates/` 文件，清单只记录已返回的文件信息。
文件清单中的权重摘要与目录响应的 `Snapshot.sha256` 分属不同内容；映射这些字段不下载权重。
数据集标签没有训练、微调或评测角色时，保留其未分类状态。
评测结果保留原始归因与 `verified`，社区提交不会因出现在模型目录中而变成厂商声明。

## 任务与模态

任务分类保留来源词表，当前使用 Hugging Face 的 `pipeline_tag`、ModelScope 的 `Tasks.Name` 和 models.dev 显式提供的 `type`。
ModelScope 的空任务名称被省略，其他标签原样保留。
models.dev 缺少 `type` 时，`tasks` 保持未知，不推导为文本生成。

`TaskFamily` 是便于筛选的有限枚举，原始任务标签是开放词表。
仅为已明确对应的标签填写 `family`：

| 来源标签 | 标准任务族 |
| --- | --- |
| Hugging Face、ModelScope：`text-generation` | `text_generation` |
| Hugging Face、ModelScope：`text-ranking`；models.dev：`reranking` | `reranking` |
| ModelScope：`sentence-embedding`；models.dev：`embedding` | `embedding` |
| Hugging Face：`text-to-image`；ModelScope：`text-to-image-synthesis` | `image_generation` |
| Hugging Face、ModelScope：`image-to-video`；Hugging Face：`text-to-video`、`image-text-to-video`；ModelScope：`text-to-video-synthesis` | `video_generation` |
| Hugging Face：`automatic-speech-recognition`；ModelScope：`auto-speech-recognition` | `speech_recognition` |
| Hugging Face、ModelScope：`text-to-speech` | `text_to_speech` |
| Hugging Face：`image-segmentation`、`mask-generation` | `segmentation` |
| Hugging Face：`time-series-forecasting` | `time_series` |
| Hugging Face：`robotics` | `robotics` |

其他标签保留 `source_label`，`family` 为 `null`。
例如 Hugging Face 的 `feature-extraction`、`sentence-similarity` 及 models.dev 的 `decision`、`video` 不自动归类。
任务标签不证明接口、输入输出张量、部署方式或特定操作能力。

标准模态包括 `text`、`image`、`audio`、`video` 和 `embedding`。
各来源的前四个同名标签直接对应；OpenRouter 的 `speech`、`transcription`、`embeddings` 分别对应 `audio`、`text`、`embedding`。
其他标签保留原文和 `canonical: null`，例如 `file`、`decisions`。

## 价格

每个 `PriceRate` 保存来源键 `key`、费用类别 `component`、金额 `amount` 和可选计价基数 `basis`。
`basis.quantity` 表示该金额覆盖多少个 `basis.unit`，不将每百万 token 的价格转换为每 token 的价格。

`amount.status: known` 的 `usd` 是非负十进制字符串，支持科学计数法，`"0"` 表示已知零价格。
OpenRouter 的 `"-1"` 保存为 `amount.status: unavailable` 和 `raw: "-1"`，不能当作负价、免费或已知金额。
已识别费率中的其他负数、非法数值文本或不符合来源契约的 JSON 类型会被拒绝。

| 来源与字段 | `component` | `basis` |
| --- | --- | --- |
| models.dev：`input`、`output` | `input`、`output` | 每 1,000,000 token |
| models.dev：`cache_read`、`cache_write`、`reasoning` | `cache_read`、`cache_write`、`reasoning` | 每 1,000,000 token |
| models.dev：`input_audio`、`output_audio` | `input_audio`、`output_audio` | 每 1,000,000 audio token |
| OpenRouter：`prompt`、`completion` | `input`、`output` | 仅在下述文本条件满足时为每 1 token，否则未知 |
| OpenRouter：`input_cache_read`、`input_cache_write`、`input_cache_write_1h`、`internal_reasoning` | `cache_read`、`cache_write`、`cache_write_one_hour`、`reasoning` | 每 1 token |
| OpenRouter：`audio`、`audio_output`、`input_audio_cache` | `input_audio`、`output_audio`、`cached_input_audio` | 每 1 audio token |
| OpenRouter：`image_token` | `image_token` | 每 1 image token |
| OpenRouter：`image`、`image_output` | `input_image`、`output_image` | 每 1 image |
| OpenRouter：`request`、`web_search` | `request`、`web_search` | 分别为每 1 request、每 1 search |

OpenRouter 的 `prompt`、`completion` 只有在输入包含 `text`、输入标签全部属于 `text`／`image`／`file`，且输出仅为 `text` 时才标记 token 基数。
音频、视频、图像生成及缺少模态信息的记录保留金额，`basis` 为 `null`，不猜测秒数、图片数或 token 数。
`component` 表达费用类别，不能单独确定单位。

`conditions` 保存 models.dev 的 `tiers`、`context_over_200k` 和 OpenRouter 的 `overrides` 原始 JSON 与证据指针。
条件数组的顺序和未知条件键保留；校验只检查已识别的结构和费率类型，不执行条件、不选择档位，也不计算有效价格。
`unclassified` 保存其余价格键及原值。
存在条件或未归类字段时，`base` 不能单独作为完整报价。

OpenRouter 的价格字符串按来源原文保留，投影不经浮点运算。
models.dev 的价格在导入时已经经过 `serde_json::Number`；当前解析配置可能使小数经过浮点表示并产生舍入。
其 `usd` 保存该数值的十进制输出，不能保证还原上游 JSON 的原始数字拼写或全部精度。
需要核查原始数字时，应查看所引用快照的原始响应字节。

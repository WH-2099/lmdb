# 同类项目对比与有限代码考察

调研日期：2026-10-15。
范围：九项相关项目或目录，三个本地只读代码快照。
返回[本目录索引](README.md)或[本轮调研索引](../README.md)。

## 1. 决策摘要

**先验证独立建设的必要性，再扩大采集范围。**
models.dev 已覆盖大量基础字段、模型与提供方分层、继承和自动同步。
仅仅换成 YAML、Rust 或增加一个查询 API，不足以解释维护第二套同类数据的成本。

LMDB 值得验证的需求，是把模型事实、接口条件与证据明确关联起来。
例如“这个接口的 system 消息是否允许图片”至少涉及模型版本、服务平台、接口形态、消息角色、输入模态和观察时间。
两个独立布尔值 `supports_system_messages=true` 与 `supports_vision=true` 无法证明它们的组合成立。
这里使用的是建模用例，并未断言某个具体 Gemini 版本一定支持或禁止该组合。

建议先人工比较一小组跨模态记录与条件案例，判断是否能通过上游贡献、独立补充文件或组合现有目录满足需求。
只有这些方式仍不足时，才将新的规范化 schema 和同步核心列入实施阶段。
该建议不改变本轮“先调研、暂不实现”的安排。

## 2. 对比总表

“未见”只表示本轮阅读的 schema 或文档未提供该结构。
不把未公开、未填写或未检查的数据解释成不支持。

| 项目 | 核心实体与格式 | 维护方式 | 特例和证据表达 | 对 LMDB 的意义 |
| --- | --- | --- | --- | --- |
| [models.dev](#31-modelsdev) | 模型本体、提供方、提供方模型；TOML → JSON | PR、schema 校验、按提供方同步 | 继承与覆盖、推理选项、价格 tiers；模型链接与评测来源；未见通用逐字段证据 | 最接近基础目录需求，优先复用 |
| [LiteLLM cost map](#32-litellm-cost-map) | API 模型或计费变体；JSON + JSON Schema | PR 与聚合源同步 | 大量能力字段、计费条件、条目级 `source` | 丰富字段与异常案例来源，语义需清洗 |
| [Hugging Face Hub](#33-hugging-face-hub) | 模型仓库、模型卡、衍生关系；YAML + Markdown/API JSON | 作者、社区与 SDK 更新 | 任务、许可证、谱系；限制可写自然语言 | 跨模型类别发现与谱系的主要输入 |
| [PWC model-index](#34-model-index-的两个含义) | 模型集合、模型、训练及评测记录；YAML/JSON/Markdown | 仓库作者维护索引 | 可扩展元信息与结果来源 | 借鉴索引和评测引用，不直接确定 API 能力 schema |
| [OpenRouter catalog](#35-openrouter-catalog) | 模型目录、路由变体、提供方端点；API JSON | 平台维护 | 端点能力、条件价格；无通用字段证据结构 | 服务事实与供应关系的结构化来源 |
| [llm-prices](#36-simonwllm-prices) | 提供方、模型、历史价格；JSON | 手工编辑并构建派生数据 | 有效日期区间、缓存价 | 历史价格的简洁参考 |
| [genai-prices](#37-pydanticgenai-prices) | 提供方、模型匹配规则、价格；YAML → JSON | 聚合差异后人工修订 | 日期、时段、数量 tiers、请求条件、检查日期 | YAML 维护与条件值表达的重点参考 |
| [jnd0/llm-registry](#38-jnd0llm-registry) | 模型、来源、评测分数；TypeScript/JSON | 导入 models.dev、人工与外部评测补充 | 模型级来源、分数级来源及验证等级 | 已有证据设计可借鉴，避免宣称溯源是空白 |
| [MLflow Registry](#39-mlflow-model-registry) | 注册模型、版本、别名、tags、运行及资产来源 | API/UI 管理生命周期 | 资产 lineage 与自由 tags | 借鉴版本和别名，不宜直接作为公共目录核心 |

## 3. 项目详述

### 3.1 models.dev

**已确认的结构。**
当前仓库的 `models/` 保存提供方无关的模型事实，`providers/<provider>/models/` 保存服务条目。
服务条目通过 `base_model` 引用模型本体，可覆盖限制、模态、价格等；生成数据用 `canonical_model_id` 暴露关联。
除 `api.json` 外，已有 `models.json` 和 `catalog.json`。
这些能力在 [README](https://github.com/anomalyco/models.dev/blob/2fbf9b9184715cd21b2969b7e112d681163baf94/README.md#adding-model-metadata) 和[继承实现](https://github.com/anomalyco/models.dev/blob/2fbf9b9184715cd21b2969b7e112d681163baf94/packages/core/src/generate-v2.ts#L445)中均可确认。

**字段和特例。**
当前 authored schema 有输入/输出模态、上下文、推理选项、结构化输出、价格 tiers、模型链接、权重链接与评测来源。
同一快照的 v2 schema 还包含 MIME 类型、图片尺寸和数量限制、工具能力、缓存目标和兼容性参数。
不应将它概括为只有布尔能力的简表。
本轮查看的模型、能力、兼容性 schema 中，未见通用的“条件断言 + 字段路径 + 证据列表”结构，亦未见按 role 定义输入模态的结构。
参考 [authored schema](https://github.com/anomalyco/models.dev/blob/2fbf9b9184715cd21b2969b7e112d681163baf94/packages/core/src/schema.ts#L178)、[v2 模态](https://github.com/anomalyco/models.dev/blob/2fbf9b9184715cd21b2969b7e112d681163baf94/packages/core/src/schema-v2.ts#L23)、[v2 能力与图片限制](https://github.com/anomalyco/models.dev/blob/2fbf9b9184715cd21b2969b7e112d681163baf94/packages/core/src/schema-v2.ts#L256)、[v2 兼容性](https://github.com/anomalyco/models.dev/blob/2fbf9b9184715cd21b2969b7e112d681163baf94/packages/core/src/schema-v2.ts#L347)。

**维护流程。**
公共 runner 统一处理读取、校验、差异与写入；提供方模块负责抓取、解析和转换。
CI 按提供方生成同步 PR，配置中有每小时触发。
Google adapter 明确保留部分人工元信息，并说明 Models API 不提供若干权威字段，证明“能列出 ID”不等于“能补全模型事实”。
参考 [runner 接口](https://github.com/anomalyco/models.dev/blob/2fbf9b9184715cd21b2969b7e112d681163baf94/packages/core/src/sync/index.ts#L82)、[工作流](https://github.com/anomalyco/models.dev/blob/2fbf9b9184715cd21b2969b7e112d681163baf94/.github/workflows/sync-models.yml#L1)、[Google adapter](https://github.com/anomalyco/models.dev/blob/2fbf9b9184715cd21b2969b7e112d681163baf94/packages/core/src/sync/providers/google.ts#L60)。

**复用判断。**
优先将它作为基础记录与标识关联的输入，并考虑把通用字段修订贡献给上游。
`last_updated`、模型链接和评测来源分别有用，但尚不能替代每个能力值的核验时间、接口条件和证据出处。
应保留上游原始 ID、commit、字段路径和原始值，避免导入后无法解释继承或覆盖结果。
根 [LICENSE](https://github.com/anomalyco/models.dev/blob/2fbf9b9184715cd21b2969b7e112d681163baf94/LICENSE) 为 MIT；记录内的模型许可证、logo 和外链内容需另行区分。

### 3.2 LiteLLM cost map

**已确认的结构。**
`model_prices_and_context_window.json` 以字符串键映射模型或计费条目。
条目可带 `litellm_provider`、`mode`、输入/输出限制、能力、地区、价格等。
键也可能编码分辨率、采样步数等计费变体，不能统一当成模型本体的规范 ID。
当前官方文档支持 `aliases`，用于共享相同价格和能力的 cost-map 条目。
参考[文件样本](https://github.com/BerriAI/litellm/blob/3cc9e7ef8ef82bb377844a54dcc3a64c527cb60e/model_prices_and_context_window.json#L42)与[维护文档](https://docs.litellm.ai/docs/provider_registration/add_model_pricing)。

**特例与溯源。**
schema 有 `supports_system_messages`、`supports_vision`、`supported_modalities`、`supported_endpoints`，以及许多上下文分档和批处理价格键。
`source` 的定义是该条目的提供方价格或模型页面 URL。
它提供条目级来源，但本轮未见按任意字段独立记录来源、核验时间和冲突的结构。
独立的 system、vision 布尔值也不能推导图片能否出现在 system 消息。
参考 [source 与接口/模态字段](https://github.com/BerriAI/litellm/blob/3cc9e7ef8ef82bb377844a54dcc3a64c527cb60e/model_prices_and_context_window.schema.json#L949)和[能力字段](https://github.com/BerriAI/litellm/blob/3cc9e7ef8ef82bb377844a54dcc3a64c527cb60e/model_prices_and_context_window.schema.json#L1122)。

**维护流程。**
支持人工 PR，仓库另有定时脚本从 OpenRouter、Vercel AI Gateway 等来源更新 JSON，再生成 schema 和 PR。
本轮只读查看该流程，没有执行脚本，也没有验证运行结果。
参考[工作流](https://github.com/BerriAI/litellm/blob/3cc9e7ef8ef82bb377844a54dcc3a64c527cb60e/.github/workflows/auto_update_price_and_context_window.yml#L1)和[聚合源入口](https://github.com/BerriAI/litellm/blob/3cc9e7ef8ef82bb377844a54dcc3a64c527cb60e/.github/scripts/auto_update_price_and_context_window_file.py#L288)。

**复用判断。**
适合作为字段候选、对账来源及接口异常案例库。
导入需要处理价格单位、别名、计费变体、provider 命名和旧 `max_tokens` 语义。
官方 sample spec 明确称 `max_tokens` 为 legacy 字段，不能直接当上下文长度。
根 [LICENSE](https://github.com/BerriAI/litellm/blob/3cc9e7ef8ef82bb377844a54dcc3a64c527cb60e/LICENSE#L1) 将 `enterprise/` 之外的内容列为 MIT 范围，cost map 位于该范围；企业目录另有许可证。

### 3.3 Hugging Face Hub

模型仓库的 `README.md` 同时承载 YAML 元数据和 Markdown 模型卡。
官方字段包括任务分类、库、许可证、`base_model`、`base_model_relation`、`new_version` 等。
衍生关系可表达微调、adapter、merge 和量化，适合补充模型谱系。
作者可以通过文件、网页界面、SDK 和集成库维护；`list_models()` 可用于发现 Hub 模型仓库。
参考[模型卡文档](https://huggingface.co/docs/hub/en/model-cards)和[模型搜索](https://huggingface.co/docs/huggingface_hub/en/guides/search)。

自由文本可记录复杂限制，但本轮已读标准字段没有统一的 `role × modality × endpoint` 条件结构，也没有通用逐字段证据封装。
同一底层模型可能对应原始、量化、adapter 和镜像仓库；Hub 仓库 ID 不能自动等同于规范模型 ID。
模型卡的来源质量也取决于发布者，组织归属和原始发布方需单独判断。

`huggingface_hub` 客户端采用 [Apache-2.0](https://github.com/huggingface/huggingface_hub/blob/main/LICENSE)。
Hub 中的模型和数据仓库由各自声明决定许可，[仓库许可说明](https://huggingface.co/docs/hub/en/repositories-licenses)不构成整个目录内容统一再分发授权。
将它作为跨类别发现、发布者声明和谱系来源；对外再发布时区分短事实、完整模型卡文本和关联资产。

### 3.4 model-index 的两个含义

Hugging Face 模型卡里的 `model-index` 字段，官方文档主要用于描述评测结果。
Hub 现在另有 `.eval_results/*.yaml` 形式，支持数据集 revision、日期、source、notes 等。
这些字段值得借鉴，但表示的是评测记录。
参考 [model-index 用法](https://huggingface.co/docs/hub/en/model-cards#evaluation-results)与[当前评测结果格式](https://huggingface.co/docs/hub/en/eval-results)。

独立的 [paperswithcode/model-index](https://github.com/paperswithcode/model-index) 则是更一般的机器学习模型元数据索引库。
它支持 YAML、JSON 和 Markdown 元数据，允许扩展字段，示例包含参数量、FLOPs、训练数据、论文、代码、权重和结果。
不要把这个库概括为“只能存 benchmark”。
本轮查询到 main 为 `09c7865ab2cbe00827d3af879a2faf7d286cb1b7`，提交日期为 2021-06-09；将其视作设计参考，采用库本身前还应核验维护状态。
参考[固定快照 README](https://github.com/paperswithcode/model-index/blob/09c7865ab2cbe00827d3af879a2faf7d286cb1b7/README.md)。

其工具 [LICENSE](https://github.com/paperswithcode/model-index/blob/09c7865ab2cbe00827d3af879a2faf7d286cb1b7/LICENSE) 为 MIT。
被索引的模型、论文、数据集和评测结果不会因此自动取得相同许可。
可借鉴跨文件索引与结果描述；任意字段可扩展不等于条件规则已有统一语义。

### 3.5 OpenRouter catalog

Models API 返回 JSON，包含模型 ID、永久 `canonical_slug`、输入/输出模态、上下文、价格、`top_provider`、支持参数与弃用时间等。
`created` 表示加入 OpenRouter 的时间，不应直接映射为模型的全球发布日期。
模型列表和端点列表是不同视角；端点条目可带提供方、上下文、量化、价格和支持参数。
参考[模型目录说明](https://openrouter.ai/docs/guides/overview/models)与[端点 API](https://openrouter.ai/docs/api/api-reference/endpoints/list-all-endpoints-for-a-model)。

当前列表默认只返回 text 输出模型。
要纳入图像、视频、音频、embedding、rerank 等，需明确使用 `output_modalities=all` 或相应筛选值。
分页为可选；启用时应沿 `links.next` 获取后续页面，不能假定第一页是完整目录。
参考[查询参数与分页](https://openrouter.ai/docs/guides/overview/models#query-parameters)。

当前 `pricing.overrides` 可表达上下文阈值、UTC 时间窗和星期条件，按条件匹配后局部覆盖价格。
这说明条件式数据不是 LMDB 独有需求，也提示 schema 需要说明边界包含关系和多规则优先级。
模型目录中的能力与价格服务于 OpenRouter；不能自动推广为底层模型在所有平台上的事实。
本轮所读公开 schema 未提供通用逐字段证据或角色与模态组合限制。
参考[条件价格说明](https://openrouter.ai/docs/guides/overview/models#pricing-object)。

它是优先级很高的结构化来源，尤其适合后续的托管端点实体。
本轮未查得 catalog 数据的独立开放数据许可证，不把“公开 API 可访问”写成“可无限再分发”。
具体使用范围和再发布授权由[数据源调研](../sources/README.md)继续跟踪。

### 3.6 simonw/llm-prices

项目使用按提供方划分的 JSON，模型条目保存 `price_history`，再构建当前和历史价格产物。
`from_date` 包含起点，`to_date` 不包含终点，适合表达价格有效期。
字段聚焦输入、输出、缓存输入价格，本轮检查的 schema 未见能力规则和逐字段证据结构。
参考[固定快照 README](https://github.com/simonw/llm-prices/blob/1029cc24e45065b4058522c2da129dee182099c0/README.md)。

最值得借鉴的是历史区间和编辑源文件、生成发布数据的分离。
对整个 LMDB 需求而言范围较窄；无需为参考其数据而引入完整网站。
本轮 [GitHub API](https://api.github.com/repos/simonw/llm-prices) 的 `license` 为 null，根目录和 README 未见明确许可证，代码和数据再利用授权均标为待确认。
这只记录检查结果，不据此断言作者禁止使用。

### 3.7 pydantic/genai-prices

项目以提供方 YAML 为维护入口，生成带 JSON Schema 的 JSON，并提供多语言消费方式。
维护工具比较 Helicone、OpenRouter、LiteLLM 和 llm-prices 等来源，形成差异供维护者处理，并记录价格检查日期。
这与 LMDB 设想的“上游发现、规范化、人工审查、发布”流程高度相关。
参考[项目 README](https://github.com/pydantic/genai-prices/blob/a7e6d36cb10bd4829d3773d37ede7a3daebaa00d/README.md)与[价格维护文档](https://github.com/pydantic/genai-prices/blob/a7e6d36cb10bd4829d3773d37ede7a3daebaa00d/prices/README.md)。

已有日期、时间段、上下文 tiers 与请求条件 `price_variants`。
`when` 对参数做精确字符串匹配，并允许局部价格覆盖。
`pricing_urls`、`prices_checked`、`price_comments` 提供来源与维护线索，但不是通用的每字段证据结构。
参考[提供方 schema](https://github.com/pydantic/genai-prices/blob/a7e6d36cb10bd4829d3773d37ede7a3daebaa00d/prices/providers/.schema.json)与[Google 样本](https://github.com/pydantic/genai-prices/blob/a7e6d36cb10bd4829d3773d37ede7a3daebaa00d/prices/providers/google.yml)。

**当前 feed 选择需注意版本。**
README 已说明 `prices/data.json` 的 v1 数据冻结，新接入应研究 `prices/new_data/v2/data.json`。
不能沿用旧集成示例就认定能持续获得更新。
参考[下载说明](https://github.com/pydantic/genai-prices/blob/a7e6d36cb10bd4829d3773d37ede7a3daebaa00d/README.md#download-data)。

仓库 [LICENSE](https://github.com/pydantic/genai-prices/blob/a7e6d36cb10bd4829d3773d37ede7a3daebaa00d/LICENSE) 为 MIT，README 明确提供数据下载和使用入口；本轮未见另设的数据许可证。
建议借鉴维护与发布流程、条件语义和检查时间，但其价格引擎不能直接替代能力约束模型。

### 3.8 jnd0/llm-registry

选择该项目是因为它明确实现来源和验证等级，可用于检查 LMDB 的差异是否已经存在。
项目以模型比较和 benchmark 为核心，导入 models.dev，并加入评测结果、模型来源和展示 API。
参考[固定快照 README](https://github.com/jnd0/llm-registry/blob/ceccfe058caab1a6acbe338cef8137207b345140/README.md)。

本轮代码检查确认两种粒度：模型对象有 `metadataSourceId`、`metadataSourceUrl`、`metadataAsOfDate`；每个分数有 `sourceId`、`sourceUrl`、`asOfDate` 和 `verificationLevel`。
来源列表独立维护；能力字段仍主要是布尔值与输入/输出模态数组。
因此它已经提供模型级和分数级溯源，但本轮未见所有规格字段统一使用的证据结构或通用 API 条件规则。
参考[实体与来源字段](https://github.com/jnd0/llm-registry/blob/ceccfe058caab1a6acbe338cef8137207b345140/src/types/index.ts#L121)、[能力结构](https://github.com/jnd0/llm-registry/blob/ceccfe058caab1a6acbe338cef8137207b345140/src/types/index.ts#L57)、[来源表](https://github.com/jnd0/llm-registry/blob/ceccfe058caab1a6acbe338cef8137207b345140/src/data/sources.ts#L1)。

导入脚本从 models.dev API 获取数据，并设置模型来源 ID 和当日日期；另有定时更新工作流。
导入时间与上游事实生效时间是不同概念，LMDB 应分别保留。
参考[导入逻辑](https://github.com/jnd0/llm-registry/blob/ceccfe058caab1a6acbe338cef8137207b345140/scripts/import-models-dev.mjs#L218)与[工作流](https://github.com/jnd0/llm-registry/blob/ceccfe058caab1a6acbe338cef8137207b345140/.github/workflows/update-models-dev.yml#L1)。

根 [LICENSE](https://github.com/jnd0/llm-registry/blob/ceccfe058caab1a6acbe338cef8137207b345140/LICENSE) 为 MIT，README 另有 Artificial Analysis、models.dev 等数据归属说明。
这些归属说明不能代替每个上游的再发布授权核验。
可借鉴证据等级和来源目录；其 benchmark 网站与 LMDB 当前边界不同，不建议直接采用整个应用。

### 3.9 MLflow Model Registry

MLflow 的核心实体是 registered model、model version、aliases、tags、description，以及相关 run、logged model 和 artifact 来源。
它适合团队管理模型从研发到部署的生命周期，别名能指向不同版本。
MLflow 3 也支持 external model，不能将它描述为必须托管权重才可使用。
参考[Registry 官方说明](https://mlflow.org/docs/latest/ml/model-registry/)与[模型工作流](https://mlflow.org/docs/latest/ml/model-registry/workflow/)。

该 lineage 主要解释一个模型资产与训练、运行或产物的关系。
它不能直接替代“公开模型某字段来自哪个网页段落，以及何时核验”的证据模型。
tags 可携带自定义值，但需要 LMDB 自己另行定义语义。
因此可借鉴版本与可变别名的分离，当前没有理由引入一个完整的模型生命周期平台。

MLflow 代码采用 [Apache-2.0](https://github.com/mlflow/mlflow/blob/master/LICENSE.txt)。
用户注册的模型和元数据不会因使用 MLflow 而自动获得相同许可。

## 4. 授权记录汇总

这是公开许可证证据清单，不是对所有上游材料再发布权利的完整审查。
尤其要区分仓库代码、人工维护的数据、模型权重、模型卡全文、logo 和第三方评测结果。

| 项目 | 本轮确认的代码或仓库许可 | 数据处理记录 |
| --- | --- | --- |
| models.dev | 根 MIT | 记录根许可证依据；模型自身许可与外链内容另存 |
| LiteLLM | `enterprise/` 之外内容 MIT | cost map 位于该范围；记录来源页面 |
| Hugging Face Hub | 客户端 Apache-2.0 | 各模型/数据仓库许可分别判断 |
| PWC model-index | 工具 MIT | 不推导被索引内容许可 |
| OpenRouter | 本轮未核验平台代码许可 | 未查得独立 catalog 数据许可证，待确认再发布范围 |
| llm-prices | 未见明确许可 | 待确认，不默认开源授权 |
| genai-prices | 根 MIT，README 提供数据下载 | 未见独立数据 LICENSE；保留授权依据与上游引用 |
| jnd0/llm-registry | 根 MIT | 外部数据另有归属说明，按来源核验 |
| MLflow | Apache-2.0 | 注册资产遵循资产自身许可 |

## 5. 可复现的代码考察记录

检查过 `/data/git/github.com` 下对应缓存路径。
未发现此次三个目标仓库的可复用裸仓；找到的 LiteLLM-Labs `litellm-agent-control-plane.git` remote 属于另一个仓库，未将其作为 BerriAI/litellm 缓存使用。
本轮使用只含当前历史的临时克隆，未修改上游仓库分支，未安装依赖，未执行上游程序。

| 仓库 | 临时路径 | 固定 commit | 实际阅读范围 |
| --- | --- | --- | --- |
| anomalyco/models.dev | `/tmp/lmdb-research/models.dev` | `2fbf9b9184715cd21b2969b7e112d681163baf94` | README、LICENSE、schema.ts、schema-v2.ts、generate-v2.ts、sync runner、Google adapter、同步工作流 |
| BerriAI/litellm | `/tmp/lmdb-research/litellm` | `3cc9e7ef8ef82bb377844a54dcc3a64c527cb60e` | cost map、JSON Schema、LICENSE、价格更新脚本与工作流、cost-map guard 的有限片段 |
| jnd0/llm-registry | `/tmp/lmdb-research/llm-registry` | `ceccfe058caab1a6acbe338cef8137207b345140` | README、LICENSE、模型类型、来源目录、models.dev 导入器与工作流 |

另外通过第一手仓库文档与 API 固定了 genai-prices `a7e6d36cb10bd4829d3773d37ede7a3daebaa00d`、llm-prices `1029cc24e45065b4058522c2da129dee182099c0`、model-index `09c7865ab2cbe00827d3af879a2faf7d286cb1b7` 的阅读链接。
这些项目没有在本轮本地克隆运行。

## 6. 下一轮需要验证的最小问题集

1. 选取文本、图像、音频、视频、embedding、reranker 等记录，将 models.dev、HF 与官方来源对应起来，记录无法确定的同一性关系。
2. 选取角色限制、模态限制、接口版本差异、推理与工具组合限制，检查现有字段是否能无歧义表达。
3. 为每个案例保留官方证据、上游字段、检查日期与冲突，不用默认值填满未知项。
4. 判断最小补充文件能否附着于现有目录；若可以，优先采用这一方式。
5. 在明确复用来源和授权依据后，再决定是否需要独立 schema、Rust 同步核心与发布格式。

本轮未运行上游校验器、同步程序、推理 API 或性能测试。
结论基于有限文档与代码检查；尚不能据此保证任何目录的实时准确性或穷尽覆盖。

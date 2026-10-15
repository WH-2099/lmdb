# 需要凭据的来源接口

[项目首页](../README.md) · [来源调研](../research/2026-10-15/sources/README.md)

## 当前状态

这些适配器保留请求说明、响应归一化和分页能力，默认禁用。
本轮没有读取真实凭据，没有发起认证请求，也没有执行推理。
测试使用自行编写的合成响应，**fixture 验证不等于在线验证**。
示例模型名、组织名、数字和时间均用于测试，不构成可发布的模型数据。

## 接口边界

实现位于 [`src/adapters/gated.rs`](../src/adapters/gated.rs)。
`request_spec` 返回固定目录 URL、凭据环境变量名、认证头格式和必要的额外请求头，本身不读取环境变量。
`normalize` 接收已保存的响应及快照，输出带绝对 JSON Pointer 的记录。
`next_page` 解析当前响应中的分页信息，返回下一页 URL 或结束标记。
上层下载器负责默认禁用检查、凭据读取、请求发送、分页 URL 的同源验证和循环检测。
认证值只进入请求头，不进入 URL、快照或日志。
本文件记录接口契约，不提供本轮实际启用或运行认证抓取的步骤。

## 来源与请求说明

| 来源键 | 目录 URL | 环境变量名 | 请求头 |
| --- | --- | --- | --- |
| `artificialanalysis` | `https://artificialanalysis.ai/api/v2/language/models/free` | `ARTIFICIAL_ANALYSIS_API_KEY` | `x-api-key` |
| `openai` | `https://api.openai.com/v1/models` | `OPENAI_API_KEY` | `Authorization: Bearer …` |
| `anthropic` | `https://api.anthropic.com/v1/models` | `ANTHROPIC_API_KEY` | `x-api-key`，另有 `anthropic-version: 2023-06-01` |
| `gemini` | `https://generativelanguage.googleapis.com/v1beta/models` | `GEMINI_API_KEY` | `x-goog-api-key` |
| `deepseek` | `https://api.deepseek.com/models` | `DEEPSEEK_API_KEY` | `Authorization: Bearer …` |
| `cohere` | `https://api.cohere.com/v1/models` | `COHERE_API_KEY` | `Authorization: Bearer …` |
| `fal` | `https://api.fal.ai/v1/models` | `FAL_KEY` | `Authorization: Key …` |
| `replicate` | `https://api.replicate.com/v1/models` | `REPLICATE_API_TOKEN` | `Authorization: Bearer …` |

Artificial Analysis 使用明确的免费字段集合接口 `/free`。
其当前 OpenAPI 将不带 `/free` 的语言模型目录标为 Pro 及以上；不同订阅的密钥调用 `/free` 仍返回免费字段集合。
这些定义来自 [Artificial Analysis 官方 OpenAPI](https://artificialanalysis.ai/api/v2/openapi)。

fal 的目录说明目前写明认证可选，认证可提高限额；同一页面的自动生成请求头段仍标记认证头为必填。
仓库暂将此接口纳入禁用的凭据路径，记录这一差异，后续可以独立评估公开采集方式。
这一配置选择不代表 fal 所有目录请求都必须登录。[fal 官方目录说明](https://fal.ai/docs/platform-apis/v1/models)

## 记录范围

这八个适配器均产生 `kind: service`，ID 为 `<source>:service:<upstream_id>`。
它表示该来源目录在当前服务或观察范围内的条目，避免把平台限额、别名、可用性或聚合价格当作模型固有属性。
这里的 `service` 是当前注册表的范围分类，Artificial Analysis 条目本身不表示该站提供模型推理服务。
与独立模型身份的关联留空，后续需要有证据的显式映射。
Replicate 的上游身份是 `owner/name`；Gemini 保留 `models/…`，fal 保留完整 endpoint ID。

只保存明确列出的字段；未知字段和顶层空值省略，保留布尔值 `false` 和数值 `0`。
复合对象保留上游原值，其内部空值也保留。
字段名可以规范化，但字段值不转换、不补全、不根据型号字符串推断。
例如 Gemini 的 `inputTokenLimit` 以 `max_input_tokens` 保存，指针仍指向 `/models/0/inputTokenLimit`。

| 来源 | 保存的主要内容 | 官方契约 |
| --- | --- | --- |
| Artificial Analysis | slug、发布日期、创建方、价格、性能统计；省略评测分数 | [OpenAPI](https://artificialanalysis.ai/api/v2/openapi) |
| OpenAI | 所有者、创建时间、公布的关闭日期 | [OpenAI Docs：List models](https://developers.openai.com/api/reference/resources/models/methods/list) |
| Anthropic | 创建时间、能力对象、输入及输出上限、生命周期和计划退役日期 | [List Models](https://platform.claude.com/docs/en/api/models/list) |
| Gemini | 基础型号、版本、描述、输入及输出上限、支持的方法和采样参数 | [Models](https://ai.google.dev/api/models) |
| DeepSeek | 所有者、上下文及输出上限、模态、推理强度和协议能力 | [Lists Models](https://api-docs.deepseek.com/api/list-models/) |
| Cohere | 端点、特征、弃用状态、上下文、分词器地址和默认参数 | [List Models](https://docs.cohere.com/reference/list-models) |
| fal | 端点分类、描述、状态、标签、更新日期和模型 URL | [Model search](https://fal.ai/docs/platform-apis/v1/models) |
| Replicate | 所有者、描述、可见性、源码/论文/许可/权重链接和最新版本身份 | [HTTP API](https://replicate.com/docs/reference/http#models.list) |

DeepSeek 的 `api_capabilities.anthropic_messages.system_prompt_update` 可以保存 `leading-only` 与 `in-history` 等协议差异。
它留在该服务记录的能力对象内，具体含义以该快照对应的官方接口说明为准。[DeepSeek 目录定义](https://api-docs.deepseek.com/api/list-models/)

## 分页与失败处理

| 来源 | 响应字段 | 下一页请求 |
| --- | --- | --- |
| Artificial Analysis | `pagination.page`、`has_more`，可用时核对 `total_pages` | 增加 `page` |
| Anthropic | `has_more`、`last_id` | `after_id=last_id` |
| Gemini | `nextPageToken` | `pageToken=nextPageToken` |
| Cohere | `next_page_token` | `page_token=next_page_token` |
| fal | `has_more`、`next_cursor` | `cursor=next_cursor` |
| Replicate | `next` | 使用返回的绝对 HTTPS URL，上层验证同源 |
| OpenAI、DeepSeek | 当前公开契约无分页 | 完成当前响应后结束 |

修改游标时保留其余查询参数，并使用 URL 编码处理游标。
错误响应、缺失记录数组、空目录、缺失身份、重复身份，以及已出现但类型错误或自相矛盾的分页字段均报错。
空目录被视为需要检查的异常，避免失败请求覆盖现有数据。
可选游标缺失或为 `null` 表示结束；非空游标必须是非空白字符串。
`has_more=true` 时缺失游标会失败，不会返回一份被误认为完整的目录。

目录不保证跨页快照一致性，线上验证时仍需检查分页期间增删条目造成的重复或遗漏。
Anthropic 默认目录不包含已经退役的型号，完整历史还需要单独选择生命周期范围。[Anthropic 目录过滤定义](https://platform.claude.com/docs/en/api/models/list)

## 离线验证

[`tests/fixtures/gated/`](../tests/fixtures/gated/) 为八个来源各提供一份合成响应。
测试验证字段原值与 JSON Pointer 一致、服务范围、完整身份、分页参数保留、游标编码和异常分页拒绝。
后续真实认证采集仍需核对账户可见范围、接口返回变化、限额，以及来源数据的存储和再分发条件。

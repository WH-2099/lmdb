# 模型 API Mock 服务研究

[返回本轮索引](README.md) · [使用文档](../../docs/mock-server.md) · [项目决定](../../project/decisions.md)

核查日期：2026-10-19。
研究目标：提供独立运行的 Rust HTTP 服务，让客户端用少量会话测试原生请求、参数限制、错误、流式响应和工具续轮。
响应内容可以固定，协议结构、状态关系和触发条件需要可解释且可重放。
本轮使用公开文档、公开榜单及已有注册表快照，没有读取真实凭据、请求认证目录或执行模型推理。

## 证据与实现的关系

官方接口文档说明协议和已明确的模型限制，目录提供精确服务 ID 与来源字段，榜单确定首批研究对象。
三者保留独立作用域，聚合目录中的能力值不会自动成为官方接口契约。
内置配置保存来源 URL、核查日期、作用域和模型说明，见 [providers.yaml](../../data/mock/providers.yaml)。
排名快照保存分数、变体、原始指针、候选接口和缺口，见 [rankings.json](../../data/mock/rankings.json)。
合成的签名、使用量、错误和延迟用于测试客户端，不能将测试成功记录为真实模型的 `observed` 能力证据。
所有服务模型都按精确 API ID 选择，未知模型或接口不会被名称相似规则自动接纳。

## 排名与首批覆盖

首批使用 [Artificial Analysis 模型榜单](https://artificialanalysis.ai/leaderboards/models) 的 Intelligence Index，而非价格、速度或某个单项评测。
页面标示的指数版本为 v4.3.2，版本说明见 [Artificial Analysis Models](https://artificialanalysis.ai/models)。
抓取时间为 `2026-10-18T22:19:39.714983+00:00`，即北京时间 2026-10-19。
公开 HTML 的 SHA-256 为 `2c80829ddfacb37f0f3669d021e2a6519aff81bb18ea3b9cb8fbec126488e273`。
本轮从页面脚本中的 `initialModels` 提取 273 个条目，排除缺少数值分数及已标记退役的条目后保留 268 个排序候选。
前 20 名的名称和四舍五入显示分数与页面表格交叉核对。
公开页面提供 slug，未取得认证 API 中的稳定 UUID。

选择单位是保留 effort 和 fallback 标签的评测变体。
按原始数值分数降序排列；精确同分使用相同 competition rank，再按 slug 稳定排列以截取固定数量。
相同显示整数不构成同分依据，当前前 20 名没有精确同分。

| 基础模型候选 | 前 20 名中的位次 | 原生 API 模型 ID | 接口状态 |
| --- | --- | --- | --- |
| Claude Opus 5.5 | 1、3、5、14 | `claude-opus-5-5` | Anthropic Messages |
| Claude Sonnet 5.5 | 2、12 | `claude-sonnet-5-5` | Anthropic Messages |
| Muse Wonder Full | 4 | 未确认 | 官方声明没有公开 Model API |
| Claude Fable 5.1 | 6、7、15、20 | `claude-fable-5-1` | Anthropic Messages |
| GPT-6 Astra | 8、11、17、19 | `gpt-6-astra` | OpenAI Chat 文本、Responses |
| Gemini 4 Argon | 9 | `gemini-4-argon` | Gemini GenerateContent |
| Muse Wonder | 10 | `muse-wonder-1.0` | Meta Chat、Responses |
| GPT-6.1 Sol | 13、16、18 | `gpt-6.1-sol` | OpenAI Chat 文本、Responses |

这 20 个变体均为推理变体，映射到 8 个基础模型候选，其中 7 个具有接口。
配置包含 7 个原生模型 profile 和 13 个网关 profile，其中 OpenRouter 6 个、Vercel 7 个。
网关绑定引用目录中明确的基础模型关系，不能由斜线或型号字符串推断。
目录来源是 [models.dev 原始目录](https://models.dev/catalog.json?type=all)，每项绑定的快照与 JSON Pointer 保存在配置说明中。
20 个服务 profile 中，19 个保留 2026-10-15 的原字段证据，Vercel Claude Fable 的字段来自 2026-10-19 本地日期获取的另一份快照。
本轮对照经过摘要核验的原始响应检查了 137 个元数据字段值与指针，最近同步时间没有覆盖未变字段的旧证据时间。
基础型号名称匹配只能确定候选，全部 AA 评测变体的 effort、fallback 和实际 API 参数对应关系仍未验证。

Meta 的 [Models 文档](https://dev.meta.ai/docs/models) 明确区分可调用的 Muse Wonder 与没有公开 API 的 Muse Wonder Full。
因此保留第 4 名的接口缺口，不把榜单名称转换成虚构的 API ID。
AA 和目录的上下文数值也有差异，例如 Muse Wonder Full 的 1,000,000 与 1,048,576；两个来源分别保留，未静默选择一个值覆盖另一个。
快照保留后续排序候选，前 50 名的接口核查与 mock 行为扩展仍是独立待办。
仅查看 `mock rankings --top 50` 不意味着已经实现前 50 名。

## OpenAI Chat Completions 与 Responses

接口分别为 `POST /v1/chat/completions` 和 `POST /v1/responses`。
Chat 的 messages、tool_calls 与 Responses 的 input/output item、function_call_output 采用不同数据结构，不能只更换路径。
请求与响应字段依据 [Chat 参考](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create) 和 [Responses 参考](https://developers.openai.com/api/reference/resources/responses/methods/create)。
Chat 的 SSE 使用增量对象和 `[DONE]`，Responses 使用带类型、序号和输出项定位的事件，依据 [Responses streaming events](https://developers.openai.com/api/reference/resources/responses/streaming-events)。

函数调用的参数是编码后的 JSON 字符串，流式增量不保证每片可单独解析。
结果必须引用调用的 `call_id` 或 `tool_call_id`，同一请求可以发出多次调用。
工具模式的字段、strict 约束与续轮说明依据 [Function calling](https://developers.openai.com/api/docs/guides/function-calling)。
结构化输出需要区分正常符合 schema 的内容、拒绝和达到输出上限的未完成状态，依据 [Structured outputs](https://developers.openai.com/api/docs/guides/structured-outputs)。

当前模型限制使用精确模型集合和显式配置。
GPT-6 Astra 与 GPT-6.1 Sol 的工具调用使用 Responses；推理打开时的采样参数限制不能套用到所有旧型号。
具体限制依据 [Latest model](https://developers.openai.com/api/docs/guides/latest-model) 和 [Reasoning](https://developers.openai.com/api/docs/guides/reasoning)。
官方通用 Chat 工具示例曾使用 GPT-6 Astra，而模型专用说明明确要求 Responses；本轮采用更具体的模型说明，保留示例冲突。

响应状态可以通过同会话的 `previous_response_id` 继续，或显式回传必要输出项。
mock 保存自己的响应与不透明推理项，用于发现丢失、篡改和错误引用。
这些状态没有提供方的加密真实性。
HTTP 状态和错误分类参考 [Error codes](https://developers.openai.com/api/docs/guides/error-codes)，固定的限流序列参考 [Rate limits](https://developers.openai.com/api/docs/guides/rate-limits)。
精确错误文案、多个错误的优先级和全部输入联合类型尚未核验。
部分生成版 Responses 文档内容不完整，初版只实现已读到明确结构的 HTTP 文本、函数工具和状态路径。

## Anthropic Messages

原生接口为 `POST /v1/messages`，本轮实现 `anthropic-version: 2023-06-01`。
认证、版本和请求 ID 依据 [API overview](https://platform.claude.com/docs/en/api/overview)，请求及内容块依据 [Messages create](https://platform.claude.com/docs/en/api/messages/create)。
当前 overview 要求版本头，而生成版参数页将其标记为可选；mock 采用要求版本头的基线，真实行为留待探针。
当前文档支持 Bearer 认证及兼容的 `x-api-key`，mock 两者均可使用合成凭据。

工具调用是 assistant `tool_use` 内容块，工具结果是下一条 user 消息中的 `tool_result`。
结果按调用 ID 对应，并位于该消息的普通文本之前；连续 user 或 assistant 消息不构成普遍错误。
这些关系依据 [Implement tool use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/implement-tool-use) 和 [Handle tool calls](https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls)。
thinking、redacted_thinking 与签名在工具续轮中需要保持规定的内容和次序，依据 [Thinking](https://platform.claude.com/docs/en/build-with-claude/thinking)。
mock 只验证本会话生成的签名和内容。

SSE 使用消息开始、内容块开始/增量/结束、消息增量与消息结束事件。
工具参数增量采用 `input_json_delta`，thinking 的签名增量在对应内容块结束前出现，usage 增量是累计值。
流中可出现 ping，开始 HTTP 200 响应后也可能出现 error 事件，依据 [Streaming](https://platform.claude.com/docs/en/build-with-claude/streaming)。
HTTP 529 的 `overloaded_error` 与成功响应中的 refusal、max_tokens 等 stop reason 分别处理，依据 [Errors](https://platform.claude.com/docs/en/api/errors) 和 [Stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons)。

adaptive、手动 thinking budget、禁用 thinking、effort 和 assistant prefill 都有模型边界。
本轮按明确型号落实，依据 [Extended thinking](https://platform.claude.com/docs/en/build-with-claude/extended-thinking) 与 [Effort](https://platform.claude.com/docs/en/build-with-claude/effort)。
通用 messages 文字说 role 只有 user/assistant，但 effort 文档明确存在 beta 的 system 消息例外；不能据此禁止所有 system 消息。
`top_p=1` 的可用性在生成版参考与 thinking 说明间存在冲突，未将其升级为所有模型的确定性限制。
图像仅校验来源形状与已知媒体类型，像素解码、真实尺寸限制和 Files API 生命周期不在初版范围；来源为 [Vision](https://platform.claude.com/docs/en/build-with-claude/vision)。

## Gemini GenerateContent

本轮作用域是 Gemini Developer API `v1beta` 的 GenerateContent。
Vertex、Interactions、Live 和文件上传接口分别具有不同契约。
部分旧文档入口已经转向 Interactions，不能使用其字段说明解释 GenerateContent。

`systemInstruction.parts` 在当前原生接口中只允许文本，普通 `contents` 的多模态能力不会产生 system 图像例外。
请求中的 `contents`、`functionCall`、`functionResponse` 和响应的 candidates、finishReason、usageMetadata 依据 [GenerateContent REST 参考](https://ai.google.dev/api/generate-content#body.request_body.FIELDS.system_instruction)。
用户最初提出的 system prompt 图片问题因此作为需要核查的范围问题处理，未虚构某个新型号例外。
现有人工声明 `gemini-generate-system-image` 与此接口范围一致。

thinkingLevel 与 thinkingBudget 的支持依赖具体模型。
例如 Gemini 2.5 Pro 不能通过 budget 0 禁用 thinking，而 Flash 可以；Gemini 4 Argon 对 level 和 budget 同时出现的限制也不能任意外推到全部 Gemini 3。
依据 [GenerateContent thinking](https://ai.google.dev/gemini-api/docs/generate-content/thinking#thinking-budgets)。

当前轮中，每一步函数调用的第一个 functionCall part 需要保持规定的 thoughtSignature。
并行调用和对应结果需要保持分组，不能重排为单个调用与结果交错的序列。
较早轮次和非函数签名的校验规则不同，不能因缺少任意签名就普遍拒绝。
依据 [Thought signatures](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures#signatures-in-function-calling-parts)。
文本流末尾可以出现空文本和签名，客户端需要保留该 part。

`streamGenerateContent?alt=sse` 返回逐条 GenerateContentResponse，依据 [Streaming responses](https://ai.google.dev/gemini-api/docs/generate-content/text-generation#streaming-responses)。
未发现本接口使用 OpenAI `[DONE]` 或 Responses 事件序列的依据。
原生错误采用 `error.code`、`message`、`status` 及可选 details，依据 [GenerateContent API errors](https://ai.google.dev/gemini-api/docs/generate-content/api-errors#error-response-format)。
相同 `RESOURCE_EXHAUSTED` 可用于余额不足的 402 与临时限流的 429，重试策略不能只看 status 字符串。
Retry-After 是否出现没有普遍保证，mock 中的重试信息属于显式合成场景。
缓存模型归属和文本 systemInstruction 依据 [CachedContent 参考](https://ai.google.dev/api/caching#CachedContent)，完整缓存生命周期另列待办。

## Meta 与其他提供方

Meta Muse Wonder 采用 `muse-wonder-1.0`，本轮实现 Chat 和 Responses 入口。
模型可用性依据 [Meta Models](https://dev.meta.ai/docs/models)，字段依据 [Chat Completions](https://dev.meta.ai/docs/protocols/chat-completions) 和 [Responses](https://dev.meta.ai/docs/protocols/responses)。
这些协议页仍有较早的 Spark 型号文字，使用新的模型目录确认 Wonder 的接口可用性，不把旧示例型号当作唯一支持集合。
reasoning effort 的允许值和禁用限制依据 [Meta reasoning](https://ai.developer.meta.com/docs/reasoning/)。
Meta 原生 Messages 的差异尚未实现，不能把 Anthropic 适配器直接作为 Meta Messages 的验证结果。

其他提供方的资料用于保留兼容协议上的差异，本轮没有为它们默认填充模型。
需要通过完整自定义配置选择有证据的模型，并按协议补齐测试。

| 提供方 | 一手来源 | 本轮关注范围 |
| --- | --- | --- |
| DeepSeek | [Chat](https://api-docs.deepseek.com/api/create-chat-completion/)、[Thinking mode](https://api-docs.deepseek.com/guides/thinking_mode) | reasoning 内容、工具历史、参数及输出差异 |
| Kimi | [Overview](https://platform.kimi.ai/docs/overview)、[Chat](https://platform.kimi.ai/docs/api/chat) | Chat 请求与工具模式；Chat 抓取内容不完整 |
| Z.AI | [Chat completion](https://docs.z.ai/api-reference/llm/chat-completion) | thinking、模型参数与停止状态 |
| MiniMax | [OpenAI compatibility](https://platform.minimax.io/docs/api-reference/text-openai-api) | 支持与忽略字段、reasoning 输出 |
| Alibaba | [Qwen OpenAI Chat](https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-openai-chat-completions) | 兼容路径、thinking 及流式限制；抓取内容不完整 |
| Mistral | [Chat endpoint](https://docs.mistral.ai/api/endpoint/chat) | Chat 原生参数及工具结构 |
| Cohere | [Compatibility API](https://docs.cohere.com/docs/compatibility-api) | OpenAI 兼容入口，未覆盖原生 Chat v2 |
| xAI | [Responses](https://docs.x.ai/developers/rest-api-reference/inference/responses) | Responses 接口，Chat 参考抓取失败 |
| OpenRouter | [Overview](https://openrouter.ai/docs/api_reference/overview)、[Errors](https://openrouter.ai/docs/api_reference/errors-and-debugging) | 网关字段、流式错误及 usage 位置 |
| Vercel | [OpenAI Chat Completions](https://vercel.com/docs/ai-gateway/sdks-and-apis/openai-chat-completions) | 兼容接口与网关模型 ID |

资料不完整的提供方保留未知项，公开页面可读也不代表进行了真实 API 验证。

## 覆盖与复现决定

服务使用正常模式、显式场景和有 seed 的覆盖顺序。
脚本能安排限流、过载、成功、工具调用、拒绝、截断和流中断，让少量会话访问多个分支。
请求前提不满足的场景记录为跳过，覆盖报告保留已覆盖、跳过和待执行状态。
协议校验失败保留脚本位置，已选中的合成 HTTP 错误推进位置，以便测试重试。
同会话的工具结果续轮保持主序列位置，避免把续轮误当成下一项随机故障。
字节分片和 Unicode 文本用于验证客户端的 SSE 与 UTF-8 增量解码。

结构化输出使用 JSON Schema 校验器验证最终值，自动样本生成有明确资源上限。
不访问外部 schema URL，无法生成时允许脚本提供通过校验的显式值。
HTTP `501` 明确标示未实现的 mock 功能，与真实模型确定拒绝某个参数的 `400` 分开表达。
实现支持的具体场景、命令和状态规则见[使用文档](../../docs/mock-server.md)，研究建议不替代当前代码行为。

## 后续验证

- [ ] 按同一榜单口径扩展前 50 名，逐条确认原生模型 ID、接口和缺口。
- [ ] 核查 AA effort、fallback 与实际请求参数的对应关系，保留无法确认的映射。
- [ ] 为真实 token 探针设计独立记录：提供方、接口、模型、参数、时间、脱敏请求/响应、费用和环境。
- [ ] 在明确授权与预算后逐变量探测认证、参数边界、被忽略字段、错误优先级与模型可用性。
- [ ] 采集真实 SSE 事件和工具多轮历史，核对 usage 位置、签名保留、断流及错误后的生命周期。
- [ ] 单独核验媒体大小、tokenizer、上下文计量、schema 子集、账户范围和限流行为。
- [ ] 按需求扩展托管工具、Files、缓存、后台响应、Meta Messages 和专用媒体接口。

本轮不执行这些真实 token 探针。
mock 的测试结果只说明当前合成协议实现符合已写断言，不能替代上游实测或性能评测。

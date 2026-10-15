# 数据来源访问采样

核查日期：2026-10-15。
返回[来源目录](../sources/catalog.md)或[本轮索引](../README.md)。

采样通过 Python 标准库执行普通只读 HTTPS 请求，保留环境代理与 TLS 校验，没有提供账号凭据。
只请求元数据、文档与 OpenAPI，没有下载模型权重、执行远端代码或调用推理。
完整公开响应仅放在临时目录 `/tmp/lmdb-research/sources/`，本报告保存请求与观察结果。
临时文件可能被清理，下面的记录不依赖它们继续存在。

## 成功与失败记录

除 ModelScope 外，下列时间为请求开始时间，均为 UTC。
这些数目仅描述当次响应，不能视为平台持续承诺或全量覆盖证明。

| 来源 | 请求 URL | UTC 时间 | 状态 | 响应观察 |
| --- | --- | --- | --- | --- |
| OpenRouter models | `https://openrouter.ai/api/v1/models?output_modalities=all&limit=2` | 07:34:59.265 | 200 | `data` 2 条，`total_count=669`，`links.next` 指向 offset=2/limit=2 |
| OpenRouter providers | `https://openrouter.ai/api/v1/providers` | 07:34:59.267 | 200 | `data` 数组 112 条 |
| OpenRouter endpoints | `https://openrouter.ai/api/v1/models/openai/gpt-5.4/endpoints` | 07:34:59.268 | 200 | `data` 为单模型对象，内部 `endpoints` 7 条 |
| HF models | `https://huggingface.co/api/models?limit=2&full=true` | 07:34:59.269 | 200 | 顶层数组 2 条，HTTP Link 提供 next cursor |
| HF detail | `https://huggingface.co/api/models/Qwen/Qwen3-8B` | 07:34:59.677 | 200 | 单对象，含 cardData/config/safetensors/sha |
| HF OpenAPI | `https://huggingface.co/.well-known/openapi.json` | 07:34:59.718 | 200 | JSON，1,166,767 bytes；没有发现上述两条 GET 列表/详情路径 |
| AA OpenAPI | `https://artificialanalysis.ai/api/v2/openapi` | 07:34:59.804 | 200 | `text/yaml`，206,396 bytes，OpenAPI 3.1.0 |
| AA language models | `https://artificialanalysis.ai/api/v2/language/models` | 07:34:59.991 | 401 | 匿名请求 Unauthorized，未重试凭据 |
| ModelScope detail | `https://modelscope.cn/api/v1/models/Qwen/Qwen3-8B` | 07:36:49.403（读取完成） | 200 | JSON envelope，`Code=200`、`Success=true`、`Data` 单对象 |

AA OpenAPI 初次按 JSON 解析失败，检查 Content-Type 和正文后确认为有效 YAML。
这是采样程序的格式假设错误，不是 API 服务故障。

## 响应头与摘要

| 来源 | 实际响应头或指纹 |
| --- | --- |
| HF models | ETag `W/"d86-2fgUgtsPWRQNbn9FoD7mgvb3QSI"`；RateLimit `"api";r=499;t=5`；RateLimit-Policy `"fixed window";"api";q=500;w=300` |
| HF detail | 返回 ETag；RateLimit 中 remaining 为 498，API 配额为 `500/300s`；响应内容的 SHA-256 见下表 |
| HF OpenAPI | ETag `W/"11cdaf-Wb6uBmMqTvCVghtPdm5RMXEt0eA"`；此次归入 media 配额 `q=10000;w=300` |
| OpenRouter / ModelScope | 采样记录的 Content-Type 为 JSON；本轮没有记录到 ETag、Last-Modified 或 RateLimit 头，不能据此推断没有缓存或限流 |

HF detail 对应仓库 SHA 为 `b968826d9c46dd6066d109eabc6255188de91218`。
HF 返回的 `config` 只是 API 暴露的配置子集，不能假定它等于仓库中的完整 `config.json`。

| 保存对象 | SHA-256 |
| --- | --- |
| OpenRouter models | `6932d7f7e9c12d715d7c6847be6dff81f2f5b5f4c6fc3e293913a4f36b0d106b` |
| OpenRouter providers | `f664bde3de0a0d095be83740d543b647f278d1102166b55aadc9db37e6c13b6a` |
| OpenRouter endpoints | `1b0c2e86f7a42b78c57a516d2b4fd5995cc703906f438abeaec71aee76d14cc9` |
| HF models | `7ab694f8adf8a3e1c532a5880f6f8a3fda7bfa6d3691f7c092929bb59a6dc7f9` |
| HF detail | `4952bf4fe526bf1e1ba7728dc403e0f5fc6e956cbd38cbdd1c6fc7d16c11463f` |
| HF OpenAPI | `f0289b053fadbc9ff26ea961da4715fa63f58c503ce90d6ccf4f468f4424458d` |
| AA OpenAPI | `e66daecd874b80cf692429fc6b908d6e520d7cececde807f5ff108d23f735e33` |
| ModelScope detail | `f0f020c183d0bc6ef04e23bcfba1ae8236344074d3d9a223a9c746e2d266fcb8` |

## 文档与实际接口差异

### OpenRouter 鉴权

[模型列表](https://openrouter.ai/docs/api/api-reference/models/list-all-models-and-their-properties)和[提供方列表](https://openrouter.ai/docs/api/api-reference/providers/list-providers)文档授权栏描述 Bearer key。
本次不带凭据的三个元数据接口均返回 200。
结论仅为“本次匿名请求成功”；不推断官方长期匿名访问承诺，也不据此绕过未来的鉴权要求。

### Artificial Analysis 三份官方资料

| 资料 | 当次内容 | 可以得出的结论 |
| --- | --- | --- |
| [原 API Reference](https://artificialanalysis.ai/api-reference) | `/api/v2/data/llms/models`，免费 1000 次/天 | 用户提供的旧入口仍可读；旧业务接口是否可用未测 |
| [Data API docs](https://artificialanalysis.ai/data-api/docs) | 示例 `/api/v2/language/models`；Free 100、Pro 500 次/24h；provider 数据 Commercial | 概览给出当前套餐和限流线索，但示例路径未充分说明套餐区别 |
| [实时 OpenAPI](https://artificialanalysis.ai/api/v2/openapi) | `/language/models/free` 适用于任意有效 key；`/language/models` 为 Pro+；providers Commercial | 接入应按具体路径验证契约与权限，不能只读概览 |

实时规范列出的免费模型形状明确排除 context window、parameters、modalities、licensing、完整评测、percentiles 和 provider detail。
规范的列表 envelope 包含 `tier`、`pagination` 和 `data`，分页样例包含 page/page_size/total_pages/has_more。
没有带账号访问，因此套餐实际权限、字段完整性和新旧接口并行情况仍待核实。
官方新概览明确要求各套餐署名，且再分发权限需联系团队；本轮没有作出再分发许可结论。

## 文档读取的访问限制

- Hugging Face API 和模型卡页面通过 Anysearch 首次提取失败，随后通过另一文档读取工具取得正文；API 实际采样成功。
- Anthropic 文档首次提取被导向区域限制页，随后通过另一文档读取工具取得正文；未测试业务 API。
- Gemini Models 页面首次提取失败，随后通过另一文档读取工具取得正文；未测试业务 API。
- ModelScope 文档首页提取失败，实际单模型公开 API 成功；源码可读，全局枚举契约尚未验证。
- 阿里云 List models 页面提取失败，模型总览可读；不据搜索摘要断言目录 API 的字段或授权行为。

没有进行全量分页、带凭据请求、负载测试或模型推理。
一次访问结果不能证明长期可用性，来源可达性、权限与字段变化需要在正式采集设计中继续验证。

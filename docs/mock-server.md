# 模型 API Mock 服务

[返回首页](../README.md) · [协议研究与证据](../research/2026-10-19/mock-server.md)

`lmdb mock serve` 启动独立运行的 Rust HTTP 服务，用确定性响应测试模型客户端。
客户端将 API 地址指向本地服务，并使用合成凭据，即可检查请求参数、错误处理、SSE 解析、工具调用与多轮状态。
响应文本没有语义保证，使用量、签名、延迟和错误场景均为测试数据。
服务不调用上游模型，也不读取上游 API 凭据。
默认提供方配置和排名快照编译进二进制，运行服务不需要注册表目录或联网。

## 启动与连接

```text
cargo build --release --locked
target/release/lmdb mock list
target/release/lmdb mock rankings --top 20
target/release/lmdb mock validate
target/release/lmdb mock serve --provider openai --model gpt-6.1-sol --port 8080
```

启动后第一行 stdout 是包含实际监听地址、提供方和 seed 的 JSON。
`--port 0` 由系统分配空闲端口，适合并行测试。
默认监听 `127.0.0.1:8080`，凭据为 `mock-key`。
`--api-key local-test-key` 可替换合成凭据，`--no-auth` 可关闭认证，两者互斥。
`--model` 支持重复传入或逗号分隔，按配置中的 API 原始 ID 精确选择。
省略 `--model` 时提供所选提供方配置中的全部模型。

另一个终端发送 Responses 请求：

```sh
curl http://127.0.0.1:8080/v1/responses \
  -H 'Authorization: Bearer mock-key' \
  -H 'Content-Type: application/json' \
  -H 'x-mock-session: quickstart' \
  -d '{"model":"gpt-6.1-sol","input":"hello"}'
```

客户端应保留所选 SDK 原有的请求格式，仅更改 API 地址、模型 ID 和测试凭据。
OpenAI SDK 的 base URL 为 `http://127.0.0.1:8080/v1`。
Anthropic SDK 的 base URL 为 `http://127.0.0.1:8080`；发送请求时保留 `anthropic-version: 2023-06-01`。
Gemini 请求使用本地 `/v1beta/models/{model}:generateContent`，模型 ID 位于 URL 中。
`--base-path /test` 为 API 和管理路径统一添加 `/test` 前缀。
每个进程服务一个提供方，需要同时测试多个提供方时使用不同端口。

## 提供方与接口

| `--provider` | 已配置接口 | 默认模型范围 |
| --- | --- | --- |
| `openai` | `/v1/chat/completions`、`/v1/responses` | `gpt-6-astra`、`gpt-6.1-sol` |
| `anthropic` | `/v1/messages` | `claude-opus-5-5`、`claude-sonnet-5-5`、`claude-fable-5-1` |
| `google`，别名 `gemini` | `/v1beta/models/{model}:generateContent`、`:streamGenerateContent?alt=sse` | `gemini-4-argon` |
| `meta` | `/v1/chat/completions`、`/v1/responses` | `muse-wonder-1.0` |
| `openrouter`、`vercel` | `/v1/chat/completions` | 已映射的网关模型 ID，使用 `mock list --provider ID` 查看 |
| `deepseek`、`moonshotai`、`zhipuai`、`minimax`、`alibaba`、`mistral`、`cohere`、`xai` | 配置声明的兼容协议及部分提供方差异 | 当前没有默认模型，需提供完整自定义配置 |

提供方别名还包括 `kimi` → `moonshotai`、`zai` → `zhipuai`、`qwen` → `alibaba`。
同一基础模型在原生 API 和网关中可使用不同 ID，不能直接互换。
`mock list` 返回模型能力、协议、来源和未确认项；缺失能力值表示未知。
已提供兼容协议处理代码不代表完成该提供方全部接口，也不代表默认收录了其模型。

首批排名依据 2026-10-19 核查的 Artificial Analysis Intelligence Index v4.3.2 快照。
前 20 名是 20 个评测变体，涉及 8 个基础模型候选；其中 7 个基础模型具有已映射接口。
Muse Wonder Full 没有已确认的公开 Model API，保留为覆盖缺口。
排名中的 effort、fallback 与可调用 API 的精确参数对应关系仍未验证。
`mock rankings --top 50` 可查看后续候选，但 21–50 名的接口与行为覆盖仍在待办中。
完整口径、来源与冲突见[研究记录](../research/2026-10-19/mock-server.md#排名与首批覆盖)。

## 原生请求示例

以下示例分别连接相应提供方启动的服务。
Anthropic Messages 请求：

```sh
curl http://127.0.0.1:8080/v1/messages \
  -H 'x-api-key: mock-key' \
  -H 'anthropic-version: 2023-06-01' \
  -H 'Content-Type: application/json' \
  -H 'x-mock-session: claude-demo' \
  -d '{"model":"claude-opus-5-5","max_tokens":256,"messages":[{"role":"user","content":"hello"}]}'
```

Gemini GenerateContent 请求：

```sh
curl http://127.0.0.1:8080/v1beta/models/gemini-4-argon:generateContent \
  -H 'x-goog-api-key: mock-key' \
  -H 'Content-Type: application/json' \
  -H 'x-mock-session: gemini-demo' \
  -d '{"systemInstruction":{"parts":[{"text":"Be concise."}]},"contents":[{"role":"user","parts":[{"text":"hello"}]}]}'
```

当前 Gemini GenerateContent 的 `systemInstruction.parts` 只接受文本。
普通 `contents` 中的图像与 system instruction 是两个不同作用域；没有文档依据的模型例外不会写入配置。
图像、文件和视频输入只校验已实现的结构与配置能力，不下载 URL 或执行媒体理解。

## 流式响应

```sh
curl -N http://127.0.0.1:8080/v1/responses \
  -H 'Authorization: Bearer mock-key' \
  -H 'Content-Type: application/json' \
  -H 'x-mock-session: stream-demo' \
  -d '{"model":"gpt-6.1-sol","input":"hello","stream":true}'
```

Chat Completions 使用 `data:` JSON 分片及 `[DONE]`。
Responses 使用有事件类型的 SSE，包括文本增量、函数参数增量和响应结束状态。
Anthropic 使用 `message_start`、内容块事件、`message_delta` 和 `message_stop`，没有 `[DONE]`。
Gemini 使用 `:streamGenerateContent?alt=sse`，每个 `data:` 是 GenerateContentResponse，也没有 `[DONE]`。
客户端需要拼接工具参数增量后再解析 JSON，并保留可能为空但携带签名的内容块。

启动时可用 `--delay-ms 100 --chunk-delay-ms 20 --chunk-bytes 1` 检查等待与增量解码。
`--delay-ms` 控制响应开始前的延迟，`--chunk-delay-ms` 控制输出片段之间的延迟。
`--chunk-bytes 0` 保留 SSE 帧边界，正整数按字节分割，可能切开 UTF-8 字符和 SSE 行，也可能跨越事件边界。
每个输出片段之前都应用 `--chunk-delay-ms`，包括第一个片段。
网络栈仍可能合并相邻写入，客户端不能依赖 HTTP 数据块恰好等于一个事件。
这些参数是可重复的测试条件，不是对真实模型响应速度的估计。

## 会话、场景和覆盖记录

`x-mock-session` 为多轮请求指定同一个会话。
省略此头时使用共享的 `default` 会话；显式 ID 允许 1–128 个 ASCII 字母、数字、点、下划线或连字符。
不同会话的脚本进度、工具历史和覆盖记录互相独立。
同一会话不接受同时进行的两个请求，应为并行测试分配不同会话 ID。
`x-mock-scenario` 可在单个请求上指定场景，例如 `rate_limit`、`parallel_tools` 或 `disconnect`。

```sh
curl http://127.0.0.1:8080/v1/responses \
  -H 'Authorization: Bearer mock-key' \
  -H 'Content-Type: application/json' \
  -H 'x-mock-session: retry-demo' \
  -H 'x-mock-scenario: rate_limit' \
  -d '{"model":"gpt-6.1-sol","input":"hello"}'
```

此请求固定产生限流场景。
后续成功请求应移除 `x-mock-scenario`；保留该头会再次选择同一错误。
要测试自动重试直到成功，可使用下文的顺序脚本。

`--suite normal` 是默认模式，使用普通文本响应和请求要求的工具或结构化输出。
`--suite coverage --seed 7` 为每个会话安排确定性的场景顺序。
顺序会重复，适用条件不满足的场景记录为 `skipped`，不会假记为已覆盖。
工具结果续轮自动返回文本，不推进主序列；显式场景覆盖也不推进主序列。
请求校验失败保留当前步骤，脚本选中的 HTTP 错误则消耗一个步骤，使重试能继续。
相同配置、seed、会话 ID 与请求序列可重放合成响应和生成的标识。
会话过期、重置和并发调度会影响可复现的前提。
报告中的 `accepted` 表示响应已生成并准备发送，不表示客户端收到了全部字节。
客户端在此后中断读取不会回滚脚本或计入接受前的 `canceled`；连接结束后会释放请求占用。

```text
target/release/lmdb mock scenarios
```

| 场景 | 用途或前提 |
| --- | --- |
| `text`、`empty` | 普通文本与空内容 |
| `tool_call`、`parallel_tools` | 需要函数定义和允许工具调用，后续回传工具结果 |
| `malformed_tool_arguments` | 模拟工具参数格式或模式错误，具体表示取决于协议 |
| `structured` | 需要输出 JSON Schema，生成或校验符合模式的测试值 |
| `reasoning` | 需要模型与当前请求支持推理，检查摘要及不透明状态 |
| `refusal`、`truncated` | 原生拒绝、长度限制或未完成状态 |
| `stop_sequence` | 需要请求中的停止序列 |
| `rate_limit`、`overloaded`、`server_error` | 检查原生错误封装与重试 |
| `unauthorized`、`forbidden`、`not_found`、`context_overflow` | 合成认证、权限、资源和上下文错误 |
| `stream_error`、`disconnect`、`unknown_event` | 需要流式请求，检查开始响应后的错误、提前结束和额外事件 |
| `slow`、`cache_miss` | 延迟与缓存缺失场景；不提供真实缓存生命周期 |

一个仅发送文本的会话无法覆盖工具、结构化输出和流式场景。
下方提供两个 Gemini 会话覆盖全部 22 个场景的可重复示例。
某些模型禁止这些功能组合，例如当前 OpenAI 首批模型的工具调用需要 Responses API。
应按照模型协议构造请求，不能为了运行覆盖序列随意加入参数。

管理接口使用同一测试凭据：

```sh
curl http://127.0.0.1:8080/__mock \
  -H 'Authorization: Bearer mock-key'
curl http://127.0.0.1:8080/__mock/sessions/retry-demo \
  -H 'Authorization: Bearer mock-key'
curl -X DELETE http://127.0.0.1:8080/__mock/sessions/retry-demo \
  -H 'Authorization: Bearer mock-key'
```

`GET /__mock` 查看服务报告，`GET /__mock/scenarios` 查看场景前提，`GET /__mock/models` 查看所选模型。
`GET /__mock/sessions/{id}` 查看会话进度、拒绝、取消、每项覆盖状态和有限长度的请求记录。
`DELETE /__mock/sessions/{id}` 重置会话；正在处理请求的会话不能重置。
管理接口属于本地 mock 控制面，不是上游 API 的一部分。
`GET /health` 无需认证，原生模型列表由 `/v1/models` 或 Google 的 `/v1beta/models` 提供。
响应携带 `x-mock-simulated: true`、`x-mock-session`、`x-mock-request-id`、`x-mock-scenario` 和 `x-mock-step`，便于关联日志。

## 两个会话覆盖全部场景

以下请求与 [HTTP 回归测试](../tests/mock_http.rs) 中的 `two_native_gemini_sessions_cover_every_scenario` 一致。
启动新服务，固定 seed；重复运行时重新启动服务或重置这两个会话：

```text
target/release/lmdb mock serve --provider google --suite coverage --seed 42 --port 8080
```

将下面的工具、推理与停止序列请求保存为 `/tmp/lmdb-coverage-tools.json`：

```json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "tools": [{"functionDeclarations": [
    {"name": "weather", "parameters": {"type": "OBJECT", "properties": {}}},
    {"name": "clock", "parameters": {"type": "OBJECT", "properties": {}}}
  ]}],
  "generationConfig": {
    "thinkingConfig": {"thinkingLevel": "HIGH", "includeThoughts": true},
    "stopSequences": ["STOP"],
    "maxOutputTokens": 1024
  }
}
```

将结构化输出请求保存为 `/tmp/lmdb-coverage-structured.json`：

```json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "generationConfig": {
    "responseMimeType": "application/json",
    "responseJsonSchema": {
      "type": "object",
      "properties": {"ready": {"type": "boolean"}},
      "required": ["ready"],
      "additionalProperties": false
    }
  }
}
```

分别发送 22 次请求，共 44 次原生 HTTP 调用。
`disconnect` 会让 curl 以非零状态退出，循环继续读取后续场景，最终以报告核对覆盖结果。

```sh
for mock_attempt in $(seq 1 22); do
  curl -sS --max-time 5 -o /dev/null \
    'http://127.0.0.1:8080/v1beta/models/gemini-4-argon:streamGenerateContent?alt=sse' \
    -H 'x-goog-api-key: mock-key' -H 'Content-Type: application/json' \
    -H 'x-mock-session: coverage-tools' \
    -d @/tmp/lmdb-coverage-tools.json || true
done
for mock_attempt in $(seq 1 22); do
  curl -sS --max-time 5 -o /dev/null \
    http://127.0.0.1:8080/v1beta/models/gemini-4-argon:generateContent \
    -H 'x-goog-api-key: mock-key' -H 'Content-Type: application/json' \
    -H 'x-mock-session: coverage-structured' \
    -d @/tmp/lmdb-coverage-structured.json || true
done
curl -fsS http://127.0.0.1:8080/__mock -H 'x-goog-api-key: mock-key' \
  | jaq -e '
    [.sessions[] | select(.session == "coverage-tools" or .session == "coverage-structured")] as $sessions
    | ([$sessions[].cases[] | select(.covered > 0) | .case] | unique) as $covered
    | {sessions: [$sessions[] | {session, accepted, rejected}], covered: ($covered | length),
       complete: (($sessions | length) == 2 and all($sessions[]; .accepted == 22 and .rejected == 0) and ($covered | length) == 22)}
    | if .complete then . else error("coverage incomplete") end'
```

预期两条会话记录均为 `accepted: 22`、`rejected: 0`，合并覆盖 `covered: 22` 且 `complete: true`。
工具会话中跳过的结构化场景由第二个会话补齐，单个会话不要求每项都覆盖。
这些请求测试场景选择与原生输出分支；工具结果的连续回传使用下一节的续轮方法。

## 工具调用续轮

以 OpenAI Responses 为例，先触发一个函数调用并保存响应：

```sh
curl http://127.0.0.1:8080/v1/responses \
  -H 'Authorization: Bearer mock-key' \
  -H 'Content-Type: application/json' \
  -H 'x-mock-session: tool-demo' \
  -H 'x-mock-scenario: tool_call' \
  -d '{"model":"gpt-6.1-sol","input":"Check status.","tools":[{"type":"function","name":"get_status","parameters":{"type":"object","properties":{},"additionalProperties":false}}]}' \
  -o /tmp/lmdb-mock-tool-response.json
```

从响应中取得真实生成的 ID，提交函数结果：

```sh
jaq -n --slurpfile response /tmp/lmdb-mock-tool-response.json \
  '{model:"gpt-6.1-sol",previous_response_id:$response[0].id,input:[$response[0].output[]|select(.type=="function_call")|{type:"function_call_output",call_id:.call_id,output:"{\"status\":\"ok\"}"}]}' \
  | curl http://127.0.0.1:8080/v1/responses \
      -H 'Authorization: Bearer mock-key' \
      -H 'Content-Type: application/json' \
      -H 'x-mock-session: tool-demo' \
      -d @-
```

两个请求使用相同会话，续轮移除 `x-mock-scenario`。
`previous_response_id` 只能引用本会话保存的响应；`store:false` 的响应不能用于该方式。
也可显式回传完整的输出项和函数结果。
Chat Completions 使用 assistant `tool_calls` 和 role `tool` 的 `tool_call_id`；Responses 使用 `function_call`、`call_id` 和 `function_call_output`。
Anthropic 回传 assistant 内容块与紧随其后的 user `tool_result`，保持 thinking 块、签名和工具 ID。
Gemini 将同一步的 `functionCall` 放在一组，并按组回传 `functionResponse`，保留当前轮所需的 `thoughtSignature`。
服务可核对自身签发的状态、调用 ID 与参数，但不能验证真实提供方的加密签名。

## 自定义配置与顺序脚本

`--config PATH` 接受 JSON 或 YAML，完整替换内置配置。
可先运行 `mock list` 将完整默认配置保存为文件，再按需要编辑。
配置字段由 [mock-config JSON Schema](../schemas/mock-config.schema.json) 描述，并由 `mock validate --config PATH` 补充语义检查。

以下 `mock.yaml` 定义一个明确为合成模型的接口和三步重试脚本：

```yaml
schema_version: 1
sources: []
providers:
  - id: local
    protocols: [openai_chat]
    models:
      - id: synthetic-text
        protocols: [openai_chat]
        input_modalities: [text]
        output_modalities: [text]
        tool_call: true
        structured_output: true
script:
  repeat: false
  steps:
    - scenario: rate_limit
    - scenario: overloaded
    - scenario: text
      text: "Recovered ✓"
```

```text
target/release/lmdb mock validate --config mock.yaml
target/release/lmdb mock serve --provider local --config mock.yaml
```

以相同 `x-mock-session` 发送合法 Chat Completions 请求，可依次得到限流、过载和成功响应。
脚本默认不重复，耗尽后返回普通文本；`repeat:true` 循环执行。
每步可设置 `text`、结构化值 `output`、工具参数对象 `tool_arguments`、`delay_ms` 和 `chunk_delay_ms`。
显式 `output` 与 `tool_arguments` 必须符合请求中的 JSON Schema。
自动样本生成受到资源限制，无法生成符合模式的值时返回 `501`，可在脚本中提供通过校验的值。
外部 `$ref` 禁用，可使用本地 `#/$defs` 引用。

配置文件上限为 4 MiB、4,096 个模型和 256 个脚本步骤。
不接受重复键、未知配置字段、多份 YAML 文档、显式标签、锚点、别名和 merge key。
模型协议必须属于提供方声明的协议，来源引用、日期、HTTPS URL、ID 和别名冲突均会校验。
新增真实模型时应补充有作用域的证据，不能只依据名称推断兼容性。

## 资源边界与实现范围

| 参数 | 默认值 | 含义 |
| --- | ---: | --- |
| `--max-body-bytes` | 2,097,152 | 单个请求体上限 |
| `--max-sessions` | 1,024 | 同时保留的会话数 |
| `--session-ttl-secs` | 1,800 | 闲置会话保存时长 |
| `--max-inflight` | 64 | 同时处理的请求数 |
| `--request-timeout-ms` | 10,000 | 读取请求体的截止时间，与合成响应延迟独立 |

会话历史和 schema 处理另有内部上限，报告会列出会话状态限制。
服务重启后会话丢失。
输入校验失败按所选协议返回错误；明确未实现的功能返回 `501`，这表示 mock 的覆盖边界，不能当作真实提供方拒绝该功能的证据。
包括托管工具执行、真实音视频输出、文件上传生命周期、后台任务与部分专用内容类型在内的能力尚未覆盖。
Gemini Live、Interactions、Vertex 接口及缓存资源管理，Meta Messages 协议，以及提供方账户、权限和费用行为都需要单独实现或核验。
未知参数与组合不保证已获得完整验证。
上下文溢出场景可以主动触发，但服务不使用提供方真实 tokenizer，也不按输入准确计算上下文消耗。
`x-mock-input-tokens` 接受无符号整数，可显式提供合成输入数量，与请求中的输出上限相加后检查配置的上下文上限。
该头用于边界测试，不会把输入 usage 估算变成真实计量。
输出预算使用一个 Unicode 标量计作一个 mock token 的规则，计入可见文本、合成 thinking 和序列化工具参数，排除签名等不透明元数据。
达到请求或模型的输出上限时，文本按预算截断并返回原生长度结束状态。
Anthropic 与 Gemini 的工具参数对象放不进剩余预算时整项省略，OpenAI 的字符串参数可被截断。
这些规则保证测试值与合成预算一致，不代表提供方实际分词或计费方式。

真实 API 探针和前 50 名扩展列在[路线与进度](../project/roadmap.md)。
探针需要另行设计凭据、预算、脱敏记录和逐变量请求，本轮没有执行真实推理。

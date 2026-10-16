# 官方页面与字段核对

[项目首页](../README.md) · [页面清单](../config/official-pages.yaml) · [数据模型](data-model.md) · [维护流程](maintenance.md)

官方页面采集保存厂商发布博客、模型卡、模型详情页和文档的 URL、关键声明与原文证据。
页面与现有模型记录显式绑定，提取的声明可以与同一记录的目录事实交叉核对。
目录事实、页面声明和人工条件规则各自保留来源，差异交由维护者审查。

## 使用

```text
cargo run --release --locked -- sync --source official
cargo run --release --locked -- documents sync --vendor qwen
cargo run --release --locked -- documents list --record huggingface:model:Qwen/Qwen3-8B
cargo run --release --locked -- documents check --record huggingface:model:Qwen/Qwen3-8B
```

默认 `sync` 包含 `official` 和原有四个公开目录来源。
`documents sync` 只采集官方页面，省略 `--vendor` 时处理配置中的全部页面。
`documents list` 输出配置来源与已保存文档，可用 `--vendor`、`--record` 同时筛选。
`documents check` 离线生成字段比较，省略 `--record` 时返回全部已有声明的比较结果。
这两个读取命令、`inspect` 和 `export` 都不会重新请求页面。

`inspect RECORD_ID` 增加 `official_sources`、`documents` 和 `document_comparisons`，`export` 的顶层也包含这三个字段。
`stats.documents` 返回已保存页面数、最近采集失败数、有内容的页面数与声明数。
失败页面可能保留旧内容，所以 `failed` 与 `with_content` 可以重叠。
配置数量、成功响应数量、含字段声明的页面数量是不同口径；当前采集结果以 `documents list` 与 `stats` 为准。

2026-10-16 的采集处理了 12 个厂商的 27 个页面，其中 25 页取得内容，19 页提取出共 79 条模型字段声明。
另外 2 页保存了失败记录：OpenAI 的 GPT-4.1 发布博客返回 HTTP 403，Anthropic 的 Claude 4 system card 入口返回 HTTP 307，后者按拒绝重定向规则处理。
完整结果与错误保存在 [`data/documents/official.yaml`](../data/documents/official.yaml)，页面内容和提取结果已通过本地证据重放校验。
交叉核对得到 17 条 `missing_catalog_fact`，表示绑定记录中尚无同名原始字段，可作为补充元数据的审查入口。
这些结果对应本轮选定页面；有内容但没有模型字段的页面仍需结合警告阅读，不能视为能力缺失。

## 页面选择与范围

[`config/official-pages.yaml`](../config/official-pages.yaml) 当前选定 12 个厂商的 27 个页面。
配置最多允许 200 个页面，每个条目要求唯一 ID、唯一 URL、厂商键、页面类型、格式及非空的模型记录绑定。
这些是为字段核对选择的具体页面，不能视为厂商或型号的完整目录。

| 厂商键 | 选定型号或系列 | 页面材料 |
| --- | --- | --- |
| `openai` | GPT-4.1 | 发布博客、模型详情页 |
| `anthropic` | Claude 4 / Sonnet 4 | 发布博客、system card 入口 |
| `google` | Gemini 2.5 Pro | 发布博客、模型详情页 |
| `meta` | Llama 3.3 70B Instruct | 官方仓库模型卡、博客 |
| `xai` | Grok Code Fast 1 | 发布博客、模型详情页 |
| `amazon` | Nova Pro | 发布博客、用户文档 |
| `deepseek` | DeepSeek V3.2 | 发布博客、模型卡、更新文档 |
| `qwen` | Qwen3-8B | 官方博客、模型卡 |
| `nvidia` | Nemotron 3 Super | 发布博客、模型卡、模型详情页 |
| `mistral` | Magistral Small 2506 | 发布博客、模型卡 |
| `cohere` | Command A | 发布博客、模型详情页 |
| `microsoft` | Phi-4 | 发布博客、模型卡 |

原始 URL 和绑定的准确记录 ID 以页面清单为准。
URL 必须属于该厂商允许的 HTTPS 域名；Hugging Face、GitHub 和原始文件域名还检查官方组织命名空间。
页面绑定仅接受已存在的 `kind: model` 记录，不沿服务引用、身份集合或名称相似性扩大范围。
固定修订的模型卡可以定位到指定版本；使用分支或动态页面的条目依靠采集时间和原文摘要区分版本。

## 提取内容与定位

每个页面保存 `source`、最近尝试时间 `checked_at`、可选 `error` 和可选 `content`。
`content` 包括原始响应快照、Content-Type 和带版本号的 `extraction`。

| 提取内容 | 保存方式与含义 |
| --- | --- |
| `title` | 页面标题及其证据位置 |
| `published_at` | 页面元数据中的发布时间，保留来源值 |
| `canonical_url` | 页面声明的规范 URL 及原始属性定位 |
| `links` | 正文中属于同一厂商官方范围的链接、标签及定位 |
| `claims` | 精确模型记录、属性、原生 JSON 值、可选单位、精度、条件和证据 |
| `warnings` | 未提取到模型字段、页面结构或目标段落无法确认等情况 |

页面的 `published_at` 不自动写为模型的 `release_date`。
博客可在模型发布后更新，模型卡也可能没有页面发布时间；缺失值保持缺失。
标题、链接或页面成功返回本身不构成能力声明。
提取器按已支持的页面结构、型号段落和表格读取字段，页面无可确认字段时保留元数据和警告。

HTML 使用 DOM 与 CSS 选择器定位，Markdown 先解析表格和正文再从渲染结果定位。
`scraper` 提供 HTML DOM 解析，`pulldown-cmark` 处理 Markdown，避免把网页标签或表格语法当作普通字符串切分。
部分页面的正文位于响应中的嵌入数据，提取器只解析已支持的数据位置，不执行页面 JavaScript。

| `evidence.locator` 形式 | 定位对象 |
| --- | --- |
| `css:...` | 原始 HTML 解析后的 DOM 节点；`@attribute` 表示属性值 |
| `markdown-rendered-css:...` | Markdown 按当前提取器渲染后的 DOM 节点 |
| `markdown-bytes:...:frontmatter/...` | Markdown 原始字节范围中的元信息 |
| `...#json-pointer=...` | DOM 中 JSON 数据内的具体字段 |
| `embedded-json:...`、`embedded-rsc:...` | 已支持的嵌入正文位置及其内部节点 |

证据摘录仅保留最多 240 个字符，DOM 文本会规范化空白；精确原始响应保存在缓存。
定位规则与 `extractor_version` 一起解释，不能把渲染后 CSS 路径当作原 Markdown 的行号。
目前只接受 UTF-8 的 HTML、Markdown 或纯文本响应，单页上限为 8 MiB。
PDF、OCR 和浏览器 JavaScript 渲染尚未实现；官方 PDF 等链接可作为链接保留，但链接内容不会被自动提取。

## 如何解释字段比较

`documents check` 仅比较声明明确绑定的那条记录中同名的原始事实。
每项结果保留页面声明、页面快照、目录事实及其 `catalog_origin`，便于核对双方证据。
模型属性、值类型、单位和精度必须适合对应比较规则。

| `status` | 含义 |
| --- | --- |
| `agreement` | 双方精确值按该属性的规则一致 |
| `difference` | 双方可比较的声明值不同，需要复核来源与适用范围 |
| `missing_catalog_fact` | 可直接比较的声明在精确绑定记录中缺少同名原始字段 |
| `conditional` | 页面声明带有额外适用条件，需要人工核对 |
| `not_comparable` | 旧内容、近似值、口径或类型不符，或尚无该属性的比较规则 |

计数需要相同单位；数字和数字字符串不会互相转换。
模态数组按已支持标签集合比较，日期要求同样的年月或年月日精度，许可声明要求同样的值结构。
总参数声明不会自动与 safetensors 元素计数或各 dtype 计数等同，缺少明确的 `parameter_count` 时返回 `not_comparable`。
近似的 `8B`、缩写的上下文长度和附加扩展条件需要保留原有精度与范围，不能补写成精确相等结论。

这里的 `claims.conditions` 是页面提取的文字条件列表，尚未转换为 `rules` 使用的 `eq` / `in` 条件。
存在文字条件时返回 `conditional`，同时保留精度和 `official_stale`，不会自动执行或裁定这些条件。
`difference` 只产生待审查的差异，不决定哪一方正确，也不会改写目录事实、合并身份或修改人工声明。
空的比较结果可能表示没有提取到声明，应同时查看页面错误和警告。

## 失败、旧内容与证据重放

采集使用匿名 HTTP 请求，认证来源的凭据不会加入请求。
重定向、HTTP 错误、非 UTF-8 内容、不支持的响应类型和提取错误都会记录为页面失败。
本轮失败时，仅当 ID、URL、厂商、格式、类型和绑定等完整 `source` 与旧记录一致，才保留旧的 `content`。
失败页面仍更新 `checked_at` 和 `error`，旧内容的快照时间保留原值。
因此 `checked_at` 表示最近尝试时间，不能当作内容获取时间。

成功页面和失败记录一起保存至 `data/documents/official.yaml`，命令在存在失败时返回非零状态。
旧内容产生的比较带有 `official_stale: true` 与 `source_error`，不会被判为新的精确一致或差异。
修改配置中的来源定义后应重新同步；普通校验会拒绝与当前配置不一致的旧文档定义。

原始响应按 SHA-256 保存为 `.cache/documents/<sha256>.bin`，Git 保存页面来源、元数据、短摘录和声明。
`validate --evidence` 校验原始字节摘要后重新执行完整提取，并核对全部提取结果，包括标题、日期、链接、声明、警告和定位。
这能证明当前提取结果可由已保存响应重放，不能证明页面声明就是模型的实际行为。

```text
cargo run --release --locked -- validate --evidence
cargo run --release --locked -- evidence archive --output dist/evidence
cargo run --release --locked -- evidence restore --input /path/to/evidence
```

归档和恢复同时处理 `snapshots/<sha256>.json` 与 `documents/<sha256>.bin`。
使用相同 Git 数据版本和完整证据目录即可离线恢复与重放；正文缓存和 HTTP 收据继续排除在 Git 之外。
新增或修订提取器后，应检查页面范围、模型绑定、证据定位和比较口径，再同步数据并检查 diff。

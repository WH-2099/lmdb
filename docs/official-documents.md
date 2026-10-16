# 官方页面与字段核对

[项目首页](../README.md) · [页面清单](../config/official-pages.yaml) · [数据模型](data-model.md) · [维护流程](maintenance.md)

官方页面采集保存厂商发布博客、模型卡、模型详情页和文档的 URL、完整选定段落、结构化声明与原文证据。
页面与现有模型记录显式绑定，声明既可与目录事实交叉核对，也可按作用域和来源优先级生成字段选择结果。
原始目录事实、页面声明和人工条件规则各自保留，选择结果携带全部候选和证据。

## 使用

```text
cargo run --release --locked -- sync --source official
cargo run --release --locked -- documents sync --vendor qwen
cargo run --release --locked -- documents list --record huggingface:model:Qwen/Qwen3-8B
cargo run --release --locked -- documents check --record huggingface:model:Qwen/Qwen3-8B
cargo run --release --locked -- documents resolve huggingface:model:Qwen/Qwen3-8B
cargo run --release --locked -- documents reparse
```

默认 `sync` 包含 `official` 和原有四个公开目录来源。
`documents sync` 只采集官方页面，省略 `--vendor` 时处理配置中的全部页面。
`documents list` 输出配置来源与已保存文档，可用 `--vendor`、`--record` 同时筛选。
`documents check` 离线生成字段比较，省略 `--record` 时返回全部已有声明的比较结果。
`documents resolve RECORD_ID` 返回字段选择状态、选中的候选索引和全部证据。
`documents reparse` 从已缓存原文离线重建提取结果，适用于提取器升级，保留页面的采集时间、最近尝试时间与错误。
`list`、`check`、`resolve`、`inspect` 和 `export` 都只读取本地数据。

`inspect RECORD_ID` 包含 `official_sources`、`documents`、`document_comparisons` 和 `resolved_metadata`。
`export` 的 `resolved_metadata` 按模型记录 ID 索引；服务记录继续从自身事实读取平台限制。
`stats.documents` 返回已保存页面数、最近采集失败数、有内容的页面数、段落数与声明数。
失败页面可能保留旧内容，所以 `failed` 与 `with_content` 可以重叠。
配置数量、成功响应数量、含字段声明的页面数量是不同口径；当前采集结果以 `documents list` 与 `stats` 为准。

2026-10-16 从已有 25 份页面快照离线重解析，保存了 50 段完整选定原文与 234 条结构化声明，覆盖 106 种属性名。
另外两页继续保留获取失败记录；本轮离线处理没有改变页面获取时间。
按模型记录与属性统计，93 项采用官方值，76 项保留限定状态；具体来源与适用条件可通过 `documents resolve` 查看。

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
| `passages` | 完整选定段落、主题、章节标题、明确主体与原文定位 |
| `claims` | 精确模型记录、属性、原生 JSON 值、可选单位、作用域、精度、条件和证据 |
| `warnings` | 未提取到模型字段、页面结构或目标段落无法确认等情况 |

`passages` 保留所选段落的原始表述与段内空白，仅去除外围空白。
每页最多保存 6 段、每段最多 2,000 个字符、全页最多 6,000 个字符；完整段落超限时跳过并留下警告。
段落主题可为概览、能力、限制、可用性、训练、评估、用法或许可，`records` 为空表示页面或模型家族层面的文字。
段落正文与最多 240 字符的证据预览分别保存，正文会参与完整证据重放。

现有页面提取规则包含以下结构化字段，实际值和适用条件仍以保存的声明为准。

| 页面样本 | 字段示例 | 需要保留的范围 |
| --- | --- | --- |
| Qwen3-8B 模型卡 | `architecture_type`、`training_stages`、`attention_heads` | 对应这份模型卡的型号；家族博客段落保持家族范围 |
| Llama 3.3 模型卡 | `languages`、`training_methods`、`training_data` | 语言列表与训练数据描述分别记录，近似数据量保留条件 |
| Phi-4 模型卡 | `training_hardware`、`training_duration`、`training_period` | GPU、时长和时间范围描述训练过程，不能直接用于部署硬件筛选 |
| Nemotron 3 Super 模型卡 | `architecture_components`、`training_precision`、`training_data_cutoff.*` | 预训练精度、下载制品精度和不同训练阶段的日期分别解释 |
| NVIDIA 下载与试用说明 | `license.interface_terms` | NIM 或托管试用的接口条款保留在接口作用域 |
| Command A 文档 | `deployment_requirements` | 描述自部署模型所需设备，保留精度、批量、显存和工作负载未明确的条件 |

`license_name` 保存许可名称，`license` 保存来源的许可声明值，二者不会仅凭名称相似相互替换。

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

`evidence.excerpt` 最多保留 240 个字符，并可规范化 DOM 文本空白；`passages.text` 保存预算内的完整段落，原始响应继续保存在缓存。
定位规则与 `extractor_version` 一起解释，不能把渲染后 CSS 路径当作原 Markdown 的行号。
目前只接受 UTF-8 的 HTML、Markdown 或纯文本响应，单页上限为 8 MiB。
PDF、OCR 和浏览器 JavaScript 渲染尚未实现；官方 PDF 等链接可作为链接保留，但链接内容不会被自动提取。

## 如何解释字段比较

`documents check` 仅比较声明明确绑定的那条记录中同名的原始事实。
每项结果保留页面声明、页面快照、目录事实及其 `catalog_origin`，便于核对双方证据。
模型属性、作用域、值类型、单位和精度必须适合对应比较规则。
`scope.kind` 区分 `model`、`interface` 和 `unspecified`，缺少作用域时默认为 `unspecified`。
接口声明的 `id` 是 HTTPS 文档地址，与人工身份集合中的接口 ID 分别解释；接口或尚未明确作用域的声明不能用于模型事实比较。

| `status` | 含义 |
| --- | --- |
| `agreement` | 双方精确值按该属性的规则一致 |
| `difference` | 双方可比较的声明值不同，需要复核来源与适用范围 |
| `missing_catalog_fact` | 可直接比较的声明在精确绑定记录中缺少同名原始字段 |
| `conditional` | 页面声明带有额外适用条件，需要人工核对 |
| `not_comparable` | 旧内容、近似值、作用域或口径不符，或尚无该属性的比较规则 |

计数需要相同单位；数字和数字字符串不会互相转换。
模态数组按已支持标签集合比较，日期要求同样的年月或年月日精度，许可声明要求同样的值结构。
总参数声明不会自动与 safetensors 元素计数或各 dtype 计数等同，缺少明确的 `parameter_count` 时返回 `not_comparable`。
近似的 `8B`、缩写的上下文长度和附加扩展条件需要保留原有精度与范围，不能补写成精确相等结论。

这里的 `claims.conditions` 是页面提取的文字条件列表，尚未转换为 `rules` 使用的 `eq` / `in` 条件。
存在文字条件时返回 `conditional`，同时保留精度和 `official_stale`，不会自动执行或裁定这些条件。
`difference` 只产生待审查的差异，不决定哪一方正确，也不会改写目录事实、合并身份或修改人工声明。
空的比较结果可能表示没有提取到声明，应同时查看页面错误和警告。

## 按来源选择模型字段

字段选择先确定作用域，再按来源类型选择最高优先级的候选。
模型卡、模型详情页和文档的优先级为 2，发布博客为 1，当前记录的原始目录事实为 0。
这些等级是本库的选择规则，所有低等级证据仍保留在候选列表中。

只有 `scope.kind: model`、精确、没有附加条件且最近获取未失败的官方声明能成为直接采用值。
最高优先级只有近似、有条件或失败后保留的旧声明时，结果为 `qualified`，不会悄悄改用较低优先级的值。
同一最高优先级仍有合格候选时，比较这些候选：相同值可选择一个确定的证据入口，值冲突则保留冲突，单位或比较口径不能对齐则保留限定状态。

| `fields[property].status` | 含义 |
| --- | --- |
| `selected` | 最高优先级存在合格且相互一致的候选，`selected` 是 `candidates` 中从 0 开始的索引 |
| `qualified` | 最高优先级的声明受精度、条件、旧内容或比较口径限制，没有可直接采用的值 |
| `conflict` | 同一最高优先级的合格候选给出不同值 |
| `unavailable` | 没有可采用的值 |

每个候选保留原始记录或文档身份、字段值、快照、定位与失败状态。
没有专门比较规则的结构化属性仍可选择，同级候选按原生 JSON 值和单位判断一致性；数组顺序保留语义。
因此 `documents check` 的 `not_comparable` 与字段选择结果分别解释。
没有官方模型声明的字段可以采用当前记录已有的目录事实。
原始 `facts` 和由它生成的 `profile` 继续表示目录来源，字段选择结果单独保存为 `resolved_metadata`。

`model` 声明可在人工审查的同一模型身份成员间使用，候选仍保留最初绑定记录的 ID 和证据。
只有显式模型成员参与这一步，服务引用、相似名称和链接不能扩大范围；成员各自的目录事实不会互相复制。
`interface` 和 `unspecified` 声明在直接绑定记录的 `scoped_claims` 中单独展示，不进入模型字段，也不沿身份传播。
服务记录保持平台自身的字段和限制。

模型查询的 `--input`、`--output`、`--min-context` 和 `--capability` 在存在官方模型声明时使用 `selected` 值。
`qualified`、`conflict` 或缺少直接采用值的字段不会命中这些筛选，也不会退回目录值。
没有相应官方模型声明时继续按来源 profile 筛选，`--task` 始终使用 profile 中的任务标签。
`query` 输出仍为原始记录，可用 `inspect` 或 `documents resolve` 查看命中的字段证据。

## 失败、旧内容与证据重放

采集使用匿名 HTTP 请求，认证来源的凭据不会加入请求。
重定向、HTTP 错误、非 UTF-8 内容、不支持的响应类型和提取错误都会记录为页面失败。
本轮失败时，仅当 ID、URL、厂商、格式、类型和绑定等完整 `source` 与旧记录一致，才保留旧的 `content`。
失败页面仍更新 `checked_at` 和 `error`，旧内容的快照时间保留原值。
因此 `checked_at` 表示最近尝试时间，不能当作内容获取时间。

成功页面和失败记录一起保存至 `data/documents/official.yaml`，命令在存在失败时返回非零状态。
旧内容产生的比较带有 `official_stale: true` 与 `source_error`，不会被判为新的精确一致或差异。
修改配置中的来源定义后应重新同步；普通校验会拒绝与当前配置不一致的旧文档定义。

原始响应按 SHA-256 保存为 `.cache/documents/<sha256>.bin`，Git 保存页面来源、元数据、预算内的完整段落、证据预览和声明。
`validate --evidence` 校验原始字节摘要后重新执行完整提取，并核对全部提取结果，包括标题、日期、链接、段落、声明、作用域、警告和定位。
这能证明当前提取结果可由已保存响应重放，不能证明页面声明就是模型的实际行为。

```text
cargo run --release --locked -- validate --evidence
cargo run --release --locked -- evidence archive --output dist/evidence
cargo run --release --locked -- evidence restore --input /path/to/evidence
```

归档和恢复同时处理 `snapshots/<sha256>.json` 与 `documents/<sha256>.bin`。
使用相同 Git 数据版本和完整证据目录即可离线恢复与重放；正文缓存和 HTTP 收据继续排除在 Git 之外。
新增或修订提取器后，应检查页面范围、模型绑定、证据定位和比较口径，再执行 `documents reparse` 并检查 diff。
重解析核对原始字节的摘要与格式，全部提取和校验完成后才替换文档 YAML；没有内容的错误记录继续保留。
缺少原文时需恢复相同证据归档；配置中的 URL、格式或绑定发生变化时仍应重新同步页面。

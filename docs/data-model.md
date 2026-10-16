# 数据模型与证据

[项目首页](../README.md) · [领域字段](domain-fields.md) · [身份与关系](relationships.md) · [规则查询](rule-queries.md) · [维护流程](maintenance.md)

当前格式为 `schema_version: 1`，原始目录和声明的类型定义位于 [`src/model.rs`](../src/model.rs) 与 [`src/assertion.rs`](../src/assertion.rs)。
领域视图、身份、关系和规则分别由 `domain`、`identity`、`relations` 与 `rules` 模块定义。
官方页面配置、提取结果与字段对照由 [`documents`](../src/documents/mod.rs) 模块定义。
Rust 类型生成目录、声明、身份、profile、关系、规则、来源约定、发现配置、发现报告、官方页面配置、文档目录与文档对照共 12 个 JSON Schema。
CLI 通过类型解析与语义检查执行校验，JSON Schema 为编辑器和其他消费方提供结构说明。
仅运行通用 JSON Schema 校验不能替代 CLI 的引用、单位、日期与冲突检查。

## 来源目录

每个来源保存为 `data/catalogs/<source>.yaml`，文件名必须与文件内的 `source` 一致。
一个 `Catalog` 包含版本、来源键、快照列表 `snapshots`、记录列表 `records` 和可选的最近同步清单 `last_sync`。

| 对象 | 字段 | 含义 |
| --- | --- | --- |
| `Record` | `schema_version`、`kind`、`id`、`source` | 格式版本、模型或服务分类、带来源的 ID |
| `Record` | `upstream_id`、`name`、可选 `model` | 上游原始 ID、显示名，以及服务明确引用的模型记录 ID |
| `Record` | `origin`、`facts` | 记录的原始位置及所选字段 |
| `Origin` | `snapshot`、`pointer` | 快照 ID 和指向原始记录对象的 JSON Pointer |
| `Fact` | `value`、`pointer`、可选 `unit` | 上游 JSON 原值、完整 JSON Pointer 和明确的计量单位 |
| `Snapshot` | `schema_version`、`id`、`source`、`url` | 快照身份、来源和实际请求 URL |
| `Snapshot` | `retrieved_at`、`sha256`、可选 `upstream_revision` | 获取时间、原始字节摘要和已知上游修订 |
| `SyncManifest` | `completed_at`、`returned_ids`、`missing_ids`、`snapshot_ids` | 最近成功同步时间、本轮返回 ID、保留但缺席的 ID、本轮采集快照 |

`id` 使用 `<source>:<kind>:<upstream_id>`。
例如 `huggingface:model:Qwen/Qwen3-8B` 与 `modelscope:model:Qwen/Qwen3-8B` 是两个来源记录。
相同上游名称不构成已验证的跨来源映射。
`stats` 中的模型数和服务数按这些记录统计，不代表去重后的全球模型数。

`kind: model` 表示来源中的模型或模型仓库记录。
`kind: service` 表示目录或平台作用域内的提供条目，可能包含平台限额、价格、别名或访问范围。
认证适配器当前统一使用服务分类；其中 Artificial Analysis 是聚合目录，这个分类不表示它提供推理服务。
当前只有 models.dev 的 `canonical_model_id` 会生成显式 `model` 引用。
OpenRouter 的 `hugging_face_id` 等显式声明可派生为 `references` 关系，但引用关系不证明模型身份相同。
HF 和 ModelScope 的 `base_models` 可派生为 `derived_from`，原始派生类型在有明确声明时另行保留。
关系不会传播能力、接口限制或许可证，详见[身份与关系](relationships.md)。

## 字段与原始响应

适配器选择有明确含义的字段，字段名称可以统一，`value` 保持上游 JSON 类型和值。
缺失字段和顶层 `null` 省略，`false`、`0`、空数组以及复合对象内部的空值按原值保留。
`facts` 中没有某个字段，表示该记录没有导入相应值，不能推导为不支持。
价格字符串不自动转换为数字，时间戳不自动改写为发布日期，模型仓库更新时间不等于模型发布时间。

目录 `facts` 中的 `context_window`、`max_input_tokens`、`max_output_tokens` 使用非负整数和 `unit: tokens`。
其他字段的单位以适配器所保留的明确语义为准，不能对所有模型统一套用语言 token。
事实中的复合对象可能需要连同来源文档才能解释，例如平台价格、任务对象和推理参数。

JSON Pointer 指向快照根对象下的绝对位置，采用 `/` 分段，并转义键中的 `~` 和 `/`。
空指针表示快照根对象。
字段可以位于记录之外，例如服务记录的提供方名称位于同一快照的提供方对象中。
原始响应字节保存在 `.cache/snapshots/<sha256>.json`，快照 ID 为 `<source>:<sha256>`。
缓存文件不提交到 Git；YAML 保存其身份、请求 URL 和证据定位。
快照摘要用于核对字节一致性，不能证明来源内容本身正确。

## 领域视图、身份与关系

`DomainMetadata` 从已知事实派生任务、模态、token 限制、参数、能力、谱系和价格结构。
派生值携带 `fact` 与 `pointer`，结合记录的 `origin.snapshot` 可以回到原始响应；原始 `facts` 继续保存完整来源值。
任务与模态保留来源标签，仅对已知标签补充规范分类，未识别标签不会被删掉。
价格视图区分金额、计费单位、数量、条件和未分类字段，不从缺失单位猜测总价。
具体映射与计量边界见[领域字段](domain-fields.md)。

`data/identities/core.yaml` 保存经审查的模型身份和接口绑定，每个集合都附文档证据。
模型身份只能包含精确的模型记录 ID，同一记录只能属于一个模型身份；接口绑定只能指向服务记录。
名称相似、共享 URL、服务引用与派生关系都不会自动加入身份集合。
`query --identity` 可查直接模型成员及其通过 `record.model` 明确关联的服务，便于查找提供方。
`identities --record` 和 `rules --record` 仍只使用记录的直接成员或接口绑定，服务查找不会使模型规则或其他接口规则被继承。

导出的每条关系包含 `from`、`to`、`kind`、来源证据与 `target_present`。
`target_present: false` 表示目标模型记录当前不在本库，保留这条上游引用有助于后续审查覆盖缺口。

## 同步后的记录与时间

同步以来源为单位合并新目录。
同一记录的名称、类别、关联、事实值和单位未变化时，保留旧记录及其证据指针。
这样字段位置、请求时间或列表顺序变化不会造成整库 diff。
字段变化时替换该记录，并关联新快照。

本轮没有返回的旧记录继续保留，目录因此包含历史保留项。
`last_sync.returned_ids` 与 `missing_ids` 明确区分本轮返回和本轮缺席的保留记录。
这两个列表覆盖当前来源的全部记录，`snapshot_ids` 保存本轮采集证据；它们独立于旧事实的证据时间。
缺席不会自动推导为退役、删除或不可调用。
同一来源文件可能引用多次获取的快照，不能视作某一时刻完整一致的上游目录。
`retrieved_at` 是所引用快照的获取时间，不表示事实生效时间、最近同步时间或最近成功核查时间。
快照列表保留当前记录和最近同步清单引用的元信息，本地缓存可能同时保留未被当前目录引用的响应。
Git 历史能保存旧记录版本，但只有同时保留对应原始响应，才能重放旧证据。
`evidence archive` 与 `evidence restore` 提供准确原始字节的目录归档和恢复，支持配合同一版本的 YAML 离线重放。

## 官方页面与模型声明

[`config/official-pages.yaml`](../config/official-pages.yaml) 显式列出要获取的厂商页面，[`src/documents/model.rs`](../src/documents/model.rs) 定义其存储结构。
2026-10-16 的配置包含 12 个厂商的 27 个页面，属于精选覆盖，页面数量不代表成功获取数量或完整型号目录。
[`config/sources.yaml`](../config/sources.yaml) 的 13 个来源约定包含 `official`，其范围为 `selected_pages`。
来源约定描述采集范围，具体页面及模型绑定由官方页面配置维护。
页面 URL 必须符合对应厂商的 HTTPS 域名或作者仓库命名空间约束，每页必须显式绑定已收录的模型记录。
`records` 中的服务记录、重复成员与缺失记录会被拒绝，配置中的相同页面 ID 或 URL 也不能重复。
链接和页面名称不会自动增加模型绑定或采集范围。

| 对象 | 字段 | 含义 |
| --- | --- | --- |
| `PageSource` | `id`、`vendor`、`kind`、`url`、`format`、`records` | 页面身份、厂商、文档种类、请求地址、解析格式与显式模型记录绑定 |
| `DocumentCatalog` | `schema_version`、`documents` | 保存于 `data/documents/official.yaml` 的文档目录 |
| `PageDocument` | `source`、`checked_at`、可选 `error`、可选 `content` | 页面配置、最近检查时间、该次错误与可用内容 |
| `PageContent` | `snapshot`、`content_type`、`extraction` | 原始响应身份、响应内容类型与提取结果 |
| `Extraction` | `extractor_version`、`title`、`published_at`、`canonical_url`、`links`、`claims`、`warnings` | 提取器版本、页面元信息、官方链接、模型声明及提取警告 |
| `LocatedValue` | `value`、`evidence` | 带来源定位的页面元信息字符串 |
| `OfficialLink` | `url`、`label`、`evidence` | 指向同一厂商允许范围的链接、页面标签及原始链接定位 |
| `Claim` | `record`、`property`、`value`、可选 `unit`、`precision`、`conditions`、`evidence` | 绑定模型、属性、JSON 值、单位、精度、文字条件和证据定位 |
| `PageEvidence` | `locator`、`excerpt` | 提取位置与至多 240 个字符的证据片段 |

`kind` 区分 `release_blog`、`model_card`、`model_page` 和 `documentation`，`format` 支持 `html` 与 `markdown`。
解析器保存 UTF-8 原始响应，Markdown 正文先渲染为 DOM，再由对应页面的规则提取字段。
提取范围由已实现的页面规则限定，只有标题、日期或链接的页面也可以保存为文档结果，并附上没有提取模型字段的警告。
标题、版本、章节或表格无法明确对应目标模型时，相关规则可以保留警告或省略声明。
页面抓取成功与模型字段提取成功因此是不同状态。

`title`、`published_at` 与 `canonical_url` 属于页面元信息，保留其来源定位。
`published_at` 取自页面的文章发布日期元数据，表示页面发布声明；它不自动生成模型的 `release_date`。
页面修改时间、普通时间标签和抓取时间也不能代替模型发布日期。
模型发布日期需要正文中的明确模型字段和独立的 `Claim`，声明仍受该页的模型绑定限制。
`canonical_url` 保存页面声明的规范地址，不改变配置中的请求 URL 或模型身份。

`precision: exact` 表示提取值按来源的明确写法保存，`approximate` 保留缩写或概数的精度限制。
例如按 `K`、`M` 或 `B` 缩放得到的数值仍可标为 `approximate`，不能仅凭换算后的整数宣称精确一致。
部分页面也直接保留带缩写的字符串值与 `unit: tokens`，因此文档声明的值形状需连同精度一起解释。
`conditions` 是该页提取出的文字条件列表，用于保留模型变体、推理模式、上下文扩展或其他适用前提。
它与人工声明中的 `eq`、`in` 条件结构分别存储，当前文档对照不会执行这些文字条件。
提取结果保存在独立文档目录中，后续审查可以据此修订数据；抓取和对照本身不覆盖 API 目录的 `facts`、领域视图或人工规则。

HTML 证据通常使用 `css:` 定位，Markdown 正文使用 `markdown-rendered-css:` 定位渲染后的节点。
属性证据可带 `@content` 或 `@href`，文章 JSON-LD 日期带 `#json-pointer=`，Markdown frontmatter 可使用 `markdown-bytes:` 字节范围。
个别页面的专用规则还会记录嵌入内容的定位前缀，回放时需使用相同提取器解释。
`excerpt` 可能经过空白归一化和长度截取，完整证据以原始响应字节为准。
这些定位属于网页或其渲染内容，与目录事实使用的原始 JSON Pointer 分开解释。

## 官方文档同步与字段对照

`documents sync` 按配置获取页面，`--vendor` 可限制到一个已配置厂商。
每次检查保存 `checked_at`；获取或提取失败时保存 `error`，并在页面配置相同的情况下保留上次可用的 `content`。
首次失败可以只有错误而没有内容，成功页面和失败状态一同写入文档目录，存在本次所选页面失败时命令返回非零状态。
`checked_at` 因此不等于内容获取时间，保留内容的时间应读取 `content.snapshot.retrieved_at`。
配置或提取器版本变化后，已有结果必须重新同步，校验会检查保存配置的一致性与提取器版本。

`documents list` 展示配置、页面元信息、声明和错误，`documents check` 从保存的声明生成字段对照。
每条 `Comparison` 保留页面 ID、模型记录 ID、属性、完整官方声明、官方快照、目录事实及其 `catalog_origin`。
`official_stale` 表示最近一次页面检查存在错误，`source_error` 保留错误说明，即使旧内容仍可读取也不会丢失该状态。
这个标记不依据内容年龄计算，对照也没有自动判断目录事实是否过期。
对照只读取 `Claim.record` 精确绑定记录中 `facts[property]` 的值，不沿身份、模型引用、别名或其他来源寻找替代值。
只有已提取声明会产生对照行，没有某项官方声明不能据此判断该字段或能力不存在。

| `status` | 含义 |
| --- | --- |
| `agreement` | 无附加条件、内容没有最近检查错误、精度明确，且两边类型、单位与属性规则匹配后值一致 |
| `difference` | 满足直接比较条件，但两边来源声明值不同；结果不裁定哪一方正确 |
| `missing_catalog_fact` | 可直接比较的官方声明在该精确目录记录中没有同名原始字段 |
| `conditional` | 官方声明带文字条件，先保留其适用前提 |
| `not_comparable` | 内容来自失败检查后保留的旧版本、值为近似、属性没有比较规则，或类型、单位、日期精度等不满足规则 |

带条件的声明优先返回 `conditional`，若页面最近检查失败，同时保留 `official_stale: true` 和错误信息。
其余近似值和旧内容不会得到精确一致或差异结论。
计数比较要求相应的 `tokens` 或 `parameters` 单位，布尔值不从参数名称推导，模态按已知标签集合比较。
日期仅在两边都是有效的 `YYYY-MM` 或 `YYYY-MM-DD` 且精度相同时比较；许可证字符串与许可证数组保留各自形状。
`parameter_count` 仅与明确的同名参数总量字段比较，缺少该字段时返回 `not_comparable`，不以 safetensors 或 dtype 计数替代。
`difference` 不会自动新增人工冲突声明，`missing_catalog_fact` 也不能推导为不支持。

## 官方文档原始证据

页面快照沿用 `Snapshot`，其来源为 `official-<vendor>`，ID 为 `official-<vendor>:<sha256>`。
HTML 与 Markdown 原始字节保存在 `.cache/documents/<sha256>.bin`，与 API 响应的 `.cache/snapshots/<sha256>.json` 分别保存。
文档证据校验先核对 SHA-256、UTF-8 和 8 MiB 大小上限，再重跑同一提取器并比较完整 `Extraction`。
因此校验覆盖页面元信息、链接、声明、条件、精度、警告和定位是否可由保留响应重现。
回放成功说明提取可复现，不能证明官方声明正确或模型实际行为符合描述。

`evidence archive` 与 `evidence restore` 同时处理 API 原始 JSON 和已保存文档引用的原始字节。
归档目录保留 `snapshots/<sha256>.json` 与 `documents/<sha256>.bin`，使用相应版本的 YAML 可以离线恢复并回放。
恢复不会请求上游页面，也不会把新抓取内容当成历史响应的替代品。
仅配置而尚未保存内容的页面没有可归档文档字节，错误记录本身保存在 YAML 中。

## 人工条件声明

`data/assertions/*.yaml` 使用独立的 `AssertionSet`，保存文档来源和人工审查的声明。
声明包含主体 `scope`、属性、原生 JSON 值、支持状态、条件、审查结果及来源定位。
详细语义与样本见[条件声明文档](assertions.md)。

`scope.id` 是模型或接口的说明性标识，允许存在尚未绑定目录记录的独立声明。
`rules --record` 只从显式身份或接口绑定选择作用域，声明不会覆盖来源字段。
条件支持 `eq` 和 `in`，使用 `--context` 提供的原生 JSON 值共同限定适用范围。
求值结果区分匹配、不匹配和条件未知，再按作用域与属性返回已知、未知、冲突或没有适用规则的结果。
缺少上下文或声明不能推导为不支持，冲突候选与证据始终保留，详见[规则查询](rule-queries.md)。
`review: accepted` 表示接受所引用文档的声明；当前证据类型仅为 `reported`，不表示实际模型行为已经验证。
`review: conflicted` 要求 `status: unknown` 和 `value: null`，保留冲突供复核。

## 校验与导出

| 操作 | 检查范围 |
| --- | --- |
| `validate` | YAML 输入边界、类型、格式版本、唯一 ID、来源一致性、模型与快照引用、指针语法、领域字段与单位、同步清单、人工声明、身份与关系、来源和发现配置，以及官方页面配置、模型绑定和提取结果结构 |
| `validate --evidence` | 上述检查，加上本地原始响应的 SHA-256、记录对象定位及字段值与指针目标的一致性，以及官方文档提取结果的完整回放 |
| `schema --check` | 已提交 JSON Schema 与当前 Rust 类型生成结果是否一致 |
| `export` | 校验本地注册表，再输出按 ID 排序的 JSON 视图 |
| `documents check` | 按精确记录与属性对照已保存官方声明和目录事实，保留条件、精度、旧内容状态及两边证据 |

证据校验不重新请求网页，人工条件声明中的文档摘录仍需人工核对，字段语义与真实模型行为也需要独立审查。
从干净克隆执行普通校验和导出不需要缓存；证据校验需要所有仍被引用的 API 原始 JSON 与官方页面原始字节。
导出对象包含 `schema_version`、`models`、`services`、`snapshots`、`syncs`、`assertion_sets`、`profiles`、`identities`、`relationships`、`official_sources`、`documents` 和 `document_comparisons`。
`profiles` 按记录 ID 索引，`syncs` 按来源键保存最近同步清单；规则结果由 `rules` 按调用上下文单独生成。
`official_sources` 保存配置中的页面来源，`documents` 保存抓取结果、失败状态及嵌套页面快照，`document_comparisons` 保存本次按目录事实派生的对照结果。
官方页面快照位于各文档的 `content.snapshot`，顶层 `snapshots` 继续保存 API 目录快照。
导出保留事实、来源指针、条件和冲突，不将它们压平成一个能力布尔值。
相同输入数据生成相同内容；JSON 是 YAML 的派生产物，维护时应修改源记录。

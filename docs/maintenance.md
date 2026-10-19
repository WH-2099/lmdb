# 维护与验证

[项目首页](../README.md) · [数据模型](data-model.md) · [官方页面](official-documents.md) · [规则查询](rule-queries.md) · [候选发现](discovery.md) · [认证来源](authenticated-sources.md)

当前工具以本地 Rust CLI 运行，Git 中的 YAML 是版本维护入口。
更新上游目录后，维护者检查 diff 与证据，再提交变更。
研究档案保留调研时的判断，当前行为以实现和本文为准。

## 命令

所有命令支持全局 `--root PATH`，路径默认为当前工作目录。
以下示例从仓库根目录执行，使用已提交的 `Cargo.lock`。

| 命令 | 用途 |
| --- | --- |
| `cargo run --release --locked -- sources` | 读取 14 个来源的范围、认证约定和条款状态 |
| `cargo run --release --locked -- sync` | 更新六个默认公开来源，包含 Vercel 网关与官方页面 |
| `cargo run --release --locked -- sync --source modelsdev,openrouter` | 仅更新指定来源，`--source` 也可重复提供 |
| `cargo run --release --locked -- sync --source vercel` | 匿名获取 Vercel AI Gateway 的服务目录 |
| `cargo run --release --locked -- sync --source official` | 更新配置中的官方页面 |
| `cargo run --release --locked -- renormalize --source huggingface` | 从已核验的历史快照补齐当前字段映射，保留证据时间和同步清单 |
| `cargo run --release --locked -- documents sync --vendor qwen` | 只更新指定厂商的官方页面 |
| `cargo run --release --locked -- documents reparse` | 从本地原始页面离线重新提取段落和字段，保留原获取时间与采集错误 |
| `cargo run --release --locked -- documents list --record huggingface:model:Qwen/Qwen3-8B` | 查看模型明确绑定的页面、声明、警告和失败 |
| `cargo run --release --locked -- documents list --vendor google` | 查看厂商页面及其中保存的未绑定目录候选 |
| `cargo run --release --locked -- documents list --kind directory` | 仅查看官方目录页及候选、警告和失败 |
| `cargo run --release --locked -- documents check --record huggingface:model:Qwen/Qwen3-8B` | 离线比较页面声明与同一记录的目录事实 |
| `cargo run --release --locked -- documents resolve huggingface:model:Qwen/Qwen3-8B` | 查看按来源优先级选择的字段及其候选、限制和冲突 |
| `cargo run --release --locked -- discover --source huggingface,modelscope --output dist/discovery.json` | 将有数量上限的公开候选保存为报告，供种子审查 |
| `cargo run --release --locked -- validate` | 离线检查目录、领域字段、声明、身份、关系、官方页面和配置 |
| `cargo run --release --locked -- validate --evidence` | 核对目录原值并从页面原文重放完整提取结果 |
| `cargo run --release --locked -- stats` | 输出来源、最近同步、字段覆盖、身份、关系、声明与页面统计 |
| `cargo run --release --locked -- query Qwen --source huggingface --kind model --limit 10` | 在 ID 与名称中匹配文本，并按来源和类型筛选 |
| `cargo run --release --locked -- query --task robotics --kind model` | 根据领域任务标签筛选模型记录 |
| `cargo run --release --locked -- query --observation observed --source huggingface` | 只返回最近一次明确采集范围内成功返回的记录 |
| `cargo run --release --locked -- inspect huggingface:model:nvidia/GR00T-N1.7-3B` | 查看原始记录、profile、绑定、关系、同步状态、规则候选、官方页面与选定元数据 |
| `cargo run --release --locked -- identities --record huggingface:model:BAAI/bge-m3` | 查看记录的直接身份成员或接口绑定 |
| `cargo run --release --locked -- relations --record huggingface:model:BAAI/bge-m3 --limit 10` | 查看涉及此记录的显式关系 |
| `cargo run --release --locked -- rules --record huggingface:model:physical-intelligence/fast --property artifact.role` | 求值记录直接绑定作用域中的指定属性 |
| `cargo run --release --locked -- evidence archive --output dist/evidence` | 归档当前目录引用的原始快照并核对证据 |
| `cargo run --release --locked -- evidence restore --input dist/evidence` | 从归档目录恢复并核对证据 |
| `cargo run --release --locked -- export --output dist/registry.json` | 生成 JSON；省略 `--output` 时使用同一路径 |
| `cargo run --release --locked -- schema` | 从 Rust 类型生成 14 个 JSON Schema 文件 |
| `cargo run --release --locked -- schema --check` | 检查 schema 是否与当前类型一致 |

`sources`、`stats`、`query`、`inspect`、`identities`、`relations`、`rules` 和 `documents` 的子命令输出便于其他工具读取的 JSON。
`query` 的文本可省略，默认最多返回 20 条记录，结果包含事实、证据指针和 `observation` 元数据。
查询按不区分大小写的子字符串匹配 ID 和名称，来源筛选使用精确来源键。
结果按 ID 排序；字段筛选支持 `--task`、`--input`、`--output`、`--min-context`、`--capability`、`--identity` 与 `--observation`。
任务继续按目录来源标签或已映射的规范标签匹配。
模型的输入、输出、最小上下文和能力过滤，在该字段有官方 `model` 声明时使用 `resolved_metadata` 的选定值，未选定时不命中，也不回退到目录值。
没有对应官方模型声明时沿用目录 profile，模态仍支持来源标签和规范标签；服务记录使用自己的目录事实。
`--capability` 可选 `reasoning`、`thinking`、`tool-call`、`structured-output`、`open-weights`、`attachment` 或 `temperature-control`，只匹配明确为真的字段。
`--min-context` 使用 token 数，详细字段口径见[领域字段](domain-fields.md)。
查询仍返回带原始事实的记录，使用 `inspect` 或 `documents resolve` 可以核对选定值、来源和选择理由。

`query --identity BAAI/bge-m3 --kind service` 查找通过 `record.model` 明确指向该身份成员的提供方记录。
`identities --record` 与 `rules --record` 只采用直接绑定，前述提供方查找不会传播模型或接口规则。
`relations` 默认返回最多 20 条边，并输出筛选后的 `total`；缺席目标仍保留在结果中。
规则使用 `--scope` 与 `--scope-kind` 选择模型或接口，或者使用 `--record` 选择记录已有的直接绑定。
不传作用域时按原作用域分组查询全部声明，指定无绑定的记录时返回空集合。

```text
cargo run --release --locked -- rules --record huggingface:model:wanglab/MedSAM2 --property segmentation.specialization --context '{"artifact.checkpoint":"MedSAM2_US_Heart.pt"}'
```

`--context` 是以条件路径为字面键的 JSON 对象，条件不匹配、缺少条件和证据冲突分别呈现。
接口上下文与已选接口作用域矛盾时命令报错，具体状态见[规则查询](rule-queries.md)。

## 公开来源更新

| 来源键 | 获取范围 | 维护入口 |
| --- | --- | --- |
| `modelsdev` | 上游模型目录与服务目录 | 适配器内的固定入口 |
| `huggingface` | 配置中每个仓库的模型详情 | [`config/huggingface-models.txt`](../config/huggingface-models.txt) |
| `modelscope` | 配置中每个仓库的模型详情 | [`config/modelscope-models.txt`](../config/modelscope-models.txt) |
| `openrouter` | 显式请求全部输出模态的模型目录并处理分页 | 适配器内的固定入口 |
| `vercel` | Vercel AI Gateway 公开服务目录 | 固定匿名入口 `https://ai-gateway.vercel.sh/v1/models` |
| `official` | 选定厂商的发布博客、模型卡、模型详情页、文档和官方目录 | [`config/official-pages.yaml`](../config/official-pages.yaml) |

Vercel 条目保存为 `vercel:service:<id>`，网关限制、模态、价格和可用性保留服务作用域，`model` 留空。
`pricing` 的数字字符串、分档和其他复合值按原值保存，领域视图将其列入 `unclassified`，当前不推断 USD 或统一计费单位。
来源约定共 6 个公开来源与 8 个需显式启用的认证来源。

精选清单每行一个 `组织/模型` 上游 ID，支持空行和以 `#` 开头的注释，重复 ID 会报错。
添加模型前确认原始组织、仓库路径、任务与资料可访问性，避免根据名称推断 ID。
`discover` 先读取公开列表生成候选报告，审查通过后再手动修改精选清单。
发现流程使用匿名 HTTP 请求，分别限制每页、每条规则与全局处理量；它不承诺枚举完整 Hub。
部分规则失败时保留完成的候选和错误信息，命令返回非零状态，详见[候选发现](discovery.md)。
首批选择依据见[种子目录研究](../research/2026-10-15/sources/seed-catalog.md)。
移除清单项会停止后续获取，已有记录仍按合并规则保留；清单不承担删除历史数据的职责。

```text
cargo run --release --locked -- sync --source huggingface
cargo run --release --locked -- validate
git diff -- data/catalogs config
cargo run --release --locked -- export
```

目录同步先校验旧注册表，再获取某来源所需的响应并验证新证据。
完整目录必须获取全部分页并完成校验后才合并新记录；任意页失败时保留旧记录并记录失败尝试。
重复记录 ID、悬空模型引用等目录结构错误同样使该次同步失败，并保留旧记录。
HF 与 ModelScope 的精选仓库分别请求和校验，单个仓库失败时保留其旧记录，其余成功项继续更新。
有成功项也有失败项的精选同步标为 `partial`，没有成功项的失败同步标为 `failed`。
每个来源的目录与本轮状态校验后原子保存，存在失败时整个命令仍返回非零状态。
整个同步不是跨所有来源的一次事务，失败后应检查已更新文件再选择重试来源。

同一来源本轮缺少的旧记录不会自动删除。
字段未变化时保留旧快照引用，因此重复同步不等于刷新每条记录的证据日期。
`last_sync` 保存本轮采集范围 `scope`、完成状态 `status`、返回与缺席 ID、快照和逐项 `errors`。
`completed_at` 是最近一次尝试的完成时间；`last_successful_at` 只在整个请求范围成功完成时更新。
`scope.kind` 为 `full_catalog` 或 `selected_models`，后者保存当轮精选上游 ID。
历史清单没有记录范围或状态时保持未知，离线迁移不会补写不存在的网络观察。

| `observation` 状态 | 含义 |
| --- | --- |
| `observed` | 本轮明确范围内成功返回该记录 |
| `missing` | 完整目录全部采集成功，但没有返回该历史记录 |
| `not_requested` | 该历史记录不在本轮精选清单内 |
| `failed` | 记录属于本轮请求范围，但未成功观察 |
| `unknown` | 没有同步清单，或旧清单缺少判断所需的范围与状态 |

`query --observation STATUS` 按这些状态筛选，`inspect`、导出和字段选择使用同一判断。
缺席、失败与未请求的目录事实保留为候选，但不能直接选定。
`unknown` 的历史值仍可参与查询与选择，输出保留未知状态；要求本轮明确返回时使用 `--observation observed`。
缺席不表示退役；最近尝试引用的快照也参与归档与证据校验。
人工修订导入字段可能被未来发生内容变化的同步替换；来源事实应通过适配器修正，独立文档结论应写入人工声明。

## 官方页面维护

官方页面按配置选择厂商与具体 URL，最多允许 200 个条目，采集匿名进行，详细范围见[官方页面与字段核对](official-documents.md)。
`kind: directory` 的 `records` 必须为空；其他页面必须恰好绑定一条已有模型记录，避免一个型号的声明被直接复制到多个记录。
`documents sync --vendor NAME` 按厂商重试；成功内容与页面失败一起写入 `data/documents/official.yaml`，存在失败时命令返回非零状态。
失败时只保留完整 `source` 定义相同的旧内容，`checked_at` 记录最近尝试，旧快照保留实际获取时间，比较结果标记 `official_stale`。
HTTP 200 的应用壳、型号不符或结构退化也属于失败；页面是否允许零字段由对应提取契约判断。
已收到但提取失败的响应保存为 `failed_snapshot`，可归档、校验，并在提取器修复后离线重试。
修改 URL、格式或绑定后通常需要重新同步；同一 HF 文件的固定提交描述迁移例外见下文。

HF 模型卡的 `repository` 配置保存 `repo`、`path` 与 `revision`。
`revision.mode: tracked` 先把 `reference` 解析成提交，再按该提交下载；`pinned` 使用明确的 `sha`。
快照的 URL 与 `upstream_revision` 保存实际提交，旧证据按旧提交回放。
旧来源为同一 HF 仓库和文件的固定 40 位 SHA URL，且快照 URL 匹配、其余身份与绑定均未改变时，`reparse` 可离线迁移为跟踪描述并回填旧 SHA。
这只更新来源描述，仍需 `documents sync` 才能观察当前 `main` 指向的提交。

`format: pdf` 使用内置 Rust 解析器提取文本，原始 PDF 仍保存在 `.cache/documents/`，段落以 `pdf:page:N` 定位。
当前 PDF 保存有限的选定文本段落，不自动提取能力声明；段落 `records` 留空，避免把多模型报告的正文推给单个型号。
PDF 上限为 8 MiB、256 页，解压与提取也有大小边界；加密、损坏或没有可提取文本的 PDF 会失败，扫描页不执行 OCR。

维护者用 `documents list` 检查来源、目录候选、错误和警告，再用 `documents check` 审查可比较字段。
页面发布时间不自动成为模型发布日期，近似数值、附加条件和旧内容不会产生精确一致结论。
字段差异不会自动改写 API 目录事实；需要核对模型版本、单位、适用条件和双方定位。
页面条件目前保存为文字说明，不能直接当作 `rules` 的 `eq` / `in` 条件执行。

目录页的 `catalog_entries` 保存标签、URL、可选型号标识和原始链接证据，每页最多 500 条，省略条目时留下警告。
每页官方链接另有 200 条上限，截断警告应与目录提取警告一起检查。
这些条目是待审查候选，不会自动新增模型、绑定身份或继续抓取目标页面，分页和交互结果也不代表已取尽。
目录页没有记录绑定，因此 `documents list --record` 和 `inspect` 不会展示它们；使用 `documents list --kind directory`，也可按厂商筛选。
`stats.documents.directories` 是已保存目录页数，`directory_entries` 是各页候选数量之和，跨页重复会分别计数。
审查候选后，为具体型号增加一条有明确模型绑定的页面配置，再获取其字段证据。

`documents list` 还展示选定原文段落以及架构、训练、语言、许可、硬件等结构化声明。
每页最多 6 段，每段最多 2,000 个字符，合计最多 6,000 个字符；超限段落整段跳过并留下警告。
正文保留原有措辞和内部空白，段落中的 `records` 为空时表示页面或模型家族范围，不应扩成具体模型事实。

`documents resolve RECORD_ID` 在模型卡、模型详情页和文档、发布博客、目录事实之间依次选择字段。
最高等级的可用候选值冲突时保留 `conflict`，只有条件、近似值或旧内容时保留 `qualified`，均不回退到较低等级。
同级存在可用的精确无条件值时可以选定，全部候选仍在输出中，完整状态说明见[数据模型](data-model.md#按来源优先级选择模型字段)。
只有 `model` 作用域的官方声明能在同一显式模型身份的成员间共享，目录事实、接口或未指定作用域声明不参与这种共享，服务使用自身事实。
缺省声明作用域是 `unspecified`，接口声明中的 `id` 是 HTTPS 文档引用，与 `rules` 使用的接口身份分别维护。
`inspect` 增加 `resolved_metadata`，导出中的同名映射只包含模型记录；原目录事实和 profile 继续保留各自来源。

当前提取器版本为 4，可在现有缓存上离线迁移已有文档结果：

```text
cargo run --release --locked -- documents reparse
cargo run --release --locked -- documents resolve huggingface:model:Qwen/Qwen3-8B
cargo run --release --locked -- validate --evidence
git diff -- data/documents
```

`reparse` 核对缓存摘要并重建已保存内容的提取结果，保留 `checked_at` 和快照时间。
它不请求上游；有 `failed_snapshot` 的页面会重试该响应的提取，成功后恢复内容，失败时保留错误。
`error_kind` 区分 `fetch` 与 `extraction`；离线恢复只清除提取错误，后续网络失败或没有错误类型的旧失败仍保留过期状态。
重放旧内容不会覆盖较新的失败响应。
只有错误而没有已保存响应的页面仍保留原错误。
新增配置页没有已保存响应时不会由 `reparse` 自动采集，需在需要获取新内容时运行对应同步。
全部页面处理并通过结构与证据校验后才原子写入 YAML，缓存缺失或损坏时原文档文件继续保留。
重解析后仍有失败页面时输出其 ID 并返回非零状态；有效的提取与失败记录已经保存，需检查结果后选择重试。
上述 HF 固定提交迁移保留旧获取时间；只有错误且没有内容或失败响应的页面也允许把格式声明改为 PDF。
其他 URL、仓库路径、引用模式、格式或绑定变化必须重新同步，离线重解析不会把旧响应归到另一个页面。

## 网络与凭据

HTTP 客户端复用连接，设置请求超时和响应体上限。
API JSON 与认证请求拒绝重定向；匿名官方文档允许最多 5 次 HTTPS 跳转，每一跳都核对厂商允许的 URL。
分页请求必须保持同源，循环游标和错误响应会终止该来源更新。
暂时连接、超时、响应中断、429 与服务器错误按有限次数重试，遵循 `Retry-After` 的秒数或日期及支持的限流重置头。
单次请求超时为 30 秒，等待预算为 30 秒，每次获取包含重试、跳转和等待的总时限为 60 秒。
等待超过剩余预算时返回 deferred 错误及可计算的 `retry_at`，不会在同次调用中无限等待。
鉴权错误和无法解析的响应不会清空旧目录。

公开响应支持 ETag 和 Last-Modified 条件请求；304 必须关联可读取且摘要匹配的已有响应。
需要凭据的请求不复用公开 HTTP 收据，密钥通过请求头发送。
当前认证来源只有显式指定 `--source NAME` 与 `--authenticated` 才可启用，凭据环境变量见[认证来源说明](authenticated-sources.md)。
设置环境变量本身不会使默认同步启用认证来源。
`config/sources.yaml` 描述来源契约，`sources` 只读这些约定，不读取环境中的凭据值；执行端点仍由适配器固定。
本轮认证适配器只进行了合成响应验证，真实账户范围、配额和分页结果仍待验证。

## 缓存与证据保存

`.cache/snapshots/<sha256>.json` 保存目录原始 JSON，`.cache/documents/<sha256>.bin` 保存官方页面原始字节，`.cache/http/` 保存公开请求的条件缓存收据。
这些缓存目录和 `dist/` 均被 Git 忽略，页面 YAML 保存元数据、目录候选、选定段落、短摘录、定位和结构化声明。
认证响应同样可能包含账户范围内的数据，不应直接发布整个缓存目录。

普通 `validate` 从 YAML 检查结构与引用，不读取缓存，也不会请求上游。
`validate --evidence` 读取每个被引用的目录快照，核对摘要、记录身份、名称回退规则、记录位置与事实值。
对官方页面，它从原始 HTML、Markdown 或 PDF 重新执行提取，核对完整结果，包括元数据、链接、目录候选、段落、声明、作用域、警告和定位。
提取失败的响应也核对原始摘要，但不要求失败响应产生有效声明。
目录候选随文档导出，并沿用 `.bin` 证据的归档和恢复；Vercel 服务目录沿用 `.json` 快照。
目录适配器不会在证据校验中重新采集，人工声明也不会联网复核；通过校验不证明模型实际支持某个功能。
干净克隆可以执行普通校验和导出，但缺少历史原始响应时无法完成证据校验。
重新同步只能取得当前响应，无法保证恢复已经变化的历史快照。
使用归档命令保存当前目录和官方页面引用的全部原始证据：

```text
cargo run --release --locked -- evidence archive --output dist/evidence
```

输出包含 `manifest.json`、`snapshots/<sha256>.json` 与 `documents/<sha256>.bin`，保留准确原始字节。
manifest 保存 `data/` 文件摘要、整体 `data_tree_sha256` 及证据清单，用于核对归档对应的数据版本。
归档按摘要去重，包含记录证据、`last_sync.snapshot_ids`、官方页面内容与 `failed_snapshot`，不复制 `.cache/http/` 收据。
命令核对大小、格式、SHA-256、记录身份、字段指针和页面提取重放结果；已有相同文件可重复使用，损坏或缺少的来源文件会使命令失败。
保存对应的 Git 数据版本和这个目录后，可在克隆中恢复并离线核对：

```text
cargo run --release --locked -- evidence restore --input /path/to/evidence
cargo run --release --locked -- validate --evidence
```

恢复写入当前数据根目录的 `.cache/snapshots/` 与 `.cache/documents/`，两个命令都不访问上游。
归档覆盖当前来源目录与官方页面引用的证据，独立发现报告的列表响应需要随报告另行保存。
长期存储、访问范围和清理策略仍由维护者决定；删除缓存前应先确认需要保留的证据已归档。

## CI 证据包

[`scripts/evidence_bundle.py`](../scripts/evidence_bundle.py) 使用 Python 3 标准库将归档目录打包为 tar，并打印文件的 SHA-256。

```text
python3 scripts/evidence_bundle.py pack dist/evidence dist/evidence.tar
python3 scripts/evidence_bundle.py unpack dist/evidence.tar /tmp/lmdb-evidence-restored EXPECTED_SHA256
cargo run --release --locked -- evidence restore --input /tmp/lmdb-evidence-restored
cargo run --release --locked -- validate --evidence
```

`EXPECTED_SHA256` 使用打包时输出的 64 位小写十六进制摘要；打包目标 tar 和解包目标目录都必须尚不存在。
解包先核对摘要，再限制文件名、类型、路径与大小，拒绝链接和归档目录以外的内容。
恢复命令继续核对 manifest 与当前 `data/` 版本，防止把其他数据版本的证据包当作本次验收。

CI 使用仓库变量 `LMDB_EVIDENCE_ARCHIVE_URL` 与 `LMDB_EVIDENCE_ARCHIVE_SHA256`，手动触发时可用 `evidence_archive_url` 与 `evidence_archive_sha256` 覆盖。
URL 必须为不含用户信息的 HTTPS 地址；下载最多跟随 3 次 HTTPS 跳转，总时限 180 秒、体积上限 256 MiB。
两项配置均缺失时 CI 明确报告归档存储待配置；只配置一项、摘要不匹配或恢复校验失败都会使检查失败。
长期托管位置尚未确定，本轮只交付配置入口与本地回放，不会自动上传证据包。

## 修改格式与实现

修改记录、声明、身份、领域视图、关系、规则、发现、官方文档或字段选择结果类型后，运行 `schema` 并检查对应 schema 变更。
提取器版本变化后先用 `documents reparse` 更新本地已有内容，再检查段落范围、声明作用域和选定字段。
目录适配器增加字段映射后，用 `renormalize --source huggingface` 回放该来源已有记录的原始快照。
`--source` 必填，可用逗号选择多个来源；缺少或损坏的缓存使迁移失败，记录 ID、证据时间和同步清单继续保留。
迁移完成后检查字段原值、指针与领域投影，再运行 `validate --evidence` 和导出，完整步骤见[实施计划](metadata-collection-plan.md#数据迁移顺序)。
输入 YAML 限制为单文档、UTF-8 和 JSON 兼容值，拒绝重复键、非字符串键、显式标签、锚点、别名、merge key、非有限数字及超限输入。
目录、声明、身份和配置等输入结构拒绝未知字段，避免字段拼错后被静默忽略。

```text
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run --release --locked -- schema --check
cargo run --release --locked -- validate
```

本地全量数据检查使用 release 构建；CI 使用 debug 构建执行格式、lint、测试、普通校验和 schema 一致性检查。
配置证据包后，CI 下载指定归档、恢复缓存并执行 `validate --evidence`，不会重新同步上游目录。
这些是提交前的检查入口，不表示每次文档编辑都需要新增测试。
更改适配器时，选择能验证字段原值、指针与错误路径的合成样本；修改网络或合并逻辑时，覆盖失败是否保留旧数据。
涉及真实来源的变化还应记录实际采集结果，fixture 通过不等于在线兼容。

当前核心直接使用 Rust，证据包脚本仅依赖 Python 3 标准库。
修改 Rust 依赖时更新并提交 `Cargo.lock`，后续验证使用 `--locked`。
未来 Python 任务需要第三方依赖时使用 `pyproject.toml` 维护直接运行时依赖，每次修改后执行 `uv lock` 并提交 `uv.lock`。
`requirements.txt` 通过 `uv export --format requirements.txt --frozen --no-header --no-annotate --no-hashes --output-file requirements.txt` 生成。
uv 部署使用 `uv sync --frozen`，CI 另用 `uv lock --check` 检查锁文件是否匹配项目定义。

## 来源与许可

项目代码采用 MIT，具体范围见[代码与数据许可](licensing.md)。
模型、模型权重、模型卡正文、目录数据和软件工具的许可证可能不同。
自动导入保留来源与许可字段，但这些字段不构成整个注册表的统一再分发授权。
维护者应按实际发布内容核查来源条款，尤其是批量数据、价格、媒体资源和完整响应。

| 来源材料 | 当前处理 |
| --- | --- |
| models.dev | 保留 [`licenses/models.dev-MIT.txt`](../licenses/models.dev-MIT.txt)；记录内的模型许可与外链资产另行判断 |
| Hugging Face、ModelScope | 保留模型仓库许可与来源证据；不把单个模型许可推广到整个平台目录 |
| OpenRouter | 保存接口声明与来源，批量再分发范围仍待确认 |
| Vercel AI Gateway | 保存平台目录声明、价格原值和证据，按网关条款确认保存与再分发范围 |
| 官方页面提取 | Git 保存来源 URL、定位、选定段落与结构化声明，完整响应在本地缓存；按实际保存内容核对再分发范围 |
| 人工文档声明 | 保留官方 URL、定位和核查时间；`accepted` 表示证据审查结果 |
| 认证来源与原始缓存 | 根据账户条款和来源要求确认保存、分享与再分发范围 |

详细来源证据见[来源目录](../research/2026-10-15/sources/catalog.md)与[同类项目授权记录](../research/2026-10-15/comparisons/projects.md#4-授权记录汇总)。
当前不承诺完整模型覆盖、目录实时可用性或任何条件组合的实际推理结果。

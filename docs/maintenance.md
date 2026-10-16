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
| `cargo run --release --locked -- sources` | 读取 13 个来源的范围、认证约定和条款状态 |
| `cargo run --release --locked -- sync` | 更新五个默认公开来源，包含官方页面 |
| `cargo run --release --locked -- sync --source modelsdev,openrouter` | 仅更新指定来源，`--source` 也可重复提供 |
| `cargo run --release --locked -- sync --source official` | 更新配置中的官方页面 |
| `cargo run --release --locked -- documents sync --vendor qwen` | 只更新指定厂商的官方页面 |
| `cargo run --release --locked -- documents reparse` | 从本地原始页面离线重新提取段落和字段，保留原获取时间与采集错误 |
| `cargo run --release --locked -- documents list --record huggingface:model:Qwen/Qwen3-8B` | 查看模型明确绑定的页面、声明、警告和失败 |
| `cargo run --release --locked -- documents check --record huggingface:model:Qwen/Qwen3-8B` | 离线比较页面声明与同一记录的目录事实 |
| `cargo run --release --locked -- documents resolve huggingface:model:Qwen/Qwen3-8B` | 查看按来源优先级选择的字段及其候选、限制和冲突 |
| `cargo run --release --locked -- discover --source huggingface,modelscope --output dist/discovery.json` | 将有数量上限的公开候选保存为报告，供种子审查 |
| `cargo run --release --locked -- validate` | 离线检查目录、领域字段、声明、身份、关系、官方页面和配置 |
| `cargo run --release --locked -- validate --evidence` | 核对目录原值并从页面原文重放完整提取结果 |
| `cargo run --release --locked -- stats` | 输出来源、最近同步、字段覆盖、身份、关系、声明与页面统计 |
| `cargo run --release --locked -- query Qwen --source huggingface --kind model --limit 10` | 在 ID 与名称中匹配文本，并按来源和类型筛选 |
| `cargo run --release --locked -- query --task robotics --kind model` | 根据领域任务标签筛选模型记录 |
| `cargo run --release --locked -- inspect huggingface:model:nvidia/GR00T-N1.7-3B` | 查看原始记录、profile、绑定、关系、同步状态、规则候选、官方页面与选定元数据 |
| `cargo run --release --locked -- identities --record huggingface:model:BAAI/bge-m3` | 查看记录的直接身份成员或接口绑定 |
| `cargo run --release --locked -- relations --record huggingface:model:BAAI/bge-m3 --limit 10` | 查看涉及此记录的显式关系 |
| `cargo run --release --locked -- rules --record huggingface:model:physical-intelligence/fast --property artifact.role` | 求值记录直接绑定作用域中的指定属性 |
| `cargo run --release --locked -- evidence archive --output dist/evidence` | 归档当前目录引用的原始快照并核对证据 |
| `cargo run --release --locked -- evidence restore --input dist/evidence` | 从归档目录恢复并核对证据 |
| `cargo run --release --locked -- export --output dist/registry.json` | 生成 JSON；省略 `--output` 时使用同一路径 |
| `cargo run --release --locked -- schema` | 从 Rust 类型生成 13 个 JSON Schema 文件 |
| `cargo run --release --locked -- schema --check` | 检查 schema 是否与当前类型一致 |

`sources`、`stats`、`query`、`inspect`、`identities`、`relations`、`rules` 和 `documents` 的子命令输出便于其他工具读取的 JSON。
`query` 的文本可省略，默认最多返回 20 条记录，结果包含事实和证据指针。
查询按不区分大小写的子字符串匹配 ID 和名称，来源筛选使用精确来源键。
结果按 ID 排序；字段筛选支持 `--task`、`--input`、`--output`、`--min-context`、`--capability` 与 `--identity`。
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
| `official` | 选定厂商的发布博客、模型卡、模型详情页和文档 | [`config/official-pages.yaml`](../config/official-pages.yaml) |

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
该来源成功完成后，合并旧记录、校验结果，并原子替换其 YAML 文件。
来源请求或解析失败时保留该来源旧文件；其他成功来源可以更新，最终命令会报告失败并返回非零状态。
整个同步不是跨所有来源的一次事务，失败后应检查已更新文件再选择重试来源。

同一来源本轮缺少的旧记录不会自动删除。
字段未变化时保留旧快照引用，因此重复同步不等于刷新每条记录的证据日期。
`last_sync.completed_at`、`returned_ids`、`missing_ids` 和 `snapshot_ids` 单独保存最近成功同步的观察结果。
`stats` 返回每来源的返回与缺席数量，`inspect` 返回该记录是否在最近同步出现。
缺席记录继续保留，不能据此宣布退役；最近同步快照也会参与归档与证据校验。
人工修订导入字段可能被未来发生内容变化的同步替换；来源事实应通过适配器修正，独立文档结论应写入人工声明。

## 官方页面维护

官方页面配置当前包含 12 个厂商的 27 个页面，最多允许 200 个条目。
页面要求准确 URL 和已有模型记录绑定，采集匿名进行，详细范围见[官方页面与字段核对](official-documents.md)。
`documents sync --vendor NAME` 按厂商重试；成功内容与页面失败一起写入 `data/documents/official.yaml`，存在失败时命令返回非零状态。
失败时只保留完整 `source` 定义相同的旧内容，`checked_at` 记录最近尝试，旧快照保留实际获取时间，比较结果标记 `official_stale`。
修改 URL、格式或绑定后重新同步，避免旧页面内容被用于新的来源定义。

维护者用 `documents list` 检查来源、错误和警告，再用 `documents check` 审查可比较字段。
页面发布时间不自动成为模型发布日期，近似数值、附加条件和旧内容不会产生精确一致结论。
字段差异不会自动改写 API 目录事实；需要核对模型版本、单位、适用条件和双方定位。
页面条件目前保存为文字说明，不能直接当作 `rules` 的 `eq` / `in` 条件执行。

`documents list` 还展示选定原文段落以及架构、训练、语言、许可、硬件等结构化声明。
每页最多 6 段，每段最多 2,000 个字符，合计最多 6,000 个字符；超限段落整段跳过并留下警告。
正文保留原有措辞和内部空白，段落中的 `records` 为空时表示页面或模型家族范围，不应扩成具体模型事实。

`documents resolve RECORD_ID` 在模型卡、模型详情页和文档、发布博客、目录事实之间依次选择字段。
最高等级的可用候选值冲突时保留 `conflict`，只有条件、近似值或旧内容时保留 `qualified`，均不回退到较低等级。
同级存在可用的精确无条件值时可以选定，全部候选仍在输出中，完整状态说明见[数据模型](data-model.md#按来源优先级选择模型字段)。
只有 `model` 作用域的官方声明能在同一显式模型身份的成员间共享，目录事实、接口或未指定作用域声明不参与这种共享，服务使用自身事实。
缺省声明作用域是 `unspecified`，接口声明中的 `id` 是 HTTPS 文档引用，与 `rules` 使用的接口身份分别维护。
`inspect` 增加 `resolved_metadata`，导出中的同名映射只包含模型记录；原目录事实和 profile 继续保留各自来源。

提取器修改后，可在现有缓存上离线更新文档结果：

```text
cargo run --release --locked -- documents reparse
cargo run --release --locked -- documents resolve huggingface:model:Qwen/Qwen3-8B
cargo run --release --locked -- validate --evidence
git diff -- data/documents
```

`reparse` 核对缓存摘要并重建所有已保存内容的提取结果，保留 `checked_at`、快照时间和最近采集错误。
它不请求上游，只有错误而没有内容的页面仍保留原错误。
全部页面处理和校验成功后才写入 YAML，缓存缺失或损坏时原文档文件继续保留。
修改页面 URL、格式或绑定后需重新同步；离线重新提取不把旧响应改绑到新的来源定义。

## 网络与凭据

HTTP 客户端复用连接，设置请求超时和响应体上限，拒绝 HTTP 重定向。
分页请求必须保持同源，循环游标和错误响应会终止该来源更新。
429 与服务器错误按有限次数重试，遵循 `Retry-After` 的秒数或日期，超过等待预算则失败。
鉴权错误和无法解析的响应不会清空旧目录。

公开响应支持 ETag 和 Last-Modified 条件请求；304 必须关联可读取且摘要匹配的已有响应。
需要凭据的请求不复用公开 HTTP 收据，密钥通过请求头发送。
当前认证来源只有显式指定 `--source NAME` 与 `--authenticated` 才可启用，凭据环境变量见[认证来源说明](authenticated-sources.md)。
设置环境变量本身不会使默认同步启用认证来源。
`config/sources.yaml` 描述来源契约，`sources` 只读这些约定，不读取环境中的凭据值；执行端点仍由适配器固定。
本轮认证适配器只进行了合成响应验证，真实账户范围、配额和分页结果仍待验证。

## 缓存与证据保存

`.cache/snapshots/<sha256>.json` 保存目录原始 JSON，`.cache/documents/<sha256>.bin` 保存官方页面原始字节，`.cache/http/` 保存公开请求的条件缓存收据。
这些缓存目录和 `dist/` 均被 Git 忽略，页面 YAML 保存元数据、选定段落、短摘录、定位和结构化声明。
认证响应同样可能包含账户范围内的数据，不应直接发布整个缓存目录。

普通 `validate` 从 YAML 检查结构与引用，不读取缓存，也不会请求上游。
`validate --evidence` 读取每个被引用的目录快照，核对摘要、记录位置与事实值。
对官方页面，它从原始 UTF-8 响应重新执行提取，核对完整结果，包括元数据、链接、段落、声明、作用域、警告和定位。
目录适配器不会在证据校验中重新采集，人工声明也不会联网复核；通过校验不证明模型实际支持某个功能。
干净克隆可以执行普通校验和导出，但缺少历史原始响应时无法完成证据校验。
重新同步只能取得当前响应，无法保证恢复已经变化的历史快照。
使用归档命令保存当前目录和官方页面引用的全部原始证据：

```text
cargo run --release --locked -- evidence archive --output dist/evidence
```

输出是包含 `snapshots/<sha256>.json` 与 `documents/<sha256>.bin` 的目录，保留准确原始字节。
归档按摘要去重，包含记录证据、`last_sync.snapshot_ids` 引用的快照和官方页面已保存的内容，不复制 `.cache/http/` 收据。
命令核对大小、JSON 或 UTF-8 格式、SHA-256、目录字段指针和页面提取重放结果；已有相同文件可重复使用，损坏或缺少的来源文件会使命令失败。
保存对应的 Git 数据版本和这个目录后，可在克隆中恢复并离线核对：

```text
cargo run --release --locked -- evidence restore --input /path/to/evidence
cargo run --release --locked -- validate --evidence
```

恢复写入当前数据根目录的 `.cache/snapshots/` 与 `.cache/documents/`，两个命令都不访问上游。
归档覆盖当前来源目录与官方页面引用的证据，独立发现报告的列表响应需要随报告另行保存。
长期存储、访问范围和清理策略仍由维护者决定；删除缓存前应先确认需要保留的证据已归档。

## 修改格式与实现

修改记录、声明、身份、领域视图、关系、规则、发现、官方文档或字段选择结果类型后，运行 `schema` 并检查对应 schema 变更。
提取器版本变化后先用 `documents reparse` 更新本地已有内容，再检查段落范围、声明作用域和选定字段。
输入 YAML 限制为单文档、UTF-8 和 JSON 兼容值，拒绝重复键、非字符串键、显式标签、锚点、别名、merge key、非有限数字及超限输入。
目录、声明、身份和配置等输入结构拒绝未知字段，避免字段拼错后被静默忽略。

```text
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run --release --locked -- schema --check
cargo run --release --locked -- validate
```

本地全量数据检查使用 release 构建；CI 当前使用 debug 构建执行格式、lint、测试、普通校验和 schema 一致性检查。
CI 不联网同步来源，也不依赖未入 Git 的原始缓存。
这些是提交前的检查入口，不表示每次文档编辑都需要新增测试。
更改适配器时，选择能验证字段原值、指针与错误路径的合成样本；修改网络或合并逻辑时，覆盖失败是否保留旧数据。
涉及真实来源的变化还应记录实际采集结果，fixture 通过不等于在线兼容。

当前核心直接使用 Rust，不引入 Python 包装 CLI。
修改 Rust 依赖时更新并提交 `Cargo.lock`，后续验证使用 `--locked`。
未来独立 Python 任务使用 `pyproject.toml` 维护直接运行时依赖，每次修改后执行 `uv lock` 并提交 `uv.lock`。
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
| 官方页面提取 | Git 保存来源 URL、定位、选定段落与结构化声明，完整响应在本地缓存；按实际保存内容核对再分发范围 |
| 人工文档声明 | 保留官方 URL、定位和核查时间；`accepted` 表示证据审查结果 |
| 认证来源与原始缓存 | 根据账户条款和来源要求确认保存、分享与再分发范围 |

详细来源证据见[来源目录](../research/2026-10-15/sources/catalog.md)与[同类项目授权记录](../research/2026-10-15/comparisons/projects.md#4-授权记录汇总)。
当前不承诺完整模型覆盖、目录实时可用性或任何条件组合的实际推理结果。

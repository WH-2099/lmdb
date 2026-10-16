# 维护与验证

[项目首页](../README.md) · [数据模型](data-model.md) · [规则查询](rule-queries.md) · [候选发现](discovery.md) · [认证来源](authenticated-sources.md)

当前工具以本地 Rust CLI 运行，Git 中的 YAML 是版本维护入口。
更新上游目录后，维护者检查 diff 与证据，再提交变更。
研究档案保留调研时的判断，当前行为以实现和本文为准。

## 命令

所有命令支持全局 `--root PATH`，路径默认为当前工作目录。
以下示例从仓库根目录执行，使用已提交的 `Cargo.lock`。

| 命令 | 用途 |
| --- | --- |
| `cargo run --release --locked -- sources` | 读取 12 个来源的范围、认证约定和条款状态 |
| `cargo run --release --locked -- sync` | 更新四个默认公开来源 |
| `cargo run --release --locked -- sync --source modelsdev,openrouter` | 仅更新指定来源，`--source` 也可重复提供 |
| `cargo run --release --locked -- discover --source huggingface,modelscope --output dist/discovery.json` | 将有数量上限的公开候选保存为报告，供种子审查 |
| `cargo run --release --locked -- validate` | 离线检查目录、领域字段、声明、身份、关系和配置 |
| `cargo run --release --locked -- validate --evidence` | 使用本地快照核对导入字段的原值 |
| `cargo run --release --locked -- stats` | 输出来源、最近同步、字段覆盖、身份、关系与声明统计 |
| `cargo run --release --locked -- query Qwen --source huggingface --kind model --limit 10` | 在 ID 与名称中匹配文本，并按来源和类型筛选 |
| `cargo run --release --locked -- query --task robotics --kind model` | 根据领域任务标签筛选模型记录 |
| `cargo run --release --locked -- inspect huggingface:model:nvidia/GR00T-N1.7-3B` | 查看原始记录、profile、绑定、关系、最近同步状态与规则候选 |
| `cargo run --release --locked -- identities --record huggingface:model:BAAI/bge-m3` | 查看记录的直接身份成员或接口绑定 |
| `cargo run --release --locked -- relations --record huggingface:model:BAAI/bge-m3 --limit 10` | 查看涉及此记录的显式关系 |
| `cargo run --release --locked -- rules --record huggingface:model:physical-intelligence/fast --property artifact.role` | 求值记录直接绑定作用域中的指定属性 |
| `cargo run --release --locked -- evidence archive --output dist/evidence` | 归档当前目录引用的原始快照并核对证据 |
| `cargo run --release --locked -- evidence restore --input dist/evidence` | 从归档目录恢复并核对证据 |
| `cargo run --release --locked -- export --output dist/registry.json` | 生成 JSON；省略 `--output` 时使用同一路径 |
| `cargo run --release --locked -- schema` | 从 Rust 类型生成 9 个 JSON Schema 文件 |
| `cargo run --release --locked -- schema --check` | 检查 schema 是否与当前类型一致 |

`sources`、`stats`、`query`、`inspect`、`identities`、`relations` 和 `rules` 输出便于其他工具读取的 JSON。
`query` 的文本可省略，默认最多返回 20 条记录，结果包含事实和证据指针。
查询按不区分大小写的子字符串匹配 ID 和名称，来源筛选使用精确来源键。
结果按 ID 排序；字段筛选支持 `--task`、`--input`、`--output`、`--min-context`、`--capability` 与 `--identity`。
任务和模态可按来源标签或已映射的规范标签匹配，缺失字段不会命中筛选。
`--capability` 可选 `reasoning`、`thinking`、`tool-call`、`structured-output`、`open-weights`、`attachment` 或 `temperature-control`，只匹配明确为真的字段。
`--min-context` 使用 token 数，详细字段口径见[领域字段](domain-fields.md)。

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

同步先校验旧注册表，再获取某来源所需的响应并验证新证据。
该来源成功完成后，合并旧记录、校验结果，并原子替换其 YAML 文件。
来源请求或解析失败时保留该来源旧文件；其他成功来源可以更新，最终命令会报告失败并返回非零状态。
整个同步不是跨所有来源的一次事务，失败后应检查已更新文件再选择重试来源。

同一来源本轮缺少的旧记录不会自动删除。
字段未变化时保留旧快照引用，因此重复同步不等于刷新每条记录的证据日期。
`last_sync.completed_at`、`returned_ids`、`missing_ids` 和 `snapshot_ids` 单独保存最近成功同步的观察结果。
`stats` 返回每来源的返回与缺席数量，`inspect` 返回该记录是否在最近同步出现。
缺席记录继续保留，不能据此宣布退役；最近同步快照也会参与归档与证据校验。
人工修订导入字段可能被未来发生内容变化的同步替换；来源事实应通过适配器修正，独立文档结论应写入人工声明。

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

`.cache/snapshots/<sha256>.json` 保存原始 JSON 字节，`.cache/http/` 保存公开请求的条件缓存收据。
这两个目录和 `dist/` 均被 Git 忽略。
认证响应同样可能包含账户范围内的数据，不应直接发布整个缓存目录。

普通 `validate` 从 YAML 检查结构与引用，不读取缓存，也不会请求上游。
`validate --evidence` 读取每个被引用的快照，核对摘要、记录位置与事实值。
它不重新运行适配器，不联网核对人工声明，也不证明模型实际支持某个功能。
干净克隆可以执行普通校验和导出，但缺少历史原始响应时无法完成证据校验。
重新同步只能取得当前响应，无法保证恢复已经变化的历史快照。
使用归档命令保存当前目录引用的全部原始快照：

```text
cargo run --release --locked -- evidence archive --output dist/evidence
```

输出是包含 `snapshots/<sha256>.json` 的目录，保留准确原始字节，不是压缩文件。
归档按摘要去重，包含记录证据与 `last_sync.snapshot_ids` 引用的快照，不复制 `.cache/http/` 收据。
命令核对大小、JSON、SHA-256 和目录字段指针；已有相同文件可重复使用，损坏或缺少的来源文件会使命令失败。
保存对应的 Git 数据版本和这个目录后，可在克隆中恢复并离线核对：

```text
cargo run --release --locked -- evidence restore --input /path/to/evidence
cargo run --release --locked -- validate --evidence
```

恢复写入当前数据根目录的 `.cache/snapshots/`，两个命令都不访问上游。
归档覆盖的是当前来源目录引用的快照，独立发现报告的列表响应需要随报告另行保存。
长期存储、访问范围和清理策略仍由维护者决定；删除缓存前应先确认需要保留的证据已归档。

## 修改格式与实现

修改记录、声明、身份、领域视图、关系、规则或发现类型后，运行 `schema` 并提交对应 schema 变更。
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
| 人工文档声明 | 保留官方 URL、定位和核查时间；`accepted` 表示证据审查结果 |
| 认证来源与原始缓存 | 根据账户条款和来源要求确认保存、分享与再分发范围 |

详细来源证据见[来源目录](../research/2026-10-15/sources/catalog.md)与[同类项目授权记录](../research/2026-10-15/comparisons/projects.md#4-授权记录汇总)。
当前不承诺完整模型覆盖、目录实时可用性或任何条件组合的实际推理结果。

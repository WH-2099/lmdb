# 数据模型与证据

[项目首页](../README.md) · [领域字段](domain-fields.md) · [身份与关系](relationships.md) · [规则查询](rule-queries.md) · [维护流程](maintenance.md)

当前格式为 `schema_version: 1`，原始目录和声明的类型定义位于 [`src/model.rs`](../src/model.rs) 与 [`src/assertion.rs`](../src/assertion.rs)。
领域视图、身份、关系和规则分别由 `domain`、`identity`、`relations` 与 `rules` 模块定义。
Rust 类型生成目录、声明、身份、profile、关系、规则、来源约定、发现配置与发现报告共 9 个 JSON Schema。
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

`context_window`、`max_input_tokens`、`max_output_tokens` 使用非负整数和 `unit: tokens`。
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
| `validate` | YAML 输入边界、类型、格式版本、唯一 ID、来源一致性、模型与快照引用、指针语法、领域字段与单位、同步清单、人工声明、身份与关系、来源和发现配置 |
| `validate --evidence` | 上述检查，加上本地原始响应的 SHA-256、记录对象定位及字段值与指针目标的一致性 |
| `schema --check` | 已提交 JSON Schema 与当前 Rust 类型生成结果是否一致 |
| `export` | 校验本地注册表，再输出按 ID 排序的 JSON 视图 |

证据校验不重新请求网页，不验证人工摘录与网页当前正文，也不保证来源字段语义正确。
从干净克隆执行普通校验和导出不需要缓存；证据校验需要所有仍被引用的原始 JSON。
导出对象包含 `schema_version`、`models`、`services`、`snapshots`、`syncs`、`assertion_sets`、`profiles`、`identities` 和 `relationships`。
`profiles` 按记录 ID 索引，`syncs` 按来源键保存最近同步清单；规则结果由 `rules` 按调用上下文单独生成。
导出保留事实、来源指针、条件和冲突，不将它们压平成一个能力布尔值。
相同输入数据生成相同内容；JSON 是 YAML 的派生产物，维护时应修改源记录。

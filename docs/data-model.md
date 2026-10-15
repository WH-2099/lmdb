# 数据模型与证据

[项目首页](../README.md) · [维护流程](maintenance.md) · [条件声明](assertions.md)

当前格式为 `schema_version: 1`，类型定义位于 [`src/model.rs`](../src/model.rs) 和 [`src/assertion.rs`](../src/assertion.rs)。
Rust 类型生成 `schemas/catalog.schema.json` 与 `schemas/assertions.schema.json`。
CLI 通过类型解析与语义检查执行校验，JSON Schema 为编辑器和其他消费方提供结构说明。
仅运行通用 JSON Schema 校验不能替代 CLI 的引用、单位、日期与冲突检查。

## 来源目录

每个来源保存为 `data/catalogs/<source>.yaml`，文件名必须与文件内的 `source` 一致。
一个 `Catalog` 包含版本、来源键、快照列表 `snapshots` 和记录列表 `records`。

| 对象 | 字段 | 含义 |
| --- | --- | --- |
| `Record` | `schema_version`、`kind`、`id`、`source` | 格式版本、模型或服务分类、带来源的 ID |
| `Record` | `upstream_id`、`name`、可选 `model` | 上游原始 ID、显示名，以及服务明确引用的模型记录 ID |
| `Record` | `origin`、`facts` | 记录的原始位置及所选字段 |
| `Origin` | `snapshot`、`pointer` | 快照 ID 和指向原始记录对象的 JSON Pointer |
| `Fact` | `value`、`pointer`、可选 `unit` | 上游 JSON 原值、完整 JSON Pointer 和明确的计量单位 |
| `Snapshot` | `schema_version`、`id`、`source`、`url` | 快照身份、来源和实际请求 URL |
| `Snapshot` | `retrieved_at`、`sha256`、可选 `upstream_revision` | 获取时间、原始字节摘要和已知上游修订 |

`id` 使用 `<source>:<kind>:<upstream_id>`。
例如 `huggingface:model:Qwen/Qwen3-8B` 与 `modelscope:model:Qwen/Qwen3-8B` 是两个来源记录。
相同上游名称不构成已验证的跨来源映射。
`stats` 中的模型数和服务数按这些记录统计，不代表去重后的全球模型数。

`kind: model` 表示来源中的模型或模型仓库记录。
`kind: service` 表示目录或平台作用域内的提供条目，可能包含平台限额、价格、别名或访问范围。
认证适配器当前统一使用服务分类；其中 Artificial Analysis 是聚合目录，这个分类不表示它提供推理服务。
当前只有 models.dev 的 `canonical_model_id` 会生成显式 `model` 引用。
OpenRouter 的 `hugging_face_id` 等字段作为事实保留，不自动建立跨来源关联。
HF 的量化、adapter、合并和微调关系也保留来源原值，不据此继承能力或许可证。

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

## 同步后的记录与时间

同步以来源为单位合并新目录。
同一记录的名称、类别、关联、事实值和单位未变化时，保留旧记录及其证据指针。
这样字段位置、请求时间或列表顺序变化不会造成整库 diff。
字段变化时替换该记录，并关联新快照。

本轮没有返回的旧记录继续保留，目录因此包含历史保留项。
当前没有由缺席自动推导退役、删除或最近可用时间的机制。
同一来源文件可能引用多次获取的快照，不能视作某一时刻完整一致的上游目录。
`retrieved_at` 是所引用快照的获取时间，不表示事实生效时间、最近同步时间或最近成功核查时间。
快照列表只保留当前记录仍引用的元信息，本地缓存可能同时保留未被当前记录引用的响应。
Git 历史能保存旧记录版本，但只有同时保留对应原始响应，才能重放旧证据。

## 人工条件声明

`data/assertions/*.yaml` 使用独立的 `AssertionSet`，保存文档来源和人工审查的声明。
声明包含主体 `scope`、属性、原生 JSON 值、支持状态、条件、审查结果及来源定位。
详细语义与样本见[条件声明文档](assertions.md)。

`scope.id` 是外部模型或接口的说明性标识，当前不要求绑定某个目录 `Record.id`。
声明不会自动覆盖来源字段，也不会因模型名称相似而自动附着到目录记录。
条件当前支持 `eq` 和 `in`，共同限定适用范围。
校验器只检查其结构，没有根据请求执行条件、推导能力或解决冲突的规则引擎。
`review: accepted` 表示接受所引用文档的声明；当前证据类型仅为 `reported`，不表示实际模型行为已经验证。
`review: conflicted` 要求 `status: unknown` 和 `value: null`，保留冲突供复核。

## 校验与导出

| 操作 | 检查范围 |
| --- | --- |
| `validate` | YAML 输入边界、类型、格式版本、唯一 ID、来源一致性、模型引用、快照引用、指针语法、token 单位、人工声明与证据引用 |
| `validate --evidence` | 上述检查，加上本地原始响应的 SHA-256、记录对象定位及字段值与指针目标的一致性 |
| `schema --check` | 已提交 JSON Schema 与当前 Rust 类型生成结果是否一致 |
| `export` | 校验本地注册表，再输出按 ID 排序的 JSON 视图 |

证据校验不重新请求网页，不验证人工摘录与网页当前正文，也不保证来源字段语义正确。
从干净克隆执行普通校验和导出不需要缓存；证据校验需要所有仍被引用的原始 JSON。
导出对象包含 `schema_version`、`models`、`services`、`snapshots` 和 `assertion_sets`。
导出保留事实、来源指针、条件和冲突，不将它们压平成一个能力布尔值。
相同输入数据生成相同内容；JSON 是 YAML 的派生产物，维护时应修改源记录。

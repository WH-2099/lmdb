# 条件声明查询

[返回项目](../README.md) · [声明格式](assertions.md) · [查询实现](../src/rules.rs) · [首批声明](../data/assertions/core.yaml)

规则查询根据调用方提供的上下文判断声明是否适用，再汇总同一作用域和属性的证据。
查询结果属于文档声明的元数据，现有证据全部是 `reported`。
条件成立不表示已经运行模型、调用服务或验证完整请求。

## 查询入口

命令行支持按目录记录、作用域或精确属性查询：

```sh
cargo run -- rules --scope google/gemini-developer.generate-content.v1beta --scope-kind interface --property input.system_instruction.image
cargo run -- rules --scope google/gemini-2.5-flash --scope-kind model --property thinking.disable --context '{"interface":"google/gemini-developer.generate-content.v1beta","generationConfig.thinkingConfig.thinkingBudget":0}'
cargo run -- rules --scope cohere/embed-v4.0 --scope-kind model --property embedding.output_dimensions --context '{"interface":"cohere/embed.v2"}'
cargo run -- rules --property thinking.disable --context '{"interface":"google/gemini-developer.generate-content.v1beta"}'
```

`--context` 默认是空对象，参数接受下文说明的平面 JSON 对象。
只提供 `--property` 可以检查不同作用域下的同一属性，每组结果仍保留自己的作用域。
命令行返回包含 `record`、`scopes`、`context` 和 `groups` 的 JSON 对象。
按目录记录查询时使用 `--record RECORD_ID`，`scopes` 列出选中的绑定作用域。
没有对应映射的记录返回空 `scopes` 和 `groups`，不会扩展为全部声明。
同时提供 `--record` 与 `--scope` 时，作用域必须属于该记录的已知绑定。
已选中接口作用域时，如果上下文同时提供 `interface`，其值必须与该作用域 ID 一致，否则命令报错。
记录有多个接口绑定时，使用 `--scope` 与 `--scope-kind interface` 先选定与上下文一致的接口。
只提供 `--property` 的跨作用域查询保留各组的主体，不将 `context.interface` 当作作用域筛选器。

Rust 接口位于 `model_metadata::rules`：

```rust
pub fn evaluate(
    conditions: &[Condition],
    context: &BTreeMap<String, Value>,
) -> ConditionMatch;

pub fn query(
    registry: &Registry,
    scopes: &[Scope],
    property: Option<&str>,
    context: &BTreeMap<String, Value>,
) -> Result<Vec<RuleGroup>>;
```

`query` 精确匹配 `scope.kind`、`scope.id` 和可选的 `property`。
`scopes` 为空时查询全部作用域；未提供 `property` 时查询所选作用域中的全部属性。
查询会先校验声明文件的结构、状态约束和证据引用。
单独调用 `evaluate` 时，条件应已经通过声明校验。

目录记录与声明主体的对应关系由独立的显式映射处理。
此模块接受已选定的作用域，不通过模型名称、来源或相似标识推断关系。
每个结果保留原作用域；同一个模型通过不同服务接口调用时，接口范围仍需分别核对。

以下是可直接传给查询接口的现有作用域与精确属性：

| `scope.kind` | `scope.id` | `property` | 需要的上下文 |
|---|---|---|---|
| `interface` | `google/gemini-developer.generate-content.v1beta` | `input.system_instruction.image` | 无附加条件 |
| `model` | `google/gemini-2.5-pro` | `thinking.disable` | `interface` |
| `model` | `google/gemini-2.5-flash` | `thinking.disable` | `interface`、`generationConfig.thinkingConfig.thinkingBudget` |
| `model` | `cohere/embed-v4.0` | `embedding.output_dimensions` | `interface` |

接口声明没有附加条件，只表示在该接口自身范围内成立。
Gemini Developer API 的 system image 限制不能因此扩展为全部 Gemini 模型、接口或托管服务的限制。

## 上下文与条件匹配

上下文是“完整路径字符串 → JSON 值”的映射。
路径按字面匹配，不解析嵌套对象，也不把路径解释为 JSON Pointer。
例如 Gemini Flash 关闭 thinking 的上下文是：

```json
{
  "interface": "google/gemini-developer.generate-content.v1beta",
  "generationConfig.thinkingConfig.thinkingBudget": 0
}
```

`eq` 比较原生 JSON 值，`in` 判断上下文值是否等于候选数组中的某一个完整成员。
数字 `0`、字符串 `"0"` 和布尔值 `false` 不互相转换。
显式 `null` 是已经提供的值；仅缺少对应键才表示上下文缺失。
如果 `in` 的候选项是字符串，上下文中的单元素数组也不会匹配该字符串。

| 单项结果 | 含义 |
|---|---|
| `matched` | 已提供对应键，且值满足条件 |
| `not_matched` | 已提供对应键，但值不满足条件 |
| `undetermined` | 缺少对应键，尚不能判断 |

条件列表按 AND 合并。
任意一个条件为 `not_matched`，整体就是 `not_matched`。
其余情况下，如果仍有缺失路径，整体为 `undetermined`；全部条件成立或列表为空时为 `matched`。
`missing_paths` 始终列出发现的缺失键，按字典序排列并去重。
即使其他条件已经不匹配，该列表仍保留缺失信息；补齐它们不会撤销已知的不匹配。

只提供 Gemini 原生接口时，Pro 的 `thinking.disable` 声明匹配，Flash 的对应声明仍缺预算条件。
再提供数字预算 `0` 后，Flash 声明才匹配。
提供预算 `1` 或字符串 `"0"` 会使 Flash 这条声明不匹配，不能据此推导模型不支持关闭 thinking。

## 结果与证据

每个 `RuleGroup` 对应一个 `(scope.kind, scope.id, property)`，包含：

- `scope` 和 `property`：这组声明的确切主体与属性。
- `candidates`：全部候选，每条保留完整 `assertion`、`condition_match` 和它引用的来源定义 `sources`。
- `resolution`：根据候选适用性和证据状态得到的保守汇总。

候选中保留不匹配及待确认的声明，便于解释条件和追查证据。
每个候选的来源定义来自其所属声明文件；不同文件中的同名来源键不会互相覆盖。
分组按作用域种类、作用域 ID、属性的字典序排列，组内候选按声明 ID 排列。

`resolution` 使用带 `state` 字段的对象：

| `state` | 含义 |
|---|---|
| `known` | 条件均已确定，至少一条声明匹配，且匹配声明均已接受、状态明确、`status/value/unit` 一致；返回 `status`、`value`，存在单位时返回 `unit` |
| `conflicted` | 已匹配声明包含 `review: conflicted`，或已匹配的明确事实在 `status/value/unit` 上不同 |
| `unknown` | 存在待确认条件，或已匹配声明本身的 `status` 为 `unknown` |
| `no_applicable_rule` | 这组声明的条件均已确定，但没有任何一条匹配 |

已经成立的冲突优先于待确认条件，因此增加缺失上下文的候选不会隐藏已有冲突。
接受了一条 `status: unknown` 的声明仍表示知识不足；它的 null 值不作为与明确事实矛盾的证据。
没有已成立的冲突时，只要存在待确认候选，本轮实现就保守返回 `unknown`，即使该候选当前记录的值与已匹配声明相同。
不同作用域之间不合并成一个能力结论。

`known` 也可能携带 `status: unsupported` 和 `value: false`，这些值会完整保留。
Cohere Embed v4 在 `interface: "cohere/embed.v2"` 条件成立时返回的维度 `[256, 512, 1024, 1536]` 是离散集合。
汇总不会将它改成连续范围。

当没有符合查询主体和属性的声明时，返回空数组。
空数组与 `no_applicable_rule` 都不能推断能力受支持或不受支持。
条件结果、事实状态、审查状态各自保留含义；查询不会重写原始证据和备注。

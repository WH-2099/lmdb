# 模型关系与身份边界

[项目首页](../README.md) · [数据模型](data-model.md) · [维护流程](maintenance.md) · [条件声明](assertions.md)

关系视图从来源记录的显式字段派生，保存服务引用、仓库链接和模型谱系。
输入仍为 `data/catalogs/*.yaml`，导出 JSON 中的 `relationships` 是可重新生成的结果。
关系的方向为 `from → to`，两端使用带来源和类别的记录 ID。
跨来源出现相同名称、共享链接或相似参数，都不自动生成 `same_as`。

## 关系字段

| 字段 | 含义 |
| --- | --- |
| `from` | 声明关系的来源记录 ID。 |
| `to` | 被引用的模型记录 ID，可以指向尚未收录的外部模型。 |
| `kind` | `serves`、`references` 或 `derived_from`。 |
| `evidence` | 支持这条关系的原始字段位置列表。 |
| `declared_derivation` | 来源明确给出的 `base_model_relation`，缺失时省略。 |
| `target_present` | `to` 是否存在于本次注册表的模型记录中。 |

`target_present: false` 表示目标尚未收录，不表示目标不存在、不可访问或已经退役。
导出不会因为目标缺席而删除这条关系，也不会为它编造模型事实。
`target_present: true` 只说明本地存在目标记录，不证明两端数据处于同一版本或同一时间。

每项 `evidence` 使用以下字段。

| 字段 | 含义 |
| --- | --- |
| `record` | 提供原始字段的记录 ID。 |
| `fact` | 该记录 `facts` 中的字段名。 |
| `snapshot` | 原始响应的快照 ID。 |
| `pointer` | 指向快照内具体证据值的 JSON Pointer。 |

数组中的 URL 和父模型 ID 定位到对应元素，而不是只定位到整个数组。
相同边合并证据，证据去重，关系及其证据均稳定排序。
多个字段指向同一目标会增加可回放的证据，不会增加互相独立的核验次数。

## 派生规则

| 来源与字段 | 关系 | 目标 |
| --- | --- | --- |
| models.dev 服务的 `canonical_model_id` | `serves` | `modelsdev:model:<canonical_model_id>`。 |
| models.dev 的 `weights` 中合格的 Hugging Face 模型 URL | `references` | `huggingface:model:<owner>/<repo>`。 |
| models.dev 的 `links` 中 `type` 为 `model_card` 或 `weights` 的合格 Hugging Face URL | `references` | `huggingface:model:<owner>/<repo>`。 |
| OpenRouter 的 `hugging_face_id` | `references` | `huggingface:model:<hugging_face_id>`。 |
| Hugging Face 的 `base_models` | `derived_from` | `huggingface:model:<base_model>`。 |
| ModelScope 的 `base_models` | `derived_from` | `modelscope:model:<base_model>`。 |

`serves` 表达来源目录中服务条目对模型的明确引用。
`references` 表达原记录链接到某个模型仓库，不能据此合并模型、推断部署权重或继承能力。
`derived_from` 表达来源声明的父模型关系，方向从子模型指向父模型。
ModelScope 的父 ID 始终留在 ModelScope 命名空间，不能因为名称相同就改连 Hugging Face。

`base_models` 可以是一个字符串或一个字符串数组，每个父 ID 分别生成一条边。
数组含多个父 ID 不能单独证明它们采用了权重合并，也可能表示不同组件的来源。
只有原字段 `base_model_relation` 的显式声明进入 `declared_derivation`，并保留声明值的证据位置。
例如显式 `finetune` 可以记录为 `declared_derivation: finetune`，它仍属于来源声明。
非空声明保留原字符串，包括来源新增的类型；空字符串按未声明处理。
`tags` 中的 `base_model:finetune:...`、`base_model:quantized:...` 等值暂不用于推断关系子类，原始 `facts` 保留它们供审查。
缺失类型不能补成 `finetune`，也不能因为仓库名带 `FP8` 就新增 `quantized` 声明。

## Hugging Face URL 边界

URL 解析仅接受 `https://huggingface.co/<owner>/<repo>` 和 `https://huggingface.co/<owner>/<repo>/tree/<revision>` 两种模型仓库地址。
必须使用精确的 `huggingface.co` 主机，不接受 URL 中的账号凭据或显式端口。
组织主页、`spaces`、`datasets`、`papers`、`blob`、`resolve` 和许可证文件链接不作为模型仓库引用。
URL 需要同时满足来源字段的用途和模型仓库路径限制，不能对记录中的任意外链猜测模型身份。
`tree/<revision>` 只用于定位仓库，原 URL 仍可从证据中回放。
由此得到的目标是仓库级 ID，不意味着分支、提交、权重文件或模型修订已经完成对应。

## 当前数据中的反例

以下样本取自本地目录，核对日期为 2026-10-16。
记录引用的原始快照可能更早，具体获取时间应读取 `snapshot.retrieved_at`。

### DeepSeek 共享权重链接

models.dev 的 `deepseek/deepseek-chat`、`deepseek/deepseek-reasoner` 和 `deepseek/deepseek-v3.2` 都链接到 `https://huggingface.co/deepseek-ai/DeepSeek-V3.2`。
其中 `deepseek-chat` 的 `reasoning` 为 `false`，另两条为 `true`。
这些记录各自生成 `references`，共同目标不能把它们压成一个身份或覆盖各自接口能力。
原始值见 [`modelsdev.yaml`](../data/catalogs/modelsdev.yaml)。

### Ornith 的普通权重与 FP8 权重

`modelsdev:model:deepreinforce/ornith-1.0-397b` 的 `weights` 同时链接到 `deepreinforce-ai/Ornith-1.0-397B` 和 `deepreinforce-ai/Ornith-1.0-397B-FP8`。
第二项还显式包含 `quantization: fp8`。
它应产生指向两个仓库的 `references`，不能把两个目标自动合并为 `same_as`。
普通权重 URL 同时出现在 `links` 和 `weights` 中，这条边合并两处证据。
原始值见 [`modelsdev.yaml`](../data/catalogs/modelsdev.yaml)。

### Molmo 的多个父模型

`huggingface:model:allenai/Molmo2-8B` 声明 `Qwen/Qwen3-8B` 和 `google/siglip-so400m-patch14-384` 两个父模型。
当前记录的 typed tag 只覆盖 Qwen 父模型，不能把同一个类型机械套到两个父模型上。
本地 Molmo2 的任务为 `image-text-to-text`，Qwen3-8B 的任务为 `text-generation`，两者参数计数也不同。
这里建立两条 `derived_from`，保留各自父 ID 的位置，关系本身不触发身份合并。
原始值见 [`huggingface.yaml`](../data/catalogs/huggingface.yaml)。

### Llama 的卡片与 tag 不一致

`meta-llama/Llama-3.1-8B-Instruct` 的 `base_models` 是 `meta-llama/Meta-Llama-3.1-8B`。
它的 typed tag 却引用 `meta-llama/Llama-3.1-8B`。
当前派生边采用卡片的父 ID，并保留 tag 原值。
仅删除名称中的 `Meta-` 无法证明重命名或等价关系，差异需要另行核验。
原始值见 [`huggingface.yaml`](../data/catalogs/huggingface.yaml)。

### ModelScope 的分支与 Hugging Face 的提交

两个来源均有 `Qwen/Qwen3-8B`，ModelScope 的 `revision` 是 `master`，Hugging Face 的 `revision` 是 `b968826d9c46dd6066d109eabc6255188de91218`。
可变分支名和另一平台的提交摘要不能直接证明版本一致，名称相同也不能补足这一证据。
此外，ModelScope 的 `Wan-AI/Wan2.2-Animate-14B` 显式声明 `finetune`，Hugging Face 同名记录的 tag 声称 `quantized`。
派生关系保留两边各自来源，typed tag 不覆盖显式字段，也不跨来源统一子类。
原始值见 [`modelscope.yaml`](../data/catalogs/modelscope.yaml) 和 [`huggingface.yaml`](../data/catalogs/huggingface.yaml)。

## 初始证据规模

以下数字统计实现前的原始字段，不能当成最终导出关系数。

| 原始证据 | 数量 |
| --- | ---: |
| Hugging Face 记录 | 147 |
| 含非空 `base_models` 的 Hugging Face 记录 | 39 |
| Hugging Face 父 ID 条目 | 41 |
| Hugging Face 唯一父 ID | 40 |
| 已收录对应目标的 Hugging Face 父 ID 条目 | 3 |
| 显式 `base_model_relation` 的 Hugging Face 记录 | 1 |
| Hugging Face typed tag 中的 `finetune` / `quantized` 记录 | 36 / 3 |
| Hugging Face typed tag 中的 `adapter` / `merge` 记录 | 0 / 0 |
| ModelScope 记录 | 22 |
| 含非空父模型且显式声明 `finetune` 的 ModelScope 记录 | 6 |

39 个 Hugging Face `base_models` 中，30 个为数组，9 个为字符串。
37 个不同的 Hugging Face 父 ID 尚未收录，对应 38 条父 ID 条目。

对同一批 723 个模型和 9,583 个服务派生得到以下关系。

| 关系 | 总数 | 目标已收录 | 目标未收录 |
| --- | ---: | ---: | ---: |
| `serves` | 6,027 | 6,027 | 0 |
| `references` | 404 | 64 | 340 |
| `derived_from` | 47 | 3 | 44 |
| 合计 | 6,478 | 6,094 | 384 |

其中 7 条 `derived_from` 含显式派生类型。
6,496 项关系证据已在 48 份本地原始快照上回放，并核对快照 SHA-256 与字段具体值。
这些数字描述本次数据范围，不代表上游目录的完整规模；增补目录后应重新统计。

## 回放证据

1. 从一条关系的 `evidence` 读取 `record` 和 `fact`，找到产生它的来源记录与原字段。
2. 用 `evidence.snapshot` 在导出的 `snapshots` 或对应目录的快照表中查找快照对象。
3. 根据快照的 `sha256` 打开 `.cache/snapshots/<sha256>.json`，并核对原始字节摘要。
4. 以快照根对象为起点解析 `evidence.pointer`，读取具体 URL、父模型 ID、canonical ID 或显式派生类型。
5. 按本文规则重做目标 ID 的定位，并检查该目标是否存在于当前模型记录中。

JSON Pointer 中 `~1` 表示 `/`，`~0` 表示 `~`，数组元素使用从零开始的索引。
例如 Ornith 的 FP8 URL 位于 `/deepreinforce~1ornith-1.0-397b/weights/1/url`。
父模型数组中的单条证据可以位于 `/cardData/base_model/0`，显式派生类型位于 `/cardData/base_model_relation`。
这些位置属于原始快照，不能拿它们直接读取导出 JSON 的 `facts` 对象。
运行 `cargo run --release --locked -- validate --evidence` 可以核对已导入字段与本地快照的对应关系。
缺失原始缓存时可以重新生成关系视图，但无法回放旧响应，重新请求上游也不保证恢复同一版本。

派生关系记录的是来源声明及其定位，不构成独立核验，也不会继承能力、上下文限制、价格或许可证。
关系视图尚未执行跨来源身份合并、权重摘要对比或推理行为验证。

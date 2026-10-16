# 发现公开模型候选

[项目首页](../README.md) · [维护流程](maintenance.md) · [发现规则](../config/discovery.yaml) · [种子选择依据](../research/2026-10-15/sources/seed-catalog.md)

`discover` 根据明确的组织、任务与搜索条件查询公开模型列表，将当前种子之外的仓库写入候选报告。
维护者核对候选的作者、任务、许可声明与具体模型身份后，再决定是否加入种子清单。
报告中的候选不会自动进入同步目录。

## 使用

从仓库根目录执行：

```text
cargo run --release --locked -- discover --source huggingface,modelscope
```

命令读取 [`config/discovery.yaml`](../config/discovery.yaml)，默认将 JSON 报告写入 `dist/discovery.json`。
已有种子分别来自 [`huggingface-models.txt`](../config/huggingface-models.txt) 和 [`modelscope-models.txt`](../config/modelscope-models.txt)。
来源内的准确仓库 ID 用于排除已有条目；相同字符串在两个来源出现时仍属于两个来源条目。
报告只包含 `--source` 选中来源的规则。

## 规则与数量上限

配置格式为 `schema_version: 1`，根级 `max_results: 200` 限制本次处理的列表条目总数。
`rules` 中每条规则有唯一 `id`、`source` 和 `limit`，默认每条最多处理 20 个条目。

| 来源 | 必需条件 | 可选条件 | 排序字段 |
| --- | --- | --- | --- |
| `huggingface` | `author` | `pipeline_tag`、`search` | `sort: createdAt` |
| `modelscope` | `owner` | `task`、`search` | `sort: last_modified` |

Hugging Face 查询显式发送 `direction=-1`，默认按创建时间从新到旧排列；降序参数见[官方参数说明](https://huggingface.co/docs/huggingface_hub/v0.34.6/en/package_reference/hf_api#huggingface_hub.HfApi.list_models)。

默认配置包含 8 条规则：

| 规则 ID | 查询范围 | 用途 |
| --- | --- | --- |
| `hf-autonlab` | `AutonLab` | 补充时间序列重建、填补及异常检测候选 |
| `hf-facebook-segmentation` | `facebook`，`image-segmentation` | 补充分割任务候选 |
| `hf-nvidia-robotics` | `nvidia`，`robotics` | 补充不同机器人本体与动作接口候选 |
| `hf-biohub` | `biohub` | 补充蛋白及生物分子结构任务候选 |
| `hf-prithvi` | `ibm-nasa-geospatial` | 补充地球观测与天气模型候选 |
| `hf-granite-geospatial` | `ibm-granite`，搜索 `geospatial` | 补充地理任务适配后的 checkpoint |
| `ms-qwen` | `Qwen` | 核对作者在 ModelScope 的公开发布 |
| `ms-wan` | `Wan-AI` | 核对视频模型作者在 ModelScope 的公开发布 |

列表每页请求 10 条，每规则最多处理 20 条，全局最多处理 200 条。
这些上限计量的是处理过的列表条目，包含已在种子中的仓库，而不是新增候选数量。
因此一条规则处理 20 条后，即使全部已存在，仍会耗尽该规则预算并产生零个新候选。
同一条目被不同规则返回时，每次处理都占用预算；候选输出按来源与仓库 ID 合并，并保留命中的规则及证据。

默认 8 条规则的预算之和为 160 条，低于全局 200 条上限。
按创建时间或修改时间排序并受数量限制的查询可能遗漏较早发布的模型。
`pipeline_tag` 和搜索词只是上游筛选条件，不能证明候选的实际能力或完整覆盖某一任务。

## 报告与证据

每个候选至少包含以下内容：

| 字段 | 含义 |
| --- | --- |
| `source` | 来源键，如 `huggingface` 或 `modelscope` |
| `upstream_id` | 列表中准确的来源内仓库 ID |
| `url` | 候选的公开仓库页面 |
| `matched_rules` | 发现该候选的规则 ID |
| `evidence` | 来源列表响应的 `snapshot` 与条目位置 `pointer` |

证据指针定位到保存的列表响应，表示上游在观察时返回了该条目。
它不代替模型详情、模型卡、权重许可或推理验证。
原始 JSON 快照和 HTTP 缓存沿用[维护流程中的缓存约定](maintenance.md#缓存与证据保存)。

报告分别记录规则执行状态：

| 状态 | 含义 |
| --- | --- |
| `complete` | 本条规则查询正常到达结束位置 |
| `limited` | 本条规则因规则预算或全局预算而停止，尚不能声明查询结果已经取尽 |
| `failed` | 本条规则因网络、响应或分页错误而未完成，报告保留错误信息 |
| `skipped` | 本条规则属于已选择的来源，但开始前全局预算已耗尽，因此本轮未执行 |

这些状态只描述配置规则的查询结果。
`complete` 不表示枚举了整个网站、组织的全部历史版本或某类模型的所有实现。
发生网络失败时仍生成包含 `failed` 状态的报告，CLI 返回非零状态；检查报告后再决定重试范围。

## 公开接口与边界

发现流程使用匿名 HTTP 请求访问固定的公开 API，不依赖 Hub SDK、账户凭据或模型权重下载。

| 来源 | 固定入口 | 参数依据 |
| --- | --- | --- |
| Hugging Face | `GET https://huggingface.co/api/models` | [Hub API 文档](https://huggingface.co/docs/hub/api)、[官方 `list_models` 参数文档](https://huggingface.co/docs/huggingface_hub/package_reference/hf_api#huggingface_hub.HfApi.list_models)及[SDK v2.2.0 参数实现](https://github.com/huggingface/huggingface_hub/blob/v2.2.0/src/huggingface_hub/hf_api.py#L2285) |
| ModelScope | `GET https://modelscope.cn/openapi/v1/models` | [官方列表实现](https://github.com/modelscope/modelscope_hub/blob/main/src/modelscope_hub/_openapi.py#L626)、[筛选参数实现](https://github.com/modelscope/modelscope_hub/blob/main/src/modelscope_hub/_openapi.py#L349)与[官方资源搜索说明](https://github.com/modelscope/modelscope-skills/blob/main/skills/ms-hub/SKILL.md#1-resource-search) |

Hugging Face 分页使用响应的 `Link` 信息，条件请求得到 304 时校验并保存响应中更新的分页链接，同时更新 ETag 与 Last-Modified；缺少相应响应头时保留缓存值。
官方 [v2.2.0 分页实现](https://github.com/huggingface/huggingface_hub/blob/v2.2.0/src/huggingface_hub/utils/_pagination.py)说明下一页链接已包含查询参数。
ModelScope 的公开 GET 列表接口失败时记录失败，不切换到私有接口、网页内部接口或认证 SDK。
官方 SDK 的 `require_token=False` 表示允许不提供 token，其认证解析仍可能读取本地 token 缓存；本项目通过直接 HTTP 请求保持匿名访问。
ModelScope 官方搜索说明给出的平台分页窗口不是本项目的采集预算；本项目始终使用每页 10 条、每规则最多 20 条的边界。
ModelScope 返回超过每页数量上限的页面，或尚未达到总数却少于每页数量的页面时，记录分页错误，避免按页码继续请求时跳过条目。
公开列表的可访问性与模型详情、权重访问权限是不同条件。

[`config/sources.yaml`](../config/sources.yaml) 保存来源范围、认证约定、条款状态与文档链接等描述信息。
它不覆盖可执行端点，也不授权发现流程使用该文件描述的认证来源。

## 值得审查的专业模型候选

以下是 2026-10-16 根据官方模型卡和作者资料整理的 8 个候选。
其中 GR00T N1.7、ESMFold2 与 MedSAM2 已经审查后加入种子，并保存人工声明，见[专业模型声明核查](specialized-models.md)。
候选表与默认发现规则各有用途；受组织条件、排序及预算限制，默认运行不保证返回表中每个仓库。

| 精确仓库 ID 与证据 | 可补充的任务样本 | 审查条件 |
| --- | --- | --- |
| [AutonLab/MOMENT-1-large](https://huggingface.co/AutonLab/MOMENT-1-large) | 时间序列重建、缺失值填补与基于重建的异常检测 | 分类接口不证明 checkpoint 已有训练好的分类头，保留具体任务的适配状态。 |
| [facebook/mask2former-swin-small-coco-panoptic](https://huggingface.co/facebook/mask2former-swin-small-coco-panoptic) | 带固定类别标签的 COCO 全景分割 | 区分具体 checkpoint 的任务和模型架构可支持的其他分割任务；卡片注明由 HF 团队撰写。 |
| [nvidia/GR00T-N1.7-3B](https://huggingface.co/nvidia/GR00T-N1.7-3B) | 多机器人本体的连续动作块预测 | 模型卡与作者仓库的 EA/GA 声明存在差异，见[声明核查](specialized-models.md#gr00t-n17)；本体标识、状态输入、动作维度与后训练要求需要分别记录。 |
| [biohub/ESMFold2](https://huggingface.co/biohub/ESMFold2) | 蛋白及生物分子复合物结构预测 | 区分序列表征与结构坐标输出；可选 MSA 条件不能推广至 Fast 变体。 |
| [ibm-granite/granite-geospatial-uki-flooddetection](https://huggingface.co/ibm-granite/granite-geospatial-uki-flooddetection) | 遥感洪水与地表水分割 | 保留光学波段、SAR、云掩码、英国与爱尔兰训练地域，以及洪水与永久水体合并为水类的标签定义。 |
| [ibm-nasa-geospatial/Prithvi-WxC-1.0-2300M-rollout](https://huggingface.co/ibm-nasa-geospatial/Prithvi-WxC-1.0-2300M-rollout) | 连续天气预测 | 该版经过额外训练，输入时间间隔和预测步长为 6 小时，区别于已有基础版。 |
| [ibm-granite/granite-geospatial-wxc-downscaling](https://huggingface.co/ibm-granite/granite-geospatial-wxc-downscaling) | 天气数据空间降尺度 | 按具体 checkpoint 保存变量、网格与放大比例，避免混合不同数据集上的实验声明。 |
| [wanglab/MedSAM2](https://huggingface.co/wanglab/MedSAM2) | 3D 医疗影像及医疗视频的提示式分割 | 仓库含多个 checkpoint；建议明确 `MedSAM2_2411.pt` 并补 revision，权重声明限研究与教育。 |

这些建议来自作者声明，尚未运行模型或审核完整许可条款。
增加种子只会增加仓库级元数据；任务条件仍应通过模型卡证据和[人工条件声明](assertions.md)保留。

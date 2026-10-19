# Large Model (Metadata) Database

用于收集、整理和维护模型结构化元数据的 Rust CLI 项目，覆盖语言、多模态、检索及其他模型类型。
YAML 保存来源目录、人工身份映射与条件声明，JSON 提供保留来源证据的结构化视图。
任务、模态、限制、参数、能力和价格从原始事实派生为有类型的领域视图（profile），支持字段筛选、显式关系查询和条件求值。
每个目录导入字段保留来源快照与 JSON Pointer，便于回溯原始响应。
官方发布博客、模型卡和详情页保存完整选定段落与带作用域的声明，可交叉核对目录字段并按来源优先级选择模型元数据。
官方目录页保存带来源定位的候选链接，便于审查后补充型号与页面清单。
当前格式为 `schema_version: 1`，仍处于早期维护阶段。

## 开始使用

需要 Rust 1.99 或更新的稳定版，以及仓库中的 `Cargo.lock`。
完整 YAML 目录建议使用 release 构建，首次编译后可直接运行 `target/release/lmdb`。
在仓库目录运行：

```text
cargo run --release --locked -- sources
cargo run --release --locked -- validate
cargo run --release --locked -- stats
cargo run --release --locked -- query Qwen --kind model --limit 10
cargo run --release --locked -- query --task robotics --kind model
cargo run --release --locked -- inspect huggingface:model:nvidia/GR00T-N1.7-3B
cargo run --release --locked -- export --output dist/registry.json
```

`sources`、`stats`、`query`、`inspect`、`identities`、`relations` 和 `rules` 输出 JSON。
本地校验、查询、导出和证据归档恢复无需联网；`sync` 更新公开来源，`discover` 查询公开候选。
首次构建由 Cargo 下载锁定依赖。
全局 `--root PATH` 指定数据目录，例如 `cargo run --release --locked -- --root /path/to/lmdb validate`。

```text
cargo run --release --locked -- sync
cargo run --release --locked -- sync --source modelsdev,openrouter
cargo run --release --locked -- sync --source vercel
cargo run --release --locked -- sync --source official
cargo run --release --locked -- documents resolve huggingface:model:Qwen/Qwen3-8B
cargo run --release --locked -- sync --source huggingface --source modelscope
cargo run --release --locked -- renormalize --source huggingface
cargo run --release --locked -- query --observation observed --source huggingface --limit 10
cargo run --release --locked -- validate --evidence
cargo run --release --locked -- schema --check
```

默认同步 `modelsdev`、`huggingface`、`modelscope`、`openrouter`、`vercel`、`official` 六个公开来源。
Hugging Face 和 ModelScope 只同步配置中明确列出的模型；`discover` 将有数量上限的公开候选写入报告，审查后再加入清单。
`sources` 读取 `config/sources.yaml` 中 14 个来源的覆盖范围、认证约定和条款状态，其中 6 个公开来源、8 个认证来源，列出约定时不读取凭据。
认证来源需要同时指定 `--source NAME` 与 `--authenticated`，本轮未进行真实认证采集。
完整目录全部分页成功后更新，HF 与 ModelScope 的精选仓库逐项保存成功结果；失败项保留旧记录，命令最终以非零状态报告失败。
官方页面按页记录失败；同一来源定义下保留的旧内容会显式标记过期。
页面身份或结构不符也会记录失败，并保留失败响应供提取器修复后重放。
`documents reparse` 可从缓存原文离线更新提取结果，`documents resolve RECORD_ID` 展示来源选择、限定条件与冲突，详见[官方页面与字段核对](docs/official-documents.md)。
`renormalize --source NAME` 从已缓存的原始 JSON 补齐当前适配器字段映射，保留证据获取时间与同步清单。

普通 `validate` 不需要原始响应。
`validate --evidence` 需要本地 `.cache/snapshots/` 的目录原始 JSON，以及 `.cache/documents/` 的官方页面原文；普通克隆不包含这些文件。
重新同步不会自动恢复已被上游覆盖的历史快照。
使用 `evidence archive --output PATH` 保存原始字节及关联数据版本的 manifest，在相同数据版本的克隆中用 `evidence restore --input PATH` 恢复后即可离线重放证据。
具体步骤见[维护流程](docs/maintenance.md#缓存与证据保存)。

## 当前数据如何理解

`stats` 返回本地各来源的模型记录、服务记录与同步状态，以及官方页面、人工条件声明和身份集合的数量。
来源记录、目录候选和人工身份各有计数范围，不能相加为去重后的模型总数。
身份集合只覆盖已审查的成员。
官方页面清单包含 20 个厂商的 56 个页面，其中 9 个为目录页；配置数量不代表成功采集数量。

| 内容 | 范围 |
| --- | --- |
| models.dev | 模型目录与服务条目，保留上游明确的模型关联 |
| Hugging Face、ModelScope | 精选仓库的身份、任务、谱系、许可及版本信息；HF 还保留 Tokenizer、量化、运行信息与文件清单等快照字段 |
| OpenRouter | 平台目录中的模态、限制、参数与价格原值 |
| Vercel AI Gateway | 匿名公开路由目录中的服务记录，保存类型、模态、限制、参数、价格与平台声明 |
| 领域视图（profile） | 任务与模态标签、限制、参数、能力和价格结构，每个派生值保留事实定位 |
| 官方页面 | 按清单采集发布博客、模型卡、详情页与文档，保存原文段落、声明、比较与字段选择结果；支持 PDF 页码和仓库引用跟踪 |
| 官方目录页 | 厂商型号、发布博客和下载索引中的候选链接，保存页面快照与链接定位，审查后再补充模型绑定 |
| 人工条件声明 | 官方文档中的角色、功能组合、音视频、检索及专业模型限制，条件求值保留未知和冲突 |
| 身份与关系 | 精确成员映射、接口绑定，以及有来源证据的服务、引用和派生关系 |
| 认证适配器 | 请求说明、字段映射和分页实现，使用合成响应验证，在线访问仍待验证 |

模型的输入输出、上下文和能力筛选可使用已选官方字段，限定或冲突字段保留状态，服务仍使用自己的平台事实。
原始 `facts` 与 `profile` 保留目录声明，`resolved_metadata` 单独呈现字段选择与全部候选。
Vercel AI Gateway 的记录 ID 为 `vercel:service:<上游 ID>`，不根据路由名称推断模型关联。
其 `pricing` 保留原始结构和证据，当前未确认币种与计费口径，价格视图将这些字段保存在 `unclassified`。

记录 ID 包含来源和类型，同一模型在不同来源出现时会有多个记录。
统计数字表示来源记录数，不能视为跨来源去重后的模型总量。
来源本轮未返回的旧记录会保留，`last_sync` 记录本次范围、完成状态、返回、缺席、失败和采集快照。
查询与导出暴露 `observed`、`missing`、`not_requested`、`failed` 或 `unknown`，可用 `query --observation STATUS` 筛选。
历史清单没有记录采集范围和完成状态时保持未知，离线迁移不会补写网络观察。
缺席不表示退役，目录中存在某条记录也不证明模型仍可调用。
字段未变化时保留旧证据，快照日期表示该证据的获取时间，最近尝试时间为 `last_sync.completed_at`，最近整个范围成功时间为 `last_sync.last_successful_at`。

专业样本包括 GR00T 的本体相关动作表示、ESMFold2 的蛋白结构输出、按 checkpoint 区分的 MedSAM2，以及 FAST 的 `action_tokenizer` 角色。
FAST 的 `robotics` 任务标签表示领域归类，其角色应连同声明读取；详见[专业模型](docs/specialized-models.md)。

## 模型 API Mock 服务

`mock serve` 启动独立的本地 HTTP 服务，用固定响应测试客户端的原生协议、参数、错误、SSE 和工具多轮会话。
内置配置随二进制发布，运行时无需注册表目录或上游凭据。

```text
cargo run --release --locked -- mock list
cargo run --release --locked -- mock serve --provider openai --model gpt-6.1-sol --port 8080
cargo run --release --locked -- mock scenarios
cargo run --release --locked -- mock rankings --top 20
```

OpenAI 客户端可将 base URL 设为 `http://127.0.0.1:8080/v1`，使用测试凭据 `mock-key`。
提供方包括 OpenAI、Anthropic、Gemini、Meta，以及 OpenRouter、Vercel 网关；其他兼容提供方需要自定义模型配置。
首批按 Artificial Analysis Intelligence Index 前 20 个评测变体选取，涉及 8 个基础模型候选，其中 7 个具有已映射接口。
Muse Wonder Full 的公开 API 缺口、精确 effort 映射以及前 50 名扩展均保留状态。
`--suite coverage`、`x-mock-session` 和 `x-mock-scenario` 可安排、重放和查看测试分支。
使用量、签名与延迟是合成数据，真实 token 探针仍是待办。
启动、各协议示例、工具续轮、脚本、覆盖报告和实现边界见[Mock 服务](docs/mock-server.md)。

## 文档索引

| 文档 | 内容 |
| --- | --- |
| [数据模型](docs/data-model.md) | ID、模型与服务、字段证据、条件声明及导出结构 |
| [领域字段](docs/domain-fields.md) | profile、任务标签、计量单位、价格和字段筛选 |
| [身份与关系](docs/relationships.md) | 精确成员、接口绑定、显式关系和缺失目标 |
| [规则查询](docs/rule-queries.md) | 三值条件匹配、请求上下文、未知与冲突 |
| [官方页面与字段核对](docs/official-documents.md) | 完整段落、目录候选、字段作用域、来源优先级、离线重解析与证据 |
| [候选发现](docs/discovery.md) | 公开仓库发现规则、官方目录候选、预算和种子审查 |
| [专业模型](docs/specialized-models.md) | 机器人、蛋白结构、医疗分割与 tokenizer 的具体样本 |
| [维护流程](docs/maintenance.md) | 命令、更新、校验、缓存、依赖与来源许可 |
| [元信息采集计划](docs/metadata-collection-plan.md) | 证据与状态修复、字段迁移、验收及后续来源接入条件 |
| [条件声明](docs/assertions.md) | 人工证据、状态、条件与实际样本的解释 |
| [认证来源](docs/authenticated-sources.md) | 来源键、环境变量、字段与分页边界 |
| [Mock 服务](docs/mock-server.md) | 本地模型 API、原生协议、确定性会话、工具续轮、SSE 与错误场景 |
| [代码与数据许可](docs/licensing.md) | 项目代码许可、上游声明与数据再分发范围 |
| [项目决定](project/decisions.md) | 已采用的实现选择与待决事项 |
| [路线与进度](project/roadmap.md) | 当前能力、验证入口与后续工作 |
| [研究档案](research/2026-10-15/README.md) | 初始来源、框架、架构与同类项目调研 |
| [Mock 协议研究](research/2026-10-19/mock-server.md) | AA 排名口径、提供方一手文档、冲突与待验证项 |

## 目录

```text
lmdb/
├── Cargo.toml / Cargo.lock   Rust 依赖与锁定版本
├── src/                     CLI、适配器、HTTP、存储与校验
├── config/                  来源约定、发现规则与精选模型清单
├── data/
│   ├── catalogs/            每个来源一个 YAML 目录
│   ├── assertions/          人工审查的条件声明
│   ├── documents/           官方页面元数据、目录候选、选定段落与声明
│   ├── mock/                Mock 提供方配置与排名快照
│   └── identities/          模型身份与接口绑定证据
├── schemas/                 由 Rust 类型生成的 JSON Schema
├── tests/fixtures/          合成响应与回归样本
├── scripts/                 仅依赖标准库的证据包工具
├── docs/                    使用与维护文档
├── reports/discovery/       按日期保留的候选发现报告
├── licenses/                已保留的上游许可证
├── project/                 项目范围、决定与进度
└── research/                按日期保存的研究档案
```

`.cache/` 保存原始响应与 HTTP 缓存，`dist/` 保存生成的导出，两者均不进入 Git。
研究档案保留当时的建议，当前实现以 `src/`、`docs/` 和 `project/decisions.md` 为准。

## 依赖与使用边界

当前核心使用 Rust，证据包脚本使用 Python 3 标准库，没有第三方运行时依赖。
未来 Python 任务需要第三方依赖时，`pyproject.toml` 作为直接运行时依赖的唯一来源。
每次修改 `pyproject.toml` 后运行 `uv lock` 并提交 `uv.lock`，再通过 `uv export --format requirements.txt --frozen --no-header --no-annotate --no-hashes --output-file requirements.txt` 生成 legacy 安装文件。
uv 部署使用 `uv sync --frozen`，依赖一致性检查使用 `uv lock --check`。

上游目录原值、人工文档声明和实际模型行为有不同的验证范围。
身份映射要求精确成员和审查证据，名称相似或共享 URL 不会合并身份。
规则求值只处理已记录的条件，不执行模型推理；价格视图也不计算请求报价。
公开 API 可访问不等于取得批量再分发许可；模型权重许可、目录数据许可和文档许可需要分别核查，详见[维护流程](docs/maintenance.md#来源与许可)。

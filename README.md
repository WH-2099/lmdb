# Large Model (Metadata) Database

用于收集、整理和维护模型结构化元数据的 Rust CLI 项目，覆盖语言、多模态、检索及其他模型类型。
YAML 保存来源目录和人工审查的条件声明，JSON 用于下游读取。
每个导入字段保留来源快照与 JSON Pointer，支持离线校验和本地证据核对。
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
cargo run --release --locked -- export --output dist/registry.json
```

`sources`、`stats`、`query` 输出 JSON。
`validate`、`stats`、`query` 和 `export` 读取本地数据；`sync` 才获取上游目录。
首次构建由 Cargo 下载锁定依赖。
全局 `--root PATH` 指定数据目录，例如 `cargo run --release --locked -- --root /path/to/lmdb validate`。

```text
cargo run --release --locked -- sync
cargo run --release --locked -- sync --source modelsdev,openrouter
cargo run --release --locked -- sync --source huggingface --source modelscope
cargo run --release --locked -- validate --evidence
cargo run --release --locked -- schema --check
```

默认同步 `modelsdev`、`huggingface`、`modelscope`、`openrouter` 四个公开来源。
Hugging Face 和 ModelScope 只采集配置中明确列出的模型。
认证来源需要同时指定 `--source NAME` 与 `--authenticated`，本轮未进行真实认证采集。
同步逐来源保存成功结果；失败来源保留旧目录，命令最终以非零状态报告失败。

普通 `validate` 不需要原始响应。
`validate --evidence` 还需要本地 `.cache/snapshots/` 中所有被目录引用的原始 JSON；普通克隆不包含这些文件。
重新同步不会自动恢复已被上游覆盖的历史快照。

## 当前数据如何理解

2026-10-15 的公开采集得到以下来源记录，已通过本地缓存证据校验。
这些是本次采集统计，后续以 `stats` 的输出为准。

| 来源 | 模型记录 | 服务记录 |
| --- | ---: | ---: |
| models.dev | 554 | 8,914 |
| Hugging Face | 147 | 0 |
| ModelScope | 22 | 0 |
| OpenRouter | 0 | 669 |
| 合计 | 723 | 9,583 |

另有 25 条人工条件声明，其中 2 条保留来源冲突。

| 内容 | 范围 |
| --- | --- |
| models.dev | 模型目录与服务条目，保留上游明确的模型关联 |
| Hugging Face、ModelScope | 精选仓库的身份、任务、谱系、许可及版本信息 |
| OpenRouter | 平台目录中的模态、限制、参数与价格原值 |
| 人工条件声明 | 官方文档中的角色、功能组合、音视频、检索及专业模型限制，包含待裁定冲突 |
| 认证适配器 | 请求说明、字段映射和分页实现，使用合成响应验证，在线访问仍待验证 |

记录 ID 包含来源和类型，同一模型在不同来源出现时会有多个记录。
统计数字表示来源记录数，不能视为跨来源去重后的模型总量。
来源本轮未返回的旧记录会保留，因此目录中存在某条记录不证明模型仍可调用。
字段未变化时保留旧证据，快照日期表示该证据的获取时间，不是最近一次同步时间。

## 文档索引

| 文档 | 内容 |
| --- | --- |
| [数据模型](docs/data-model.md) | ID、模型与服务、字段证据、条件声明及导出结构 |
| [维护流程](docs/maintenance.md) | 命令、更新、校验、缓存、依赖与来源许可 |
| [条件声明](docs/assertions.md) | 人工证据、状态、条件与实际样本的解释 |
| [认证来源](docs/authenticated-sources.md) | 来源键、环境变量、字段与分页边界 |
| [代码与数据许可](docs/licensing.md) | 项目代码许可、上游声明与数据再分发范围 |
| [项目决定](project/decisions.md) | 已采用的实现选择与待决事项 |
| [路线与进度](project/roadmap.md) | 当前能力、验证入口与后续工作 |
| [研究档案](research/2026-10-15/README.md) | 初始来源、框架、架构与同类项目调研 |

## 目录

```text
lmdb/
├── Cargo.toml / Cargo.lock   Rust 依赖与锁定版本
├── src/                     CLI、适配器、HTTP、存储与校验
├── config/                  HF 与 ModelScope 精选模型清单
├── data/
│   ├── catalogs/            每个来源一个 YAML 目录
│   └── assertions/          人工审查的条件声明
├── schemas/                 由 Rust 类型生成的 JSON Schema
├── tests/fixtures/          合成响应与回归样本
├── docs/                    使用与维护文档
├── licenses/                已保留的上游许可证
├── project/                 项目范围、决定与进度
└── research/                按日期保存的研究档案
```

`.cache/` 保存原始响应与 HTTP 缓存，`dist/` 保存生成的导出，两者均不进入 Git。
研究档案保留当时的建议，当前实现以 `src/`、`docs/` 和 `project/decisions.md` 为准。

## 依赖与使用边界

当前实现直接使用 Rust，没有 Python 包装层。
未来出现独立 Python 任务时，`pyproject.toml` 作为直接运行时依赖的唯一来源。
每次修改 `pyproject.toml` 后运行 `uv lock` 并提交 `uv.lock`，再通过 `uv export --format requirements.txt --frozen --no-header --no-annotate --no-hashes --output-file requirements.txt` 生成 legacy 安装文件。
uv 部署使用 `uv sync --frozen`，依赖一致性检查使用 `uv lock --check`。

上游目录原值、人工文档声明和实际模型行为有不同的验证范围。
本项目不从名称相似推断模型同一性，也没有执行条件规则或推理探针。
公开 API 可访问不等于取得批量再分发许可；模型权重许可、目录数据许可和文档许可需要分别核查，详见[维护流程](docs/maintenance.md#来源与许可)。

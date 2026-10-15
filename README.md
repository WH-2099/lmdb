# Large Model (Metadata) Database

用于收集、整理和维护模型结构化元数据的项目，覆盖语言、多模态及其他模型类型。
当前处于调研阶段，尚无正式 schema、数据集或采集程序。

本仓库围绕模型身份、来源、能力、条件限制和模型关系建立可用 Git 审查的元数据层，后续扩展到提供方和托管平台。
计划以 YAML 保存审定记录。

## 从这里开始

- [项目跟踪](project/README.md)：目标、阶段、决定与待办。
- [项目范围](project/brief.md)：原始企划与本阶段边界。
- [第一轮调研](research/2026-10-15/README.md)：来源、同类项目、技术栈与架构建议。
- [调研区维护约定](research/README.md)：临时材料的整理与归档方式。

## 当前目录

```text
lmdb/
├── README.md
├── project/                  持续跟踪项目
│   ├── README.md
│   ├── brief.md
│   ├── decisions.md
│   └── roadmap.md
└── research/                 临时调研工作区，按批次归档
    ├── README.md
    └── 2026-10-15/
        ├── README.md
        ├── sources/          来源目录与真实限制
        ├── comparisons/      同类项目与代码考察
        ├── architecture/     身份、证据和采集流程
        ├── stack/            Rust、Python、存储候选
        └── evidence/         方法、访问记录与验证边界
```

## Python 依赖约定

后续引入 Python 时，`pyproject.toml` 是直接运行时依赖的唯一来源。
每次修改 `pyproject.toml` 后运行 `uv lock`，提交 `uv.lock`，再运行 `uv export --format requirements.txt --frozen --no-header --no-annotate --no-hashes --output-file requirements.txt` 生成 legacy 安装文件。
走 uv 的部署使用 `uv sync --frozen`。
本轮只有研究文档，因此还没有创建这些依赖文件。

# 第一轮：模型元数据来源与架构调研

[返回调研区](../README.md) · [项目跟踪](../../project/README.md) · [路线与待办](../../project/roadmap.md)

调研日期：2026-10-15。
状态：第一轮完成，结论用于下一阶段样本设计，正式 schema 与依赖尚未定案。
范围：二十六组来源调查、九项同类项目比较、五个组织的十七类接口限制、四类专业模型样本、有限源码考察、采集与存储技术选型。

## 先读这些结论

1. **先验证现有目录能满足多少需求。**
   models.dev 已有模型本体、服务条目、继承、细化能力和同步流程；LiteLLM、genai-prices 也提供大量可借鉴字段与维护方法。
   新项目应先验证「字段证据、条件限制、来源冲突」的实际缺口，再决定独立数据层的规模。
   见 [同类项目及固定提交证据](comparisons/projects.md)。
2. **不能把特例压成几个独立布尔值。**
   普通图片输入和 system instruction 图片是不同问题。
   本轮读取的 Gemini generateContent 文档限定系统指令为文本，尚无可靠证据支持“部分型号允许 system 图片”的差异。
   见 [Gemini 核查与其他真实限制](sources/constraints.md)。
3. **目录的覆盖范围必须显式记录。**
   OpenRouter 的默认输出模态过滤、分页和提供方端点视角会影响可见集合；Hugging Face 和 ModelScope 的仓库条目也不能直接当作去重后的模型本体。
   Artificial Analysis 的旧参考页、新文档和 OpenAPI 存在端点与访问层级差异。
   见 [来源目录](sources/catalog.md) 与 [实际采样](evidence/source-probes.md)。
4. **YAML 与 Git 适合作为审定数据起点。**
   原始资料、提取声明和人工审定值需要能关联，但不必因此部署多个存储系统。
   JSON 及按需提供的 SQLite 可以从同一审定版本生成。
   见 [架构建议](architecture/README.md) 与 [存储权衡](stack/storage.md)。
5. **Rust 为核心方向，具体依赖仍需验收。**
   建议 Tokio、reqwest、Serde 为主要基础，Python 与浏览器用于确有必要的辅助采集。
   YAML 解析器、HTTPX 系列及新 Python 版本的维护和兼容状态值得重新核查，不能沿用过时模板。
   见 [技术栈总览](stack/README.md)。

## 树形索引

```text
2026-10-15/
├── README.md
├── sources/
│   ├── README.md             来源阅读入口
│   ├── catalog.md            官方、聚合、跨任务来源地图
│   ├── constraints.md        真实限制、冲突、YAML 讨论示例
│   └── specialized-models.md 视觉、时序、机器人和科学模型
├── comparisons/
│   ├── README.md
│   └── projects.md           九项对比、固定提交代码考察
├── architecture/
│   ├── README.md
│   ├── metadata.md           身份、关系、条件、证据、时间
│   └── pipeline.md           获取、规范化、校验、审查、发布
├── stack/
│   ├── README.md
│   ├── collection.md         Rust、异步 Python、浏览器、抽取框架
│   └── storage.md            YAML、JSON Schema、数据库与快照
└── evidence/
    ├── README.md             方法、验证程度与边界
    └── source-probes.md      少量公开 API 请求的实测记录
```

## 按问题阅读

| 需要回答的问题 | 报告 |
| --- | --- |
| 哪些渠道能提供哪些数据，缺哪些类别？ | [来源目录](sources/catalog.md) |
| 非语言模型有哪些专属信息和边界？ | [专业模型样本](sources/specialized-models.md) |
| 能否表达角色、模态、工具和参数组合？ | [限制案例](sources/constraints.md) |
| 同类项目已经做了什么，值得复用哪些部分？ | [项目比较](comparisons/projects.md) |
| 如何避免把不同平台的事实混在一起？ | [元数据与证据](architecture/metadata.md) |
| 抓取失败、资料冲突、更新和发布如何处理？ | [采集流程](architecture/pipeline.md) |
| Rust 和 Python 如何分工，选哪些候选库？ | [采集技术栈](stack/collection.md) |
| YAML 怎样校验，需要什么数据库？ | [存储技术栈](stack/storage.md) |
| 哪些结论经过 API 或源码核查？ | [证据索引](evidence/README.md) |
| 接下来具体推进什么？ | [项目路线](../../project/roadmap.md) |

## 建议的最小方向

| 环节 | 本轮建议 | 当前状态 |
| --- | --- | --- |
| 范围 | 以约 20 个跨任务、跨平台样本验证表达能力 | 样本名单待选 |
| 数据语义 | 稳定身份、服务作用域、证据、条件和未知值 | 讨论阶段 |
| 外部输入 | 优先现有目录加官方文档，保留原始 ID 与字段来源 | 完成来源地图与部分采样 |
| 核心实现 | Rust 批处理；共享校验、规范化和导出 | 尚未实施 |
| 辅助工作 | Python 异步脚本；必要时 Playwright | 尚未安装或兼容测试 |
| 维护 | YAML + Git + 人工审查，轻量 CI | 尚未建立正式数据或工作流 |
| 消费 | 确定性 JSON，按需派生查询存储 | 尚未发布 |

用户已确定的约束与本轮建议分别登记在 [决定记录](../../project/decisions.md)。

## 下一轮需要解决的事

先选真实样本，逐条附上来源，尝试表达同模型跨平台差异、Gemini 系统指令规则、工具组合、媒体限制、向量维数、模型衍生关系和滚动别名。
同时保留至少一处未裁定冲突，检查 schema 是否能诚实表达“不知道”。
用这些材料判断是否只需给现有目录增加补充记录，再确定第一版 schema 和采集范围。
性能、库兼容、数据再分发条件及实际推理行为尚未验证，见 [验证边界](evidence/README.md)。

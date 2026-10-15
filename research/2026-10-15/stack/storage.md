# YAML、校验、快照与查询存储

调研日期：2026-10-15。
状态：存储分工与解析器候选研究；未定义正式 schema，未创建数据库。

[返回技术栈索引](README.md) · [采集与任务执行](collection.md) · [元数据架构](../architecture/metadata.md) · [处理流程](../architecture/pipeline.md)

## 先明确各类数据的职责

建议把已审查记录、采集证据和可重建的查询产物放到不同位置，避免原始网页和运行状态不断扩大 Git 历史。

| 内容 | 建议位置 | 写入规则 | 可否重建 |
| --- | --- | --- | --- |
| 已审查的模型与关联元数据 | Git 中的 YAML | 通过 PR 形成可解释的变更 | Git 是权威来源 |
| 原始 API 响应、HTML、相关文档快照 | 独立本地目录；需要时转对象存储 | 以内容摘要定位，避免覆盖已有证据 | 上游变化后可能无法重新获取 |
| 抓取状态、ETag、游标、最近成功时间 | 起初小型状态文件；并发和查询需要时用 SQLite | 与已审查数据分开，失败可诊断 | 部分可重建，历史运行证据不一定可补 |
| JSON、SQLite、Parquet 等发布产物 | CI artifact / release / 静态存储 | 从明确的 Git commit 确定性生成 | 必须可重建 |

这是项目建议的职责划分，不要求第一版同时引入四种存储系统。
最小实现可以从 YAML + Git、独立快照目录和 JSON 导出开始。
只有实际查询或运行状态管理需要时才增加 SQLite。

## YAML 的价值与约束

用户已选择 YAML 作为维护格式，优势是记录清晰、可注释、适合 PR 审查。
代价是 YAML 的类型、别名、标签及键语义比 JSON 更丰富；“能解析”不等于“符合项目约定”。
YAML 1.2.2 规范定义了 mappings、tags、anchors/aliases 与多种 schema，因此格式约定必须明确到解析后的值，而不只检查文件扩展名。[YAML 1.2.2 规范](https://yaml.org/spec/1.2.2/)

建议下一阶段先讨论以下受限输入约定，再选择解析器：

- UTF-8、每个文件一个文档。
- 值限定为 JSON 可表达的字符串、布尔、有限数字、null、数组与对象；对象键必须是字符串。
- 重复键直接报错，避免同一字段被静默覆盖。
- 暂不允许锚点、别名、merge key、自定义 tag、include、环境变量替换或表达式求值。
- 模型 ID、日期、版本和枚举文本按 schema 显式规定为字符串，避免隐式类型推断。
- 数字溢出、NaN、Infinity、多文档、超深嵌套和超大文件均应明确拒绝。

这些是待确认的项目约定，不是 YAML 规范默认禁止的内容，也不能假设任何候选库全部默认实现。
若要拒绝所有 tag，普通 DOM 可能已丢失内建 tag 的表示信息；实际实现可能需要解析事件检查。
应先用真实需求验证是否值得保留这些严格限制，不能为了完整支持 YAML 语言而写一套大型适配器。

推荐“一条可独立审查的记录对应一个文件”作为布局起点，具体实体边界由架构讨论决定。
字段顺序稳定、格式化统一、无语义变化不重写文件，可以减少同步 PR 的噪声。
注释适合解释维护理由；必须机器读取的来源、条件和不确定状态应写入正式字段，不能只存在于注释。

## Rust YAML 解析器

以下状态于 2026-10-15 核验，版本依据优先采用 registry 与固定版本文档。
GitHub Releases、搜索摘要和 README 缓存可能滞后，因此不把搜索结果顶部的版本当作最新发布。

| 项目 | 已核实状态 | 本项目判断 | 官方依据 |
| --- | --- | --- | --- |
| serde_yaml | 2024-03-25 归档，明确停止维护 | 新项目排除 | [仓库](https://github.com/dtolnay/serde-yaml) |
| serde_yaml_ng | 0.10.0 / 2024-05-26；README 明确 YAML 1.1；仍依赖 unsafe-libyaml | 历史比较，暂不选 | [README](https://github.com/acatton/serde-yaml-ng)、[registry](https://crates.io/api/v1/crates/serde_yaml_ng)、[Cargo.toml](https://raw.githubusercontent.com/acatton/serde-yaml-ng/master/Cargo.toml) |
| serde_yml | 2026-05-28 归档；0.0.13 是转发至 noyalib 的弃用兼容层 | 新项目排除 | [仓库](https://github.com/sebastienrousseau/serde_yml)、[RustSec](https://rustsec.org/advisories/RUSTSEC-2025-0068.html) |
| yaml-rust2 | 0.13.0 / 2026-09-11；维护者将发展重心转向 Saphyr | 低层解析备选 | [registry](https://crates.io/api/v1/crates/yaml-rust2)、[维护说明](https://github.com/Ethiraric/yaml-rust2/issues/26) |
| saphyr / saphyr-parser | 0.2.2 / 2026-10-15；YAML 1.2，DOM 与事件解析分层，MSRV 1.85 | 需要表示信息与精细 lint 时优先评估 | [固定版本文档](https://docs.rs/saphyr/0.2.2/saphyr/)、[registry](https://crates.io/api/v1/crates/saphyr) |
| serde-saphyr | 2.0.0 / 2026-10-13；MSRV 1.89；当前后端为 granit-parser | 重点候选 | [固定版本文档](https://docs.rs/serde-saphyr/2.0.0/serde_saphyr/)、[版本列表](https://docs.rs/crate/serde-saphyr/latest) |
| yaml_serde | 0.10.7 / 2026-08-18；YAML 组织维护；MSRV 1.82 | 重点候选 | [仓库](https://github.com/yaml/yaml-serde)、[registry](https://crates.io/api/v1/crates/yaml_serde) |

`serde-saphyr` 并非 Saphyr 官方项目，而且 2.0.0 已不使用 saphyr-parser。
不能根据名称或旧 README 判断传递依赖。
它自身禁止 unsafe，但这不等于传递依赖全部无 unsafe，也不等于对内存耗尽有完整防护。[serde-saphyr 2.0.0](https://docs.rs/serde-saphyr/2.0.0/serde_saphyr/)

`yaml_serde` 当前依赖 `libyaml-rs 0.3`，其来源是 libyaml 的 c2rust 转译，仍包含 unsafe Rust。
“无需 C 编译器”和“全部 safe Rust”是两个不同事实；组织归属也不能替代输入边界测试。[Cargo.toml](https://raw.githubusercontent.com/yaml/yaml-serde/main/Cargo.toml)、[libyaml-rs](https://github.com/yaml/libyaml-rs)

`serde_yml` 作者对 0.0.13 的修复描述与 RustSec 的状态存在差异：本次 RustSec 页面仍标记无已修复版本。
本项目没有采用这个已弃用兼容层的必要，也不应以作者声明代替安全审计结论。[RUSTSEC-2025-0068](https://rustsec.org/advisories/RUSTSEC-2025-0068.html)

### 两个重点候选的实际边界

| 验收点 | serde-saphyr 2.0.0 | yaml_serde 0.10.7 |
| --- | --- | --- |
| 重复键 | 默认报错，也允许选择覆盖策略；本项目应固定报错 | Mapping 路径检查重复键；不能据此推断任意 Rust map 路径均相同 |
| YAML 1.1 布尔值 | `strict_booleans` 默认 false，需要显式调整 | 需要对实际反序列化目标验收 `yes/no/on/off` |
| merge key | 默认展开，可设为报错 | `apply_merge()` 是显式展开接口；项目仍需拒绝相关语法 |
| tag | 可拒绝不支持的 tag，但这不等于拒绝全部 tag | Value 可包含 Tagged 值；需要检查后再转换 |
| 输入预算 | 支持字节、深度、节点、事件、锚点/别名预算 | 资源上限及实际路径需进一步验证 |
| JSON 值转换 | 默认拒绝泛型中的非有限浮点；不得以转换成 null 掩盖非法输入 | 任意键、tag、数字边界需在转换前验证 |

依据：[Options](https://docs.rs/serde-saphyr/2.0.0/serde_saphyr/struct.Options.html)、[Budget](https://docs.rs/serde-saphyr/2.0.0/serde_saphyr/struct.Budget.html)、[yaml_serde Mapping 源码](https://raw.githubusercontent.com/yaml/yaml-serde/main/src/mapping.rs)、[Value 反序列化](https://raw.githubusercontent.com/yaml/yaml-serde/main/src/value/de.rs)、[Value 与 merge](https://raw.githubusercontent.com/yaml/yaml-serde/main/src/value/mod.rs)。

先保留这两个候选，用同一组十余个小样例检查重复键、非字符串键、别名放大、tag、深度、隐式布尔、版本/日期字符串、非有限数字、数字溢出和多文档。
样例必须沿未来真正使用的反序列化路径执行，并同时检查错误位置和内存上限。
当前不冻结解析器，也不宣称某个 fork 已成为社区唯一方向。

## 结构校验与语义校验

Rust `jsonschema` 是推荐的结构校验候选，本次 docs.rs 为 0.58.6（2026-10-06）。
它支持 Draft 4/6/7/2019-09/2020-12，覆盖程度随 draft 而不同，因此不能声称所有关键词都完整实现。[jsonschema 0.58.6](https://docs.rs/jsonschema/0.58.6/jsonschema/)

建议 schema 草案明确声明 Draft 2020-12，YAML 先解析并通过输入约定检查，再转为 JSON 兼容值进行校验。
同一份 schema 编译后复用，验证阶段主要是同步内存计算，无需每条记录都启动异步任务。
需要 URI、日期等格式检查时，显式配置 `format` 校验并加入对应样例；不能依赖各 draft 的默认行为。[jsonschema 文档](https://docs.rs/jsonschema/0.58.6/jsonschema/)

该库能解析网络或本地文件 `$ref`。
建议关闭默认解析功能，使用内存 Registry 注册仓库内的 schema，或明确使用 offline 模式，避免数据校验隐式访问外部网络或任意本地文件。[引用解析选项](https://docs.rs/jsonschema/0.58.6/jsonschema/)

结构校验以外仍需要项目规则：跨文件 ID 唯一性、引用目标存在、模型别名关系、有效时间、来源缺失、条件冲突和单位一致性。
这些规则由同一 Rust 核心承担，schema、Python 脚本和导出器之间避免重复实现。
本报告只提出校验职责，字段和最终规则见后续 schema 讨论。

## 查询与分发存储

| 选项 | 优点 | 限制 | 建议 |
| --- | --- | --- | --- |
| YAML + Git | 人工维护与 PR 审查直接，完整版本历史 | 大量聚合查询和全量读取需要额外处理 | 权威数据起点 |
| JSON / JSONL | 下游无需 YAML 语义，跨语言消费简单 | 索引和关联查询需要消费者实现 | 首个发布格式，按消费者需求决定 JSONL |
| SQLite | 单文件、无需常驻服务，适合筛选、关系查询和本地状态 | 同一文件同一时刻仅一个写者；共享网络文件需谨慎 | 首个可选查询产物或本地运行状态库 |
| DuckDB | 分析大量快照、JSON/Parquet 与批量统计方便 | 嵌入模式的写入并发有边界；远程方案增加组件 | 有历史分析需求时采用 |
| PostgreSQL | 多会话事务、并发写入和服务端访问 | 需要服务运维、备份、迁移与权限管理 | 多工作进程共享任务状态或在线服务真正需要时采用 |
| Parquet / Arrow | 大规模列式分析和工具交换 | 第一版小型注册表增加额外产物与类型转换 | 暂缓，跟随历史快照分析需求 |

依据：[SQLite 适用范围](https://www.sqlite.org/whentouse.html)、[DuckDB 并发](https://duckdb.org/docs/current/connect/concurrency.html)、[PostgreSQL 并发控制](https://www.postgresql.org/docs/current/mvcc.html)、[DuckDB Parquet](https://duckdb.org/docs/current/data/parquet/overview)。

SQLite 的单写者约束不表示它不适合小型服务；短事务可以排队完成，是否需要更强并发取决于实际负载。
应避免多个机器直接共享同一个 SQLite 网络文件，把读写边界放在同一运行节点。[SQLite 官方说明](https://www.sqlite.org/whentouse.html)

DuckDB 的边界必须说明运行模式。
当前官方文档中，嵌入模式读写由一个进程管理，内部可有多个写线程；多进程写入另有 Quack 远程协议与 DuckLake/PostgreSQL 路线。
文档对 Quack 仍有 beta 与预计成熟时间的描述，不能把时间表视为已完成，也不能继续笼统声称 DuckDB 完全不支持多进程写入。
本项目第一版只需要可重建的分析视图，暂不采用这些额外服务。[DuckDB 当前并发文档](https://duckdb.org/docs/current/connect/concurrency.html)

不建议数据库与 YAML 同时接受人工更新。
当二者都能修改同一事实时，必须维护冲突解决和双向同步；目前没有这种需求依据。
查询产物应记录源 Git commit、schema 版本和生成器版本，并能够从干净环境重建。

## 快照、缓存与对象存储

缓存用于减少网络工作；快照用于在上游变化后仍能解释某条事实的来历。
只有被事实引用或用于重放的内容才需要长期保留，缓存过期不应连带删除仍被引用的证据。

建议快照至少能关联：来源 URL、最终 URL、获取时间、内容类型、必要的响应状态与校验头、内容摘要、抽取器版本和使用范围。
具体字段属于后续 schema 与运行清单设计，这里不固定名称。
API key、认证头、cookies、带签名的下载参数以及无关个人信息不应进入公开快照或 Git。

开始时使用独立目录和内容摘要命名即可。
当多个任务需要共享证据、保留量明显增大或备份要求出现时，再迁往对象存储。
Rust `object_store` 提供本地文件与多种对象存储的统一接口，可以作为届时的候选；没有必要提前包一层自定义存储抽象。[object_store 文档](https://docs.rs/object_store/latest/object_store/)

公开发布记录和公开发布原始页面全文应逐来源分别判断。
上游代码仓库的开源许可证、模型权重许可证、聚合数据许可证和 API 条款覆盖的对象可能不同，不能由其中一个推出其他对象均可再分发。
本项目应保留来源条款与许可核查记录，在范围不明时先保留链接和必要的事实证据，不默认公开整份抓取材料。
这是待执行的来源治理规则，本轮未对全部来源作法律许可判定。

## 确定性输出与失败恢复

- 相同快照、规则版本和配置生成相同结果；抓取时间保留在运行证据中，避免每次同步造成整库无意义 diff。
- 输出稳定排序并保持字段顺序；无变化不重写已审查记录。
- 先完整生成与校验候选产物，再通过原子替换或 PR 更新，避免失败留下半份记录。
- 需要重跑时复用原始快照；人工修订通过可追踪的规则或记录表达，不能被下一次抓取静默覆盖。
- 允许保留来源冲突与未知状态，不能靠覆盖较旧值强行生成看似一致的数据。

这些规则应在后续最小验证中结合真实记录检验，尤其是异常输入、来源暂时缺失与人工修订保护。

本轮未运行解析器验收样例、数据库负载测试、漏洞扫描或快照重放。
解析器维护状态与新版本默认行为变化较快，实施时应按实际锁定版本重新核验。

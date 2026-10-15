# 采集、抽取与任务执行

调研日期：2026-10-15。
状态：技术选择建议；表中的版本来自当日官方 registry，能力来自项目官方文档。

[返回技术栈索引](README.md) · [存储与校验](storage.md)

## 按来源形态选择工具

建议按下面的顺序判断每个来源，逐步增加依赖。

| 来源形态 | 首选方式 | 应保留的证据 | 主要风险 |
| --- | --- | --- | --- |
| 官方结构化 API | Rust reqwest + Serde | 响应正文、请求参数、抓取时间、分页完成状态 | 限流、账户范围不同、列表不代表全部能力 |
| 官方维护的 JSON/YAML/Markdown 仓库 | 固定 Git commit 读取 | 仓库 URL、commit、文件路径、内容摘要 | 代码许可证与所聚合数据的使用条件不同 |
| 官方静态 HTML | reqwest + scraper | 原始 HTML、最终 URL、相关标题/表格/脚注位置 | DOM 变更、表格外的限制被漏掉 |
| 官方动态页面 | 先检查公开数据接口，再考虑 Playwright | 渲染结果、相关网络响应、采集环境 | 浏览器成本、地区或登录状态差异 |
| 非结构化说明 | 人工确认，必要时辅助抽取 | 原文片段与页面位置 | 摘要省略否定、适用范围与时间条件 |

一次请求成功不等于该来源同步完整。
分页没有走完、返回 429/403、解析失败、字段突然减少，都应让本轮来源进入失败或待审查状态。
模型是否删除需要独立判断，不能直接由一轮列表缺失推出。

## Rust 基础工具

| 工具 | 当日稳定版本 / 发布时间 | 角色与判断 | 官方依据 |
| --- | --- | --- | --- |
| Tokio | 1.53.3 / 2026-10-13 | 推荐异步运行时；与 reqwest 及周边网络生态直接配合 | [官网](https://tokio.rs/)、[registry](https://crates.io/api/v1/crates/tokio) |
| reqwest | 0.13.5 / 2026-09-08 | 推荐 HTTP 客户端；复用 Client、连接池、流式读取、显式超时 | [文档](https://docs.rs/reqwest/0.13.5/reqwest/)、[registry](https://crates.io/api/v1/crates/reqwest) |
| Serde | 1.0.229 / 2026-07-18 | 推荐序列化基础；与 serde_json 连接 API 数据及内部结构 | [官网](https://serde.rs/)、[registry](https://crates.io/api/v1/crates/serde) |
| scraper | 0.27.0 / 2026-05-11 | 推荐静态 HTML 的初始选择；以 html5ever 和 CSS selectors 为基础 | [文档](https://docs.rs/scraper/0.27.0/scraper/)、[registry](https://crates.io/api/v1/crates/scraper) |
| lol_html | 3.0.1 / 2026-07-29 | 候选流式 HTML 解析器；页面体积与 DOM 内存确实成为瓶颈后比较 | [文档](https://docs.rs/lol_html/3.0.1/lol_html/)、[registry](https://crates.io/api/v1/crates/lol_html) |

推荐理由是职责集中、生态能直接组合、能对输入和错误作显式处理。
不把各项目宣传的吞吐量视作本项目的性能结论。
reqwest 默认会跟随重定向，因此来源白名单要检查最终目的地和重定向链，不能只验证初始 URL。
其 Client 文档说明连接池应复用；超时、响应体大小上限和单来源并发仍需要在应用边界明确设置。[reqwest 文档](https://docs.rs/reqwest/0.13.5/reqwest/)

核心实现优先 Rust，具体性能投入由测量决定。
早期网络等待和站点限流可能比解析更昂贵；这是待测假设。
确定性规范化、重复数据处理、批量校验和导出适合放在同一 Rust 核心中，避免 Python 与 Rust 各维护一套规则。
Python 需要调用核心时，首先考虑一次批量 CLI 调用与文件交换；只有进程开销被测量证明有影响时，才评估 PyO3。

## 框架与浏览器候选

| 方案 | 已确认的能力 | 推荐状态 | 本项目的选择条件 |
| --- | --- | --- | --- |
| Spider（Rust） | 并发爬取、页面流、可选 Chrome，HTTP 与渲染结合 | 候选 | 需要跨页面发现、队列或递归爬取，简单来源清单已不够用 |
| Playwright Python | 官方同步/异步 API，Chromium、Firefox、WebKit | 浏览器首选 | 少量文档必须依赖 JavaScript，使用独立浏览器任务 |
| chromiumoxide（Rust） | 基于 Chrome DevTools Protocol 的异步浏览器控制 | 候选 | 浏览器适配明确要与 Rust 同进程，且接受自行处理生命周期 |
| Crawlee Python | 统一 HTTP/浏览器爬虫、重试、并发、存储与自适应渲染 | 候选 | 多个动态来源需要共享队列和浏览器管理能力 |
| Scrapy | 完整爬虫管线、调度、去重与扩展点；当前默认 asyncio | 候选 | 大量传统抓取规则、站点级中间件或现有 Scrapy 经验能抵消框架成本 |
| Crawl4AI | 面向 Markdown 与结构抽取的异步采集，可选择 CSS 或 LLM 抽取 | 暂缓 | 需要大规模文档正文抽取时再评估，先验证脚注和表格保真 |

依据：[Spider](https://github.com/spider-rs/spider)、[Playwright](https://playwright.dev/python/docs/library)、[chromiumoxide](https://docs.rs/chromiumoxide/0.9.1/chromiumoxide/)、[Crawlee 架构](https://crawlee.dev/python/docs/guides/architecture-overview)、[Scrapy asyncio](https://docs.scrapy.org/en/latest/topics/asyncio.html)、[Crawl4AI Quick Start](https://docs.crawl4ai.com/core/quickstart/)。

Spider 的 registry 版本为 2.53.9（2026-09-05），chromiumoxide 为 0.9.1（2026-02-25）。
Spider 提供较多功能开关，接入前需要审查启用的 HTTP、Chrome、robots 和并发行为；本轮没有验证默认值能满足项目约束。[Spider registry](https://crates.io/api/v1/crates/spider)、[chromiumoxide registry](https://crates.io/api/v1/crates/chromiumoxide)

Crawlee 的 AdaptivePlaywrightCrawler 可在 HTTP 和浏览器方式间选择，但会限制处理器可用的统一上下文接口。
若页面分类错误导致漏掉模型限制，省下渲染成本也没有意义，因此需用实际来源样本验证。[Crawlee 自适应爬虫](https://crawlee.dev/python/docs/examples/adaptive-playwright-crawler)

Scrapy 的旧版本经验需要更新。
当前 2.19 文档明确新项目默认启用 asyncio，同时仍记录 reactor 与 Deferred 的集成边界，不能称其“不能异步”或“必须手工改造才能使用 asyncio”。[Scrapy asyncio](https://docs.scrapy.org/en/latest/topics/asyncio.html)

浏览器任务应明确等待目标元素或接口完成，并限制页面数量、总时长和下载大小。
Playwright 官方文档提示正在进行的调用不支持任意 asyncio task 取消；需要以其原生超时和清理流程管理，必要时用独立进程隔离任务生命周期。
不能把普通 HTTP TaskGroup 的取消方式直接套在所有浏览器调用上。[Playwright 已知问题](https://playwright.dev/python/docs/library#known-issues)

## Python 脚本与异步模型

采用当前稳定的 Python 3.15 作为目标基线，优先标准库 `pathlib`、`tomllib`、`asyncio` 和现代类型标注。
普通网络任务使用 `asyncio.TaskGroup` 统一等待与处理异常，并单独限制总并发与每个来源并发。
TaskGroup 本身不限制并发数；其中一个子任务异常会影响同组其他任务，因此独立来源的可恢复失败应在来源边界转为明确结果，取消异常仍应传播。[Python asyncio 文档](https://docs.python.org/3/library/asyncio-task.html)

不要因为使用了 `async def` 就把同步的 HTML 解析、文件压缩或大量 CPU 计算放到事件循环中。
必要时将阻塞调用放入线程或批量交给 Rust 核心，是否这样做由数据规模和耗时决定。
脚本负责流程编排，正式规则保持单一实现。

### Python HTTP 客户端

| 工具 | 本次核验 | 结论 |
| --- | --- | --- |
| aiohttp | 3.14.5，2026-10-11；原生 async ClientSession 与连接池 | 独立异步维护脚本的优先选择；复用 Session，并实测 Python 3.15 |
| HTTPX | 最新稳定发布仍为 0.28.1，2024-12-06；维护者于 2026-02-27 公告关闭 issues/discussions | 保留生态背景，不将原项目未经复核设为新项目默认依赖 |
| HTTPX2 | Pydantic 维护的 HTTPX 延续项目；提供同步/异步及 HTTP/2，当前 Crawlee 文档已引用 | 需 HTTP/2、统一同步/异步 API 或相关 SDK 集成时重点候选 |

依据：[aiohttp 客户端](https://docs.aiohttp.org/en/stable/client_quickstart.html)、[aiohttp registry](https://pypi.org/pypi/aiohttp/json)、[HTTPX registry](https://pypi.org/pypi/httpx/json)、[原维护者公告](https://github.com/encode/httpx/discussions/3784)、[HTTPX2 官方说明](https://github.com/pydantic/httpx2)、[HTTPX2 文档](https://pydantic.dev/docs/httpx2/get-started/)、[Crawlee 架构](https://crawlee.dev/python/docs/guides/architecture-overview)。

HTTPX 稳定版较旧这一事实不能单独证明它有漏洞或不可用；这里的取舍同时考虑新项目的维护渠道。
也不能因为 fork 较新就认定它已经成为唯一方向。
本轮没有运行 HTTP 客户端对比测试，正式选择还要核查所需代理、CA、超时、连接池与平台支持。

### Python 采集相关版本快照

| 包 | 官方 registry 版本 | 发布时间 |
| --- | --- | --- |
| [playwright](https://pypi.org/pypi/playwright/json) | 1.64.0 | 2026-10-13 |
| [crawlee](https://pypi.org/pypi/crawlee/json) | 1.10.4 | 2026-10-06 |
| [scrapy](https://pypi.org/pypi/scrapy/json) | 2.19.0 | 2026-09-10 |
| [beautifulsoup4](https://pypi.org/pypi/beautifulsoup4/json) | 4.15.0 | 2026-06-07 |
| [lxml](https://pypi.org/pypi/lxml/json) | 6.1.3 | 2026-09-02 |
| [trafilatura](https://pypi.org/pypi/trafilatura/json) | 2.3.1 | 2026-10-06 |

以上是 registry 返回的版本快照，未安装这些包。
搜索摘要、文档页生成日期和 release 发布日期可能不同，例如 scraper 的搜索结果日期晚于 registry 的发布时间；应使用 registry 的发布时间描述 release。

## 抽取工具与信息保真

| 工具 | 适用场景 | 本项目约束 |
| --- | --- | --- |
| scraper | Rust 静态 HTML、按 CSS 选择表格及相邻段落 | 固定页面样例应覆盖脚注和限制说明 |
| BeautifulSoup | Python 调研、结构不规则的 HTML、快速定位节点 | 显式选定 parser；不同 parser 可能生成不同树 |
| lxml | XPath、XML/sitemap、HTML 结构抽取 | 外部实体、网络访问与大文档限制需明确；异步下载不使解析本身异步 |
| Trafilatura | 正文、文章元信息、辅助阅读视图 | 正文筛选可能丢弃表格周边限定语，不作为唯一证据 |
| LLM 辅助抽取 | 难以写固定规则的条件描述 | 仅产生待审查候选，保留原文、模型版本与抽取过程信息 |

依据：[BeautifulSoup 文档](https://www.crummy.com/software/BeautifulSoup/bs4/doc/)、[lxml 解析文档](https://lxml.de/parsing.html)、[Trafilatura 抽取参数](https://trafilatura.readthedocs.io/en/latest/corefunctions.html)。

例如“支持图像输入”旁边可能另有角色限制、接口限制或模型版本说明。
抽取器必须保留这些条件与对应证据，不能只提取表格里的一个布尔值。
将 HTML 转成 Markdown 适合辅助阅读，但转换后的文本需要关联原始响应；转换结果不能取代原始证据。

## 依赖管理

沿用用户指定的三个文件关系：`pyproject.toml` 是直接运行时依赖来源，`uv.lock` 提交到 Git，`requirements.txt` 只由锁文件导出。
修改 `pyproject.toml` 后执行 `uv lock`，再执行以下既定导出命令。

```text
uv export --format requirements.txt --frozen --no-header --no-annotate --no-hashes --output-file requirements.txt
```

部署使用 `uv sync --frozen`。
需要补充一个检查概念：`--frozen` 使用已有锁文件，跳过“锁文件是否匹配项目定义”的检查，因此 CI 还应运行 `uv lock --check` 并检查重新导出的文件是否产生 diff。
这能发现只修改 `pyproject.toml`、忘记更新另外两个文件的情况。[uv locking and syncing](https://docs.astral.sh/uv/concepts/projects/sync/)、[uv export](https://docs.astral.sh/uv/concepts/projects/export/)

本阶段只记录工作流，不创建依赖清单或锁文件。
Python 发布产物、Rust `Cargo.lock`、浏览器版本和抽取器版本均应能关联到具体采集运行，方便后续重放差异。

## 任务调度

| 方案 | 判断 | 何时值得采用 |
| --- | --- | --- |
| 手动 CLI + CI 定时任务 | 推荐起点 | 日级/周级更新，可重复运行，普通失败可下次重试 |
| Prefect | 暂缓 | Python 任务较多，需要部署管理、人工重跑与统一运行界面 |
| Dagster | 暂缓 | 大量数据资产、分区回补与依赖追踪已成为维护负担 |
| Temporal | 暂缓 | 长时间运行、跨步骤持久恢复与严格任务状态确有必要 |

依据：[GitHub Actions 调度](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule)、[Prefect deployments](https://docs.prefect.io/v3/concepts/deployments)、[Dagster partitions](https://docs.dagster.io/guides/build/partitions-and-backfills/defining-dependencies-between-partitioned-assets)、[Temporal](https://docs.temporal.io/)。

GitHub Actions 官方说明定时任务在高负载时可能延迟，严重时可能丢弃排队任务。
因此更新时间应以最近成功采集记录为准，并保留手动触发入口，不能把 cron 时间当作准时承诺。[官方调度说明](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule)

这些平台不消除应用自身的幂等、来源限流和证据保存要求。
在单进程流程足够时，额外引入服务、队列与数据库只会增加运行状态与恢复路径。

## 采集阶段必须明确的边界

- 网络：官方来源白名单，校验重定向，限制响应体大小和解压后大小，保留 TLS 验证与部署环境的代理/CA 配置。
- 并发：全局和来源级分别限流；遵循服务的配额及 Retry-After，设置重试上限和抖动，区分可恢复错误与鉴权错误。
- 缓存：存在 ETag 或 Last-Modified 时按服务语义使用条件请求；304 需要关联已有快照，不能记录成空正文。
- 日志：记录来源、阶段、状态、耗时、页数、结果数与变化数；移除认证头、cookies 和签名参数。
- 发布：部分失败只产生状态报告和候选变更，不覆盖该来源已有的已审查事实。
- 外部内容：文档与页面中的文字只作为数据处理，不能成为采集器执行命令、读文件或改变输出目的地的指令。

上述是建议的工程边界，具体参数待来源规模和实际样本确定。
后续小规模验证至少包括分页中断、429、空列表、DOM 改版、错误编码、超大正文与跨域重定向。

本轮完成文档与 registry 核验，未执行上游项目代码、真实批量抓取、浏览器安装或性能测试。
因此尚不能承诺任一框架在本项目的全部来源上兼容或达到特定吞吐量。

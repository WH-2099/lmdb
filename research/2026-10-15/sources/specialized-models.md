# 专业模型补充：分割、时序、动作与蛋白质表示

核查日期：2026-10-15。
返回[来源目录](catalog.md)或[来源索引](README.md)。
本页补齐通用模型目录之外的四组作者资料，编号延续 S23–S26。
所有证据均为 `docs_read`：通过 Anysearch 读取官方 README 或作者模型卡，未克隆、下载权重或运行模型。
下面的限制是作者声明；适用范围判断是本轮研究结论，不是新增 schema。

| 编号 | 代表对象 | 作者资料 | 最能检验现有设计的事实 |
| --- | --- | --- | --- |
| S23 | SAM 2 / SAM 2.1 | Meta 官方仓库 | 输入提示包含点/框，输出是分割 mask，视频需要状态 |
| S24 | Chronos-2 | Amazon Science 仓库、amazon 模型卡 | 历史与预测长度按时间步计量，协变量有过去/已知未来之分 |
| S25 | OpenVLA 7B | OpenVLA 仓库、作者模型卡 | 输出动作需按机器人/数据集反归一化，官方许可声明有冲突 |
| S26 | ESMC 6B | Biohub 仓库、作者模型卡 | 输入是蛋白质序列，某些分类头必须微调，模型卡含复合许可 |

## S23 · SAM 2：视觉分割

已读 [Meta 官方 README](https://github.com/facebookresearch/sam2/blob/main/README.md)。
任务是图像与视频中的可提示视觉分割，支持自动 mask 生成和视频多对象跟踪。
README 示例把图像交给 image predictor，把视频交给有状态的 video predictor。
视频提示支持点击或边界框，输出含 `frame_idx`、`object_ids` 与 `masks`；“prompt”在这里有明确的空间语义。

需要保留的条件：

- SAM 2.1 checkpoints 需要相应版本的模型代码，示例也明确配对 checkpoint 与配置 YAML。
- 视频 predictor 的行为跟随软件版本变化；README 记录了允许跟踪开始后再添加对象的更新。
- 图像、视频、交互式提示和自动 mask 生成是不同操作入口，不能用“支持 image”替代任务描述。

作者明确声明模型 checkpoints、训练代码和 demo 代码使用 Apache 2.0。
同页另外注明 demo 使用的字体与 emoji 有 SIL Open Font License，说明仓库总体许可不能覆盖每个附带资产。
这确认了作者对相关模型与软件的声明，本轮未审查所有第三方文件或数据集许可。
system/user 消息角色、聊天最大输出 token、tool calling 对这里所述分割接口不适用。
“视频有 streaming memory”也不能直接映射为“API 提供文字流式响应”。
本轮未核实 mask 张量的全部尺寸、坐标约定、目标类别与 GPU 实际表现。

## S24 · Chronos-2：时序预测

已读 [Amazon Science README](https://github.com/amazon-science/chronos-forecasting/blob/main/README.md)与 [amazon/chronos-2 模型卡](https://huggingface.co/amazon/chronos-2)。
Chronos-2 是 120M 参数的时序模型，模型卡声明支持单变量、多变量及含协变量预测。
示例输入为历史目标值、时间戳、系列 ID 与过去协变量，并可提供已知未来协变量。
输出为多个未来时间步的预测与分位数，`prediction_length` 是预测步数。

模型卡列出最大历史长度 8192、最大预测长度 1024；这里应按时序观测/预测步数理解，不能标成自然语言 token 容量。
过去协变量与已知未来协变量必须区分，否则会掩盖未来信息是否在预测时可获得。
Chronos / Chronos-Bolt 与 Chronos-2 的原生协变量支持不同；外接回归器形成的能力也不应归给原始模型。
README 的软件项目许可为 Apache 2.0，模型卡单独声明模型许可为 `apache-2.0`。
训练数据集及云托管服务仍有各自的来源与条款，不能从模型许可推导它们的许可。

对元数据的含义：

- 保留数值单位、时间粒度、变量/协变量角色和分位数语义。
- 模型预测跨度、数据采样频率与平台批次限制是不同事实。
- system prompt、消息角色和工具调用对该预测接口不适用；概率预测不是聊天温度的同义字段。

本轮未验证不规则采样、缺失值、最大批次或长度超限时的实际行为。

## S25 · OpenVLA：机器人动作策略

已读 [OpenVLA README](https://github.com/openvla/openvla/blob/main/README.md)与 [openvla/openvla-7b 模型卡](https://huggingface.co/openvla/openvla-7b)。
输入是自然语言指令和机器人工作区相机图像；输出是归一化的七维末端执行器动作增量。
模型卡给出位置、旋转和夹爪分量，并要求按机器人、数据集对应的统计量反归一化后使用。
README 示例中的 `unnorm_key` 正是选择这种上下文，而非模型全局固定常数。

模型卡明确限制零样本使用范围：未见过的机器人具身或训练分布之外的布置需要适配。
README 对原版 OpenVLA 建议约 5–10 Hz 的采集控制频率，说明它未以 action chunking 训练；这是经验建议，不应存成平台硬限流。
同一 README 还链接后续 OFT 方法；不能把后续方法的多图、高频控制能力直接归给原始 7B checkpoint。

两个作者来源的许可表述需要并存：

| 对象与来源 | 已读声明 | 当前处理 |
| --- | --- | --- |
| 作者仓库代码 | MIT | 可记录为软件许可声明 |
| 作者模型卡的 checkpoints | MIT | 保留原声明及来源 |
| 作者仓库的预训练模型说明 | 基于 Llama-2，受 Llama Community License 约束 | 标为与模型卡概括不一致，待作者澄清或继续核对依赖条款 |

动作值的坐标系、单位、归一化统计和具身平台会影响含义，应与具体版本关联。
聊天最大输出 token 不能代替动作维度、动作步数或控制频率；图片输入能力也不能证明任意机器人适配。
本轮没有连接机器人或运行策略，也没有据 MIT 标签作出权重可自由再许可的结论。

## S26 · ESMC：蛋白质表示与掩码预测

已读 [Biohub ESM README](https://github.com/Biohub/esm/blob/main/README.md)与 [biohub/ESMC-6B 模型卡](https://huggingface.co/biohub/ESMC-6B)。
ESMC 使用蛋白质序列作为输入，可提供 logits、逐位置表示以及选择性返回的各层 hidden states。
模型卡还描述掩码氨基酸预测，并将 ESMFold2、SAE 等下游模型作为不同对象关联。
模型卡所称 2048 token 上限属于蛋白质 tokenizer 的语境，不能换算为 2048 个自然语言词。

有两个不能从库接口名称推断的条件：

- `output_attentions=True` 与示例中的 `flash_attention_2` 不兼容，属于实现/运行配置条件。
- Token/Sequence Classification 的分类头未预训练，需要下游微调；能够实例化该类不等于 checkpoint 已具备某种分类能力。

README 概括模型按 MIT 发布，但具体 ESMC 模型卡元数据同时列 `mit` 和 `other`，并链接第三方声明。
软件项目、checkpoint、第三方组件及平台服务的许可应分别记录；本轮没有读完第三方声明或判定全部适用条款。
当前模型卡的这一声明也不能自动套用到 ESM 的其他年代、其他家族或历史快照。
普通聊天角色、通用 function calling 和自然语言回答长度，对这里的表示提取接口不适用。
序列输入的字符形式也不足以把它分类成“通用文本模型”；需要保留任务与领域语义。
本轮未验证权重访问条件、显存、数值输出或任何科学预测结果。

## 对现有架构的检查

| 检查项 | 四组资料带来的要求 |
| --- | --- |
| 模型身份 | 家族、checkpoint、配置、任务头和外部适配方法需要可区分 |
| 输入输出 | 除 text/image 外，必须能表达 mask、时间序列、动作和逐位置表示的语义 |
| 数值与单位 | token、时间步、帧、维度和 Hz 不能共用一个无单位长度字段 |
| 适用范围 | 模型原生能力、软件配置、机器人具身和托管服务必须有清晰归属 |
| 证据冲突 | 许可证和能力也可能在同一作者的多个来源间出现不一致 |
| 缺失含义 | 不适用、尚未核实、不支持需要区别；通用聊天字段不能强制填满 |

这些样本支持继续沿用按来源、适用条件和版本记录事实的方向，暂不需要提前新增领域专用 schema。
表格模型、图模型、其他视觉任务和更多科学模型仍未建立专项样本。
本页仅提供四组文档证据，没有全面实测各类别，也没有审定所有依赖与数据集的许可。

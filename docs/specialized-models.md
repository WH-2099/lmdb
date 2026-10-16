# 专业模型声明

[返回项目](../README.md) · [声明数据](../data/assertions/specialized.yaml) · [身份绑定](../data/identities/core.yaml) · [声明约定](assertions.md)

本批补充机器人、蛋白质结构预测和医学图像分割的 9 条声明。
四个身份分别绑定作者发布的精确 Hugging Face 仓库记录。
证据使用匿名元数据 API 返回的提交 ID 固定作者模型卡，核查日期为 2026-10-16。
所有声明均为 `reported`。

| 模型或仓库 | 声明内容 | 范围 |
| --- | --- | --- |
| `physical-intelligence/fast` | `artifact.role = action_tokenizer` | 将机器人动作序列编码为离散 token，供自回归 VLA 训练使用 |
| `nvidia/GR00T-N1.7-3B` | 相机、本体感知、指令和 embodiment ID 的输入表示；连续动作向量的输出含义 | 限于该模型卡描述的表示，不推导 SDK 必填字段、固定动作长度或控制单位 |
| `biohub/ESMFold2` | 氨基酸序列输入、可选 MSA、全原子结构及置信信息输出 | 只绑定 ESMFold2；Fast 和 Experimental 变体需要独立证据 |
| `wanglab/MedSAM2` | 心脏超声视频、MRI 肝病灶和 CT 病灶三个微调文件的专长 | 每条规则必须匹配 `artifact.checkpoint` |

## FAST

[固定作者卡](https://huggingface.co/physical-intelligence/fast/blob/ec4d7aa71691cac0b8bed6942be45684db2110f4/README.md)明确将该仓库描述为动作 tokenizer。
`robotics` 任务检索仍会返回它；查询 `artifact.role` 可进一步识别它在动作处理流程中的角色。

## GR00T N1.7

输入和输出依据固定[作者模型卡的 Input 与 Output 章节](https://huggingface.co/nvidia/GR00T-N1.7-3B/blob/2fc962b973bccdd5d8ce4f67cc63b264d6886495/README.md)。
动作向量对应的控制含义依赖机器人 embodiment 的自由度。
这些信息不足以给所有机器人指定相同关节数、单位或动作长度。

该卡仍出现 EA 字样，已核查的[作者代码仓库版本](https://github.com/NVIDIA/Isaac-GR00T/blob/dac54b313160c201c450f97df0fd74ebd379c892/README.md)则称 GA。
本批不从这组材料推导发布阶段或部署可用性。

## ESMFold2

[固定模型卡](https://huggingface.co/biohub/ESMFold2/blob/69869f737beffec5294845ede23db5fc0b4f509e/README.md)的 Model Details 和 Model Variants 明确区分 ESMFold2 与 ESMFold2-Fast。
前者支持单序列预测及可选 MSA 条件输入，后者不支持 MSA 条件输入。
本批记录前者，避免把包含 Fast 示例的 Usage 代码当作基础模型的完整接口契约。
输出条目只记录作者列出的结构内容与指标名称，未推断张量形状、坐标单位或返回字典键。

## MedSAM2

[固定作者卡的 Available Models](https://huggingface.co/wanglab/MedSAM2/blob/e4a6f35edd7e091619cbc0750f462f1574e23955/README.md)列出多个权重文件。
`wanglab/MedSAM2` 身份因此表示这个 checkpoint 仓库，不代表其中所有文件具有相同专长。
三条断言分别要求 `MedSAM2_US_Heart.pt`、`MedSAM2_MRI_LiverLesion.pt` 或 `MedSAM2_CTLesion.pt`。
缺少 checkpoint 条件时，规则查询应保留未知适用性；不把这三条当作互相矛盾的无条件模型能力。

作者卡的 YAML 许可标签与正文使用范围说明不同，本批没有将其中一方转成统一许可结论。
已核查的现有 SAM2 视频状态声明属于 SDK 接口；本批不把它绑定到某个模型权重。

没有下载权重、读取凭据或运行推理。
这些内容记录作者声明，不构成真实机器人执行或医学应用效果的验证。

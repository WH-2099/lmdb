# 公开 Hub 采集种子

核查日期：2026-10-15。
本轮选入 147 个 Hugging Face 仓库和 22 个 ModelScope 仓库，共 169 个来源条目。
清单位于 [huggingface-models.txt](../../../config/huggingface-models.txt) 和 [modelscope-models.txt](../../../config/modelscope-models.txt)，每行保存一个大小写准确的仓库 ID，并按字典顺序去重排列。
HF 清单涉及 38 个发布命名空间；ModelScope 清单涉及 5 个发布命名空间。
两份清单有 13 个相同的 `owner/repo` 字符串，保留它们是为了核对跨来源差异；该数字不代表已证明的模型同一性。

通过作者公开列表发现候选，再读取详情 JSON 和模型卡，优先保留模型作者或所属项目组织发布的仓库。
部分家族保留不同任务、版本或结构的代表仓库，没有批量加入社区量化副本。
该清单是采集器的初始覆盖集，不是 Hub 全量枚举，也不是模型推荐榜。

## 访问结果

| 来源 | 入选数量 | 详情读取结果 | 模型卡读取结果 |
| --- | ---: | --- | --- |
| Hugging Face | 147 | 全部 HTTP 200，`private=false` | 127 个原始 README 返回 200；另外 20 个 gated 仓库的公开模型卡 HTML 返回 200 |
| ModelScope | 22 | 全部 HTTP 200，响应 `Code=200`，`Success=true` | 详情中的 `ReadMeContent` 均非空，并抽查了作者项目和跨 Hub 链接 |

HF 详情观察时间为 `2026-10-15T07:52:31Z` 至 `2026-10-15T07:55:30Z`；补充公开模型卡读取完成于 `2026-10-15T08:01:32Z`。
ModelScope 详情观察时间为 `2026-10-15T07:56:55Z` 至 `2026-10-15T07:57:09Z`。
HF 的 20 个 gated 仓库中，10 个标为 `manual`，10 个标为 `auto`。
其中 9 个原始 README 请求返回 401，随后读取了公开页面；其余 11 个直接读取公开页面。
这些结果只确认元数据访问情况，不能推导权重访问权、模型表现或再分发许可。

全部请求为匿名读取，未登录、使用凭据、下载权重或运行推理。
候选和响应缓存在本次工作环境的 `/tmp/lmdb-research`；正式导入仍需由采集器保存来源响应、观察时间与内容哈希。

## 覆盖与作者证据

下表列出已读模型卡的代表证据；准确的全部仓库 ID 以两份配置文件为准。
模型卡声明用于确认来源和任务边界，不作为运行时验证结果。

| 范围 | 代表仓库及证据 | 需要保留的区别 |
| --- | --- | --- |
| 语言、代码、推理 | [Qwen3](https://huggingface.co/Qwen/Qwen3-8B)、[DeepSeek-R1](https://huggingface.co/deepseek-ai/DeepSeek-R1)、[OLMo](https://huggingface.co/allenai/Olmo-3.1-32B-Instruct)、[Mistral](https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3) | 仓库版本与模型家族分开保存；名称不能代替架构或模态字段。 |
| 安全分类 | [Llama Prompt Guard 2](https://huggingface.co/meta-llama/Llama-Prompt-Guard-2-86M)、[Llama Guard 4](https://huggingface.co/meta-llama/Llama-Guard-4-12B)、[WildGuard](https://huggingface.co/allenai/wildguard) | 分类任务不能自动归入通用聊天模型。 |
| 视觉语言、OCR | [Qwen3-VL](https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct)、[DeepSeek-OCR-2](https://huggingface.co/deepseek-ai/DeepSeek-OCR-2)、[Granite Docling](https://huggingface.co/ibm-granite/granite-docling-258M)、[Molmo2](https://huggingface.co/allenai/Molmo2-8B) | 文档解析、OCR、图像问答的输出及任务不同。 |
| embedding、rerank | [Qwen3 Embedding](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B)、[BGE-M3](https://huggingface.co/BAAI/bge-m3)、[Jina Reranker](https://huggingface.co/jinaai/jina-reranker-v3.5)、[GTE](https://huggingface.co/Alibaba-NLP/gte-multilingual-base) | embedding、打分、ColBERT 表征不能只按模型名称合并。 |
| 图像与 3D 生成 | [FLUX.2](https://huggingface.co/black-forest-labs/FLUX.2-klein-4B)、[Qwen Image](https://huggingface.co/Qwen/Qwen-Image-2.1)、[TRELLIS.2](https://huggingface.co/microsoft/TRELLIS.2-4B)、[TripoSR](https://huggingface.co/stabilityai/TripoSR) | 图像编辑、RGBA 图层输出、3D 重建具有不同输入输出。 |
| 视频与音视频 | [Wan2.2](https://huggingface.co/Wan-AI/Wan2.2-T2V-A14B)、[Wan S2V](https://huggingface.co/Wan-AI/Wan2.2-S2V-14B)、[LTX-2.5](https://huggingface.co/Lightricks/LTX-2.5) | S2V 卡描述音频驱动视频；LTX 卡描述音视频生成；单一任务标签会遗漏这些信息。 |
| 语音、音乐、声码器 | [Whisper](https://huggingface.co/openai/whisper-large-v3-turbo)、[CosyVoice](https://huggingface.co/FunAudioLLM/Fun-CosyVoice3-0.5B-2512)、[Kokoro](https://huggingface.co/hexgrad/Kokoro-82M)、[BigVGAN](https://huggingface.co/nvidia/bigvgan_v2_22khz_80band_256x)、[MusicGen](https://huggingface.co/facebook/musicgen-medium) | ASR、TTS、音乐生成和声码器分别记录；Kokoro 已核对模型卡与作者代码仓的双向引用。 |
| 分割、检测、深度、视觉表征 | [SAM 2.1](https://huggingface.co/facebook/sam2.1-hiera-large/blob/665f8e2ad61cf5f53d65644ff27c8ee525124610/README.md)、[Grounding DINO](https://huggingface.co/IDEA-Research/grounding-dino-base)、[DepthPro](https://huggingface.co/apple/DepthPro)、[DINOv2](https://huggingface.co/facebook/dinov2-base) | 相对深度与度量深度、特征提取与训练好的分类头分别记录。 |
| 时间序列 | [Chronos-2](https://huggingface.co/amazon/chronos-2/blob/29ec3766d36d6f73f0696f85560a422f50e8498c/README.md)、[TimesFM 2.5](https://huggingface.co/google/timesfm-2.5-200m-pytorch) | 时间序列长度的单位不能默认写为自然语言 token。 |
| 机器人 | [OpenVLA](https://huggingface.co/openvla/openvla-7b/blob/47a0ec7fc4ec123775a391911046cf33cf9ed83f/README.md)、[FAST](https://huggingface.co/physical-intelligence/fast/blob/ec4d7aa71691cac0b8bed6942be45684db2110f4/README.md) | FAST 是动作 tokenizer；不能把它当作独立机器人控制策略。 |
| 蛋白、DNA | [ESM-2](https://huggingface.co/facebook/esm2_t33_650M_UR50D)、[ESM C](https://huggingface.co/biohub/ESMC-300M/blob/e4bac860f0c502cd80f3aeac3d0ce6524c6627cb/README.md)、[Nucleotide Transformer](https://huggingface.co/InstaDeepAI/nucleotide-transformer-v2-500m-multi-species) | 序列表征不能自动等同结构预测或任务专用预测。 |
| 表格、文档结构 | [TabPFN 分类](https://huggingface.co/Prior-Labs/TabPFN-v2-clf)、[TabPFN 回归](https://huggingface.co/Prior-Labs/TabPFN-v2-reg)、[Table Transformer](https://huggingface.co/microsoft/table-transformer-structure-recognition) | 分类、回归、表格位置检测与结构识别是不同任务。 |
| 地球观测、天气 | [Prithvi EO](https://huggingface.co/ibm-nasa-geospatial/Prithvi-EO-2.0-300M)、[Prithvi WxC](https://huggingface.co/ibm-nasa-geospatial/Prithvi-WxC-1.0-2300M) | 基础表征、下游适配与连续预测 checkpoint 分开。 |

ModelScope 的作者依据包括 [Qwen Image](https://modelscope.cn/models/Qwen/Qwen-Image)、[Wan2.2 Animate](https://modelscope.cn/models/Wan-AI/Wan2.2-Animate-14B)、[BGE-M3](https://modelscope.cn/models/BAAI/bge-m3)、[DeepSeek-R1](https://modelscope.cn/models/deepseek-ai/DeepSeek-R1) 和 [InternVL3](https://modelscope.cn/models/OpenGVLab/InternVL3-8B) 的公开详情及模型卡。
这些卡包含对应作者项目链接；Qwen Image 和 Wan 卡还列出各自的 HF 与 ModelScope 入口。
Whisper、CLIP 与 Table Transformer 的卡注明有 Hugging Face 团队撰写或改写内容，因此发布命名空间不等于每段卡片文字的作者。

## 最小字段映射

先保存普通字段和一条记录级来源引用；仅在存在字段冲突、条件限制或跨来源推导时增加单独证据。
原始详情快照提供字段追溯，不需要把每个字符串都包装成相同的证据对象。

| 字段含义 | HF 详情 | ModelScope 详情 `Data` | 处理规则 |
| --- | --- | --- | --- |
| 来源内仓库 ID | `id` | `Path` 与 `Name` | 保留来源与准确大小写；实际响应 ID 与请求 ID 不同时记录别名关系。 |
| 发布命名空间 | `author` | `Organization.Name` | 表示发布方，不直接当作模型开发者。 |
| 任务标签 | `pipeline_tag` | `Tasks[].Name` | 保留原值；缺失或空字符串保持未知。 |
| 许可声明 | `cardData.license` | `License` | 只是该来源的声明；空值不从另一来源静默补齐。 |
| 基础模型引用 | `cardData.base_model` | `BaseModel` | 统一为引用数组；关系类型另看 `base_model_relation` 或模型卡。 |
| 架构 | `config.architectures` | `Architectures` | 上游给出时才保存，不从仓库名猜测。 |
| 仓库 revision | `sha` | `Revision` | HF 样本为 commit SHA；ModelScope 本次均为可变的 `master`，不可称为固定提交。 |
| 来源更新时间 | `lastModified` | `LastUpdatedTime` | ModelScope 样本为 Unix 秒；统一时间格式时保留原始值，不能称为发布日期。 |

记录级来源至少保存请求 URL、观察时间、响应 SHA-256 与采集器版本；HF revision 可另用于固定模型卡链接。
ModelScope `Description` 有时为空或使用富文本结构，HF 详情也没有可依赖的统一简介字段，因此第一轮不强行生成简介。
`ReadMeContent` 和 HF README 是证据文本，不能把其中代码示例当作采集器指令运行。

已观察到的差异包括：Qwen3 Embedding 的 HF 标签为 `feature-extraction`，ModelScope 标签为 `sentence-embedding`；DeepSeek-V3.2 的 ModelScope `License` 为空，而 HF 卡有声明。
Wan2.2 Animate 与 DeepSeek-R1 的部分 ModelScope 任务条目没有英文 `Name`，需要跳过空名称并保留原始快照。
HF 清单有 11 条详情缺少 `pipeline_tag`，不得默认归为 `text-generation`。

## 排除与后续边界

`FunAudioLLM/SenseVoiceSmall` 与 `FunAudioLLM/CosyVoice2-0.5B` 在本次 ModelScope 请求返回 404，未加入该源清单；同名项目可能使用其他命名空间。
`microsoft/graphormer-base-pcqm4mv1` 的 HF 详情返回 401，未据此推断模型不存在。
`microsoft/Graphormer` 是多个 checkpoint 的集合，本轮没有建立目录内模型身份，故未选入。
`lerobot/pi0_base` 是 LeRobot 的移植，本轮优先采用原作者仓库，未选入。
旧 `EvolutionaryScale/esmc-300m-2024-12` 请求返回 Biohub 命名空间；旧卡推荐新的 `biohub/ESMC-300M`，本轮采用后者，未假定新旧仓库权重完全相同。
模型卡和命名空间可能变化，正式刷新需要重新读取并报告重定向、删除、访问状态变化及字段差异。

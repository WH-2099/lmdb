# 2026-10-19 研究

[返回研究索引](../README.md) · [项目跟踪](../../project/README.md)

- [模型 API Mock 服务](mock-server.md)：排名选择、原生协议、提供方差异、覆盖边界与待验证项。
- [模型元信息采集扩充与实现审查](metadata-collection-review.md)：字段缺口、官方结构化来源、匿名接口抽样与采集正确性问题。
- [官网原文与解析结果审查](parser-review.md)：实际请求 56 个官方页面，复核目录 API 与解析结果，记录八项待修复问题及复现证据。

本轮只读取公开资料和已有目录证据，没有读取真实凭据或调用模型推理接口。
Mock 的实现和使用方法见[Mock 服务文档](../../docs/mock-server.md)。
元信息采集报告提出后续建议，核查摘要见[机器可读记录](metadata-observations.json)。
后续修复与来源接入安排见[实施计划](../../docs/metadata-collection-plan.md)，验收结果见[项目进度](../../project/roadmap.md#元信息采集修复)。

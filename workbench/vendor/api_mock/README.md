# WB 接口模拟测试数据包（api_mock）

用途：WB 侧开发与测试**不依赖真实服务**即可联调——每个接口的请求/响应样例均取自
KG-Palantir 真实运行样本（斯迪克 2026Q3 案例主线）。
形态对齐原始 `financial_workbench_api_list`（非必要不新增接口）。
配套文档：`Ref/WB功能与接口清单-final.md`（终版清单）。

## 目录（按原始 API 清单分组）

### Financial Data APIs
| 文件 | 端点 | 说明 |
|---|---|---|
| `stocks_search.response.json` | `GET /stocks/search?q=` | 公司池检索（ticker↔object_id） |
| `financial_reports.response.json` | `GET /financial/reports/{symbol}?period=` | 定期报告事件与摘录（斯迪克 Q3） |
| `financial_reports.fenghua.json` | 同上 | 风华高科 Q3（WAITING_REVIEW 主线） |
| `financial_reports.jiemei.json` | 同上 | 洁美科技 Q3（离型膜同业） |
| `financial_reports.yongguan_failed.json` | 同上 | 永冠新材 Q3（数字校验失败案例，-61%） |
| `financial_reports.luxshare_failed.json` | 同上 | 立讯精密 Q3（失败案例） |
| `financial_reports.pledge_sdic.json` | 同上 | **非业绩类样本**：斯迪克控股股东质押公告（news 类） |
| `financial_indicators.response.json` | `GET /financial/indicators/{symbol}?period=` | 斯迪克 Q3 指标（原 I4 metrics，8012 已运行） |
| `financial_indicators.{fenghua,jiemei,yongguan_failed,luxshare_failed}.json` | 同上 | 四家公司的确定性计算指标 |
| — | `/market/*` | 不启用（市场/股价面） |

### AI Analysis and Prediction APIs
| 文件 | 端点 | 说明 |
|---|---|---|
| `analysis_task.response.json` | `GET /analysis/{id}` | 斯迪克 Q3 任务全字段（WAITING_REVIEW） |
| `analysis_task.fenghua.json` | 同上 | 风华高科任务（WAITING_REVIEW） |
| `analysis_task.jiemei.json` | 同上 | 洁美科技任务（WAITING_REVIEW） |
| `analysis_task.yongguan_failed.json` | 同上 | 永冠新材任务（FAILED，含真实违例 0.246） |
| `analysis_task.luxshare_failed.json` | 同上 | 立讯精密任务（FAILED） |
| `analysis_history.response.json` | `GET /analysis/history` | 队列摘要（三状态样本） |
| `analysis_explanation.response.json` | `GET /analysis/{id}/explanation` | 推断节 + 数字校验通过样例 |
| `analysis_explanation.{yongguan_failed,luxshare_failed}.json` | 同上 | 校验未通过样例（真实 violations） |
| — | `POST /analysis`、`/predictions/*` | 不实现/不启用（事件驱动；股价面） |

### Audit and Verification APIs
| 文件 | 端点 | 说明 |
|---|---|---|
| `audit_task.response.json` | `GET /audit/{analysis_id}` | 状态机 history + 审计链 |
| `audit_evidence.response.json` | `GET /audit/{analysis_id}/evidence` | 原文+摘录+位置+抓取时间（原 I4 evidence） |
| `audit_methodology.response.json` | `GET /audit/{analysis_id}/methodology` | 流水线步骤+版本+校验口径 |
| — | `/audit/{id}/performance` | 不启用（回测属股价面） |

### System
| 文件 | 端点 |
|---|---|
| `system_health.json` | `GET /health` |
| `system_models.json` | `GET /models`（单模型，单元素列表） |
| `system_status.json` | `GET /system/status` |

### WB 侧通道（原始清单之外，必需且仅三件，v2 契约）
| 文件 | 接口 | 说明 |
|---|---|---|
| `i1_webhook_payload.json` | I1 通知 | 新待审核触发（载荷不消费、幂等） |
| `i2_brief_sample.md` | I2 简报文件 | vault 契约格式（frontmatter 13 字段 + 四节） |
| `i3_*.json` | I3 MCP 工具 | list_pending/get_task/get_context/submit_review（含 409 冲突、**修改后通过 diff 样例对**） |

### `internal/`（内部参考，**非服务接口**）
- `i7_*`：HTTP 审核提交样例（I3 的等价兜底，已降级为 I2 文件契约）
- `i8_*`：数字校验样例——**WB 内置护栏的测试 fixture**（"扣非低于归母约 1652 万"
  被拦的真实案例，护栏误报率联调可直接复现）
- `i9_*`：触发线告警样例（留在 KG 门户侧，WB 不接入）

## 用法

- **零代码**：`cd Ref/testdata/api_mock && python -m http.server 8899`，WB 开发环境
  把 Base URL 指到静态文件即可。
- **测试替身**（推荐）：vitest / Rust 测试里直接引用 JSON 作为 `mcp_call_tool` 与
  HTTP client 的替身返回，与 WP-A 通道测试、WP-E 面板测试同构。

## 字段口径速查

- 金额单位：亿元（`net_profit=0.669378` = 6693.78 万元）；百分比为数值（`47.86` = 47.86%）
- `object_id`：`S`+ticker（如 `S300806`）；`event_id`：`EV-<日期>-<巨潮公告ID>`
- `task_id`：`RT-<prediction_id>`；`trace_id` = prediction_id（贯穿事件→任务→审核）
- 状态机：RECEIVED → TASK_CREATED → CONTEXT_READY → RUNNING → WAITING_REVIEW → SUCCEEDED；
  异常态 IGNORED / WAITING_DATA / FAILED / CANCELLED
- 错误约定：404 对象不存在 / 409 版本或状态冲突 / 503 数据缺失 / 400 参数错误

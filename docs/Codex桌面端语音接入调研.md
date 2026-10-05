# EasyInput V2 → Codex 桌面端语音接入调研

调研日期：2026-10-05。初轮范围为只读调研；后经用户授权完成专用测试聊天的真实 IPC 投递验证。未修改业务代码、未启动第二个执行服务。

## 页面失败与修正（后续验收）

用户打开原测试聊天后提供了「ChatGPT 遇到了问题」截图。因此撤回原先隐含的“桌面页面也已验收成功”结论：原先只验证了后台消息、回复与完成事件。

桌面日志的 `LocalConversationPage` error boundary 明确报 `Cannot read properties of undefined (reading 'some')`。对照安装代码，渲染函数会读取文本输入的 `text_elements.some(...)`。原型请求只提供 `type` / `text`，缺少桌面内部文本结构要求的 `text_elements` 数组。

脚本已修正为每项文本同时提供 `text_elements: []`。在原聊天追加正确消息不能消除旧回合缓存中的缺字段输入；重新打开原聊天仍观察到同一异常，原聊天未修复。

为保留历史且重新加载规范化记录，已从原测试聊天建立同目录分支「VoxQueue 桌面投递验证（修正版）」。打开该分支后，通过修正脚本发出一条固定测试消息，收到 `VOXQUEUE_FIXED_PAGE_OK`，约 7 秒完成。日志记录该页面路由与活动视图，期间未新增同类 error boundary。该分支保留原测试历史，原证据未删除。

当前结论：后台投递已验证；缺字段导致的崩溃已定位并修正请求，修正版分支的日志复查通过。尚未进行屏幕像素级检查，页面的最终视觉验收仍以用户打开修正版后的实际显示为准。不要继续打开旧链接作为成功演示。

## 实际验证结果（2026-10-05）

已验证：外部 Python 程序能通过 Windows 桌面 IPC 向桌面端拥有的聊天发送消息，由桌面端执行并形成用户消息、助手回复及完成事件；没有运行 `codex exec resume`。

使用专用聊天「VoxQueue 桌面投递验证」，初始基线由桌面工具创建，随后五条测试消息全部由独立 Python 程序通过命名管道发送。初始基线不计入 IPC 成功次数。

| 检查 | 结果 |
| --- | --- |
| IPC 初始化与聊天拥有者发现 | 成功 |
| 空闲聊天发送固定文本 | 成功，回复 `VOXQUEUE_DESKTOP_IPC_OK_1`，回合约 6 秒完成 |
| 重复消息检查 | 五个测试标识各出现一次用户消息和一次助手回复，无重复 |
| 忙碌时紧接着发送 A/B | 两次均接受，返回同一回合 ID；日志中两条消息及两条回复都存在，不是独立回合 FIFO |
| 等待 A 完成再发送 B | 成功，`VOXQUEUE_FIFO_A` 与 `VOXQUEUE_FIFO_B` 分属独立回合，先后完成 |
| 完成事件 | 测试 rollout 中有匹配的 `task_started` / `task_complete`；五个开始回合全部完成（含基线） |

第一条请求曾因缺少 `turnStart.request.threadId` 被明确拒绝，报 `Turn request thread does not match the conversation`。补齐后发送成功；日志确认该标识只生成一次用户消息，不是超时后的盲目重发。

结论：桌面投递路径已由真实消息验证。要接入原产品，必须由 Host 保持同聊天串行，等待权威完成后投递下一条。不能因为 IPC 请求返回成功就将队列任务标为完成，也不能把连续提交视为自动生成两个独立回合。

边界：尚未接入 MiniMax ASR、实体键盘、灯光或总结播放；尚未测试四槽并发、桌面重启和断线恢复。消息及回复通过桌面聊天读取接口和 rollout 双重核对，没有进行屏幕像素级视觉检查。

验证脚本：`desktop-validation/ipc_probe.py`；脱敏汇总：`desktop-validation/results.json`。脚本要求显式传入目标聊天，仅在给定 `--prompt` 时投递；不自动重发，不作为生产运行器。

## 结论

存在已验证的直接桌面端文本投递路径：通过桌面端内部 IPC 转发给当前对话拥有者，由桌面端开始回合。五条固定文本均完成真实投递与回复验证；VoxQueue 的语音输入和实体键盘端到端接线仍未完成。

该路径属于内部、未见公开稳定承诺的接口，适合作业原型和限定版本实验。正式产品应优先使用经验证的公开接口，或为内部接口设置版本检测、功能开关和失败保护。

## 当前项目

- Windows 开发源码：`WindowsPreviewSource/`；桌面管理程序已更名为 VoxQueue。
- 输入链：EasyInput V2 → 局域网录音 → MiniMax ASR → Host FIFO → CodexRunner。
- `app/host/src/codex_runner.rs` 仍启动 `codex exec … resume`，并在 Windows 固定模型与 sandbox 参数；不是向桌面输入框发送。
- `update_report_part1.xml` 记录此前独立进程恢复桌面对话失败，报 `already has an active writer`。这是历史记录，本轮没有重做恢复操作。
- 本次要解决的是投递层；ASR、固件录音和播放链无需因这一目标全部重写。

## 公开接口核查

1. 深链接：`codex://threads/<thread-id>` 可打开已有聊天；文档描述的新聊天 `prompt` 参数只预填输入框，不自动发送。未找到公开的“向已有聊天自动发送”深链接。
   来源：https://learn.chatgpt.com/docs/reference/commands
2. App-server：有 `thread/resume`、`turn/start`、`turn/steer` 和回合完成事件。协议可用于自建客户端，但并不保证新进程能接管桌面端已占用的对话。WebSocket 传输被文档标为实验性、不受生产支持。
   来源：https://learn.chatgpt.com/docs/app-server
3. 插件界面：MCP Apps `ui/message` / `window.openai.sendFollowUpMessage` 能请求组件发送后续消息。它是运行在宿主内的组件桥接，不是任意后台程序调用的 HTTP 接口；Codex 聊天支持、自动发送行为、组件生命周期及四槽定向仍须实测。
   来源：https://developers.openai.com/plugins/build/chatgpt-ui
   来源：https://developers.openai.com/plugins/reference

## 本机的新证据

- 安装包版本：`OpenAI.Codex 26.930.4958.0`。
- 桌面端存在运行中的 app-server，但本轮未发现该 Codex 进程开放 TCP 监听。Host 的 `127.0.0.1:2855` 是 VoxQueue 服务，不是 Codex 投递接口。
- 本机 CLI 有 `app-server proxy`，但 `app-server daemon version` 无法连接默认 control socket。安装代码中，本地 daemon 的该选用条件排除了 Windows；不能把 proxy 命令存在视为当前桌面服务可连接。
- 本机安装代码定义 Windows IPC 地址 `\\.\pipe\codex-ipc`。
- 内部代码有 `thread-owner-discovery`、`thread-follower-start-turn`、`thread-follower-steer-turn` 和队列相关方法。开始回合的处理器会调用拥有者的 `startTurn`，提供了绕开独立进程重新取得写入权的设计路径。
- IPC 采用带 4 字节小端长度前缀的 JSON 帧，与 app-server 的 JSONL 不是同一种线协议。不能把 `turn/start` 原样写入这个管道。
- 使用明确标识为研究客户端的连接，仅发送 `initialize`；返回 `response / initialize / success`，得到客户端标识后立即断开。没有发送用户消息、查阅其他聊天正文或触发模型执行。

本机代码依据：安装包 `app/resources/app.asar` 内 `.vite/build/application-network-startup-BEAX-hka.js`、`bootstrap-BXPOZU-a.js`、`src-C1dW0Du8.js`。这些名称和接口均可能随版本变化。

## 接入路线比较

| 路线 | 当前证据 | 对作业的适合程度 |
| --- | --- | --- |
| 桌面内部 IPC，将输入交给对话拥有者 | 握手、真实文本投递与回复已通过；忙碌时共用回合 | 可以推进单槽集成；保留 Host FIFO |
| 宿主内插件组件发送后续消息 | 有正式文档；当前 Codex 兼容性与四槽行为未测 | 后续探索公开扩展路线 |
| 打开聊天，再自动操作输入框和发送按钮 | 设计上可避开第二写入者；本轮未操作 UI 验证 | 备用演示方案，依赖前台、焦点及界面状态 |
| 独立 app-server / CLI resume 接管 | 项目已有 writer 冲突记录 | 不宜继续当作直接桌面投递方案 |

## 建议的最小验证

1. 先用独立测试聊天和固定无害文本验证 IPC 投递；不先接真实录音，也不向正在调研的聊天发送测试消息。
2. 验证正确对话收到且仅收到一次消息、桌面显示完整回复、无需新 CLI 执行器接管。
3. 验证目标聊天忙碌时排队语义；若使用 steer，必须明确它是在修改当前回合，不能默认为原项目的严格 FIFO。
4. 验证完成事件与最终回复归属于本次投递，之后再接 Host 的灯态、总结与播放；完成不等于任务成功。
5. 接入一个槽位的 MiniMax ASR，再扩展四槽，测试切换聊天、重启桌面端、连接中断与重复投递。
6. 超时后不能盲目重发：区分未投递与已投递但回执丢失，防止同一指令执行两次。先验证投递身份和回执，不能用“管道写入成功”充当执行成功。

推荐保留现有 CLI 后端作为可选择模式，新增独立的桌面投递后端。未通过端到端验证前，不替换当前已跑通的四槽链路。

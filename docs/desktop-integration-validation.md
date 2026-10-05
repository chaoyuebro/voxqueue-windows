# Windows 桌面投递接入验证

日期：2026-10-05。验证桌面版本：OpenAI.Codex 26.930.4958.0。

## 接入内容

Windows Host 使用 `desktop_runner.rs` 经当前用户的 `\\.\pipe\codex-ipc` 投递语音识别文本，不再为槽位任务启动 `codex exec resume`。macOS 保留原执行器；独立总结执行器没有变更。

继续使用原有持久队列：最多四个槽位任务同时执行，同一聊天一次只有一个任务在运行。等待桌面聊天空闲后投递；只有匹配回合的完成事件、有最终回答且无错误时才标记成功。取消和模型繁忙等失败会显示失败，完成观察器仍将成功回复写入总结账本。

每条输入包含 `text_elements: []`，继承聊天模型、权限和其他桌面设置。不得省略该字段，此前原型已证明会触发桌面渲染异常。

`run/desktop-delivery.sqlite3` 保存 intent、accepted、completed、aborted，SQLite FULL 同步。发送前写 intent，收到回执后写回合 ID。重启后 accepted 继续监听，completed 不重发；intent 表示结果未知，阻止继续向同一聊天投递。旧 CLI 的恢复任务若没有桌面证据，也不自动重发。不要为“恢复队列”删除此数据库；这会丢失防重复证据。

新绑定仍从有界目录列表选择；已绑定聊天移出最近八条后，按 ID 重新核验归档状态、用户所有权和本机路径，避免四槽并行时因列表变化失效。目录快照遇到并行写入变化时，在取消可检查的期限内重试。

## 自动化验证

命令：

```powershell
cargo check -p easy-codex-host
cargo test -p easy-codex-host --test desktop_protocol --test windows_storage_smoke --test windows_catalog_verbatim
pnpm --dir app/desktop test
pnpm --dir app/desktop typecheck
```

结果：Rust 定向测试 11 项通过，界面测试 7 项通过，类型检查通过。既有平台相关警告仍存在；没有宣称整个仓库的 Unix 单元测试已在 Windows 通过。

定向协议测试覆盖部分 JSONL 写入、无关回合完成、取消、超时、模型错误、过大/畸形/截断记录、持久投递状态和渲染必要字段。目录测试验证聊天移出最近列表后绑定仍有效，归档后失效。

## 真实桌面验证

验证工具：`cargo run -p easy-codex-host --example desktop_validation -- <测试聊天ID>`。一个 ID 将四槽绑定到同一聊天，四个 ID 使用四个独立聊天。工具使用隔离临时 Host 数据库，不改现有键盘绑定，不修改项目文件。

同聊天四槽 FIFO：通过。标记 `5cf0dcf1-1cb2-4aff-8a8d-0fc42173221e`，四个独立回合分别回答 `VOXQUEUE_RUST_SLOT_1_OK` 到 `VOXQUEUE_RUST_SLOT_4_OK`，四条任务 completed，四个对应完成 ID 写入原 RolloutObserver 完成账本。

独立聊天：通过。最后一轮标记 `3e7a0349-d421-46c7-9bd8-3347f6e6dc0c`，四槽绑定四个独立聊天，四条任务 completed，四个独立回合有最终回复，四个对应完成 ID 写入完成账本。旧练习聊天的 `gpt-6-sol` 曾返回 `server_overloaded`；已验证此类回合标记失败，不生成成功总结。

## 本机更新

已生成 `target/release/bundle/nsis/VoxQueue_0.1.0_x64-setup.exe`，安装包 SHA256：`C9E87C5B13A2979380C3064A64B897C2D65331C9E368BB5599D6261D2234D8D9`。

已在确认旧 Host 无 queued/running 任务后更新本机 VoxQueue 的 Host 和桌面二进制，保留原文件备份。最终界面修正也已更新，本机 Host 二进制与构建文件 SHA256 一致。最终健康检查 ready，恢复任务数 0，PID 35652。Wi-Fi 和 API Key 未变；槽位 1 的测试改绑见下文。

## 使用和验收

先在 Codex 桌面端打开要绑定的聊天，再在 VoxQueue 绑定 S1–S4；保持桌面端运行。按住上排键说话，松键后识别文字进入桌面对话；同聊天的后续语音等待上一回合完成。下排键播放对应未听总结。

S1 实体语音：通过。用户确认“出现识别文字和回复”。本机队列请求 `lan-ec1fc5684bac0980`，绑定 generation 6，任务 completed；桌面回合 `01a10c33-9497-7cd2-be9e-7a00827b3066` 的投递账本 completed，对应完成账本记录 1 条，已发布新的未听总结 generation 4。

S5 实体播报和灯态：通过。用户确认“能播报，灯也熄灭”，后台总结 generation 4 已变为 heard，完成 S1 输入 → 桌面回复 → 新总结 → S5 播放的实机闭环。S2–S4 的实体录音及 S6–S8 的本次播放未逐键实测；四槽任务投递和完成账本已通过软件集成测试，旧 CLI 实机结果不能代替本次逐键验收。

用户反馈旧 CLI 练习聊天在桌面显示为“新聊天”，因此槽位 1 已改绑至「VoxQueue 桌面投递验证（修正版）」，其余槽位绑定未改。原槽位 1 绑定为 `01a0e117-f8f1-7fa0-b678-621a55d2d93e`。通过现有 Host 控制接口改绑成功，generation 从 5 增至 6；`bind-slot` 命令也已支持运行中的 Host，避免直接打开数据库导致 AlreadyRunning。

限制：接口是按当前版本验证的内部协议，升级 Codex 后须复验。投递前检查 rollout 空闲，但桌面同时手动发送与语音投递仍存在竞态，运行期间请等候回合结束。已发出任务不会被 Host 超时或退出中断；未确认投递不会自动重发。若提示 delivery_uncertain，应先核对聊天记录和回合 ID，再处理投递账本，不能盲目重试。

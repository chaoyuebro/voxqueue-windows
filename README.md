# voxqueue-windows

VoxQueue 是基于 EasyInput V2 的 Windows 四槽语音任务键盘。本仓库保存当前 Windows / MiniMax 开发进度，后续开发使用本仓库。

## 当前进度（2026-10-05）

- 已实现 Windows Host、VoxQueue 桌面管理界面、MiniMax ASR/TTS 和四槽 CLI 任务链路；实体键盘验证见 [C4 实测记录](docs/C4-实测记录.md)。
- 已通过独立 Python 原型验证向 Codex 桌面聊天投递文字、接收回复及观察完成事件。原型曾因缺少 `text_elements` 导致页面异常，字段已修正；现已通过 S1 实体语音 → 桌面回复 → S5 总结播报及熄灯验收。
- Windows 槽位执行器现已接入原生 Rust 桌面 IPC，保留四槽调度、同聊天 FIFO 和完成监听。投递记录持久化，回执不确定时停止重发。四槽绑定同一真实桌面聊天的任务、回复和完成账本已通过；四聊天并行及本次实体键盘验收见 [桌面接入验证](docs/desktop-integration-validation.md)。
- Windows 任务状态和总结收集已改为直接订阅 Codex 桌面通知，不再扫描聊天历史文件；只保存输入、最终回答和必要状态，工具正文及图片丢弃。通知、真实完成和去重验证见 [桌面任务状态通知](docs/desktop-state-notifications.md)。
- Windows 最右侧状态灯已改为随 ChatGPT 桌面软件运行状态亮灭；需要 Host 与键盘固件一起升级，编译验证及安装状态见 [桌面软件运行指示灯](docs/desktop-presence-led.md)。
- 内部桌面接口需要随 Codex 版本复验；完整调研与限制见 [桌面端接入记录](docs/Codex桌面端语音接入调研.md)，实验脚本位于 `tools/desktop-validation/`。

当前版本不代表所有平台测试或长稳测试已经通过；旧安装包需要升级才能使用桌面投递。

每个槽位已增加“清除待听”按钮，已看过的对应队列可单独标记已读；新完成内容继续提示。使用和验证见 [清除待听队列](docs/clear-summary-queue.md)。

Windows MiniMax 语音开发候选的安装说明见 [docs/windows-minimax-setup.md](docs/windows-minimax-setup.md)。原 `v0.1.0-windows-preview.1` 发布包仍使用百炼。

EasyInput V2 的本地语音任务键盘。上排 S1–S4 按住说话，把要求交给各自绑定的 Codex 任务；下排 S5–S8 在开发板扬声器播放对应任务的未听总结。键盘与电脑通过同一局域网通信，电脑运行 Host 和桌面管理程序。

## Windows 版

Windows 10/11 x64 的原生 Host 和桌面程序已完成四槽真机闭环。安装、固件、个人账号配置和配网步骤见 [Windows 安装与配网](docs/windows-setup.md)。发布包包含 Windows 安装程序和 EasyInput V2 对应的固件镜像；每位使用者自行配置 Codex 登录、百炼 API Key、任务绑定和自己的 2.4 GHz Wi-Fi。发布包不包含这些个人数据。

当前为预览版：四槽输入、总结和板载播报、播放抢占、Host 重启后的未听恢复已有真机验证；登录自启、各键 20 轮长稳和第 5 颗灯在部分网络下的空闲颜色仍待验收。使用前请看指南中的已知限制。

## macOS 版与源码

本项目最初是 macOS 本地 Wi-Fi 版本。Windows 和 macOS 共用 Rust Host 核心、Tauri 2 桌面界面及 ESP32-S3 固件。源码目录：`app/host/`、`app/desktop/`、`firmware/`。架构和协议分别见 [总体方案](docs/总体方案.md)、[端到端架构](docs/端到端架构.md) 和 [协议草案](docs/协议草案.md)。

macOS 版的键位和语音流程相同；其 LaunchAgent、Keychain 和安装方式与 Windows 不同。Windows 请以 [Windows 安装与配网](docs/windows-setup.md) 为准。

## 来源与许可

本仓从 [`easy-codex-input@52949d3`](https://github.com/Larkspur-Wang/easy-codex-input) 抽取；固件来源和许可边界见 [firmware/UPSTREAM.md](firmware/UPSTREAM.md)。本仓代码采用 [Apache-2.0](LICENSE)。

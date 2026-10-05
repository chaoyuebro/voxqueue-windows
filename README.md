# voxqueue-windows

VoxQueue 是基于 EasyInput V2 的 Windows 四槽语音任务键盘。本仓库保存当前 Windows / MiniMax 开发进度，后续开发使用本仓库。

## 当前进度（2026-10-05）

- 已实现 Windows Host、VoxQueue 桌面管理界面、MiniMax ASR/TTS 和四槽 CLI 任务链路；实体键盘验证见 [C4 实测记录](docs/C4-实测记录.md)。
- 已通过独立 Python 原型验证向 Codex 桌面聊天投递文字、接收回复及观察完成事件。原型曾因缺少 `text_elements` 导致页面异常，字段已修正；修正版聊天日志检查通过，最终视觉验收仍待确认。
- 桌面 IPC 原型尚未接入 VoxQueue 的槽位执行器，目前仅绑定桌面聊天不能视为语音链路已经完成。下一步接入单槽，保留 Host FIFO，再验证录音、灯光、总结和实体播放。
- 内部桌面接口需要随 Codex 版本复验；完整调研与限制见 [桌面端接入记录](docs/Codex桌面端语音接入调研.md)，实验脚本位于 `tools/desktop-validation/`。

本次 Git 保存为现有开发状态快照，不代表所有平台测试、长稳测试或桌面端语音集成已经通过。

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

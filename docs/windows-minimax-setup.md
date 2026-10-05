# Windows + MiniMax 语音版（开发候选）

本页对应本仓源码构建的候选包，不适用于 `v0.1.0-windows-preview.1` 原发布包。原包只支持百炼；不要把 MiniMax Key 输入到 `save-windows-bailian.py`。

## 语音接口

- 语音识别：MiniMax `asr-1.0`，完整录音以 WAV 上传。
- 总结播报：MiniMax `speech-2.8-hd`、`Chinese (Mandarin)_Lyrical_Voice`，接收 44.1 kHz 单声道 PCM 后转换成板子所需的 48 kHz PCM。
- Codex 任务执行与总结文本生成仍使用已登录的本机 Codex CLI。
- MiniMax 语音调用使用用户自己的 MiniMax API Key，按 MiniMax 账号规则消耗额度。配置前应确认 Key 能实际调用上述两个模型。

## 密钥

在 Windows 本机复制 MiniMax Key 到剪贴板，在源码根目录运行：

```powershell
Get-Clipboard -Raw | python .\scripts\save-windows-minimax.py
Set-Clipboard -Value ''
```

脚本只从标准输入读取 Key，保存到当前 Windows 用户的凭据管理器 `EasyCodexInput/MINIMAX_API_KEY`。不要把 Key 发到聊天、放进命令参数或写进仓库。用 `easy-codex-host.exe minimax-key-status` 可只读查看是否已配置；这不验证账号额度或模型权限。

## 构建与安装

在源码根目录运行 `pnpm install --frozen-lockfile`，然后运行 `./scripts/build-windows-app.ps1`。该脚本先编译 Host，再将其装入 NSIS 安装包。构建完成后核对安装包的 SHA-256，再安装。首次打开后检查顶部 Host 状态及 MiniMax 模型显示。

固件和板子配网协议未因语音服务切换而变化。实体板识别、三段镜像 SHA、烧录授权、2.4 GHz 配网及 C4 七项仍按原 Windows 指南逐项进行；任何软件结果不能替代灯光和扬声器的实测。

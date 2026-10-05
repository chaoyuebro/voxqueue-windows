# 桌面软件运行指示灯

用户要求最右侧灯在 ChatGPT 桌面软件启动时亮，退出后熄灭。

## 行为

- Windows Host 每 250 ms 使用只读进程快照检查 ChatGPT.exe。这台电脑的 Codex 桌面外壳也使用这个进程名。
- 软件运行时，最右侧灯保留现有任务数量颜色；软件退出时，无论还有多少旧运行任务，该灯都灭。
- 最小化仍属于运行。需要真正退出软件，而非只隐藏窗口。
- 键盘每 2 秒（空闲时 4 秒）通过认证心跳获取状态，因此开关软件后通常在 2–4 秒内更新。
- 如果 Host 也停止或网络断开，10 秒没有新状态会熄灭运行指示灯。其他四槽待听灯保持其队列语义。
- 按键、录音及配置等临时反馈依旧可覆盖状态灯；反馈结束后恢复当前状态。

## 协议与兼容

EIMB 从 v3 升级为 v4，长度仍为 32 bytes，byte 7 为 desktop_running（仅 0/1），参与 HMAC。
新版解码器继续接受 v3，将其视为旧版常亮语义。新版 Host 发 v4，需要升级固件；旧版固件无法解析 v4。
必须先烧录新版固件，再替换本机 Host；不能只更新 Host，否则旧固件会丢弃信箱状态。

## 验证

- Rust 定向测试：4 项通过，覆盖桌面进程名选择、认证通信、跨语言固定向量、运行/关闭状态及队列字段保留。
- 真实 Windows 进程检查：检测到当前打开的桌面程序（1 项通过）。
- C++ mailbox_led_tests、codex_playback_wire_tests：2 项通过，覆盖关闭时最右侧黑色、其他槽灯不变、v3 兼容、v4 开关及非法标记拒绝。
- Windows NSIS 安装包已构建；固件在 ASCII 路径构建，避开 Xtensa objdump 对中文路径的限制。
- 2026-10-05 经用户确认后，已在 COM7 对 EasyInput V2 执行 application-only 烧录，写入 0x10000，esptool 输出 Hash of data verified 并自动重启。
- 已更新本机 Host 和桌面程序，安装文件与构建文件 SHA-256 完全相同。Host ready（schema 7）；重启后 16 个认证心跳、16 次信箱状态发送成功，无发送失败。四槽绑定及 generation 保持原值。
- 软件启动/退出对应灯的实体视觉验收仍待用户确认；未主动退出当前正在使用的 ChatGPT。

## 本机构建

ESP-IDF v5.5.5 位于 D:/v5.5.5/esp-idf，工具位于 D:/Espressif。
补齐约束文件，并将其明确排除的 esp-coredump 1.17.0 更新为符合约束的 1.17.2。
固件源码复制到 D:/Temp/voxqueue-desktop-led-20261005/firmware 后编译；不改变组件锁定版本或键盘 NVS。

## 已构建镜像

- 应用固件 SHA-256：`d7670a545bd35461e0fd6cf3f04d24a4ec1f3ffae64f54924ee3a5a18a7a1cc0`。
- Bootloader SHA-256：`be3abea605a6be7f04c2d0f4011bd90688f799a834a164cdc6a29b16c3324287`。
- 分区表 SHA-256：`7c541b70dcac8f920c2d11589f06745e1b033fa9b95b8343de2748bb8312a278`（与此前相同）。
- NSIS SHA-256：`a89da0eeccfbed74881075f2e4c7d918c0ef7bbec83e81ab0e6fef2ebf1e35e3`。

本机已有对应分区布局，只需应用更新：确认上述应用固件 SHA 后，以已确认的键盘 USB serial 运行
`scripts/flash-windows-v2.py --serial <serial> --application-only`。该方式只写 0x10000 的应用镜像，保留 bootloader、分区表、NVS 和声音资源。
烧录成功后再更新 Host，最后验收：启动桌面软件→亮，真正退出→2–4 秒后灭，再启动→恢复。

## 本机更新记录

- Host SHA-256：`298ff0ebbdd19f94c56b2e4a112dfe16b83491ede27bfc2443a8101e72f55a35`。
- 桌面程序 SHA-256：`665b0ff782a181cc465110216fbcc8f2d45a93331c16bf04234832739a91be74`。
- 备份：安装目录内 `backup-before-presence-led-20261005-234151`，含此前两份 exe 及 SQLite 在线备份。
- Host 日志：数据目录 `run/presence-host-stdout.log`、`run/presence-host-stderr.log`。

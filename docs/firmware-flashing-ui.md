# 桌面内完整恢复 VoxQueue

更新日期：2026-10-06。

## 使用

工作台导航保留“键盘总览”和“固件烧录”。槽位绑定、语音服务、设备诊断仍在总览中。
进入固件烧录页，当前操作为“完整恢复 VoxQueue”，可从 EasyInput V2 官方固件切回。

1. 用 USB 数据线连接 EasyInput V2，查看当前恢复包和校验值，点击“开始恢复”。
2. 将键盘电源关机、开机；如果持续等待，在开机状态短按并松开 BOOT 一次进入下载模式。官方 0.5.4 的本机恢复包也说明了这种 BOOT 操作。
3. 写入期间保持连接和供电。页面显示三份镜像的总体进度与日志；全部校验成功后键盘自动重启。

等待串口最多 120 秒，可取消。写入期间不能取消，窗口关闭被阻止。
这里的完整恢复是恢复 VoxQueue 系统镜像，保留现有用户数据，且不执行整片擦除。

## 为什么要恢复三段

本机官方 EasyInput 0.5.4 bootstrap 分区表具有 factory、otadata、ota_0、ota_1。
只向 factory（0x10000）写 VoxQueue，无法保证引导程序放弃 OTA 应用。
因此更新为同时写回 VoxQueue 的 bootloader、分区表和应用。
VoxQueue 分区表仅含一个 factory 应用，没有 otadata 或 OTA 应用入口，启动时不再引用原官方 OTA 选择。
原 OTA 区域不额外擦除；它们在新的分区表中不再参与启动。

## 内置恢复镜像

| 镜像 | 写入地址 | 固定 SHA-256 |
| --- | --- | --- |
| 引导程序 | 0x0 | `be3abea605a6be7f04c2d0f4011bd90688f799a834a164cdc6a29b16c3324287` |
| 分区表 | 0x8000 | `7c541b70dcac8f920c2d11589f06745e1b033fa9b95b8343de2748bb8312a278` |
| VoxQueue 应用 | 0x10000 | `d7670a545bd35461e0fd6cf3f04d24a4ec1f3ffae64f54924ee3a5a18a7a1cc0` |

恢复包校验值：`05c7a56072db82e888c46c45375943dc604c746ccffb9e831632f52b61281c86`。它包含三段镜像的地址与 SHA，页面确认的是整套恢复计划。

Git 内保存固定发布镜像：
- `firmware/releases/bootloader-20261005.bin`
- `firmware/releases/partition-table-20261005.bin`
- `firmware/releases/desktop-presence-20261005.bin`

三份镜像均来自前次 ESP-IDF 5.5.5 构建，应用行为保持桌面软件运行指示灯版本。
安装包携带镜像和官方 esptool 4.12.0 独立程序，用户无需安装 Python 或 ESP-IDF。
工具来源：https://github.com/espressif/esptool/releases/tag/v4.12.0；包内附带官方 LICENSE 和 README。
官方工具归档 SHA-256：`42fddc5e6a05716868ad77fb43acbf53be041f97abed87ff850df1dc88140889`。

## 数据保留与验证

仅写 0x0、0x8000、0x10000，按 4 KiB 擦除边界核对，保护现有 NVS/PHY（0x9000–0x10000）与声音资源（0x310000–0x430000）。
不会写入这些区域，因此恢复本身不清空现有配网、音量、声音；其他固件之前已经修改或清除的配置无法凭这三份系统镜像找回。

开始前逐一检查三份镜像的尺寸和 SHA；等待串口后、写入前再次检查。
仅接受存在且启动的 VID 303A / PID 1001 USB devnode 和有效 COM 映射；多个目标时拒绝选择。
不接受前端任意文件路径或写入地址。状态日志最多 180 行，每行最多 4096 bytes。
只有 esptool 退出成功且三份镜像分别输出 Hash of data verified，才报告完整恢复成功。

## 构建与测试

`prepare-windows-flasher.ps1` 校验并打包三份固定发布镜像，校验官方 esptool 归档；常规构建可下载工具，`-Offline` 使用缓存。

定向 Rust 测试 7 项通过，包括：
- 三份真实发布镜像的校验，以及任何镜像篡改、缺失或版本不匹配时拒绝。
- 实际写入参数只包含三段系统镜像，不含擦除命令；按擦除扇区验证保护数据区域。
- 恢复分区表只含 factory，排除 OTA 引导选择。
- 三镜像总体进度、写入阶段取消拒绝、日志有界及串口检测。

JavaScript 语法与 Git diff 检查通过。新增的官方固件→VoxQueue 实体完整恢复仍待用户实测；不将之前应用单段烧录验收替代完整恢复验收。
界面确认以用户截图为准，不自行使用 computer-use。

2026-10-06：离线 Windows 构建成功，本机桌面程序及三份恢复镜像已替换，并逐份核对 SHA-256 后重新启动 VoxQueue。未进行实体键盘烧录。

- 桌面程序 SHA-256：`3d085c9a8f2412190dcb01d2e3cdcd7c62085eecabc1240a1edd83a190b0f1c3`
- NSIS 安装包 SHA-256：`317e6908799461e58ee2daE98ad78331f35290885028ea325aecccd1d07e1ed5`

## 历史

`29e6268` 首版页面仅写应用，已被本次完整恢复替代。

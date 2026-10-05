# 桌面内烧录当前固件

日期：2026-10-06。

## 使用

工作台导航保留“键盘总览”和“固件烧录”。原先的槽位绑定、语音服务、设备诊断导航只是页内锚点；这些内容仍在总览中。

进入固件烧录页，查看内置固件名称与校验值，连接 EasyInput V2 的 USB 数据线。
点击“开始烧录”后，将键盘电源关机、开机一次，不用按 BOOT。
页面先等待瞬时 ESP32-S3 USB 串口，再写入并显示进度和日志。
“烧录成功”仅在 esptool 正常退出且输出 Hash of data verified 后显示。键盘会自动重启。

等待阶段最多 120 秒，可取消。写入阶段不能取消，窗口关闭被阻止；请保持 USB 连接和供电。
只更新 0x10000 的应用镜像，不覆盖 bootloader、分区表、NVS、音量或声音资源。

## 内置资源

- 当前固件：2026.10.05 · 桌面软件运行指示灯。
- SHA-256：`d7670a545bd35461e0fd6cf3f04d24a4ec1f3ffae64f54924ee3a5a18a7a1cc0`。
- Git 保存已确认的发布镜像 `firmware/releases/desktop-presence-20261005.bin`。该镜像与此前已烧录并校验通过的固件完全一致，不包含键盘运行时 NVS。
- Windows 安装包携带固件和官方 esptool 4.12.0 独立程序，使用者无需安装 Python 或 ESP-IDF，也无需在烧录时下载工具。
- esptool 来源：https://github.com/espressif/esptool/releases/tag/v4.12.0；工具目录附带官方 LICENSE 和 README。
- 官方归档 SHA-256：`42fddc5e6a05716868ad77fb43acbf53be041f97abed87ff850df1dc88140889`。

## 实现

`firmware_flash.rs` 在独立 worker 执行烧录，UI 查询状态；状态日志上限 180 行，每行上限 4096 bytes。
开始前检查 UI 已确认的 hash，并计算固件实际 SHA-256，限制在应用分区大小内。
仅扫描 USB VID 303A / PID 1001，要求对应 Windows devnode 已启动，且 COM 映射存在；拒绝多设备场景。
不把历史 COM 注册表条目或其他普通串口当作烧录目标，也不接受用户输入任意执行路径或写入地址。

构建脚本 `prepare-windows-flasher.ps1` 从已确认发布镜像准备固件资源，并校验官方 esptool 归档。
常规构建可下载该工具；`-Offline` 使用本机缓存。构建新固件后，需明确更新发布镜像和代码中的固件名称/hash。

## 验证与部署

- Rust 定向测试 5 项通过：进度格式与边界、固件篡改和版本不匹配拒绝、写入阶段取消拒绝、日志有界、Windows 串口探测。
- JavaScript 语法检查、Git diff 检查通过，Windows NSIS 构建完成。
- 已更新本机桌面程序、内置固件和独立工具，安装文件与构建文件 hash 一致。
- 用户截图确认“固件烧录”独立页面正常显示，“固件已就绪”、版本、操作步骤和“开始烧录”按钮可见。
- 新增页面尚未完成一次实体写入验收。此前同一固件通过命令行 esptool 写入并校验通过；不能将该结果替代新页面的完整烧录验收。

## 最终构建

- Windows NSIS SHA-256：`d84e98ec3378968c570f40d3a350e0e8f3370a7a29a3e0b7ed2343d7cd23cef9`。
- 桌面 exe SHA-256：`175d5272b3a3f2772bd9ac581b2e35a1df44b2a5865c8a50e58891da6e5e14ae`。
- 用户关闭 VoxQueue 后，已替换最后编译的桌面 exe，并验证安装文件 hash 一致。
- 界面验收以用户提供的截图为准；不自行使用 computer-use 操作或采集界面。

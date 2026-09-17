# Windows 10 下提取 Mevo+ 原始雷达与相机测量点

> 主运行环境：Windows 10 x86_64 笔记本
>
> 目标：把 Mevo+ 的球/杆头原始雷达测量点和相机逐帧跟踪像素点保存到本地。
>
> 不做：Face Impact 解算、雷达与相机坐标融合、三维重建、视频下载、数据库和云同步。

WSL2 Ubuntu 只作为备选运行方式，单独见 [`WSL_UBUNTU_ALTERNATIVE.md`](WSL_UBUNTU_ALTERNATIVE.md)。

零基础首次使用建议按 **第 4 节安装 → 第 5 节连接设备 → 第 9 节采集 → 第 10 节检查结果** 的顺序操作。第 6、7 节是出现问题时使用的单通道验证步骤，可以先跳过。

## 1. 当前实现状态

`ironsight` 已经具备协议解析能力，本次又加入了可直接运行的本地导出程序 `examples/raw_point_export.rs`。

### 已经具备

| 能力 | 当前实现 |
|---|---|
| Mevo+ 控制和雷达数据 | TCP `5100`、`BinaryClient` |
| 球雷达原始点 | `PrcData (0xEC)` / `PrcPoint` |
| 杆头雷达原始点 | `ClubPrc (0xEE)` / `ClubPrcPoint` |
| 击球摘要 | D4、E8、ED、EF、D9 |
| 相机控制和跟踪结果 | TCP `1258`、`GvpClient`、`BallTrackerResult` |
| 球/杆头像素点 | GVP Track，`trackId=0/1` |
| 双通道真机示例 | `examples/gvp_testing.rs` |
| 本地原始点导出 | `examples/raw_point_export.rs`，CSV + JSON |

### 本次已完成

1. 支持 `--device`、`--mode`、`--range-mm`、`--height-mm`、`--output`；
2. 用 GUID 将 TCP 5100 雷达数据与 TCP 1258 GVP 结果关联；
3. 同时处理 `Message`、`ShotDatum`、`ShotComplete` 和 `GvpEvent::Result`；
4. 合并、排序并去重程序实际收到的 PRC/Club PRC 页面；
5. 输出雷达 CSV、相机 CSV、解码 GVP JSON 和设备原始 GVP JSON；
6. 输出设备、Session、每杆摘要、点数、重复数和完整性告警；
7. 先写临时文件，再 rename 到最终文件名；
8. 在 `PROCESSED` 后主动重取全部 0xEC 球 PRC 页面；
9. 在取得最终 ClubResult 后主动重取全部 0xEE 杆头 PRC 页面；
10. 分页短页、预期点数、超时和 64 页安全上限控制，失败后仍继续 re-arm。
11. 雷达与相机生命周期解耦：相机忙、超时或断线不会阻止雷达完成本杆和准备下一杆；
12. GVP RESULT 等待 10 秒后放弃本杆相机结果并重置连接；
13. GVP 断线后仅在雷达已经 Arm 时短超时重连，恢复到 `IDLE` 后再参与后续杆；
14. `ShotComplete` 后明确打印雷达已经重新 Arm，相机不可用时后续杆自动降级为 radar-only。
15. 默认不再逐行打印普通 GVP 调试日志，只保留状态、结果和明显失败信息，减少 Windows 控制台输出对雷达轮询的干扰。

### 仍需真机确认

- 当前 Mevo+ 固件是否完全遵循仓库抓包记录的 0xEC page index 语义；
- 实际点数是否与 TrackingStatus / ClubResult 报告一致；
- Windows Ctrl+C 的优雅退出和“正在采集但尚未 ShotComplete”的 partial shot 强制写盘；
- 用真实 Mevo+ 连续击球验证相机标定参数和不同固件下的消息时序。

### 明确不需要新增

- Face Impact 或杆面撞击位置算法；
- 雷达点与相机点的空间融合；
- 相机 `u/v` 到三维坐标的转换；
- FRP；
- MJPEG/MP4 下载；
- 数据库或服务器。

## 2. 要保存的数据

### 2.1 球雷达原始点

`PrcPoint` 包含：

- `index`、`buf_idx`、`flags`；
- 原始时间计数 `time`；
- `az`、`el`；
- `dist`、径向速度 `vel`；
- `snr`、`peak`；
- `az1/az2/az3`、`el1/el2`；
- 六路峰值 `pk[6]`；
- `sync_idx`、`sync_buf` 和 `n`。

这些是雷达实际测量的离散点。典型每杆约 46–112 点，但不能把该范围写成硬限制。

### 2.2 杆头雷达原始点

`ClubPrcPoint` 包含时间、距离、方向、两组速度、信号质量和分天线测量。`buf_ofs < 0` 通常是撞击前，`buf_ofs > 0` 通常是撞击后。

### 2.3 相机原始点

GVP `RESULT` 的 Track 包含：

```text
trackId
frameNumber[]
timestamp[]
u[]
v[]
radius[]
circularityFactor[]
shutterTime_ms[]
```

- `trackId=0`：球；
- `trackId=1`：杆头；
- `trackId=2–4`：参考标记。

这些是相机跟踪器输出的二维像素测量点，不是未经处理的图像帧；不做雷达融合或三维转换即可原样保存。

### 2.4 建议同时保存的元数据

从 D4 `FlightResult` 保存 shot counter、球速、VLA/HLA、Carry、flight time、起点、落地点、旋转和 `poly_x/y/z`。这些字段只用于关联和质量检查，不需要额外生成模拟轨迹点。

## 3. 本地输出结构

### 3.1 输出目录树

```text
C:\MevoData\
└── 2026-09-15_073042Z\                 # UTC 时间；重名时追加 _001
    ├── device.json
    ├── session.json
    ├── session.log
    ├── shot_000001\
    │   ├── summary.json
    │   ├── radar_ball_raw.csv
    │   ├── radar_club_raw.csv
    │   ├── camera_ball_raw.csv
    │   ├── camera_club_raw.csv
    │   ├── camera_reference_raw.csv
    │   ├── gvp_result.json
    │   └── gvp_result_raw.json
    └── shot_000002\
        └── ...
```

雷达两个 CSV 和 `summary.json` 在 ShotComplete 时生成；三个相机 CSV 与两个 GVP JSON 只有收到对应 GUID 的 RESULT 后才生成。`gvp_result_raw.json` 还要求原始 JSON callback 捕获成功。

### 3.2 每个输出文件包含什么

| 文件 | 主要内容 | 来源 | 用途 |
|---|---|---|---|
| `device.json` | 设备型号、SSID、DSP/AVR/PI 信息、DSP 类型、产品信息 | Handshake | 标识采集设备和固件环境 |
| `session.json` | 开始/更新时间、程序版本、模式、RadarCal、地址、输出路径、杆数 | 程序配置 | 记录整次采集条件；每次落盘后更新 |
| `session.log` | 启动参数、握手完成、Trigger、Result、断线和写盘事件 | 两条连接及程序状态 | 排查漏点、断线和时序问题 |
| `summary.json` | shot ID、GUID、D4 摘要、各类点数、完整状态和 warnings | 二进制 + GVP | 快速判断这一杆是否完整 |
| `radar_ball_raw.csv` | 每一条 `PrcPoint` 的原始解码字段 | 0xEC | 保存球的雷达离散测量点 |
| `radar_club_raw.csv` | 每一条 `ClubPrcPoint` 的原始解码字段 | 0xEE | 保存撞击前后的杆头雷达点 |
| `camera_ball_raw.csv` | `trackId=0` 的逐帧时间、u/v、半径和形状指标 | GVP RESULT | 保存球的二维相机跟踪点 |
| `camera_club_raw.csv` | `trackId=1` 的逐帧时间、u/v、半径和形状指标 | GVP RESULT | 保存杆头的二维相机跟踪点 |
| `camera_reference_raw.csv` | `trackId=2–4` 的参考点 | GVP RESULT | 保留完整相机测量上下文 |
| `gvp_result.json` | `BallTrackerResult` 已解码结构的完整 JSON | `GvpEvent::Result` | 方便程序再次读取和分析 |
| `gvp_result_raw.json` | 设备发送的原始 RESULT JSON 字符串；成功捕获时生成 | `GvpConnection::set_on_recv` | 防止解码结构忽略未知字段 |

### 3.3 `device.json`

当前实现保存：

```json
{
  "schemaVersion": 1,
  "model": "Mevo+",
  "ssid": "FS M2-XXXXXX",
  "dspType": 128,
  "dspFirmware": "...",
  "avrFirmware": "...",
  "piFirmware": "..."
}
```

不要保存 Handshake 中可能出现的设备 Wi-Fi 密码。

### 3.4 `session.json`

当前实现保存：

- `schemaVersion`；
- 开始时间和最近更新时间（Unix epoch）；
- exporter/crate 版本；
- Indoor/Outdoor 模式；
- `range_mm`、`height_mm`；
- 二进制和 GVP 地址；
- 输出目录；
- 已创建杆数、已完成雷达生命周期杆数和已收到相机结果杆数。

### 3.5 `summary.json`

这是每杆的索引和完整性报告，建议至少包含：

```json
{
  "schemaVersion": 1,
  "shotId": 1,
  "guid": "{...}",
  "shotLifecycleComplete": true,
  "radarComplete": true,
  "activePaginationEnabled": true,
  "pagination": {
    "ballComplete": true,
    "clubComplete": true,
    "ballPagesRequested": 20,
    "ballPagesReceived": 20,
    "clubPagesRequested": 5,
    "clubPagesReceived": 5
  },
  "cameraResultReceived": true,
  "cameraTriggerSent": true,
  "cameraTimedOut": false,
  "cameraFailed": false,
  "ballPrcPointCount": 80,
  "clubPrcPointCount": 14,
  "cameraBallPointCount": 10,
  "cameraClubPointCount": 15,
  "duplicateBallPrcPoints": 0,
  "duplicateClubPrcPoints": 0,
  "warnings": []
}
```

同时嵌入 D4/E8、Club、Spin、SpeedProfile 和分页状态。`radarComplete` 只有在 Shot 生命周期完成、球与杆头分页完成、存在飞行结果、存在球 PRC，且已知的预期点数检查通过时才为 true。

相机状态不参与 `radarComplete`。即使 `cameraTimedOut=true` 或 `cameraFailed=true`，雷达仍会完成保存和 re-arm；这些字段只说明这一杆的相机结果是否可用。

### 3.6 雷达 CSV

`radar_ball_raw.csv` 每行对应一个 `PrcPoint`：

```text
shot_id,guid,page_sequence,index,buf_idx,flags,time_tick,time_seconds,
n,az_deg,el_deg,radial_velocity_mps,dist_m,sync_idx,sync_buf,
snr,peak,az1_deg,az2_deg,az3_deg,el1_deg,el2_deg,
pk0,pk1,pk2,pk3,pk4,pk5
```

`radar_club_raw.csv` 每行对应一个 `ClubPrcPoint`：

```text
shot_id,guid,index,buf_ofs,phase,peak,snr,buf_idx,time_tick,time_seconds,
n,az_deg,el_deg,velocity_mps,velocity2_mps,dist_m,f30,f33,version,f39,f42,f45,
az1_deg,az2_deg,az3_deg,el1_deg,el2_deg,
pk0,pk1,pk2,pk3,pk4,pk5
```

`phase` 由 `buf_ofs` 标记为 `pre_impact`、`impact` 或 `post_impact`。CSV 保留原始角度、距离和时间，不做融合或坐标改写。

### 3.7 相机 CSV 和 JSON

三个相机 CSV 使用同一列格式，每行对应一个 Track 数组下标：

```text
shot_id,guid,track_id,point_index,frame_number,timestamp,
u_px,v_px,radius_px,circularity_factor,shutter_time_ms
```

`gvp_result.json` 是已解码结构重新序列化的 JSON。若还要逐字节保留设备原消息，应在创建 `GvpClient` 前给 `GvpConnection` 设置 `set_on_recv` callback，将匹配当前 GUID 的原始 RESULT 字符串写入 `gvp_result_raw.json`。

### 3.8 写入规则

- 所有文件先写 `.tmp`，flush 成功后再 rename；
- 雷达 ShotComplete 后先写雷达文件；
- GVP RESULT 晚到时再补写相机文件；
- 同一路径存在时创建新的 Session，不覆盖旧数据；
- 相机无结果时保留雷达文件，并在 summary 记录原因；
- 每次 ShotComplete 或 GVP RESULT 到达时立即更新文件；GVP 断线或等待 RESULT 超过 10 秒时也会更新 summary；当前无额外依赖的最小版尚未实现 Ctrl+C 时强制保存尚未完成的 partial shot；
- 密码、授权证书和不必要的个人信息不写入导出目录。

### 3.9 实施后的完整项目结构

图例：

- `[现有]`：当前仓库已经存在；
- `[已实现-本次新增]`：本次新增且已经完成；
- `[已修改-分页]`：本次为主动 PRC 分页修改；
- `[新增-建议]`：从单文件示例工程化时建议增加；
- `[新增-文档]`：本次分析新增。

```text
ironsight\
├── .github\
│   └── workflows\
│       ├── ci.yml                         [现有]
│       └── release.yml                    [现有]
├── docs\
│   ├── CAMERA.md                          [现有]
│   ├── SEQUENCE.md                        [现有]
│   └── WIRE.md                            [现有]
├── examples\
│   ├── client.rs                          [现有]
│   ├── event_loop.rs                      [现有]
│   ├── gvp_testing.rs                     [现有]
│   ├── mode_change_test.rs                [现有]
│   └── raw_point_export.rs                [已实现-本次新增]
├── src\
│   ├── bin\
│   │   └── ironsight-frp.rs               [现有]
│   ├── export\                            [新增-建议]
│   │   ├── mod.rs                         [新增-建议]
│   │   ├── collector.rs                   [新增-建议]
│   │   ├── model.rs                       [新增-建议]
│   │   ├── writer.rs                      [新增-建议]
│   │   └── validation.rs                  [新增-建议]
│   ├── frp\
│   │   ├── convert.rs                     [现有]
│   │   └── mod.rs                         [现有]
│   ├── gvp\
│   │   ├── client.rs                      [现有]
│   │   ├── config.rs                      [现有]
│   │   ├── conn.rs                        [现有]
│   │   ├── log.rs                         [现有]
│   │   ├── mod.rs                         [现有]
│   │   ├── result.rs                      [现有]
│   │   ├── splitter.rs                    [现有]
│   │   ├── status.rs                      [现有]
│   │   ├── track.rs                       [现有]
│   │   ├── trigger.rs                     [现有]
│   │   └── video.rs                       [现有]
│   ├── protocol\
│   │   ├── ack.rs                         [现有]
│   │   ├── camera.rs                      [现有]
│   │   ├── config.rs                      [现有]
│   │   ├── handshake.rs                   [现有]
│   │   ├── mod.rs                         [已修改-分页：增加 0xEC/0xEE 请求命令]
│   │   ├── shot.rs                        [已修改-分页：增加请求结构和编码测试]
│   │   └── status.rs                      [现有]
│   ├── addr.rs                            [现有]
│   ├── client.rs                          [已修改-分页：增加启用开关]
│   ├── codec.rs                           [现有]
│   ├── conn.rs                            [现有]
│   ├── error.rs                           [现有]
│   ├── frame.rs                           [现有]
│   ├── lib.rs                             [修改-建议：导出 export 模块]
│   └── seq.rs                             [已修改-分页：分页状态机、超时和状态]
├── tests\
│   ├── fixtures\                         [新增-建议]
│   │   ├── radar_shot_frames.bin          [新增-建议]
│   │   └── gvp_result.json                [新增-建议]
│   ├── export_files.rs                    [新增-建议]
│   ├── gvp_pcap.rs                        [现有]
│   └── gvp_pcap_messages.json             [现有]
├── .gitignore                             [现有]
├── Cargo.toml                             [现有；加依赖时才修改]
├── LICENSE-APACHE                         [现有]
├── LICENSE-MIT                            [现有]
├── Makefile                               [现有]
├── MEVO_PLUS_TRAJECTORY_EXPORT_GUIDE.md   [新增-文档]
├── NOTICE                                 [现有]
├── PROJECT_REPORT.md                      [新增-文档]
├── README.md                              [现有]
├── rust-toolchain.toml                    [现有]
└── WSL_UBUNTU_ALTERNATIVE.md              [新增-文档]
```

Collector 和 writer 仍集中在 `raw_point_export.rs` 中，没有增加第三方依赖。主动分页所需的命令编码、客户端开关和 Shot 状态机已经分别加入 `protocol/shot.rs`、`protocol/mod.rs`、`client.rs` 和 `seq.rs`。真机验证稳定后，可再把导出逻辑拆分到 `src/export/`。

## 4. Windows 10 从零安装

本节按“刚拿到一台 Windows 10 电脑、没有安装过编程工具”的情况编写。只想运行导出器，也必须先完成一次编译；编译成功后，以后采集时只需要运行生成的 `raw_point_export.exe`，不用每次重新安装。

整个过程分成两种网络状态：

1. **电脑连接普通互联网时**：安装工具、下载项目、下载 Rust 依赖并完成编译；
2. **电脑连接 Mevo+ Wi-Fi 时**：运行已经编译好的程序并采集数据。

Mevo+ Wi-Fi 通常不能访问互联网，所以一定要先完成第 4 节，再切换到 Mevo+ Wi-Fi。

### 4.1 开始前准备

需要准备：

- 一台 64 位 Windows 10 笔记本；
- 可用的普通互联网连接；
- 至少约 10 GB 可用磁盘空间，主要供 Visual Studio Build Tools 使用；
- 已解压的本项目源码，或者可以访问 GitHub 下载源码；
- Mevo+ 及其 Wi-Fi 密码；
- 一把卷尺，用于测量 Mevo+ 到球的距离，以及击球面相对设备基准的高度参数。

先确认 Windows 是 64 位：

1. 按键盘 `Win + I` 打开“设置”；
2. 进入“系统” → “关于”；
3. 查看“系统类型”；
4. 应显示“64 位操作系统，基于 x64 的处理器”。

如果是 32 位 Windows，不要继续安装本指南中的 x64 工具链，应先更换为 64 位 Windows。

### 4.2 认识 PowerShell

后面的命令都在 PowerShell 中执行：

1. 单击 Windows 左下角“开始”；
2. 输入 `PowerShell`；
3. 打开“Windows PowerShell”；
4. 看到类似 `PS C:\Users\你的用户名>` 的提示符后，即可输入命令。

除非某一步明确要求，否则不需要“以管理员身份运行”。每次只复制代码框中的命令，不要复制前面的 `PS C:\...>` 提示符。

可以先测试：

```powershell
Write-Host "PowerShell 可以正常使用"
```

如果屏幕打印出“PowerShell 可以正常使用”，说明操作正确。

### 4.3 安装 Visual Studio 2022 Build Tools

Rust 在 Windows 上需要微软的 C++ 链接器。这里安装的是免费的编译工具，不需要安装完整的 Visual Studio，也不需要登录微软账号。

1. 保持电脑连接普通互联网；
2. 用浏览器打开 <https://visualstudio.microsoft.com/visual-cpp-build-tools/>；
3. 下载“Build Tools for Visual Studio 2022”；
4. 双击下载到的 `vs_BuildTools.exe`；
5. 如果 Windows 弹出“是否允许此应用对你的设备进行更改”，选择“是”；
6. 等待“Visual Studio Installer”启动；
7. 在“工作负荷（Workloads）”页勾选“使用 C++ 的桌面开发（Desktop development with C++）”；
8. 查看右侧“安装详细信息”，确认至少包含以下内容：
   - MSVC v143 - VS 2022 C++ x64/x86 build tools；
   - Windows 10 SDK，或者安装器提供的 Windows 11 SDK；
9. 单击右下角“安装（Install）”；
10. 等待安装完成。如果安装器要求重启电脑，就先重启。

安装器中其他 C++ 可选组件不必全部勾选。以后如果编译时看到 `link.exe not found`，重新打开“Visual Studio Installer”，单击 Build Tools 旁边的“修改”，确认上述工作负荷和 SDK 已安装。

### 4.4 安装 Rust 1.94 工具链

1. 用浏览器打开 <https://rustup.rs/>；
2. 下载并运行 `rustup-init.exe`；
3. 出现黑色安装窗口后，输入 `1` 并按回车，采用默认安装；
4. 等待看到 Rust 安装完成的提示；
5. 关闭之前打开的所有 PowerShell 窗口；
6. 重新打开一个 PowerShell，让新的 `PATH` 环境变量生效。

先确认 `rustup` 已经可用：

```powershell
rustup --version
```

然后安装项目指定的 Rust 1.94、MSVC 目标和 Clippy：

```powershell
rustup set default-host x86_64-pc-windows-msvc
rustup toolchain install 1.94-x86_64-pc-windows-msvc
rustup component add clippy --toolchain 1.94-x86_64-pc-windows-msvc
```

检查安装结果：

```powershell
rustup show
rustc +1.94-x86_64-pc-windows-msvc --version
cargo +1.94-x86_64-pc-windows-msvc --version
```

应能看到包含以下内容的输出，后面的小版本信息可能略有不同：

```text
rustc 1.94...
cargo 1.94...
x86_64-pc-windows-msvc
```

如果提示“无法将 `rustup`、`rustc` 或 `cargo` 识别为命令”：

1. 先关闭 PowerShell，再重新打开；
2. 重新执行 `rustup --version`；
3. 仍然失败时，在当前窗口执行下面这行临时加入路径，然后重试：

```powershell
$env:Path += ";$env:USERPROFILE\.cargo\bin"
```

### 4.5 获取项目源码

建议把项目放在英文、无空格的短路径中。本指南使用：

```text
C:\Users\你的用户名\source\ironsight
```

下面两种方法任选一种。已经拿到包含本指南和 `examples\raw_point_export.rs` 的完整项目文件夹时，直接使用“方法 B”。

#### 方法 A：使用 Git 下载

先安装 Git：

1. 打开 <https://git-scm.com/download/win>；
2. 下载 64 位 Git for Windows 安装器；
3. 双击安装；
4. 不确定某个选项时保留默认值，一直单击“Next”，最后单击“Install”；
5. 安装完成后关闭并重新打开 PowerShell。

确认 Git 可用：

```powershell
git --version
```

创建源码目录并下载项目：

```powershell
New-Item -ItemType Directory -Force "$env:USERPROFILE\source"
Set-Location "$env:USERPROFILE\source"
git clone https://github.com/Apollo66666/ironsight.git
Set-Location "$env:USERPROFILE\source\ironsight"
```

如果 `git clone` 提示目标文件夹已经存在，不要反复下载。进入已有目录：

```powershell
Set-Location "$env:USERPROFILE\source\ironsight"
```

#### 方法 B：解压收到的 ZIP 项目包

1. 在资源管理器中找到 ZIP 文件；
2. 右键 ZIP → “全部提取”；
3. 将解压后的项目文件夹移动到 `C:\Users\你的用户名\source\ironsight`；
4. 打开该文件夹，确认里面能直接看到 `Cargo.toml`、`src` 和 `examples`；
5. 如果出现 `ironsight\ironsight\Cargo.toml` 这种双层目录，应把包含 `Cargo.toml` 的内层目录作为项目目录。

然后在 PowerShell 中进入项目目录：

```powershell
Set-Location "$env:USERPROFILE\source\ironsight"
```

确认目录正确：

```powershell
Get-ChildItem Cargo.toml, .\examples\raw_point_export.rs
```

两项都能列出来才继续。如果提示找不到 `Cargo.toml`，说明当前目录不对；在资源管理器里找到 `Cargo.toml` 所在文件夹，再复制其完整路径用于 `Set-Location`。

本仓库的 `.gitignore` 明确忽略了 `Cargo.lock`，所以刚 `git clone` 或 `git pull` 后看不到该文件是正常现象。第一次执行 `cargo fetch` 或 `cargo build` 时，Cargo 会在本地自动生成它；不要手工创建空的 `Cargo.lock`。由于仓库没有提交锁文件，首次运行命令不能加 `--locked`。

### 4.6 在普通互联网下下载依赖

确认此时电脑仍连接普通互联网，不要连接 Mevo+ Wi-Fi。进入项目目录后执行：

```powershell
Set-Location "$env:USERPROFILE\source\ironsight"
cargo fetch
```

第一次执行会下载若干 Rust 软件包，等待命令结束并重新出现 `PS ...>` 提示符。黄色 `warning` 通常只是警告；红色 `error` 才表示失败。

如果这里提示网络超时，先用浏览器确认普通互联网正常，再重新执行同一条命令。必须在连接 Mevo+ 之前完成依赖下载。

### 4.7 编译导出器

仍在项目目录中执行：

```powershell
cargo build --release --features gvp --example raw_point_export
```

第一次编译可能需要几分钟。成功时最后会看到类似：

```text
Finished `release` profile ...
```

编译完成后的程序位置是：

```text
C:\Users\你的用户名\source\ironsight\target\release\examples\raw_point_export.exe
```

用下面的命令确认文件确实存在：

```powershell
Test-Path .\target\release\examples\raw_point_export.exe
```

应返回：

```text
True
```

再查看程序帮助：

```powershell
.\target\release\examples\raw_point_export.exe --help
```

能看到 `--device`、`--mode`、`--range-mm`、`--height-mm` 和 `--output`，就说明安装和编译已经成功。

### 4.8 创建数据输出目录

默认输出目录是 `C:\MevoData`。先尝试创建：

```powershell
New-Item -ItemType Directory -Force "C:\MevoData"
```

如果提示“拒绝访问”，不要用管理员权限硬改，可以改用当前用户肯定有权限的文档目录：

```powershell
New-Item -ItemType Directory -Force "$env:USERPROFILE\Documents\MevoData"
```

后面运行时把 `--output C:\MevoData` 对应改成：

```text
--output "$env:USERPROFILE\Documents\MevoData"
```

### 4.9 可选的开发检查

以下命令用于检查代码，不是正常采集的必需步骤。需要时在普通互联网下执行：

```powershell
cargo check --features gvp --example raw_point_export
cargo clippy --features gvp --example raw_point_export -- -D warnings
cargo test --lib --features gvp
```

当前完整的 `cargo test --features gvp` 还会触发一个仓库既有的 `fusion_preset` 断言不一致；它不涉及导出器使用的 `raw_fusion_preset`，详见项目报告第 17 节。只运行导出器不需要执行完整测试。

## 5. Windows 10 连接 Mevo+

### 5.1 切换到 Mevo+ Wi-Fi

1. 确认第 4.7 节已经编译成功；
2. 打开 Mevo+，等待设备完成启动；
3. 单击 Windows 任务栏右下角的网络图标；
4. 在 Wi-Fi 列表中找到 `FS M2-XXXXXX`；
5. 单击“连接”；
6. 按设备标签、屏幕或官方说明输入该设备的 Wi-Fi 密码；
7. 等待 Windows 显示“已连接，无 Internet”或“无 Internet，安全”。

“无 Internet”是正常现象，因为这个 Wi-Fi 用来直连 Mevo+，不是用来上网。不要因为该提示切回家中 Wi-Fi。

连接前还应做这些事：

- 完全退出 FS Golf、E6、Awesome Golf 等可能连接 Mevo+ 的程序；
- 让手机和平板断开 Mevo+ Wi-Fi，避免另一个客户端占用设备；
- 暂停 VPN 和代理软件；
- 笔记本同时插网线时，首次测试建议先拔掉网线，避免路由走错；
- 长时间采集时接通电源，并临时关闭自动睡眠。

### 5.2 确认电脑拿到正确地址

打开 PowerShell，执行：

```powershell
ipconfig
```

找到“无线局域网适配器 WLAN”或“Wireless LAN adapter Wi-Fi”，确认 IPv4 地址类似：

```text
192.168.2.2
192.168.2.10
192.168.2.xxx
```

只要前三段是 `192.168.2` 即可，不要求最后一段完全相同。Mevo+ 默认地址是 `192.168.2.1`。

如果电脑地址不是 `192.168.2.x`：

1. 在 Wi-Fi 列表中断开 `FS M2-XXXXXX`；
2. 等待 5 秒后重新连接；
3. 再执行一次 `ipconfig`；
4. 仍然不对时，重启 Mevo+ 和电脑的 Wi-Fi，再试一次。

### 5.3 测试雷达和相机端口

在 PowerShell 中逐条执行：

```powershell
Test-NetConnection 192.168.2.1 -Port 5100
Test-NetConnection 192.168.2.1 -Port 1258
```

每条命令都要查看最后的：

```text
TcpTestSucceeded : True
```

判断方法：

- 5100 为 `True`：雷达服务可连接，这是启动导出器的必要条件；
- 1258 为 `True`：GVP 相机服务当前已经可连接；
- 5100 为 `True`、1258 为 `False`：先不要判定失败。导出器会通过 5100 启动并切换相机模式，然后才连接 1258；可以继续运行一次，以程序是否显示 `GVP connected. Waiting for camera IDLE...` 为最终判断；
- 两者都为 `False`：通常是 Wi-Fi 连错、其他 App 正占用设备、VPN/路由干扰，或者 Mevo+ 尚未启动完成。

端口失败时按以下顺序排查：

1. 再确认当前 Wi-Fi 名称确实是 `FS M2-XXXXXX`；
2. 退出所有官方和第三方高尔夫软件；
3. 关闭 VPN；
4. 重启 Mevo+，重新连接 Wi-Fi 后等待约 30 秒；
5. 再执行两条 `Test-NetConnection`；
6. 仍失败时执行 `route print`，确认 `192.168.2.0` 没有被其他网卡或 VPN 占用。

不建议为了测试而永久关闭 Windows 防火墙。如果首次运行程序时 Windows 防火墙弹窗询问是否允许访问，只勾选“专用网络”，然后选择“允许访问”。

## 6. 可选：单独验证雷达原始点

如果只想尽快使用最终导出器，可以跳过第 6、7 节，直接进入第 9 节。遇到问题、需要判断是雷达通道还是相机通道失败时，再回来分别运行这两个底层示例。

先构建现有低层示例：

```powershell
Set-Location "$env:USERPROFILE\source\ironsight"
cargo build --release --example event_loop
```

连接 Mevo+ 后运行：

```powershell
.\target\release\examples\event_loop.exe
```

正常启动应看到：

```text
Handshake complete
Configured
ARMED
```

击球后重点寻找：

```text
Flight result
PRC: seq=... points=...
Club PRC: points=...
PROCESSED
IDLE
```

### 检测模式

现有示例默认使用 `MODE_CHIPPING`。普通室内全挥杆应改为 `MODE_INDOOR`，室外使用 `MODE_OUTDOOR`：

```rust
use ironsight::protocol::config::MODE_INDOOR;

AvrSettings {
    mode: MODE_INDOOR,
    // ...
}
```

`RadarCal.range_mm` 和 `height_mm` 仍应按实际安装几何填写；两者不是从击球结果直接读取的字段。

本步骤验收：

- 完成 Handshake 和 Arm；
- 一杆后收到 D4 或 E8；
- 至少收到一页 `PrcData`；
- 击球结束后自动重新 Arm。

## 7. 可选：单独验证相机原始点

相机原始点来自 TCP 1258 的 GVP `RESULT`，不是 8080 视频流。

构建现有双通道示例：

```powershell
Set-Location "$env:USERPROFILE\source\ironsight"
cargo build --release --features gvp --example gvp_testing
```

运行：

```powershell
.\target\release\examples\gvp_testing.exe
```

该示例会：

1. 连接 5100 并完成握手；
2. 标准模式预热相机；
3. 切换 Raw Fusion 相机配置；
4. 连接 1258；
5. 击球时发送带 GUID 的 GVP Trigger；
6. 根据雷达数据发送 Expected Club/Ball Track；
7. 接收并打印 GVP RESULT。

预期看到：

```text
[gvp] Connected
[gvp] STATUS: TRIGGERED
[gvp] STATUS: PROCESSING
[gvp] RESULT guid=...
trackId=1 (club) points=...
trackId=0 (ball) points=...
```

你不需要计算 Face Impact，但 GVP 跟踪器仍可能需要 Expected Track 作为搜索提示。可复用现有示例逻辑。示例中的相机焦距、主点、高度和倾角是实验性硬编码值；如果 RESULT 不稳定，需要再校准这些参数。

`gvp_testing.rs` 同样默认使用 `MODE_CHIPPING` 和示例 RadarCal。运行普通全挥杆测试前，也要按第 6 节改成实际 Indoor/Outdoor 模式及真实摆位参数。

GVP、Raw Fusion 或杆头跟踪可能受设备固件和已购买功能影响。本项目不会绕过授权。

本步骤验收：

- 1258 可以连接；
- 相机 warmup 完成；
- 收到与当前 GUID 相同的 RESULT；
- RESULT 中存在 `trackId=0` 或 `trackId=1`；
- frame、timestamp、u/v 数组有有效数据。

## 8. 已实施的最小导出器

本次新增的 `examples/raw_point_export.rs` 已实现以下流程：

```text
连接 5100 → 三段握手 → 配置雷达 → 相机 Standard 预热 → Raw Fusion
     ↓
连接 1258 → Arm
     ↓
BALL TRIGGER → 创建 shot ID/GUID → 发送 GVP Trigger
     ↓
并行轮询 5100/1258
     ├── 合并 Message、ShotDatum、ShotComplete
     ├── 主动逐页请求 0xEC 球点和 0xEE 杆头点
     ├── 发送 Expected Ball/Club Track 搜索提示
     └── 按 GUID 接收 GVP RESULT
     ↓
ShotComplete 先写雷达；RESULT 晚到时补写相机并更新 summary
```

导出器内部按 GUID 保存多个 `ShotCollector`，所以后一杆开始后，前一杆迟到的 GVP RESULT 仍可找到正确目录。雷达点、杆头点和相机点分别按第 3 节的 key 去重；相机平行数组只写共同有效长度，并把异常写入 `warnings`。

Expected Track 只是给设备内相机跟踪器提供搜索区域，不会把雷达点与相机点融合，也不会改变导出的原始测量值。
导出器把 GVP 的 `save_videos_enabled` 设为 `false`，并忽略视频可用通知，不下载或保存视频。

雷达循环不等待 GVP RESULT。相机在上一杆仍为 `PROCESSING`、连接已经断开或尚未恢复到 `IDLE` 时，新一杆仍会正常走雷达流程，只是该杆跳过 GVP Trigger 并在 summary 中标记为 radar-only。GVP 结果超过 10 秒未到达时，程序关闭该相机连接并后台重连，不影响雷达端继续接收击球。

## 9. Windows 10 第一次采集：逐步照做

第 4 节只需安装一次。以后每次采集，从本节开始操作即可。

### 9.1 摆放设备并记录参数

先按 Mevo+ 官方要求摆好设备、球和击球区域，再记录下面三个参数：

| 参数 | 怎样填写 | 示例 |
|---|---|---|
| `--mode` | 默认是室内；室内填 `indoor`，室外填 `outdoor` | `indoor` |
| `--range-mm` | Mevo+ 到球的水平距离，单位毫米 | 2.743 米 = `2743` |
| `--height-mm` | 击球面相对设备基准的高度参数（surface height），单位毫米 | 2.5 厘米 = `25` |

换算方法：

- 米乘以 1000 等于毫米，例如 `2.5 米 = 2500 毫米`；
- 厘米乘以 10 等于毫米，例如 `3 厘米 = 30 毫米`。

不要直接照抄示例数值，应填写现场实际测量值。`height-mm` 不是简单地填写“设备离地高度”，而是协议中的击球面高度校准量。当前程序的可填写范围是 `0–255`；如果你的测量值明显超出该范围，先核对测量基准和单位，不要随意截断数字。

#### 为什么距离和高度需要手工填写

这两个值属于击球前发送给 AVR 的 `RadarCal (0xA4)` 安装校准参数：

- `range-mm` 是设备到球位（tee）的安装距离；
- `height-mm` 是击球面高度校准量；
- 设备收到后只会回显这两个值，表示配置已接收，并不会返回一组独立测量出来的安装距离和高度。

不能直接拿击球后的雷达结果代替，主要有三个原因：

1. 雷达在 Arm 和跟踪之前就需要这些安装参数，击球结果此时还不存在；
2. PRC 点中的 `dist` 是球或杆头在各采样时刻相对雷达的动态斜距，不是固定的“雷达到球位水平距离”；
3. FlightResult 的起点、轨迹和落点已经经过设备内部配置与解算，用它反推同一组校准参数会形成循环依赖。

当前协议实现没有发现“让设备自动测出并返回 RadarCal 安装几何”的独立读取命令。理论上可以另外开发静止球/标定板识别和自动标定流程，但那是新的标定算法，不是简单读取现有击球结果。本程序因此要求启动时明确填写并写入 `session.json`；其中 `range-mm` 还会用于生成相机 Expected Track，`height-mm` 当前只发送给设备作为 RadarCal。

### 9.2 做启动前检查

逐项确认：

- [ ] 第 4.7 节中的 `Test-Path` 返回 `True`；
- [ ] Windows 当前连接的是 Mevo+ 的 `FS M2-XXXXXX` Wi-Fi；
- [ ] `ipconfig` 显示电脑地址为 `192.168.2.x`；
- [ ] 5100 的 `TcpTestSucceeded` 是 `True`；1258 最好为 `True`，但启动前为 `False` 时仍可继续运行一次，由程序先启动相机服务；
- [ ] 手机、平板和其他电脑没有同时连接或控制这台 Mevo+；
- [ ] FS Golf 等软件已经完全退出；
- [ ] 已经量好 `range-mm` 和 `height-mm`；
- [ ] 击球区域安全，电脑已接通电源且不会自动睡眠。

### 9.3 打开 PowerShell 并进入项目目录

打开一个新的 PowerShell，执行：

```powershell
Set-Location "$env:USERPROFILE\source\ironsight"
```

确认程序仍然存在：

```powershell
Test-Path .\target\release\examples\raw_point_export.exe
```

如果返回 `False`，回到第 4.7 节重新编译。注意：此时电脑连接 Mevo+，通常没有互联网；如果 Rust 依赖之前没有下载完整，需要先切回普通互联网完成编译，再重新连接 Mevo+。

### 9.4 复制命令并替换三个现场参数

默认室内示例，输出到 `C:\MevoData`：

```powershell
.\target\release\examples\raw_point_export.exe --device 192.168.2.1:5100 --mode indoor --range-mm 2743 --height-mm 25 --output "C:\MevoData"
```

室外示例：

```powershell
.\target\release\examples\raw_point_export.exe --device 192.168.2.1:5100 --mode outdoor --range-mm 2438 --height-mm 25 --output "C:\MevoData"
```

请根据现场修改 `indoor/outdoor`、距离和高度。整条命令应一次性复制到 PowerShell 中，再按回车。

如果第 4.8 节选择了“文档”目录，则使用：

```powershell
.\target\release\examples\raw_point_export.exe --device 192.168.2.1:5100 --mode indoor --range-mm 2743 --height-mm 25 --output "$env:USERPROFILE\Documents\MevoData"
```

所有参数含义：

| 参数 | 必须修改吗 | 说明 |
|---|---|---|
| `--device 192.168.2.1:5100` | 通常不用 | Mevo+ 默认雷达地址和端口 |
| `--mode indoor` | 按环境修改 | 默认 `indoor`；只允许 `indoor` 或 `outdoor`，大小写均可，建议使用小写 |
| `--range-mm 2743` | 必须实测 | 雷达到球的水平距离，范围 `1–65535` 毫米 |
| `--height-mm 25` | 必须实测 | 击球面高度校准量，范围 `0–255`，不是简单的设备离地高度 |
| `--output "C:\MevoData"` | 可选 | 数据保存位置；带空格的路径必须加双引号 |

随时可查看帮助：

```powershell
.\target\release\examples\raw_point_export.exe --help
```

### 9.5 等待程序完成初始化

按回车后不要马上击球。程序会依次显示类似信息：

```text
Output session: C:\MevoData\2026-09-17_...
Connecting to binary protocol at 192.168.2.1:5100...
Connected to binary protocol.
DSP sync...
AVR sync...
PI sync...
Configuring mode=indoor range=2743mm height=25mm...
Starting camera (standard warmup)...
Switching camera to Raw Fusion...
Connecting to GVP at 192.168.2.1:1258...
GVP connected. Waiting for camera IDLE...
[gvp] status=IDLE
Camera ready for the next shot.
ARMED - hit a ball.
```

只有看到下面这一行后才击球：

```text
ARMED - hit a ball.
```

如果 Windows 防火墙首次弹窗，只允许“专用网络”访问即可。如果程序在 `Connecting`、`sync` 或相机启动阶段报错并退出，先按第 14 节排查，不要继续击球。

### 9.6 击球并等待文件保存

击球后，程序会打印类似：

```text
Shot #1 triggered; guid=...
Shot #1 radar saved: ball=... club=... radarComplete=...
Radar re-armed; ready for the next shot.
Shot #1 camera files saved.
```

含义如下：

- `triggered`：设备识别到一杆，并为它创建了唯一 GUID；
- `radar saved`：这一杆的雷达 CSV 和摘要已经写盘；
- `camera files saved`：对应相机跟踪结果已经写盘；
- `Radar re-armed; ready for the next shot.`：雷达已经可以接收下一杆；
- `Camera ready for the next shot.`：相机也已回到 `IDLE`。

雷达优先时，只要看到 `Radar re-armed` 就可以打下一杆；如果尚未看到 `Camera ready`，下一杆会自动降级为 radar-only，不会阻塞雷达。第一次测试仍建议只打一杆，确认输出完整后再做连续采集。

`radarComplete=false` 不等于文件没保存，它表示完整性检查发现缺页、点数不一致或超时；应查看该杆的 `summary.json` 和 `session.log`。

### 9.7 打开并检查输出文件

程序启动时第一行 `Output session:` 后面的路径，就是本次采集目录。另开一个 PowerShell 窗口，可直接打开输出根目录：

```powershell
explorer.exe "C:\MevoData"
```

如果输出在文档目录：

```powershell
explorer.exe "$env:USERPROFILE\Documents\MevoData"
```

进入最新时间命名的文件夹，再进入 `shot_000001`。完整的一杆通常应至少看到：

```text
summary.json
radar_ball_raw.csv
radar_club_raw.csv
camera_ball_raw.csv
camera_club_raw.csv
gvp_result.json
```

某类点没有被设备返回时，对应 CSV 可能不存在或为空；应结合 `summary.json` 的点数与 `warnings` 判断，不要只看文件名。

### 9.8 正确结束采集

1. 等最后一杆显示 `radar saved`；
2. 最好再等待 `camera files saved`；
3. 确认程序显示 `Radar re-armed`；相机文件也需要时，最好再等待 `Camera ready for the next shot.`；
4. 在正在运行程序的 PowerShell 窗口中按 `Ctrl + C`；
5. 等 PowerShell 回到 `PS C:\...>` 提示符；
6. 再关闭窗口、关闭 Mevo+ 或切换 Wi-Fi。

已经完成的杆会在事件到达时立即写盘。当前版本不保证保存“已经触发、但还没有到 ShotComplete，也没有收到 GVP RESULT”的半截杆，所以不要在击球处理过程中直接关机、断开 Wi-Fi 或强制关闭 PowerShell。

### 9.9 第二次及以后运行

只要项目代码没有更新，就不需要再次安装或编译。以后只需：

1. 打开 Mevo+；
2. 连接 `FS M2-XXXXXX` Wi-Fi；
3. 退出其他 Mevo+ 客户端，确认 5100 可连接，并检查 1258；
4. 打开 PowerShell；
5. `Set-Location "$env:USERPROFILE\source\ironsight"`；
6. 运行第 9.4 节的一行命令；
7. 等待 `ARMED` 后击球；
8. 完成后按 `Ctrl + C`。

每次启动都会创建新的时间戳 Session 目录，不会覆盖上一次的数据。

## 10. 如何检查一次输出

打开最新 Session 的 `summary.json`，优先查看：

- `shotLifecycleComplete`：是否走完本杆处理与 re-arm；
- `radarComplete`：当前收到的数据是否通过飞行结果、球点和已知点数检查；
- `activePaginationEnabled`：当前为 `true`；
- `pagination.ball/clubComplete`、请求页数、收到页数和 timeout；
- `cameraResultReceived`、`rawGvpJsonCaptured`；
- 三类 radar/camera point count；
- duplicate count 和 `warnings`。

然后分别检查：

1. `radar_ball_raw.csv` 是否有连续的 `time_tick`、合理的距离和角度；
2. `radar_club_raw.csv` 是否包含 `pre_impact` 和/或 `post_impact`；
3. `camera_ball_raw.csv`、`camera_club_raw.csv` 是否有逐帧 `u_px/v_px`；
4. `gvp_result_raw.json` 是否保留设备原始 RESULT；
5. `session.log` 是否存在断线、发送失败或未匹配 GUID。

## 11. 主动 PRC 分页如何工作

分页已集成到 `ShotSequencer`，由导出器调用：

```rust
binary.set_prc_pagination_enabled(true);
```

球 PRC：

1. 收到 `PROCESSED` 后发送两次 ShotDataAck；
2. 从 `start_index=0` 发送 `0xEC [03 00 00 08]`；
3. v4 页面最多返回 4 点，下一页按实际返回点数推进；
4. 达到 TrackingStatus 的预期点数，或收到少于 4 点的短页时结束；
5. 单页 1 秒无响应时标记 `ballTimedOut`，但不中断 re-arm。

杆头 PRC：

1. IDLE 后请求最终 ClubResult；
2. 根据 `num_club_prc_points` 判断预期点数；
3. 从 `start_index=0` 发送 77 字节 0xEE 请求，每个完整页面推进 3；
4. 达到预期点数或收到少于 3 点的短页时结束；
5. 重复 ClubResult 1 秒未返回时，仍直接探测 0xEE 第 0 页；
6. 0xEE 单页超时后记录 `clubTimedOut` 并继续 re-arm。

若固件没有发送 IDLE，但主 FlightResult 已到达，3 秒 Drain 超时后也会直接探测杆头第 0 页，再进入 re-arm。球和杆头均限制最多请求 64 页，主动重取结果会与设备此前主动推送的点合并去重。协议格式来自仓库已有抓包文档，但仍需用你的 Mevo+ 固件做真机确认；若不兼容，`summary.json` 会显示分页未完成或超时，而不是误报完整。

## 12. 验收清单

### 雷达

- 每杆至少收到一页 PRC；
- 点按时间排序；
- 重传不会生成重复行；
- 点数与 TrackingStatus 提示值进行比较；
- Club PRC 点数与 `num_club_prc_points` 比较；
- 分页完成状态、请求/响应页数和 timeout 写入 summary；
- 缺页和 timeout 写入 warnings，而不是静默标记为完整。

### 相机

- 每杆使用唯一 GUID；
- RESULT 的 GUID 能匹配对应 shot；
- ball/club track 分文件保存；
- frame、timestamp、u/v 等数组长度经过检查；
- 原始 RESULT JSON 完整保留；
- 相机无结果时仍保存雷达文件。

### 连续运行与边界

- 连续 50 杆不丢失 re-arm，需要真机验证；
- 相机忙、RESULT 超时或 GVP 断线时，雷达仍应保存并 re-arm；
- 相机未处于 `IDLE` 时，新一杆跳过 GVP Trigger，不等待相机；
- GVP 断线后自动短超时重连，重连失败不终止雷达采集；
- 当前任一文件写入失败会终止程序并显示错误，避免静默丢数据；
- Ctrl+C 不保证保存尚未进入 ShotComplete 且没有 GVP RESULT 的 partial shot；
- 输出目录不会覆盖旧 Session；
- CSV 可被 Excel 或 pandas 正常读取。

## 13. 当前最终范围

```text
Windows 10 笔记本
  ├── TCP 5100：保存球/杆头雷达原始点
  ├── TCP 1258：保存球/杆头相机像素点
  ├── GUID：关联同一杆的两个通道
  └── C:\MevoData：本地 CSV/JSON
```

已经完成的最小新增代码为：

- `examples/raw_point_export.rs`；
- ShotCollector；
- 雷达 CSV writer；
- GVP JSON/CSV writer；
- 去重和完整性检查；
- 本地文件命名和临时文件安全写入。

这套方案只保存雷达与相机的原始测量点，不进行任何目标融合。主动 PRC 分页已经实现；剩余工作是用当前 Mevo+ 固件做真机确认，以及可选的 Ctrl+C partial shot 写盘和长期稳定性增强。

## 14. Windows 10 常见问题速查

### 14.1 `cargo`、`rustc` 或 `rustup` 不是命令

原因通常是 Rust 刚安装完，旧 PowerShell 还没有读取新环境变量。

处理：关闭所有 PowerShell，重新打开，再执行：

```powershell
rustup --version
```

仍失败时临时加入路径：

```powershell
$env:Path += ";$env:USERPROFILE\.cargo\bin"
rustup --version
```

### 14.2 `could not find Cargo.toml`

原因是 PowerShell 当前不在项目根目录。执行：

```powershell
Set-Location "$env:USERPROFILE\source\ironsight"
Get-ChildItem Cargo.toml
```

如果仍找不到，在资源管理器中搜索 `Cargo.toml`，进入它所在的文件夹，不要停在 ZIP 文件或外层同名目录。

### 14.3 `link.exe not found`、找不到 Windows SDK 或链接失败

原因通常是 Visual Studio Build Tools 的 C++ 工作负荷没有安装完整。

处理：

1. 打开“Visual Studio Installer”；
2. 找到 Visual Studio 2022 Build Tools，单击“修改”；
3. 勾选“使用 C++ 的桌面开发”；
4. 确认 MSVC v143 x64/x86 和 Windows SDK 已勾选；
5. 安装完成后重启电脑；
6. 重新执行第 4.7 节的编译命令。

### 14.4 `cargo fetch` 下载失败或超时

确认电脑连接的是能上网的普通 Wi-Fi，而不是 `FS M2-XXXXXX`。浏览器能正常打开网页后，回到项目目录重新执行：

```powershell
cargo fetch
```

下载成功并完成第 4.7 节编译后，才切换到 Mevo+ Wi-Fi。

如果看到 `the lock file needs to be updated but --locked was passed`，说明执行的是旧版命令。这个仓库没有提交 `Cargo.lock`，请删除命令中的 `--locked`，不要手工创建空文件。

### 14.5 `Access is denied` 或输出目录无法创建

改用当前用户的文档目录：

```powershell
New-Item -ItemType Directory -Force "$env:USERPROFILE\Documents\MevoData"
```

运行时把输出参数改为：

```text
--output "$env:USERPROFILE\Documents\MevoData"
```

### 14.6 `Connection refused`、`timed out` 或连接立即断开

先不要反复启动程序。依次检查：

1. Windows 当前 Wi-Fi 是否为 `FS M2-XXXXXX`；
2. `ipconfig` 是否得到 `192.168.2.x`；
3. FS Golf、E6、Awesome Golf 等其他客户端是否已经退出；
4. VPN 是否已经暂停；
5. 5100 是否通过 `Test-NetConnection`，以及 1258 的测试结果是什么；
6. Mevo+ 是否已完全启动；必要时重启设备并等待约 30 秒。

### 14.7 5100 成功，但 1258 失败

启动导出器之前测得 1258 为 `False`，不一定代表最终失败：程序会先通过 5100 完成握手、Standard 相机预热和 Raw Fusion 切换，然后才连接 1258。因此只要 5100 为 `True`，可以继续运行一次。

如果程序已经显示 `Switching camera to Raw Fusion...`，随后仍在 `Connecting to GVP at 192.168.2.1:1258...` 阶段报错，程序会继续 radar-only 并后台重连。此时检查设备固件、相机状态和账号已购买功能。本项目不会绕过设备授权。不能把端口 1258 随意改成 8080；8080 是视频流，不是本项目使用的相机跟踪结果端口。

### 14.8 参数报错

常见提示及处理：

- `--mode must be indoor or outdoor`：只能填写 `indoor` 或 `outdoor`，程序不区分大小写；
- `--range-mm must be greater than zero`：距离必须是 `1–65535` 范围内的整数毫米；
- `number too large to fit in target type`：`height-mm` 超过 `255`，检查单位和测量基准；
- `missing value after ...`：某个参数后面漏了数值；
- `unknown argument`：参数拼写错误，执行 `raw_point_export.exe --help` 对照。

### 14.9 程序能运行，但没有检测到击球

确认已经看到 `ARMED - hit a ball.`，并检查：

- `indoor/outdoor` 是否选对；
- `range-mm` 和 `height-mm` 是否填写为毫米且符合实际摆位；
- 球和雷达是否按 Mevo+ 官方要求对齐；
- 设备是否被另一个 App 同时控制；
- `session.log` 中是否有断线或配置失败。

### 14.10 有雷达文件，但没有相机文件

打开该杆的 `summary.json`，查看 `cameraResultReceived` 和 `warnings`，再检查：

- 1258 端口是否连通；
- 是否打印过 `GVP connected. Waiting for camera IDLE...` 和 `Camera ready for the next shot.`；
- GVP RESULT 是否因为固件、授权、相机标定或跟踪失败而没有返回；
- `session.log` 是否记录 `GVP disconnected` 或未匹配 GUID。

即使相机结果失败，已经完成的雷达文件仍会保留。

### 14.11 出现 `GVP disconnected` 或相机长期停在 `PROCESSING`

新版导出器会立即保留雷达流程，并打印：

```text
Radar re-armed; next shot is allowed. Camera is unavailable or still busy, so the next shot may be radar-only.
```

此时可以继续击球，雷达不会等待相机。雷达处于 Arm 状态时，程序每隔 2 秒尝试恢复 GVP；重连成功后显示 `GVP reconnected`，收到 `IDLE` 后显示 `Camera ready for the next shot.`。

如果一杆触发后 10 秒仍没有 RESULT，程序会在 `summary.json` 中写入 `cameraTimedOut=true` 和 `cameraFailed=true`，重置 GVP 连接并继续雷达采集。该杆无法补出相机 CSV，但不会影响下一杆的雷达文件。

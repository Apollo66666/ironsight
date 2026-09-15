# Windows 10 下提取 Mevo+ 原始雷达与相机测量点

> 主运行环境：Windows 10 x86_64 笔记本
>
> 目标：把 Mevo+ 的球/杆头原始雷达测量点和相机逐帧跟踪像素点保存到本地。
>
> 不做：Face Impact 解算、雷达与相机坐标融合、三维重建、视频下载、数据库和云同步。

WSL2 Ubuntu 只作为备选运行方式，单独见 [`WSL_UBUNTU_ALTERNATIVE.md`](WSL_UBUNTU_ALTERNATIVE.md)。

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
- 每次 ShotComplete 或 GVP RESULT 到达时立即更新文件；当前无额外依赖的最小版尚未实现 Ctrl+C 时强制保存尚未完成的 partial shot；
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

## 4. Windows 10 环境准备

### 4.1 安装编译工具

安装 Visual Studio 2022 Build Tools，选择：

- Desktop development with C++；
- MSVC x64/x86 build tools；
- Windows 10 或 Windows 11 SDK。

安装 Rustup 后，在新 PowerShell 中执行：

```powershell
rustup toolchain install 1.94
rustup component add clippy --toolchain 1.94
rustup show
rustc --version
cargo --version
```

默认 host 应为：

```text
x86_64-pc-windows-msvc
```

### 4.2 准备项目

建议项目目录：

```text
C:\dev\ironsight
```

在仍有互联网时执行：

```powershell
Set-Location C:\dev\ironsight
cargo fetch
cargo check --features gvp --example raw_point_export
cargo clippy --features gvp --example raw_point_export -- -D warnings
cargo test --lib --features gvp
```

连接 Mevo+ Wi-Fi 后通常没有互联网，所以依赖必须提前下载。当前完整的 `cargo test --features gvp` 还会触发一个仓库既有的 `fusion_preset` 断言不一致；它不涉及导出器使用的 `raw_fusion_preset`，详见项目报告第 17 节。

## 5. 连接 Mevo+

1. 打开 Mevo+；
2. 在 Windows 中连接 `FS M2-XXXXXX` Wi-Fi；
3. 等待 Wi-Fi 获得 `192.168.2.x` 地址；
4. 关闭其他正在访问 Mevo+ 的客户端；
5. 暂停 VPN；
6. 测试两个端口。

```powershell
ipconfig
Test-NetConnection 192.168.2.1 -Port 5100
Test-NetConnection 192.168.2.1 -Port 1258
```

判断：

- 5100 成功：可以读取雷达数据；
- 1258 成功：可以连接 GVP 相机服务；
- 5100 成功、1258 失败：先完成雷达验证，再检查相机启动和设备功能；
- 两者都失败：检查 Wi-Fi、`route print`、防火墙和其他客户端。

Windows 显示“无 Internet，已连接”是正常现象。长时间采集时关闭自动睡眠和 Wi-Fi 网卡节能。

## 6. 第一步：验证雷达原始点

先构建现有低层示例：

```powershell
Set-Location C:\dev\ironsight
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

`RadarCal.range_mm` 和 `height_mm` 必须按雷达到球的真实距离和高度设置。

本步骤验收：

- 完成 Handshake 和 Arm；
- 一杆后收到 D4 或 E8；
- 至少收到一页 `PrcData`；
- 击球结束后自动重新 Arm。

## 7. 第二步：验证相机原始点

相机原始点来自 TCP 1258 的 GVP `RESULT`，不是 8080 视频流。

构建现有双通道示例：

```powershell
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

## 9. Windows 10 手把手运行

先在可联网状态完成构建：

```powershell
Set-Location C:\dev\ironsight
cargo fetch
cargo build --release --features gvp --example raw_point_export
```

连接 Mevo+ Wi-Fi，关闭其他可能占用设备的 App，然后检查端口：

```powershell
Test-NetConnection 192.168.2.1 -Port 5100
Test-NetConnection 192.168.2.1 -Port 1258
```

测量并填写实际摆位：

- `range-mm`：Mevo+ 到球的水平距离，单位毫米；
- `height-mm`：Mevo+ 雷达参考高度，单位毫米；
- 室内全挥杆用 `--mode indoor`，室外用 `--mode outdoor`。

运行：

```powershell
.\target\release\examples\raw_point_export.exe `
  --device 192.168.2.1:5100 `
  --mode indoor `
  --range-mm 2743 `
  --height-mm 25 `
  --output C:\MevoData
```

程序会打印实际 Session 目录。看到 `ARMED - hit a ball.` 后击球；看到 `radar saved` 表示雷达文件已写入，看到 `camera files saved` 表示相机结果已补齐。采集结束可按 Ctrl+C；已经完成的杆都已在事件到达时落盘。

查看参数帮助：

```powershell
.\target\release\examples\raw_point_export.exe --help
```

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

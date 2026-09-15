# Windows 10 + WSL2 Ubuntu 备选运行方案

> 本文只说明 WSL2 Ubuntu 环境差异。
>
> Windows 原生仍是主方案。数据目标和开发步骤见 [`MEVO_PLUS_TRAJECTORY_EXPORT_GUIDE.md`](MEVO_PLUS_TRAJECTORY_EXPORT_GUIDE.md)。

## 1. 使用条件

只有在 Windows 原生已经确认 Mevo+ 端口可达后，再使用 WSL2：

```powershell
Test-NetConnection 192.168.2.1 -Port 5100
Test-NetConnection 192.168.2.1 -Port 1258
```

WSL2 适合熟悉 Linux 工具链的开发方式，但多一层 NAT 网络。Windows 原生可以连接而 WSL2 不能连接时，应排查 WSL 网络，不要修改 ironsight 协议。

## 2. 安装 WSL2 Ubuntu

管理员 PowerShell：

```powershell
wsl --install -d Ubuntu
wsl --set-default-version 2
wsl --list --verbose
```

如果 Windows 10 不支持 `wsl --install`，先运行 `winver` 检查系统更新，并启用：

- Windows Subsystem for Linux；
- Virtual Machine Platform。

安装和重启完成后，打开 Ubuntu 并创建 Linux 用户。`wsl --list --verbose` 中 Ubuntu 的 VERSION 应为 `2`。

## 3. 安装 Rust 环境

在 Ubuntu 中执行：

```bash
sudo apt update
sudo apt install -y build-essential netcat-openbsd curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup toolchain install 1.94
rustup component add clippy --toolchain 1.94
rustc -vV
cargo --version
```

Rust host 应为：

```text
x86_64-unknown-linux-gnu
```

## 4. 项目目录

建议把源码和 `target/` 放在 WSL 文件系统：

```bash
mkdir -p ~/projects
cd ~/projects
git clone <仓库地址> ironsight
cd ironsight
```

Windows 资源管理器可访问：

```text
\\wsl$\Ubuntu\home\<Linux用户名>\projects
```

不建议长期在 `/mnt/c/...` 中编译 Rust 项目，因为大量小文件访问通常更慢。

在有互联网时完成：

```bash
cargo fetch
cargo test
cargo test --features gvp
cargo clippy --all-targets --features gvp
```

## 5. 连接 Mevo+

Wi-Fi 由 Windows 管理。先在 Windows 网络菜单连接 `FS M2-XXXXXX`，再在 Ubuntu 中测试：

```bash
ip route
ping -c 2 192.168.2.1
nc -vz -w 3 192.168.2.1 5100
nc -vz -w 3 192.168.2.1 1258
```

ping 失败但 `nc` 成功时可以继续，因为设备可能不响应 ICMP。

Windows 10 的 WSL2 默认使用 NAT。通常可以向局域网设备发起连接，但应以这台电脑上的实际端口测试为准。

如果 PowerShell 测试成功而 WSL 测试失败：

1. 在 PowerShell 执行 `wsl --shutdown`；
2. 重新打开 Ubuntu；
3. 暂停 VPN、代理和虚拟网卡软件；
4. 检查 Windows Defender Firewall；
5. 检查 Ubuntu 的 `ip route`；
6. 仍失败时使用 Windows 原生方案。

## 6. 运行现有验证程序

雷达：

```bash
cargo build --release --example event_loop
./target/release/examples/event_loop
```

雷达和相机：

```bash
cargo build --release --features gvp --example gvp_testing
./target/release/examples/gvp_testing
```

运行已经实现的 `raw_point_export.rs`：

```bash
cargo build --release --features gvp --example raw_point_export
./target/release/examples/raw_point_export \
  --device 192.168.2.1:5100 \
  --mode indoor \
  --range-mm 2743 \
  --height-mm 25 \
  --output /mnt/c/MevoData
```

参数含义、输出文件、主动 PRC 分页状态和真机验收方法见主指南。

## 7. 输出文件位置

直接输出到 Windows：

```text
/mnt/c/MevoData
```

或者保存在 WSL：

```text
/home/<Linux用户名>/MevoData
```

Windows 资源管理器访问：

```text
\\wsl$\Ubuntu\home\<Linux用户名>\MevoData
```

每杆数据量较小，直接写 `/mnt/c/MevoData` 通常足够。业务代码、ShotCollector 和文件格式应与 Windows 原生方案保持一致，仅默认路径不同。

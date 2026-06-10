# SFTP Download

一个使用 Rust 实现的 SSH/SFTP 文件下载工具，支持下载远程文件和递归下载目录。

## 功能特性

- ✅ 自动检测文件/目录类型
- ✅ 下载单个文件
- ✅ 递归下载目录（包含子目录）
- ✅ 支持密码认证
- ✅ 支持私钥认证
- ✅ 断点续传
- ✅ 并发下载
- ✅ 进度条显示

## 安装

```bash
# 编译发布版本
cargo build --release
```

编译后的二进制文件位于 `target/release/sftp-download.exe`

## 开发运行

开发阶段使用 `cargo run` 运行程序时，参数需要放在 `--` 后面：

```bash
# 显示帮助
cargo run -- --help

# 下载文件或目录（自动检测）
cargo run -- -H 192.168.1.100 -u admin -P password --remote /path/to/file --local ./file

# 下载目录并指定并发数
cargo run -- -H server.com -u user --key ~/.ssh/id_rsa --remote /data --local ./backup --parallel 8
```

> **说明**：
> - `--` 分隔符告诉 cargo 后面的参数是传给程序的
> - 在 Git Bash 中，远程路径需要加 `MSYS_NO_PATHCONV=1` 环境变量防止路径转换

## 使用方法

### 基本用法

```bash
# 下载文件或目录（自动检测类型）
sftp-download -H 192.168.1.100 -u admin -P password --remote /path/to/remote --local ./local

# 使用密钥认证
sftp-download -H server.com -u user --key ~/.ssh/id_rsa --remote /data --local ./backup

# 使用非标准端口
sftp-download -H server.com -p 2222 -u user -P password --remote /file --local ./file
```

### 高级选项

```bash
# 断点续传（仅文件）
sftp-download -H server.com -u user -P password --remote /large/file.iso --local ./file.iso --resume

# 覆盖已存在的文件
sftp-download -H server.com -u user -P password --remote /data --local ./data --force

# 指定并发下载数（仅目录）
sftp-download -H server.com -u user -P password --remote /project --local ./project --parallel 8
```

### 命令行参数

| 参数 | 简写 | 说明 | 默认值 |
|------|------|------|--------|
| `--host` | `-H` | SSH 服务器地址 | localhost |
| `--port` | `-p` | SSH 服务器端口 | 22 |
| `--user` | `-u` | SSH 用户名 | (必需) |
| `--password` | `-P` | 密码认证 | - |
| `--key` | `-k` | 私钥文件路径 | - |
| `--key-passphrase` | - | 私钥密码 | - |
| `--timeout` | - | 连接超时(秒) | 30 |
| `--remote` | `-r` | 远程文件/目录路径 | (必需) |
| `--local` | `-l` | 本地保存路径 | (必需) |
| `--force` | `-f` | 覆盖已存在文件 | false |
| `--resume` | - | 断点续传（仅文件） | false |
| `--parallel` | - | 并发下载数（仅目录） | 4 |

## 示例输出

```
Total: 8 files, 24.31 MiB (parallel: 4)
============================================================

Total: [===============>           ] 12.5 MiB/24.31 MiB (3.2 MiB/s) [3/8 files]
[1/8] file1.log - Done (10.00 MiB)
[2/8] file2.log - Done (5.00 MiB)
[3/8] file3.log - Done (2.50 MiB)

============================================================
Completed: 8/8 files, 24.31 MiB transferred
Speed: 3.2 MiB/s, Time: 7.60s
```

## 技术栈

- **russh** - 纯 Rust SSH 实现
- **russh-sftp** - SFTP 协议实现
- **tokio** - 异步运行时
- **clap** - 命令行解析
- **indicatif** - 进度条显示

## 许可证

MIT

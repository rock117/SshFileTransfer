# SFTP Download

一个使用 Rust 实现的 SSH/SFTP 文件下载工具，支持下载远程文件和递归下载目录。

## 功能特性

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

# 下载单个文件（密码认证）
cargo run -- -H 192.168.1.100 -u admin -P password download-file --remote /path/to/file.txt --local ./file.txt

# 下载目录（密钥认证）
cargo run -- -H server.com -u user --key ~/.ssh/id_rsa download-dir --remote /data --local ./backup

# 断点续传
cargo run -- -H server.com -u user -P password download-file --remote /large/file.iso --local ./file.iso --resume
```

> **说明**：`--` 分隔符告诉 cargo 后面的参数是传给程序的，而不是给 cargo 自己的。

## 使用方法

### 下载单个文件

```bash
# 使用密码认证
sftp-download -H 192.168.1.100 -u admin -P password download-file \
    --remote /path/to/remote/file.txt \
    --local ./local/file.txt

# 使用私钥认证
sftp-download -H server.com -u user --key ~/.ssh/id_rsa download-file \
    --remote /data/backup.tar.gz \
    --local ./backup.tar.gz

# 断点续传
sftp-download -H server.com -u user -P password download-file \
    --remote /large/file.iso \
    --local ./file.iso \
    --resume

# 覆盖已存在的文件
sftp-download -H server.com -u user -P password download-file \
    --remote /file.txt \
    --local ./file.txt \
    --force
```

### 下载目录

```bash
# 递归下载目录
sftp-download -H 192.168.1.100 -u admin -P password download-dir \
    --remote /var/www/html \
    --local ./website_backup

# 指定并发数
sftp-download -H server.com -u user --key ~/.ssh/id_rsa download-dir \
    --remote /project \
    --local ./project \
    --parallel 8

# 覆盖已存在的文件
sftp-download -H server.com -u user -P password download-dir \
    --remote /data \
    --local ./data \
    --force
```

### 命令行参数

#### 全局参数

| 参数 | 简写 | 说明 | 默认值 |
|------|------|------|--------|
| `--host` | `-H` | SSH 服务器地址 | localhost |
| `--port` | `-p` | SSH 服务器端口 | 22 |
| `--user` | `-u` | SSH 用户名 | (必需) |
| `--password` | `-P` | 密码认证 | - |
| `--key` | `-k` | 私钥文件路径 | - |
| `--key-passphrase` | - | 私钥密码 | - |
| `--timeout` | - | 连接超时(秒) | 30 |

#### download-file 子命令

| 参数 | 简写 | 说明 |
|------|------|------|
| `--remote` | `-r` | 远程文件路径 |
| `--local` | `-l` | 本地保存路径 |
| `--force` | `-f` | 覆盖已存在文件 |
| `--resume` | `-r` | 断点续传 |

#### download-dir 子命令

| 参数 | 简写 | 说明 | 默认值 |
|------|------|------|--------|
| `--remote` | `-r` | 远程目录路径 | - |
| `--local` | `-l` | 本地保存目录 | - |
| `--force` | `-f` | 覆盖已存在文件 | false |
| `--parallel` | `-p` | 并发下载数 | 4 |

## 技术栈

- **russh** - 纯 Rust SSH 实现
- **russh-sftp** - SFTP 协议实现
- **tokio** - 异步运行时
- **clap** - 命令行解析
- **indicatif** - 进度条显示

## 许可证

MIT

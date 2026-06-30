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
- ✅ 配置文件加载（TOML/JSON/YAML）
- ✅ glob 通配符过滤文件（include/exclude）
- ✅ 按最近修改时间过滤文件（since/until/latest）
- ✅ 启动时打印解析后的命令行（密码自动脱敏）

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

# 使用配置文件
cargo run -- --config config.toml
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

### 配置文件

支持从配置文件加载参数，格式通过扩展名自动识别（`.toml` / `.json` / `.yaml` / `.yml`）。指定了 `--config` 但文件不存在时打印警告并忽略；不传 `--config` 则纯命令行模式运行。纯命令行模式依然可用。

优先级：**命令行参数 > 环境变量 > 配置文件 > 内置默认值**。

```bash
# 显式指定配置文件
sftp-download --config config.toml

# 不指定配置文件时，所有参数都从命令行传入
sftp-download -H server.com -u admin -P password --remote /data --local ./data
```

示例 `config.toml`（项目根目录有 `config.example.toml` 模板）：

```toml
host = "server.com"
port = 22
user = "admin"
password = "password"
remote = "/data"
local = "./data"
parallel = 8
exclude = ["*.tmp", "*~"]
include = ["*.log"]
ignore_case = false
since = "2026-06-01"
latest = 10
```

### 文件过滤

支持使用 glob 通配符对文件名（basename）进行过滤。语法：

- `*` 匹配任意字符序列（包括空）
- `?` 匹配单个字符
- `[abc]` / `[a-z]` / `[!abc]` 字符集

```bash
# 只下载 .log 和 .txt 文件
sftp-download -H server.com -u admin -P password --remote /data --local ./data \
  --include "*.log" --include "*.txt"

# 下载所有 .log 文件，但排除以 debug 开头的
sftp-download -H server.com -u admin -P password --remote /data --local ./data \
  --include "*.log" --exclude "debug*"

# 排除临时文件和备份文件
sftp-download -H server.com -u admin -P password --remote /data --local ./data \
  --exclude "*.tmp" --exclude "*~" --exclude "*.bak"

# 大小写不敏感匹配
sftp-download -H server.com -u admin -P password --remote /data --local ./data \
  --include "*.LOG" --ignore-case
```

**过滤规则**：
- `include` 为空：不启用白名单，所有文件都进入下一步
- `include` 非空：只保留匹配任一模式的文件
- `exclude` 为空：不剔除任何文件
- `exclude` 非空：从 `include` 过滤后的结果中剔除匹配的文件

### 时间过滤

按远程文件的修改时间（mtime）过滤：

```bash
# 只下载 2026-06-01 之后修改的文件
sftp-download -H server.com -u admin -P password --remote /data --local ./data --since "2026-06-01"

# 只下载 2026-06-30 之前修改的文件
sftp-download -H server.com -u admin -P password --remote /data --local ./data --until "2026-06-30"

# 下载最近 7 天内修改的最新 10 个文件
sftp-download -H server.com -u admin -P password --remote /data --local ./data \
  --since "2026-06-23" --latest 10
```

日期格式：
- `YYYY-MM-DD`：`--since` 取当天 00:00:00，`--until` 取当天 23:59:59（含边界当天）
- `YYYY-MM-DD HH:MM:SS`：精确到秒

### 高级选项

```bash
# 断点续传（仅文件）
sftp-download -H server.com -u user -P password --remote /large/file.iso --local ./file.iso --resume

# 跳过已存在的文件（默认：覆盖）
sftp-download -H server.com -u user -P password --remote /data --local ./data --skip

# 指定并发下载数（仅目录）
sftp-download -H server.com -u user -P password --remote /project --local ./project --parallel 8

# 组合：配置文件 + 命令行覆盖
sftp-download --config config.toml --host real.server.com --latest 5
```

### 命令行参数

| 参数 | 简写 | 说明 | 默认值 |
|------|------|------|--------|
| `--config` | - | 配置文件路径（.toml/.json/.yaml/.yml） | - |
| `--host` | `-H` | SSH 服务器地址 | localhost |
| `--port` | `-p` | SSH 服务器端口 | 22 |
| `--user` | `-u` | SSH 用户名 | (必需) |
| `--password` | `-P` | 密码认证 | - |
| `--key` | `-k` | 私钥文件路径 | - |
| `--key-passphrase` | - | 私钥密码 | - |
| `--timeout` | - | 连接超时(秒) | 30 |
| `--remote` | `-r` | 远程文件/目录路径 | (必需) |
| `--local` | `-l` | 本地保存路径 | (必需) |
| `--skip` | `-s` | 跳过已存在文件 | false（默认覆盖） |
| `--resume` | - | 断点续传（仅文件） | false |
| `--parallel` | `-j` | 并发下载数（仅目录） | 4 |
| `--exclude` | `-x` | 排除匹配 glob 的文件（可重复） | - |
| `--include` | `-i` | 只下载匹配 glob 的文件（可重复） | - |
| `--ignore-case` | - | include/exclude 大小写不敏感 | false |
| `--since` | - | 只下载该日期后修改的文件 | - |
| `--until` | - | 只下载该日期前修改的文件 | - |
| `--latest` | - | 只下载最近修改的 N 个文件 | - |

## 示例输出

启动时打印解析后的命令行（密码/密钥密码自动脱敏为星号）：

```
> sftp-download -H server.com -p 22 -u admin -P "********" --timeout 30 -r /data -l ./data -j 4 -x "*.tmp" -i "*.log"
```

下载目录：
```
8 files, 24.31 MiB, parallel: 4
------------------------------------------------------------
[===============>                       ] 12.5 MiB/24.31 MiB (3.2 MiB/s) [3/8 files]
(1/8) 20260610_Warning.log      9.96 MiB  100%  372 KiB/s
(2/8) 20260610_Error.log          967 KiB  100%  372 KiB/s
(3/8) 20260610_Info.log          1.88 MiB  100%  355 KiB/s
...
------------------------------------------------------------
Downloaded 8/8 files, 24.31 MiB in 7.60s (3.2 MiB/s)
```

下载单个文件：
```
(1/1) backup.tar.gz             256.00 MiB  100%  5.2 MiB/s

Downloaded 256.00 MiB in 49.23s (5.2 MiB/s)
```

## 技术栈

- **russh** - 纯 Rust SSH 实现
- **russh-sftp** - SFTP 协议实现
- **tokio** - 异步运行时
- **clap** - 命令行解析
- **indicatif** - 进度条显示
- **chrono** - 日期时间解析
- **serde/toml/serde_json/serde_yaml** - 配置文件解析

## 许可证

MIT

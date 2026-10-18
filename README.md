# LLibrary

私人书库目录与发现服务。P1 提供只读索引、作品聚合、元数据/封面、搜索与 OPDS 1.2；不移动、不改名、不删除任何书籍文件。

## 当前范围（P1）

- 多来源扫描：本地路径、WebDAV
- 新增来源立即扫描一次；手动扫描；可选 cron 定时（默认关闭）
- 增量扫描：path + size + mtime + hash；缺失文件只标记
- 作品聚合：同一本书的 epub/azw3/pdf/mobi 归为一个作品
- 元数据：EPUB OPF、PDF 基础信息、文件名/路径回退
- 封面：EPUB 提取、PDF 按需生成、占位图
- SQLite + FTS5 搜索
- Web/PWA 浏览、搜索、详情
- OPDS 1.2 导航与 acquisition feed
- HTTP Range 文件流，支持大 PDF
- 单用户鉴权（Web 会话 + OPDS Basic）

不在 P1：中图法分类（P2）、网页阅读器（P3）、Send-to-Kindle（P4）、监控与备份加固（P5）。

## 环境变量

| 变量 | 必填 | 默认值 | 说明 |
|---|---|---|---|
| `AUTH_USER` | 是 | - | 登录用户名 |
| `AUTH_PASSWORD_HASH` | 是 | - | Argon2 PHC 哈希 |
| `DATA_DIR` | 否 | `./data` | 数据目录（SQLite/封面/日志） |
| `DB_PATH` | 否 | `$DATA_DIR/library.db` | SQLite 路径 |
| `BIND_ADDR` | 否 | `127.0.0.1:8080` | 监听地址 |
| `LLIBRARY_SECRET` | 否 | - | WebDAV 凭据加密密钥；添加 WebDAV 凭据时必填 |
| `SCAN_CRON` | 否 | 空（关闭） | cron 表达式，例如 `0 3 * * *` |

生成密码哈希：

```bash
cargo run -- hash-password 'change-me'
```

## 本地运行

```bash
cargo build
DATA_DIR=./data \
AUTH_USER=reader \
AUTH_PASSWORD_HASH='<上一步输出的哈希>' \
BIND_ADDR=127.0.0.1:8080 \
cargo run
```

打开 `http://127.0.0.1:8080`，用用户名/密码登录；OPDS 地址为 `http://127.0.0.1:8080/opds`。

## 管理来源

```bash
# 添加本地来源；默认添加后立即扫描
DATA_DIR=./data AUTH_USER=reader AUTH_PASSWORD_HASH='<hash>' \
cargo run -- sources add --name 主书库 --kind local --location /path/to/books

# 只添加、不立即扫描
... cargo run -- sources add --name 旧书 --kind local --location /path/to/books --no-scan

# 添加 WebDAV 来源（需要 LLIBRARY_SECRET）
DATA_DIR=./data AUTH_USER=reader AUTH_PASSWORD_HASH='<hash>' LLIBRARY_SECRET='...' \
cargo run -- sources add --name 远程 --kind webdav \
  --location http://<nas>:5005/public/%E4%B9%A6/ \
  --username reader --password '...'

# 查看来源
... cargo run -- sources list

# 手动扫描全部 / 单个来源
... cargo run -- scan
... cargo run -- scan --source 1
```

定时扫描默认关闭。`SCAN_CRON` 支持标准 5 段 cron（秒默认为 0），例如 `0 3 * * *`。确认首次全量扫描成本后再启用。

首轮扫描会读取每个 EPUB/PDF 解析元数据并计算 BLAKE3 指纹，104 GB 书库可能需要较长时间；建议在 NAS 空闲时手动触发，观察 `/scans` 的耗时后再决定是否开启定时扫描。

## 测试

```bash
cargo test
```

包含：配置、数据库/FTS、本地与 WebDAV 来源、增量扫描、元数据/文件名规范化、封面、鉴权、Web/OPDS/Range、CLI 与端到端测试。

## Docker

构建：

```bash
docker build -t llibrary .
docker run --rm -p 8080:8080 \
  -e AUTH_USER=reader \
  -e AUTH_PASSWORD_HASH='<hash>' \
  -e DATA_DIR=/data \
  -v /path/to/books:/sources/books:ro \
  -v /path/to/llibrary-data:/data \
  llibrary
```

Compose：

```bash
cp .env.example .env    # 填入 AUTH_USER / AUTH_PASSWORD_HASH
docker compose up -d
```

镜像默认监听 `0.0.0.0:8080`，以非 root 用户 `10001` 运行。挂载的 `/data` 需要对 uid 10001 可写；部署步骤见 [docs/deployment.md](docs/deployment.md)。

## 目录结构

```text
src/
  auth.rs       会话/Basic 鉴权
  config.rs     环境配置与数据目录
  covers.rs     封面提取/缩放/占位图/PDF 封面
  crypto.rs     WebDAV 凭据加密
  db.rs         SQLite 迁移与查询
  metadata.rs   EPUB/PDF/文件名解析与规范化
  scanner.rs    扫描队列、增量扫描、cron
  sources/      本地与 WebDAV 来源
  web.rs         Axum 路由、HTML、OPDS、Range
  main.rs       CLI / 服务启动
templates/      Askama 模板
static/         CSS、PWA manifest、service worker、htmx
```

## 数据与备份

- 数据库、封面、日志均在 `DATA_DIR`；不复制书籍文件。
- 备份 `DATA_DIR` 即可保留目录、封面与人工修正。
- 数据库丢失时可重新扫描恢复；人工分类修正（P2）需要靠备份保留。

## Roadmap

- P2：中图法一级+二级虚拟分类、LLM 批量分类、复核队列
- P3：Web/PWA 阅读器与进度同步
- P3/P4：Send-to-Kindle
- P5：监控、备份与运维加固

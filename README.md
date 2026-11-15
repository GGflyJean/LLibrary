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
- 第一版阅读器：EPUB/PDF/TXT/AZW3/MOBI 浮层阅读与新标签页；AZW3/MOBI 无配对 EPUB 时也可在线预览
- 中图法一级+二级虚拟分类与首页分类折叠树
- OPDS 1.2 导航与 acquisition feed
- HTTP Range 文件流，支持大 PDF
- 单用户鉴权（Web 会话 + OPDS Basic）

不在 P1：中图法分类（P2）、网页阅读器（P3）、Send-to-Kindle（P4）、监控与备份加固（P5）。

## 分类浏览

- 首页默认按中图法一级/二级分类折叠；分类只显示数量，展开二级后才通过 HTMX 按需加载该分类下的作品，避免首屏渲染全部作品。
- 首版分类器使用标题/作者/出版社/简介中的关键词评分，不依赖外部 API；无法识别的作品进入“未分类”。
- 搜索时仍返回平铺结果；出版社、格式、来源筛选会作用于分类树。
- 分类只写入数据库，不移动、不改名、不删除任何书库文件。
- 作品详情页提供“调整分类”表单；手工分类带“手工”标记，不会被自动分类或“重新分类全部作品”覆盖。
- 规则分类是首版方案，后续可接入 LLM 批量分类和人工修正。
- 修改分类规则后，可在 `/sources` 点击“重新分类全部作品”，或执行 `llibrary reclassify` 重建分类号。

## 预览视图

首页筛选栏提供“详情 / 简洁”切换：

- 详情：按中图法分类折叠，展开二级后显示封面、书名、作者、出版社、格式
- 简洁：保留同一个中图法分类树，分类默认折叠，展开二级后每本书只显示一行书名、作者、出版社
- 切换通过 URL 的 `view=compact` / `view=detail` 保持，当前搜索框内容和出版社/格式/来源筛选都会带入
- 搜索结果和分类树都会显示匹配总数；搜索时结果仍为平铺列表

## 阅读器（第一版）

详情页对 EPUB/PDF/TXT/AZW3/AZW/MOBI 文件提供：

- **浮层阅读**：覆盖整个视口的浮层，左上角关闭按钮；关闭后回到详情
- **新标签页**：直接打开 `/read/{file_id}`，适合长时间阅读
- **底部操作栏**：翻页、跳转、缩放和目录入口固定在底部，方便手机单手握持
- EPUB 使用纵向滚动模式，避免分栏导致的封面半页、短页面空白；上一页/下一页、目录和按页跳转仍可用
- 支持左右滑动翻页（EPUB/PDF/AZW3/AZW/MOBI）

实现方式：

- EPUB：`epub.js` + `JSZip`（已放入 `static/epubjs/`），同源读取 `/files/{id}`；对格式不严格的 XHTML 会回退为 HTML 解析，避免部分章节空白
  - 目录：读取 EPUB 自带目录，点击章节跳转
  - 跳转：输入页码跳转，底部显示总页数
- 阅读进度：EPUB/PDF/TXT/AZW3 都会自动保存到服务端 SQLite 的 `reading_progress` 表（按作品记录），随 `DATA_DIR`/数据卷持久化，重建镜像不会丢失；`localStorage` 仅作为即时缓存
- PDF：内置 PDF.js Canvas 阅读器，不依赖浏览器原生 PDF 查看器
  - 页码：页码输入、上一页/下一页、目录抽屉、缩放/适宽、左右滑动翻页
  - 移动端同样使用 Canvas 渲染，避免 iframe PDF 白屏
- TXT：服务端自动识别 UTF-8/GB18030 并转为 UTF-8；阅读器按视口分页，支持上一页/下一页、页码跳转和字号缩放，不提供编辑
- EPUB 缩放：`A− / A+` 调整字号（60%-220%），按设备保存在 localStorage
- AZW3/AZW/MOBI：如果同作品有 EPUB，会自动跳到该 EPUB 阅读页；没有 EPUB 时用内置 Kindle 阅读器（`foliate-js`，已放入 `static/foliate/`）在浏览器内解析 MOBI6/KF8，支持目录、上一页/下一页、页码跳转、左右滑动翻页、字号缩放和服务端进度
  - Kindle 阅读器通过 HTTP Range 按块读取文件，打开时只解析并渲染首个章节，后续跳转/翻页再按需加载；KF8 的 RESC/PAGE 资源扫描对可重排书籍延后/跳过，避免打开时下载整本
  - EPUB/PDF 本身按章节/页面加载；TXT 加载时显示状态。下载按钮使用 `Content-Disposition: attachment`，不会在浏览器内打开文件
- 不处理 DRM；极少数 epub.js 不兼容的 EPUB 仍可下载到本地阅读器

## 元数据

- EPUB：读取 OPF 的标题、作者、出版社、ISBN、简介、语言、日期；OPF 声明 GBK/GB18030/Big5 或文件名是 Latin-1 乱码时会自动转码修复。
- AZW3/AZW/MOBI：读取 EXTH 元数据（书名、作者、出版社、ISBN、简介、日期），不再只依赖文件名和目录。
- PDF：读取 Info 中的标题、作者、出版社；标题/作者缺失时才用文件名兜底。
- 出版社目录兜底只在目录名看起来像出版社（包含“出版社/书局/印书馆/大学/人民/中信”等）时使用，避免把批次目录当成出版社。
- 修改提取规则后，可执行 `llibrary reparse`（可加 `--source <id>`）重新解析已入库文件，不重新计算哈希；手工分类不受影响。

## 环境变量

| 变量 | 必填 | 默认值 | 说明 |
|---|---|---|---|
| `AUTH_USER` | 否 | `reader` | 登录用户名（当前 Web/OPDS 不启用登录，保留备用） |
| `AUTH_PASSWORD_HASH` | 否 | 空 | Argon2 PHC 哈希（当前不启用登录，保留备用） |
| `DATA_DIR` | 否 | `./data` | 数据目录（SQLite/封面/日志） |
| `DB_PATH` | 否 | `$DATA_DIR/library.db` | SQLite 路径 |
| `BIND_ADDR` | 否 | `127.0.0.1:8080` | 监听地址 |
| `LLIBRARY_SECRET` | 否 | - | WebDAV 凭据加密密钥；添加 WebDAV 凭据时必填 |
| `SCAN_CRON` | 否 | 空（关闭） | cron 表达式，例如 `0 3 * * *` |

生成密码哈希：

```bash
cargo run -- hash-password 'change-me'
```

哈希里包含 `$`。直接写进 Compose YAML 时，必须把每个 `$` 写成 `$$`，否则会被 Compose 当成变量插值；也可以直接生成转义后的值：

```bash
cargo run -- hash-password 'change-me' | sed 's/\$/$$/g'
```

## 本地运行

```bash
cargo build
DATA_DIR=./data \
BIND_ADDR=127.0.0.1:8080 \
cargo run
```

打开 `http://127.0.0.1:8080` 即可直接访问，无需登录；OPDS 地址为 `http://127.0.0.1:8080/opds`。

## 管理来源

推荐直接在 Web 页面 `/sources` 管理来源：添加本地/WebDAV 来源、删除来源、对单个来源触发扫描，不需要容器终端。

- 本地路径填**容器内路径**，例如 `/sources/public/书`；新增宿主目录仍需先在 Compose 中增加挂载。
- WebDAV 来源需要用户名/密码，并要求配置 `LLIBRARY_SECRET`。
- 删除来源只清理索引，不会删除书库文件。

CLI 仍然可用，适合脚本或首次批量操作：

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

# 重新解析已有文件的元数据（不重新计算哈希）
... cargo run -- reparse
... cargo run -- reparse --source 1

# 全量重建索引（重算指纹、重建分类和 FTS）
... cargo run -- rebuild
... cargo run -- rebuild --source 1
```

定时扫描默认关闭。`SCAN_CRON` 支持标准 5 段 cron（秒默认为 0），例如 `0 3 * * *`。确认首次全量扫描成本后再启用。

Web 的 `/sources` 和 `/scans` 提供两种操作：

- **增量扫描**：只处理新增或变化的文件，和定时扫描一致。
- **索引重构**：重新读取全部文件、重算内容指纹，并重建分类和搜索索引，适合元数据/分类规则调整后使用，耗时较长。

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

## 内存与资源

- 服务端文件接口是流式传输；PDF 封面在本地来源下直接用文件路径调用 `pdftoppm`，不再把整本 PDF 读进服务进程内存。
- 首次全量扫描会逐个读取文件计算指纹和元数据，大 PDF 可能造成短时内存峰值；扫描结束后 Rust/glibc 不一定立即把内存还给操作系统。
- Docker 默认不限制容器内存。可在 Compose 增加 `mem_limit: 3g`（按 `docker stats` 实测调整），防止极端文件占满 16G 主机内存。
- 浏览器阅读器：PDF.js/EPUB.js 会占用内存；关闭浮层会主动销毁 PDF 文档/worker、EPUB rendition 和 Canvas。但浏览器/OS 的内存高水位不一定马上回落，彻底释放请关闭标签页或刷新页面。
- WebDAV 来源的 PDF 封面仍需要临时读取文件；本地挂载来源没有这个问题。

## 数据与备份

- 数据库、封面、日志均在 `DATA_DIR`；不复制书籍文件。
- 备份 `DATA_DIR` 即可保留目录、封面与人工修正。
- 数据库丢失时可重新扫描恢复；人工分类修正（P2）需要靠备份保留。

## Roadmap

- P2 后续：LLM 批量分类与人工复核（首版规则分类已可用）
- P3 后续：阅读器主题/划线注释
- P4：Send-to-Kindle
- P5：监控、备份与运维加固

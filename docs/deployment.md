# 极空间部署运行手册

## 1. 准备数据目录

在极空间上创建：

```text
/public/LLibrary-data
```

容器以 uid `10001` 运行，需要该目录可写。若极空间终端可用：

```bash
mkdir -p /public/LLibrary-data
chown -R 10001:10001 /public/LLibrary-data
```

如果无法修改属主，可在 Compose 中临时把 `user` 设为 `"0:0"`，完成初始化后再改回。

## 2. 准备环境变量

在 `LLibrary` 目录生成密码哈希：

```bash
cargo run -- hash-password '你的密码'
```

创建 `.env`：

```text
AUTH_USER=reader
AUTH_PASSWORD_HASH=<上一步输出>
LLIBRARY_SECRET=<随机长字符串，可选>
SCAN_CRON=
```

`LLIBRARY_SECRET` 只在添加 WebDAV 来源时需要；生成后不要丢失，否则已保存的 WebDAV 凭据无法解密。

> Argon2 哈希包含 `$`。直接粘贴到 Compose YAML 时，要把每个 `$` 写成 `$$`，否则 Compose 会把它当变量插值。可以这样生成转义后的值：
>
> ```bash
> cargo run -- hash-password '你的密码' | sed 's/\$/$$/g'
> ```

## 3. 部署

**不需要把源码放到极空间。** 镜像里已经包含编译好的二进制、模板和静态文件；NAS 上只需要：

1. `docker-compose.yml`
2. `.env`
3. 一个可用的镜像

镜像来源有四种，**前两种都不需要本地 Docker**：

方式 A：GHCR 直接拉取（推荐）

当前 GitHub Actions 已构建成功，`ghcr.io/ggflyjean/llibrary:latest` 可以匿名拉取，不需要 `docker login`：

```bash
docker compose pull
docker compose up -d
```

如果以后把 package 改成私有，再执行 `docker login ghcr.io -u <GitHub 用户名> --password <PAT>`，或改用方式 B。

方式 B：从 GitHub Actions 下载镜像 tar（无本地 Docker；GHCR 不可达时）

1. 打开仓库的 Actions → `docker` → 最近一次成功运行
2. 页面底部 Artifacts → 下载 `llibrary-image`（约 42 MB）
3. 解压得到 `llibrary-image.tar.gz`，再执行 `gunzip llibrary-image.tar.gz`
4. 在极空间 Docker 界面导入 `llibrary-image.tar`（镜像名是 `llibrary:local`）
5. 把 `docker-compose.yml` 的 `image` 改为 `llibrary:local` 后启动

这条路径不需要你在 Mac 上安装 Docker，也不需要极空间能访问 GHCR。

方式 C：在本机构建（需要 Docker/OrbStack）

```bash
docker build -t llibrary:local .
docker save llibrary:local -o llibrary-local.tar
```

把 `llibrary-local.tar` 上传到极空间，在 Docker 界面导入镜像，然后把 `docker-compose.yml` 的 `image` 改为 `llibrary:local` 后启动。

方式 D：把源码拉到极空间构建（不推荐）

需要极空间能运行 Docker build 或 SSH 构建；Rust release 编译较吃 CPU/内存，且极空间 Docker 界面未必支持 build。除非前三种方式都不可用，否则不建议。

## 4. 首次配置

1. 打开 `http://<nas>:8080`（或你映射的宿主端口），用 `AUTH_USER` / 密码登录。
2. 打开 `/sources`，在“添加来源”里填：
   - 名称：`主书库`
   - 类型：本地路径
   - 位置：`/sources/public/书`
   - 勾选“添加后立即扫描”
   然后提交。页面会显示来源和最近扫描状态，不需要容器终端。
3. 扫描页 `/scans` 查看进度与报告；`/api/scans/current` 返回 JSON 进度。

也可以继续用 CLI（脚本/自动化）：

```bash
docker exec -it llibrary llibrary sources add \
  --name 主书库 --kind local --location /sources/public/书
```

## 5. 结果核对

首轮扫描预期规模（基于 2026-10 的书库）：

```text
文件：约 9010
体积：约 104 GB
作品：约 5089
格式：epub 3765 / azw3 3765 / pdf 1455 / mobi 24 / azw 1
```

在 `/` 搜索、打开详情、用 OPDS 客户端访问 `http://<nas>:8080/opds` 验证。

## 6. 备份

需要备份的只有 `DATA_DIR`（默认 `/public/LLibrary-data`）：

```bash
tar -czf /public/LLibrary-data-$(date +%F).tar.gz -C /public LLibrary-data
```

书籍文件不需要备份到该目录；服务从不修改它们。

## 7. 回滚

```bash
docker compose down
rm -rf /public/LLibrary-data
```

书库文件不受影响。重新部署后重新添加来源即可重建索引。

## 8. 升级

```bash
docker compose pull
docker compose up -d
```

数据库迁移在启动时自动执行。升级前建议备份 `DATA_DIR`。

## 9. 定时扫描

默认关闭。观察到首次全量扫描耗时后，可在 `.env` 设置：

```text
SCAN_CRON=0 3 * * *
```

然后 `docker compose up -d`。扫描任务串行执行，不会并发读写 NAS。

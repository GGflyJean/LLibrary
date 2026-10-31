# 公网访问方案

当前服务默认只监听局域网（例如 `http://192.168.0.108:18084`）。极空间的公网直连域名主要暴露 WebDAV 和 Web 管理端口，**不能假设 18084 可以直接从外网访问**。下面按推荐顺序列出方案。

## 方案 A：Tailscale（推荐，私有）

NAS 和你的手机/电脑加入同一个 tailnet，之后通过 Tailscale 分配的 IP 访问，不需要路由器端口映射，也不把服务暴露给公网。

1. 在 NAS 上安装 Tailscale：
   - 如果极空间应用中心有 Tailscale，直接安装；
   - 或用 Docker 运行 `tailscale/tailscale`，需要 `network_mode: host`、`/dev/net/tun` 和 `NET_ADMIN` 权限。
2. 让 NAS 加入 tailnet，记下它的 Tailscale IP。
3. 手机/电脑安装 Tailscale 并登录同一账号。
4. 访问：

```text
http://<NAS 的 Tailscale IP>:18084
```

OPDS 地址同样使用这个 URL。优点是加密、私有、无公网暴露；缺点是需要每台客户端安装 Tailscale。

如果 NAS 的 Docker 不支持 host 网络，可以在局域网里找一台常开设备（Mac mini、树莓派、N100 小主机）安装 Tailscale，并作为子网路由器宣告 `192.168.0.0/24`，然后通过 `http://192.168.0.108:18084` 访问。

## 方案 B：Cloudflare Tunnel + Access（公网域名）

`cloudflared` 从 NAS 主动建立出站隧道，Cloudflare 把 `https://library.example.com` 转发到 `http://192.168.0.108:18084`，不需要开放入站端口。

1. 在 Cloudflare 创建 Tunnel，拿到 token。
2. 在极空间 Docker 运行 `cloudflare/cloudflared`：

```yaml
services:
  cloudflared:
    image: cloudflare/cloudflared:latest
    container_name: cloudflared
    restart: unless-stopped
    command: tunnel --no-autoupdate run --token <TUNNEL_TOKEN>
```

3. 在 Cloudflare 配置 Public Hostname：

```text
library.example.com -> http://192.168.0.108:18084
```

4. 在 Cloudflare Access 给这个域名加身份策略（邮箱/Google/GitHub 等），避免任何人直接访问登录页。
5. 访问 `https://library.example.com`；OPDS 同样使用该地址。

优点：HTTPS、无需公网 IP、可在边缘做身份校验。缺点：依赖 Cloudflare 和自有域名。

## 方案 C：极空间自带远程访问

如果极空间 Docker 应用支持把容器端口映射到极空间远程访问域名，可以复用它的 HTTPS 域名。

- 在 Docker 容器/项目里把宿主 `18084` 暴露到极空间远程访问；
- 确认外部访问地址和 TLS；
- 仍然要保留 `AUTH_USER` / `AUTH_PASSWORD_HASH`，不要在公网裸奔。

当前极空间的直连域名只验证过 5005/5006/5055/5056，不能保证 18084 可用；需要以实际界面为准。

## 方案 D：FRP / rathole + VPS

如果已有公网 VPS，可在 VPS 上跑 frps/rathole 服务端，NAS 主动连接并映射 18084。需要在 VPS 上配置 TLS 反向代理，维护成本比 Tailscale/Cloudflare 高。

## 方案 E：路由器端口映射 + Caddy/Nginx TLS

把公网 443 转发到 NAS 上的 Caddy/Nginx，再由它反代 `http://127.0.0.1:18084`，用 Let's Encrypt 签发证书。可行但不推荐直接暴露；必须：

- 使用强密码（不要用弱口令）；
- 强制 HTTPS；
- 只开放必要端口；
- 考虑 IP 白名单、fail2ban 或额外的身份代理。

## 安全建议

- 当前密码 `909400` 较弱；公网访问前换一个强密码并重新生成哈希。
- 单用户模型没有按书权限；能登录就能读全部书库。
- 公网入口优先使用 Tailscale 或 Cloudflare Access 这类身份层，而不是只依赖应用密码。
- OPDS 客户端使用 HTTPS 地址和同一组 Basic 凭据。
- 若后续要长期公网运行，建议把会话 Cookie 改成 `Secure` 并加 CSRF 防护（P5）。

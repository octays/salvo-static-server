# Salvo Static Server 部署指南

## 快速开始

### 本地运行

```bash
# 安装开发依赖
make install-dev

# 设置示例文件
make setup

# 开发模式（热重载）
make dev

# 调试模式
make dev-debug
```

## Docker 部署

### 构建 Docker 镜像

```bash
# 构建镜像
make docker-build

# 或者直接使用 docker
docker build -t salvo-static-server:latest .
```

### 运行容器

```bash
# 基本运行
make docker-run

# HTTPS 模式（需要域名）
make docker-run-https

# 生产环境部署
make deploy-prod
```

### Docker Compose

```bash
# 开发环境
docker-compose --profile dev up -d

# 生产环境
DOMAIN=yourdomain.com docker-compose --profile prod up -d

```

## 配置选项

### 基本配置

```bash
# 仅 HTTP（默认）
./static-server

# 自定义端口
./static-server --http-addr 0.0.0.0:3000

# 启用压缩（默认开启）
./static-server

# 禁用压缩
./static-server --no-compression
```

### HTTPS 配置

```bash
# HTTPS + ACME 证书
./static-server \
  --https-enable \
  --acme-domain yourdomain.com \
  --force-https

# HTTPS + HTTP3
./static-server \
  --https-enable \
  --acme-domain yourdomain.com \
  --http3-enable

# 多个静态目录
./static-server \
  --static-dir /var/www/html \
  --static-dir /var/www/assets \
  --try-file index.html \
  --default-page 404.html
```

## 生产环境部署

Compose 提供 `dev` 和 `prod` 两种模式，由静态服务器直接监听公开端口。
发布前可运行 `make precompress STATIC_DIR=./static` 生成 `.gz`；安装 brotli CLI 后加 `PRECOMPRESS_ARGS=--brotli` 可同时生成 `.br`。
源文件和压缩文件必须同步更新，并通过静态目录挂载一起提供服务。

### 系统服务（Systemd）

创建服务文件 `/etc/systemd/system/salvo-static.service`:

```ini
[Unit]
Description=Salvo Static Server
After=network.target

[Service]
Type=simple
User=salvo
WorkingDirectory=/opt/salvo-static
ExecStart=/opt/salvo-static/static-server \
  --https-enable \
  --acme-domain yourdomain.com \
  --force-https \
  --log-level info
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

启用服务：

```bash
sudo systemctl enable salvo-static
sudo systemctl start salvo-static
```

### Nginx 反向代理

```nginx
server {
    listen 80;
    server_name yourdomain.com;
    return 301 https://$server_name$request_uri;
}

server {
    listen 443 ssl http2;
    server_name yourdomain.com;

    ssl_certificate /etc/letsencrypt/live/yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/yourdomain.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

### 环境变量

```bash
# 日志级别
export RUST_LOG=info

# 运行服务器
./static-server --https-enable --acme-domain yourdomain.com
```

## 监控和日志

### 日志配置

```bash
# 调试模式
./static-server --log-level debug

# 错误日志
./static-server --log-level error

# 结构化日志
RUST_LOG_FORMAT=json ./static-server
```

### 健康检查

```bash
# HTTP 健康检查
curl -f http://localhost:8080/ || exit 1

# Docker 健康检查已内置在镜像中
```

### 性能监控

```bash
# 查看连接数
ss -tuln | grep :8080

# 监控进程
ps aux | grep salvo-static-server

# 资源使用
top -p $(pgrep salvo-static-server)
```

## 故障排除

### 常见问题

1. **端口被占用**
   ```bash
   sudo netstat -tulpn | grep :8080
   # 更换端口
   ./static-server --http-addr 0.0.0.0:9090
   ```

2. **权限问题**
   ```bash
   # 使用非特权端口（>1024）
   # 或使用 sudo
   sudo ./static-server --http-addr 0.0.0.0:80
   ```

3. **SSL 证书问题**
   ```bash
   # 确保域名可访问
   nslookup yourdomain.com
   
   # 检查防火墙
   sudo ufw allow 80
   sudo ufw allow 443
   ```

4. **静态文件访问**
   ```bash
   # 检查文件权限
   ls -la /path/to/static/
   chmod -R 755 /path/to/static/
   ```

## 安全建议

1. **使用 HTTPS**：生产环境始终启用 HTTPS
2. **防火墙配置**：只开放必要的端口
3. **文件权限**：限制静态文件的写入权限
4. **定期更新**：保持系统和依赖更新
5. **日志监控**：监控访问日志和错误日志

## 性能优化

1. **启用压缩**：默认启用 Gzip 压缩
2. **缓存策略**：配置适当的缓存头
3. **CDN 集成**：使用 CDN 分发静态资源
4. **负载均衡**：多实例部署负载均衡
5. **资源优化**：压缩图片、合并 CSS/JS

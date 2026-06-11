# 多阶段构建 - 构建阶段
FROM rust:alpine AS builder
# 设置工作目录
WORKDIR /app
# 复制 Cargo 文件
COPY Cargo.toml ./
# 创建空的源目录以缓存依赖
RUN mkdir src && echo "fn main() {}" > src/main.rs
# 构建依赖（利用 Docker 层缓存）
RUN cargo build --release && rm -rf src
# 复制源代码
COPY src ./src
# 构建应用
RUN touch src/main.rs && cargo build --release
# 运行阶段
FROM alpine:latest AS runtime
# 安装运行时依赖
RUN apk update && apk add --no-cache \
    ca-certificates openssl
# 创建应用目录和静态文件目录
RUN mkdir -p /app/static
# 从构建阶段复制二进制文件
COPY --from=builder /app/target/release/static-server /usr/local/bin/

# 设置工作目录
WORKDIR /app

# 暴露端口
EXPOSE 8080 8443

# 健康检查
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/ || exit 1

# 启动命令
CMD ["static-server", "--http-addr", "0.0.0.0:8080", "--static-dir", "/app/static"]

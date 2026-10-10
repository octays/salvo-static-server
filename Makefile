# Makefile for static-server

.PHONY: all build build-local push precompress help

# 变量定义
BINARY_NAME=static-server
IMAGE_NAME=novo-one/static-server
VERSION?=$(shell grep '^version = ' Cargo.toml | cut -d '"' -f 2)
REGISTRY?=hub.yeastardigital.com
TAG?=$(VERSION)
STATIC_DIR?=static
PRECOMPRESS_ARGS?=

# 默认目标
all: build

# 为发布目录生成预压缩文件；Brotli 可通过 PRECOMPRESS_ARGS=--brotli 启用。
precompress:
	python3 tools/precompress.py "$(STATIC_DIR)" $(PRECOMPRESS_ARGS)

# 构建
build:
	@echo "Building $(BINARY_NAME)..."
	cargo build --release
	@echo "Build complete: target/release/$(BINARY_NAME)"

# 本地多架构构建
build-local:
	@echo "Building Docker image $(IMAGE_NAME):$(TAG) for amd64..."
	docker buildx build \
		--platform linux/amd64 \
		-t $(IMAGE_NAME)-amd64:$(TAG) \
		-t $(IMAGE_NAME)-amd64:latest \
		--load \
		.
	@echo "Building Docker image $(IMAGE_NAME):$(TAG) for arm64..."
	docker buildx build \
		--platform linux/arm64 \
		-t $(IMAGE_NAME)-arm64:$(TAG) \
		-t $(IMAGE_NAME)-arm64:latest \
		--load \
		.
	@echo "Multi-arch Docker images built locally!"
	@echo "Images: $(IMAGE_NAME)-amd64:$(TAG), $(IMAGE_NAME)-arm64:$(TAG)"

# 推送
push: build
	@echo "Building Docker image $(IMAGE_NAME):$(TAG) for multi-arch..."
	docker buildx build --push \
		--platform linux/amd64,linux/arm64 \
		-t $(REGISTRY)/$(IMAGE_NAME):$(TAG) \
		-t $(REGISTRY)/$(IMAGE_NAME):latest \
		.
	@echo "Multi-arch Docker image pushed successfully!"

# 帮助
help:
	@echo "Available commands:"
	@echo "  all         - Build static-server (default)"
	@echo "  build       - Build release binary"
	@echo "  build-local - Build Docker images for amd64 and arm64 locally"
	@echo "  push        - Build and push multi-arch Docker image"
	@echo "  precompress - Generate .gz assets (PRECOMPRESS_ARGS=--brotli adds .br)"

# Multi-stage build: builder stage
FROM rust:alpine AS builder
# Set the working directory
WORKDIR /app
# Copy the Cargo manifest and lockfile
COPY Cargo.toml Cargo.lock ./
# Create placeholder source to cache dependency builds
RUN mkdir src && echo "fn main() {}" > src/main.rs
# Build dependencies using the Docker layer cache
RUN cargo build --release --locked && rm -rf src
# Copy source files
COPY src ./src
# Build the application
RUN touch src/main.rs && cargo build --release --locked
# Runtime stage
FROM alpine:latest AS runtime
# Install runtime dependencies
RUN apk update && apk add --no-cache \
    ca-certificates openssl curl
# Create the application and static file directories
RUN mkdir -p /app/static
# Copy the executable from the builder stage
COPY --from=builder /app/target/release/static-server /usr/local/bin/

# Set the working directory
WORKDIR /app

# Expose server ports
EXPOSE 8080 8443

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/ || exit 1

# Startup command
CMD ["static-server", "--http-addr", "0.0.0.0:8080", "--static-dir", "/app/static"]

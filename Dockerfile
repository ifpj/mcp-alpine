FROM alpine AS builder
ARG TARGETARCH
COPY target/x86_64-unknown-linux-musl/release/mcp-alpine /bin/mcp-alpine-amd64
COPY target/aarch64-unknown-linux-musl/release/mcp-alpine /bin/mcp-alpine-arm64
RUN if [ "$TARGETARCH" = "amd64" ]; then \
      cp /bin/mcp-alpine-amd64 /bin/mcp-alpine ; \
    elif [ "$TARGETARCH" = "arm64" ]; then \
      cp /bin/mcp-alpine-arm64 /bin/mcp-alpine ; \
    fi

FROM alpine
RUN apk add --no-cache curl ca-certificates tzdata
ENV TZ=Asia/Shanghai
COPY --from=builder /bin/mcp-alpine /usr/local/bin/mcp-alpine
EXPOSE 3000
ENTRYPOINT ["mcp-alpine"]

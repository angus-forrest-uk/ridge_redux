# Multi-stage build: frontend, then a static musl binary, then a small runtime.
# The binary embeds the frontend (build.rs -> rust-embed), so the final image
# is just the one executable: `docker run` needs nothing beside it.

FROM node:22-alpine AS web
WORKDIR /src
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1-alpine AS build
# cc (gcc + musl headers) for build scripts and linking.
RUN apk add --no-cache gcc musl-dev
WORKDIR /src

# Manifests first, with stub sources, so the dependency layers cache across
# releases: only the workspace crates recompile when the source changes.
COPY Cargo.toml Cargo.lock ./
COPY crates/ridge-core/Cargo.toml crates/ridge-core/README.md crates/ridge-core/
COPY crates/ridge_redux/Cargo.toml crates/ridge_redux/build.rs crates/ridge_redux/
# Both READMEs are compiled in: ridge-core's is its rustdoc preface,
# the workspace one is served at /api/readme.
COPY README.md .
COPY --from=web /src/dist web/dist
RUN mkdir -p crates/ridge-core/src/bin crates/ridge_redux/src \
    && echo "" > crates/ridge-core/src/lib.rs \
    && echo "fn main() {}" > crates/ridge-core/src/bin/render.rs \
    && echo "" > crates/ridge_redux/src/lib.rs \
    && echo "fn main() {}" > crates/ridge_redux/src/main.rs \
    && cargo build --release --locked -p ridge_redux

COPY crates/ridge-core/src crates/ridge-core/src
COPY crates/ridge_redux/src crates/ridge_redux/src
RUN cargo build --release --locked -p ridge_redux

FROM alpine:3.22
# TLS roots (belt and braces: ureq bundles webpki-roots) and a cache home.
RUN apk add --no-cache ca-certificates \
    && adduser -D -h /data -s /sbin/nologin ridge \
    && mkdir -p /data/srtm \
    && chown -R ridge /data
COPY --from=build /src/target/release/ridge_redux /usr/local/bin/ridge_redux

USER ridge
VOLUME /data
EXPOSE 8420
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD wget -q -O /dev/null http://127.0.0.1:8420/healthz || exit 1
# 0.0.0.0 so the published port reaches the server; publish it to loopback
# unless you trust the network (the server has no authentication).
ENTRYPOINT ["/usr/local/bin/ridge_redux"]
CMD ["--no-open", "--addr", "0.0.0.0:8420", "--cache-dir", "/data/srtm"]

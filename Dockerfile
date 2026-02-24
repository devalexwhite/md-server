# ── Stage 1: Build ────────────────────────────────────────────────────────────
# rust:alpine gives us musl libc → fully static binary, no glibc dependency.
FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

# Copy manifests first so dependency compilation is cached independently.
COPY Cargo.toml Cargo.lock ./

# Build a throw-away main so cargo fetches and compiles all dependencies.
# The fingerprint files are removed so the real build re-links against them.
RUN mkdir src && \
    echo 'fn main() {}' > src/main.rs && \
    cargo build --release && \
    rm -rf src \
           target/release/md-server \
           target/release/deps/md_server* \
           target/release/.fingerprint/md-server*

# Now build the real source.
COPY src ./src
RUN cargo build --release && \
    strip target/release/md-server


# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
# Alpine is minimal (~7 MB) and gives us a proper user database and shell for
# debugging.  A fully static musl binary runs here without any extra libraries.
FROM alpine:3.21

RUN adduser -D -H -s /sbin/nologin mdserver

COPY --from=builder /app/target/release/md-server /usr/local/bin/md-server

# Content is mounted at runtime; create the mount point with correct ownership.
RUN mkdir /www && chown mdserver:mdserver /www

USER mdserver

# Bind only to loopback inside the container; the reverse proxy reaches this
# via the Docker bridge network, not through an exposed host port.
EXPOSE 3000

ENTRYPOINT ["/usr/local/bin/md-server"]
CMD ["--root", "/www", "--host", "0.0.0.0", "--port", "3000"]

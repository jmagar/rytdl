# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets

RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        libchromaprint-tools \
        openssh-client \
        rsync \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /usr/sbin/nologin --uid 10001 ytdl

COPY --from=builder /app/target/release/ytdl-mcp /usr/local/bin/ytdl-mcp

ENV HOME=/home/ytdl \
    YTDLP_STAGING_DIR=/tmp/ytdl-mcp \
    FPCALC_PATH=/usr/bin/fpcalc

RUN mkdir -p /tmp/ytdl-mcp /home/ytdl/.local/state/ytdl-mcp /home/ytdl/.cache \
    && chown -R ytdl:ytdl /tmp/ytdl-mcp /home/ytdl

USER ytdl
WORKDIR /work

VOLUME ["/library", "/home/ytdl/.ssh", "/home/ytdl/.local/state/ytdl-mcp", "/home/ytdl/.cache"]

ENTRYPOINT ["ytdl-mcp"]
CMD ["serve"]

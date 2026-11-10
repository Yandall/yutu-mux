# Cross-build image for the Windows zip: Rust + MinGW-w64 + 7-Zip.
# Pin the Rust series to the toolchain in AGENTS.md (rust 1.99+) so the
# container never drifts ahead of what contributors build with.
FROM rust:1.99-bookworm
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    mingw-w64 p7zip-full curl ca-certificates file python3 binutils-mingw-w64-x86-64 \
 && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-pc-windows-gnu
WORKDIR /work
ENTRYPOINT ["packaging/windows/build-windows-cross.sh"]

ARG RUST_VERSION=1.71.1
FROM rust:${RUST_VERSION}

RUN apt update && apt upgrade -y

ARG ARCH_NAME
RUN if [ "${ARCH_NAME}" = "aarch64" ]; then \
      # We assume the docker image's arch is x86_64, so cross-compiling for aarch64
      apt install -y g++-aarch64-linux-gnu libc6-dev-arm64-cross \
      libssl-dev pkg-config && \
      rustup toolchain install stable-aarch64-unknown-linux-gnu --force-non-host; \
    fi
RUN rustup target add "${ARCH_NAME}-unknown-linux-gnu"

WORKDIR /usr/src/app

ENV CARGO_HOME=/usr/src/app/.cargo

ENV ARCH_NAME=${ARCH_NAME} \
    # Generate static builds
    RUSTFLAGS="-C target-feature=+crt-static" \
    # Use the correct linker for aarch64 target
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++

CMD cargo build --release --target "${ARCH_NAME}-unknown-linux-gnu"

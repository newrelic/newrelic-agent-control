ARG RUST_VERSION=1.71.1
FROM rust:${RUST_VERSION}

ARG ARCH_NAME
RUN rustup target add "${ARCH_NAME}-unknown-linux-gnu"

WORKDIR /usr/src/app

ENV ARCH_NAME=${ARCH_NAME} \
    # Generate static builds
    RUSTFLAGS="-C target-feature=+crt-static" \
    # Use the correct linker for aarch64 target
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++

CMD cargo build --release --target "${ARCH_NAME}-unknown-linux-gnu"

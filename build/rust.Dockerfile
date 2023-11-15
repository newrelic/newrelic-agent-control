ARG RUST_VERSION=1.71.1
FROM rust:${RUST_VERSION}-buster

RUN apt update && apt upgrade -y

ARG ARCH_NAME
RUN if [ "${ARCH_NAME}" = "aarch64" ]; then \
      # We assume the docker image's arch is x86_64, so cross-compiling for aarch64
      apt install -y g++-aarch64-linux-gnu libc6-dev-arm64-cross pkg-config && \
      rustup toolchain install stable-aarch64-unknown-linux-gnu --force-non-host; \
    fi
RUN apt install -y libssl-dev
RUN rustup target add "${ARCH_NAME}-unknown-linux-gnu"

WORKDIR /usr/src/app

ENV CARGO_HOME=/usr/src/app/.cargo

ENV ARCH_NAME=${ARCH_NAME} \
    # Generate static builds
    # RUSTFLAGS="-C target-feature=+crt-static" \
    # Use the correct linker for aarch64 target
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++

# Persist the ARG value into an ENV so it's available at runtime
ARG BUILD_MODE
ENV BUILD_MODE_ENV=${BUILD_MODE}
ARG BUILD_FEATURE
ENV BUILD_FEATURE_ENV=${BUILD_FEATURE}

# Execute the command dynamically at runtime
CMD [ "sh", "-c", "\
     CMD_STRING='cargo build'; \
     [ \"$BUILD_MODE_ENV\" != 'debug' ] && CMD_STRING='cargo build --release'; \
     CMD_STRING=\"$CMD_STRING --features $BUILD_FEATURE_ENV\"; \
     CMD_STRING=\"$CMD_STRING --target $ARCH_NAME-unknown-linux-gnu\"; \
     $CMD_STRING \
"]

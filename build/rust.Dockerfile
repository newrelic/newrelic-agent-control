ARG RUST_VERSION=1.78.0
FROM rust:${RUST_VERSION}-bookworm

RUN apt update && apt upgrade -y
RUN apt install -y gcc musl-dev musl-tools

ARG ARCH_NAME
RUN if [ "${ARCH_NAME}" = "aarch64" ]; then \
      # We assume the docker image's arch is x86_64, so cross-compiling for aarch64
      apt install -y g++-aarch64-linux-gnu libc6-dev-arm64-cross && \
      rustup toolchain install stable-aarch64-unknown-linux-musl --force-non-host; \
    fi
RUN if [ "${ARCH_NAME}" = "x86_64" ]; then \
      rustup toolchain install stable-x86_64-unknown-linux-musl; \
    fi
RUN rustup target add "${ARCH_NAME}-unknown-linux-musl"

WORKDIR /usr/src/app

ENV CARGO_HOME=/usr/src/app/.cargo

ENV ARCH_NAME=${ARCH_NAME} \
    # Generate static builds
    RUSTFLAGS="-C target-feature=+crt-static" \
    # Use the correct linker for targets
    # x86_64
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-linux-gnu-gcc \
    CC_x86_64_unknown_linux_musl=x86_64-linux-gnu-gcc \
    CXX_x86_64_unknown_linux_musl=x86_64-linux-gnu-g++ \
    AR_x86_64_unknown_linux_musl=x86_64-linux-gnu-ar \
    # aarch64
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_musl=aarch64-linux-gnu-gcc \
    CXX_aarch64_unknown_linux_musl=aarch64-linux-gnu-g++ \
    AR_aarch64_unknown_linux_musl=aarch64-linux-gnu-ar

# Persist the ARG value into an ENV so it's available at runtime
ARG BUILD_MODE
ENV BUILD_MODE_ENV=${BUILD_MODE}
ARG BUILD_FEATURE
ENV BUILD_FEATURE_ENV=${BUILD_FEATURE}
ARG BUILD_BIN
ENV BUILD_BIN_ENV=${BUILD_BIN}
ARG BUILD_PKG
ENV BUILD_PKG_ENV=${BUILD_PKG}

# Execute the command dynamically at runtime
CMD [ "sh", "-c", "\
     CMD_STRING='cargo build'; \
     [ \"$BUILD_MODE_ENV\" != 'debug' ] && CMD_STRING='cargo build --release'; \
     CMD_STRING=\"$CMD_STRING --package $BUILD_PKG_ENV\"; \
     [ \"$BUILD_FEATURE_ENV\" != '' ] && CMD_STRING=\"$CMD_STRING --features $BUILD_FEATURE_ENV\"; \
     CMD_STRING=\"$CMD_STRING --target $ARCH_NAME-unknown-linux-musl\"; \
     CMD_STRING=\"$CMD_STRING --bin $BUILD_BIN_ENV\"; \
     CMD_STRING=\"$CMD_STRING --target-dir target-$BUILD_BIN_ENV\"; \
     echo $CMD_STRING; \
     $CMD_STRING \
"]

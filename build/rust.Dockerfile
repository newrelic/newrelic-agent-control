ARG RUST_VERSION=1.71.1
#FROM rust:${RUST_VERSION}-buster
FROM centos:centos7

ARG ARCH_NAME

RUN yum update -y

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | bash -s -- -y # should be changed to not interactive installer
RUN source "$HOME/.cargo/env"
# RUN rustup install ${RUST_VERSION}
#RUN rustup default 1.71.1-${ARCH_NAME}-unknown-linux-gnu

RUN if [ "${ARCH_NAME}" = "aarch64" ]; then \
      # We assume the docker image's arch is x86_64, so cross-compiling for aarch64
      yum -y install epel-release && \
      yum install gcc-aarch64-linux-gnu pkgconfig -y && \
      rustup toolchain install stable-aarch64-unknown-linux-gnu --force-non-host; \
    fi
#       yum install -y g++-aarch64-linux-gnu libc6-dev-arm64-cross pkg-config && \


#RUN if [ "${ARCH_NAME}" = "x86_64" ]; then \
#      # We assume the docker image's arch is x86_64, so cross-compiling for aarch64
#      rpm -i g++-amd64-linux-gnu libc6-dev-amd64-cross pkg-config && \
#      rustup toolchain install stable-aarch64-unknown-linux-gnu --force-non-host; \
#    fi
RUN yum install openssl-devel
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
ARG BUILD_BIN
ENV BUILD_BIN_ENV=${BUILD_BIN}

# Execute the command dynamically at runtime
CMD [ "sh", "-c", "\
     CMD_STRING='cargo build'; \
     [ \"$BUILD_MODE_ENV\" != 'debug' ] && CMD_STRING='cargo build --release'; \
     CMD_STRING=\"$CMD_STRING --features $BUILD_FEATURE_ENV\"; \
     CMD_STRING=\"$CMD_STRING --target $ARCH_NAME-unknown-linux-gnu\"; \
     CMD_STRING=\"$CMD_STRING --bin $BUILD_BIN_ENV\"; \
     CMD_STRING=\"$CMD_STRING --target-dir target-$BUILD_BIN_ENV\"; \
     $CMD_STRING \
"]

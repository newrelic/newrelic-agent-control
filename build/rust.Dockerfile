ARG RUST_VERSION=1.71.1
FROM rust:${RUST_VERSION}-alpine

RUN apk add musl-dev

ARG ARCH_NAME
RUN rustup target add "${ARCH_NAME}-unknown-linux-musl"

WORKDIR /usr/src/app

ENV ARCH_NAME=${ARCH_NAME} RUSTFLAGS="-C target-feature=+crt-static"

CMD cargo build --release --target "${ARCH_NAME}-unknown-linux-musl"

#syntax=docker/dockerfile:1.2
ARG arch
FROM --platform=linux/${arch} rust:1.68-alpine as build-env

WORKDIR /build
RUN apk add --no-cache \
    bash \
    clang-dev \
    cmake \
    curl \
    eudev-dev \
    g++ \
    git \
    musl-dev \
    linux-headers \
    make \
    ninja \
    openssl-dev \
    perl \
    pkgconfig \
    protoc \
    python3 \
    wget

# TODO: verify checksums of downloads

# Build the Solana CLI because there's no prebuilt aarch64 version
RUN wget https://github.com/solana-labs/solana/archive/refs/tags/v1.15.2.tar.gz -O solana-src.tar.gz && \
    tar -xzf solana-src.tar.gz && \
    rm solana-src.tar.gz && \
    mv solana-1.15.2 solana-src && \
    # We don't care about spl-token-cli and it doesn't compile on musl: https://github.com/solana-labs/solana/issues/28156
    sed -i '145,148d' solana-src/scripts/cargo-install-all.sh && \
    RUSTFLAGS="-C target-feature=-crt-static" solana-src/scripts/cargo-install-all.sh /build/solana && \
    rm -rf solana-src

# Build Solana platform tools because there's no prebuilt aarch64 version
RUN wget https://github.com/solana-labs/platform-tools/archive/refs/tags/v1.37.tar.gz -O solana-platform-tools.tar.gz && \
    tar -xzf solana-platform-tools.tar.gz && \
    rm solana-platform-tools.tar.gz && \
    mv platform-tools-1.37 solana-platform-tools-src

COPY platform-tools/ /tmp/platform-tools/
RUN cp -r /tmp/platform-tools/* solana-platform-tools-src/ && \
    RUSTFLAGS="-C target-feature=-crt-static" solana-platform-tools-src/build.sh /tmp/solana-platform-tools && \
    mkdir -p /build/solana-platform-tools && \
    apkArch="$(apk --print-arch)" && \
    case "$apkArch" in \
        x86_64) \
            tar -xjf /build/solana-platform-tools-src/platform-tools-linux-x86_64.tar.bz2 -C /build/solana-platform-tools \
            ;; \
        aarch64) \
            tar -xjf /build/solana-platform-tools-src/platform-tools-linux-aarch64.tar.bz2 -C /build/solana-platform-tools \
            ;; \
        *) echo >&2 "error: unsupported architecture ($apkArch)"; exit 1 ;; \
    esac && \
    rm -rf solana-platform-tools-src /tmp/solana-platform-tools /tmp/platform-tools *.tar.bz2 

ARG arch
FROM --platform=linux/${arch} rust:1.68-alpine as toolchain

WORKDIR /build

RUN adduser --disabled-password --uid 1000 default
# todo: what other deps do we need?
RUN apk add --no-cache \
    bash \
    git \
    musl-dev

COPY --from=build-env /build/solana /build/solana
COPY --from=build-env /build/solana-platform-tools/ /home/default/.cache/solana/v1.32/sbf-tools
RUN chown -R default:default /home/default/.cache && \
    mkdir -p /build/solana/bin/sdk/sbf/dependencies && \
    ln -s /home/default/.cache/solana/v1.32/sbf-tools /build/solana/bin/sdk/sbf/dependencies/sbf-tools

ENV PATH="/build/solana/bin:$PATH"
RUN rustup toolchain link sbf /build/solana/bin/sdk/sbf/dependencies/sbf-tools/rust
#    cp /build/solana/bin/sdk/sbf/dependencies/sbf-tools/version.md /build/solana/bin/sdk/sbf/dependencies/sbf-tools-v1.32.md

USER default
WORKDIR /home/default

CMD /bin/sh

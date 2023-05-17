#syntax=docker/dockerfile:1.2
ARG arch
FROM --platform=linux/${arch} rust:1.68-alpine as download-env
WORKDIR /build

RUN apk add --no-cache \
    git

RUN git clone https://github.com/Ellipsis-Labs/solana-verifiable-build.git && \
    cd solana-verifiable-build && \
    git checkout 7244eccc75a2e0e42f3344a1a86c31926ae1d4ef && \
    rm -rf .git

# Pre-download dependencies
RUN cd solana-verifiable-build/examples/hello_world && \
    CARGO_HOME=/build/cargo cargo vendor && \
    mkdir .cargo && \
    echo -e "\
[source.crates-io]\n\
replace-with = \"vendored-sources\"\n\
\n\
[source.vendored-sources]\n\
directory = \"vendor\"\
" > .cargo/config

ARG arch
FROM --platform=linux/${arch} ellipsislabs/solana-jchen:latest

COPY --from=download-env --chown=default:default /build/solana-verifiable-build/examples/hello_world solana-verifiable-build/examples/hello_world
# Default $CARGO_HOME location
COPY --from=download-env --chown=default:default /build/cargo .cargo

# Get the code of the program to build and verify
RUN cargo-build-sbf --manifest-path=solana-verifiable-build/examples/hello_world/Cargo.toml -- --locked --frozen --offline

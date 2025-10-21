FROM --platform=linux/amd64 rust@sha256:b7b25312e49dfbe6cab04c89d5a8ed5df2df971406a3b5c5ac43e247b5821b5f

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.anza.xyz/v1.18.26/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
# Call cargo build-sbf to trigger installation of associated platform tools
RUN cargo init temp --edition 2021 && \
    cd temp && \
    cargo build-sbf && \
    rm -rf temp
WORKDIR /build

CMD /bin/bash

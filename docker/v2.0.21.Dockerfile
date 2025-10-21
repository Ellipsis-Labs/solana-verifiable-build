FROM --platform=linux/amd64 rust@sha256:653bd24b9a8f9800c67df55fea5637a97152153fd744a4ef78dd41f7ddc40144

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.anza.xyz/v2.0.21/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
# Call cargo build-sbf to trigger installation of associated platform tools
RUN cargo init temp --edition 2021 && \
    cd temp && \
    cargo build-sbf && \
    rm -rf temp
WORKDIR /build

CMD /bin/bash

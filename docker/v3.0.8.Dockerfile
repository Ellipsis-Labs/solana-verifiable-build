FROM --platform=linux/amd64 rust@sha256:878ca0e8df1305dcbbfffac5bb908cce6a4bc5f6b629c518e7112645ee8851d4

RUN apt-get update && apt-get install -qy git gnutls-bin curl ca-certificates
RUN curl -sSfL "https://release.anza.xyz/v3.0.8/install" -o /tmp/solana_install.sh && \
    ACTUAL=$(sha256sum /tmp/solana_install.sh | awk '{print $1}') && \
    test "$ACTUAL" = "c2a487e9990dc7f7211ce0ccb42c0c749fa3a839be1bee443ddb62c78c5ed25e" && \
    sh /tmp/solana_install.sh && \
    rm -f /tmp/solana_install.sh

ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
# Call cargo build-sbf to trigger installation of associated platform tools
RUN cargo init temp --edition 2021 && \
    cd temp && \
    cargo build-sbf && \
    rm -rf temp
WORKDIR /build

CMD /bin/bash

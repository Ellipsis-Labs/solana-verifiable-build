FROM --platform=linux/amd64 rust@sha256:e5a28b9e772535dc50205b4684b4e1cd113bb52e02e54ff387015c55c561e477

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.solana.com/v1.17.32/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build

CMD /bin/bash

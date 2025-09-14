FROM --platform=linux/amd64 rust@sha256:6a2ac38604fce995fd586c8d760147f71d9113dcbe84a7fcddcb30c60a1ec7ee

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.solana.com/v1.17.0/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build

CMD /bin/bash

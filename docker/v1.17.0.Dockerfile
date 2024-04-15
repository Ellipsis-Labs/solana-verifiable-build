FROM --platform=linux/amd64 rust@sha256:6562d50b62366d5b9db92b34c6684fab5bf3b9f627e59a863c9c0675760feed4

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.solana.com/v1.17.0/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build

CMD /bin/bash

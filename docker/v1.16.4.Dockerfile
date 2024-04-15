FROM --platform=linux/amd64 rust@sha256:b7e0e2c6199fb5f309742c5eba637415c25ca2bed47fa5e80e274d4510ddfa3a

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.solana.com/v1.16.4/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build

CMD /bin/bash

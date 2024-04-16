FROM --platform=linux/amd64 rust@sha256:b7f381685785bb4192e53995d6ad1dec70954e682e18e06a4c8c02011ab2f32e

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.solana.com/v1.18.3/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build

CMD /bin/bash

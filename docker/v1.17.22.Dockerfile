FROM --platform=linux/amd64 rust@sha256:1a72737690460c06dcd48ea215f3179e93d2ae5957c4c874b721df29d123fa0b

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.solana.com/v1.17.22/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build

CMD /bin/bash

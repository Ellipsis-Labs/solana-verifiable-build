FROM --platform=linux/amd64 rust@sha256:7ec316528af3582341280f667be6cfd93062a10d104f3b1ea72cd1150c46ef22

RUN apt-get update && apt-get install -qy git gnutls-bin
RUN sh -c "$(curl -sSfL https://release.solana.com/v1.17.23/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build

CMD /bin/bash

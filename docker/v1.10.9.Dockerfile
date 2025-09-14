FROM --platform=linux/amd64 rust@sha256:b33af7ffbb3bf98940f8326d9563ca403e315a33d9434303df76bdc325b0f5c4

RUN apt-get update && apt-get install -qy git gnutls-bin curl

# Download and modify the Solana install script to install the specified version
RUN curl -sSfL "https://release.solana.com/v1.18.20/install" -o solana_install.sh && \
    chmod +x solana_install.sh && \
    sed -i "s/^SOLANA_INSTALL_INIT_ARGS=.*/SOLANA_INSTALL_INIT_ARGS=v1.10.9/" solana_install.sh && \
    ./solana_install.sh && \
    rm solana_install.sh

ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build
CMD /bin/bash

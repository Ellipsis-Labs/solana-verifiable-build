FROM --platform=linux/amd64 rust@sha256:9fe1f39bec70576e2bd568fafb194b2a532a6f2928bc0b951ac2c0a69a2be9fe

RUN apt-get update && apt-get install -qy git gnutls-bin curl

# Download and modify the Solana install script to install the specified version
RUN curl -sSfL "https://release.solana.com/v1.18.20/install" -o solana_install.sh && \
    chmod +x solana_install.sh && \
    sed -i "s/^SOLANA_INSTALL_INIT_ARGS=.*/SOLANA_INSTALL_INIT_ARGS=v1.11.2/" solana_install.sh && \
    ./solana_install.sh && \
    rm solana_install.sh

ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build
CMD /bin/bash

FROM --platform=linux/amd64 rust@sha256:79892de83d1af9109c47a4566a24a0b240348bb8c088f1bccc52645c4c70ec39

RUN apt-get update && apt-get install -qy git gnutls-bin curl

# Download and modify the Solana install script to install the specified version
RUN curl -sSfL "https://release.solana.com/v1.18.20/install" -o solana_install.sh && \
    chmod +x solana_install.sh && \
    sed -i "s/^SOLANA_INSTALL_INIT_ARGS=.*/SOLANA_INSTALL_INIT_ARGS=v1.14.11/" solana_install.sh && \
    ./solana_install.sh && \
    rm solana_install.sh

ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
WORKDIR /build
CMD /bin/bash

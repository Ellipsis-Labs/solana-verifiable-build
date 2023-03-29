FROM solanalabs/rust:1.68.0

RUN apt-get update && apt-get install -qy clang libudev-dev tmux vim git netcat zsh
RUN sh -c "$(curl -fsSL https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/master/tools/install.sh)" "" --unattended

WORKDIR /build

RUN sh -c "$(curl -sSfL https://release.solana.com/v1.14.14/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"
CMD /bin/zsh

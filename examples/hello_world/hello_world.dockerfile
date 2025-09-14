# Get rust and solana toolchain
FROM ellipsislabs/solana:latest
# Install the Solana Verify CLI
RUN git clone https://github.com/Ellipsis-Labs/solana-verifiable-build.git /build
RUN git checkout 08c4a64
# Get the code of the program to build and verify
RUN cd examples/hello_world && cargo build-sbf -- --locked --frozen
#!/usr/bin/env bash

# Vendored from https://github.com/solana-labs/platform-tools/blob/v1.37/build.sh to support aarch64 and musl
# Note that the changes break libc support
set -ex

unameOut="$(uname -s)"
case "${unameOut}" in
    Darwin*)
        EXE_SUFFIX=
        if [[ "$(uname -m)" == "arm64" ]] ; then
            HOST_TRIPLE=aarch64-apple-darwin
            ARTIFACT=platform-tools-osx-aarch64.tar.bz2
        else
            HOST_TRIPLE=x86_64-apple-darwin
            ARTIFACT=platform-tools-osx-x86_64.tar.bz2
        fi;;
    MINGW*)
        EXE_SUFFIX=.exe
        HOST_TRIPLE=x86_64-pc-windows-msvc
        ARTIFACT=platform-tools-windows-x86_64.tar.bz2;;
    Linux* | *)
        EXE_SUFFIX=
        if [[ "$(uname -m)" == "aarch64" ]] ; then
            HOST_TRIPLE=aarch64-unknown-linux-musl
            ARTIFACT=platform-tools-linux-aarch64.tar.bz2
        else
            HOST_TRIPLE=x86_64-unknown-linux-musl
            ARTIFACT=platform-tools-linux-x86_64.tar.bz2
        fi
esac

cd "$(dirname "$0")"
OUT_DIR=$(realpath "${1:-out}")

rm -rf "${OUT_DIR}"
mkdir -p "${OUT_DIR}"
pushd "${OUT_DIR}"

git clone --single-branch --branch solana-tools-v1.37 https://github.com/solana-labs/rust.git
echo "$( cd rust && git rev-parse HEAD )  https://github.com/solana-labs/rust.git" >> version.md

git clone --single-branch --branch solana-tools-v1.37 https://github.com/solana-labs/cargo.git
echo "$( cd cargo && git rev-parse HEAD )  https://github.com/solana-labs/cargo.git" >> version.md

pushd rust
if [[ "${HOST_TRIPLE}" == "x86_64-pc-windows-msvc" ]] ; then
    # Do not build lldb on Windows
    sed -i -e 's#enable-projects = \"clang;lld;lldb\"#enable-projects = \"clang;lld\"#g' config.toml
fi
if [[ "${HOST_TRIPLE}" == "aarch64-unknown-linux-musl" ]]; then
    # Disable crt-static for aarch64-unknown-linux-musl
    echo -e "[target.aarch64-unknown-linux-musl]\ncrt-static = false" >> config.toml
fi
./x.py build --stage 1 --target ${HOST_TRIPLE},sbf-solana-solana
popd

pushd cargo
if [[ "${HOST_TRIPLE}" == "x86_64-unknown-linux-gnu" ]] ; then
    OPENSSL_STATIC=1 OPENSSL_LIB_DIR=/usr/lib/x86_64-linux-gnu OPENSSL_INCLUDE_DIR=/usr/include/openssl cargo build --release
else
    OPENSSL_STATIC=1 cargo build --release
fi
popd

if [[ "${HOST_TRIPLE}" != "x86_64-pc-windows-msvc" ]] ; then
    git clone --single-branch --branch solana-tools-v1.37 https://github.com/solana-labs/newlib.git
    echo "$( cd newlib && git rev-parse HEAD )  https://github.com/solana-labs/newlib.git" >> version.md
    mkdir -p newlib_build
    mkdir -p newlib_install
    pushd newlib_build
    CC="${OUT_DIR}/rust/build/${HOST_TRIPLE}/llvm/bin/clang" \
      AR="${OUT_DIR}/rust/build/${HOST_TRIPLE}/llvm/bin/llvm-ar" \
      RANLIB="${OUT_DIR}/rust/build/${HOST_TRIPLE}/llvm/bin/llvm-ranlib" \
      ../newlib/newlib/configure --target=sbf-solana-solana --host=sbf-solana --build="${HOST_TRIPLE}" --prefix="${OUT_DIR}/newlib_install"
    make install
    popd
fi

# Copy rust build products
mkdir -p deploy/rust
cp version.md deploy/
cp -R "rust/build/${HOST_TRIPLE}/stage1/bin" deploy/rust/
cp -R "cargo/target/release/cargo${EXE_SUFFIX}" deploy/rust/bin/
mkdir -p deploy/rust/lib/rustlib/
cp -R "rust/build/${HOST_TRIPLE}/stage1/lib/rustlib/${HOST_TRIPLE}" deploy/rust/lib/rustlib/
cp -R "rust/build/${HOST_TRIPLE}/stage1/lib/rustlib/sbf-solana-solana" deploy/rust/lib/rustlib/
find . -maxdepth 6 -type f -path "./rust/build/${HOST_TRIPLE}/stage1/lib/*" -exec cp {} deploy/rust/lib \;
mkdir -p deploy/rust/lib/rustlib/src/rust
cp "rust/build/${HOST_TRIPLE}/stage1/lib/rustlib/src/rust/Cargo.lock" deploy/rust/lib/rustlib/src/rust
cp -R "rust/build/${HOST_TRIPLE}/stage1/lib/rustlib/src/rust/library" deploy/rust/lib/rustlib/src/rust

# Copy llvm build products
mkdir -p deploy/llvm/{bin,lib}
while IFS= read -r f
do
    bin_file="rust/build/${HOST_TRIPLE}/llvm/build/bin/${f}${EXE_SUFFIX}"
    if [[ -f "$bin_file" ]] ; then
        cp -R "$bin_file" deploy/llvm/bin/
    fi
done < <(cat <<EOF
clang
clang++
clang-cl
clang-cpp
clang-15
ld.lld
ld64.lld
llc
lld
lld-link
lldb
lldb-vscode
llvm-ar
llvm-objcopy
llvm-objdump
llvm-readelf
llvm-readobj
EOF
         )
cp -R "rust/build/${HOST_TRIPLE}/llvm/build/lib/clang" deploy/llvm/lib/
if [[ "${HOST_TRIPLE}" != "x86_64-pc-windows-msvc" ]] ; then
    cp -R newlib_install/sbf-solana/lib/lib{c,m}.a deploy/llvm/lib/
    cp -R newlib_install/sbf-solana/include deploy/llvm/
    cp -R rust/src/llvm-project/lldb/scripts/solana/* deploy/llvm/bin/
    cp -R rust/build/${HOST_TRIPLE}/llvm/lib/liblldb.* deploy/llvm/lib/
fi

# Check the Rust binaries
while IFS= read -r f
do
    "./deploy/rust/bin/${f}${EXE_SUFFIX}" --version
done < <(cat <<EOF
cargo
rustc
rustdoc
EOF
         )
# Check the LLVM binaries
while IFS= read -r f
do
    if [[ -f "./deploy/llvm/bin/${f}${EXE_SUFFIX}" ]] ; then
        "./deploy/llvm/bin/${f}${EXE_SUFFIX}" --version
    fi
done < <(cat <<EOF
clang
clang++
clang-cl
clang-cpp
ld.lld
llc
lld-link
llvm-ar
llvm-objcopy
llvm-objdump
llvm-readelf
llvm-readobj
solana-lldb
EOF
         )

tar -C deploy -jcf ${ARTIFACT} .

# Package LLVM binaries for Move project
MOVE_DEV_TAR=${ARTIFACT/platform-tools/move-dev}
mkdir move-dev
if [[ "${HOST_TRIPLE}" == "x86_64-pc-windows-msvc" ]] ; then
    rm -f rust/build/${HOST_TRIPLE}/llvm/bin/{llvm-ranlib.exe,llvm-lib.exe,llvm-dlltool.exe}
fi
cp -R "rust/build/${HOST_TRIPLE}/llvm/"{bin,include,lib} move-dev/
tar -jcf "${MOVE_DEV_TAR}" move-dev

popd

mv "${OUT_DIR}/${ARTIFACT}" .
mv "${OUT_DIR}/${MOVE_DEV_TAR}" .

echo "Saved ${ARTIFACT} to $(pwd)/${ARTIFACT}"
echo "Saved ${MOVE_DEV_TAR} to $(pwd)/${MOVE_DEV_TAR}"

# Build linux binaries on macOS in docker
if [[ "$(uname)" == "Darwin" ]] && [[ $# == 1 ]] && [[ "$1" == "--docker" ]] ; then
    docker system prune -a -f
    docker build -t solanalabs/platform-tools .
    id=$(docker create solanalabs/platform-tools /build.sh "${OUT_DIR}")
    docker cp build.sh "${id}:/"
    docker start -a "${id}"
    docker cp "${id}:${OUT_DIR}/solana-sbf-tools-linux-x86_64.tar.bz2" "${OUT_DIR}"
fi

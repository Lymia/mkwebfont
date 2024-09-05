#!/bin/sh -eu

if which zig; then
    if [ "$(zig version)" != "0.11.0" ]; then
        echo "Zig version must be 0.11.0"
        exit 1
    fi
    true
else
    if which nix-shell; then
        echo "(zig binary not found, grabbing via nix-shell)"
        nix-shell -p zig_0_11 --run "$0"
        exit 0
    else
        echo "Could not find zig binary."
        exit 1
    fi
fi

BIN_NAME="mkwebfont"
VERSION="0.2.0-alpha7"

rm -rfv dist ||:
mkdir dist

rustup toolchain add beta
rustup target add --toolchain beta x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-gnu
cargo install cargo-zigbuild

cargo +beta clean
cargo +beta zigbuild -p $BIN_NAME --target x86_64-unknown-linux-musl --release
cargo +beta zigbuild -p $BIN_NAME --target aarch64-unknown-linux-musl --release
cargo +beta zigbuild -p $BIN_NAME --target x86_64-apple-darwin --release
cargo +beta zigbuild -p $BIN_NAME --target aarch64-apple-darwin --release
cargo +beta zigbuild -p $BIN_NAME --target x86_64-pc-windows-gnu --release

cp -v target/x86_64-unknown-linux-musl/release/$BIN_NAME dist/"$BIN_NAME-$VERSION-x86_64-linux"
cp -v target/aarch64-unknown-linux-musl/release/$BIN_NAME dist/"$BIN_NAME-$VERSION-aarch64-linux"
cp -v target/x86_64-apple-darwin/release/$BIN_NAME dist/"$BIN_NAME-$VERSION-x86_64-macos"
cp -v target/aarch64-apple-darwin/release/$BIN_NAME dist/"$BIN_NAME-$VERSION-aarch64-macos"
cp -v target/x86_64-pc-windows-gnu/release/$BIN_NAME.exe dist/"$BIN_NAME-$VERSION-x86_64-win32.exe"

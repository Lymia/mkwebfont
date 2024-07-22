#!/bin/sh

BIN_NAME="mkwebfont"
VERSION="0.2.0-alpha6"

rm -rfv dist
mkdir dist

cargo zigbuild -p $BIN_NAME --target x86_64-unknown-linux-musl --release || exit 1
cargo zigbuild -p $BIN_NAME --target aarch64-unknown-linux-musl --release || exit 1
cargo zigbuild -p $BIN_NAME --target x86_64-apple-darwin --release || exit 1
cargo zigbuild -p $BIN_NAME --target aarch64-apple-darwin --release || exit 1
cargo zigbuild -p $BIN_NAME --target x86_64-pc-windows-gnu --release || exit 1

cp -v target/x86_64-unknown-linux-musl/release/$BIN_NAME dist/"$BIN_NAME-$VERSION-x86_64-linux" || exit 1
cp -v target/aarch64-unknown-linux-musl/release/$BIN_NAME dist/"$BIN_NAME-$VERSION-aarch64-linux" || exit 1
cp -v target/x86_64-apple-darwin/release/$BIN_NAME dist/"$BIN_NAME-$VERSION-x86_64-macos" || exit 1
cp -v target/aarch64-apple-darwin/release/$BIN_NAME dist/"$BIN_NAME-$VERSION-aarch64-macos" || exit 1
cp -v target/x86_64-pc-windows-gnu/release/$BIN_NAME.exe dist/"$BIN_NAME-$VERSION-x86_64-win32.exe" || exit 1

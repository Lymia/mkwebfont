#!/bin/sh

VERSION="0.2.0-alpha5"
SHORT_VERSION="$(echo "$VERSION" | sed "s/-.*//g")"

rm -rfv dist
mkdir dist

cargo zigbuild -p mkwebfont --target x86_64-unknown-linux-musl --release || exit 1
cargo zigbuild -p mkwebfont --target aarch64-unknown-linux-musl --release || exit 1
cargo zigbuild -p mkwebfont --target x86_64-apple-darwin --release || exit 1
cargo zigbuild -p mkwebfont --target aarch64-apple-darwin --release || exit 1
cargo zigbuild -p mkwebfont --target x86_64-pc-windows-gnu --release || exit 1

cp -v target/x86_64-unknown-linux-musl/release/mkwebfont dist/"mkwebfont-$VERSION-x86_64-linux" || exit 1
cp -v target/aarch64-unknown-linux-musl/release/mkwebfont dist/"mkwebfont-$VERSION-aarch64-linux" || exit 1
cp -v target/x86_64-apple-darwin/release/mkwebfont dist/"mkwebfont-$VERSION-x86_64-macos" || exit 1
cp -v target/aarch64-apple-darwin/release/mkwebfont dist/"mkwebfont-$VERSION-aarch64-macos" || exit 1
cp -v target/x86_64-pc-windows-gnu/release/mkwebfont.exe dist/"mkwebfont-$VERSION-x86_64-win32.exe" || exit 1

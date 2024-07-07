#!/bin/sh

VERSION="0.2.0-alpha3"
SHORT_VERSION="$(echo "$VERSION" | sed "s/-.*//g")"

rm -v mkwebfont*.AppImage
rm -rfv dist

mkdir dist

#nix develop -c cargo zigbuild -p mkwebfont --target x86_64-pc-windows-gnu --release || exit 1
nix bundle --bundler github:ralismark/nix-appimage?rev=17dd6001ec228ea0b8505d6904fc5796d3de5012 .?submodules=1#mkwebfont --impure || exit 1

#cp -v target/x86_64-pc-windows-gnu/release/mkwebfont.exe dist/"mkwebfont-no_data-$VERSION-x86_64.exe" || exit 1
cp -v "mkwebfont-$SHORT_VERSION-x86_64.AppImage" dist/"mkwebfont-$VERSION-x86_64.AppImage" || exit 1

chmod 755 dist/*
rm -v mkwebfont*.AppImage

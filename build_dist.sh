#!/bin/sh

VERSION="0.2.0-alpha1"

rm -v mkwebfont*.AppImage
rm -rfv dist

mkdir dist

nix develop -c cargo zigbuild -p mkwebfont --target x86_64-pc-windows-gnu --release || exit 1
nix bundle --bundler github:ralismark/nix-appimage .?submodules=1#mkwebfont --impure || exit 1
nix bundle --bundler github:ralismark/nix-appimage .?submodules=1#mkwebfont-no_data --impure || exit 1

cp -v target/x86_64-pc-windows-gnu/release/mkwebfont.exe dist/"mkwebfont-no_data-$VERSION-x86_64.exe" || exit 1
cp -v mkwebfont-x86_64.AppImage dist/"mkwebfont-$VERSION-x86_64.AppImage" || exit 1
cp -v mkwebfont-no_data-x86_64.AppImage dist/"mkwebfont-$VERSION-no_data-x86_64.AppImage" || exit 1

chmod 755 dist/*
rm -v mkwebfont*.AppImage

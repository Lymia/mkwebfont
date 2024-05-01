#!/bin/sh

rm -v mkwebfont*.AppImage
rm -rfv dist

mkdir dist

nix develop -c cargo build --release -p mkwebfont --target x86_64-pc-windows-gnu || exit 1
nix bundle --bundler github:ralismark/nix-appimage .?submodules=1#mkwebfont --impure || exit 1
nix bundle --bundler github:ralismark/nix-appimage .?submodules=1#mkwebfont-no_data --impure || exit 1

cp -v mkwebfont*.AppImage dist
chmod o+r dist/*
rm -v mkwebfont*.AppImage

{
    description = "mkwebfont flake";

    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
        flake-utils.url = "github:numtide/flake-utils";
        rust-overlay.url = "github:oxalica/rust-overlay";
    };

    outputs = inputs: with inputs; flake-utils.lib.eachDefaultSystem (system:
        let
            nixpkgsImport = import nixpkgs {
                system = "x86_64-linux";
                overlays = [ rust-overlay.overlays.default ];
            };
            pkgs = nixpkgsImport;

            mkwebfont-datapkg-adjacency = pkgs.fetchurl {
                url = "https://github.com/Lymia/mkwebfont/releases/download/mkwebfont-data-v0.1.0/mkwebfont-datapkg-adjacency-v0.1.0";
                sha256 = "sha256-1vSXQfya5BvqNjkF9QyYwHQUPx64RcDixuotBdJ6rZ4=";
            };
            mkwebfont-datapkg-validation = pkgs.fetchurl {
                url = "https://github.com/Lymia/mkwebfont/releases/download/mkwebfont-data-v0.1.0/mkwebfont-datapkg-validation-v0.1.0";
                sha256 = "sha256-Bm97H+OjVPUvUeWCxb9iFAEFPWH8aoe1gW4G+615ZBg=";
            };

            mkwebfont-data = pkgs.stdenv.mkDerivation {
                pname = "mkwebfont-data";
                version = "0.1.0";
                unpackPhase = "true";

                installPhase = ''
                    mkdir -p $out/share/mkwebfont-data
                    cp ${mkwebfont-datapkg-adjacency} $out/share/mkwebfont-data/mkwebfont-datapkg-adjacency-v0.1.0
                    cp ${mkwebfont-datapkg-validation} $out/share/mkwebfont-data/mkwebfont-datapkg-validation-v0.1.0
                '';
            };


            mkwebfont-common = (pkgs: flags: pkgs.rustPlatform.buildRustPackage {
                pname = "mkwebfont";
                version = "0.2.0";
                src = ./.;
                cargoBuildFlags = flags;

                doCheck = false;

                cargoLock = {
                   lockFile = ./Cargo.lock;
                };
                nativeBuildInputs = [
                   pkgs.rustPlatform.bindgenHook
                ];
            });

            mkwebfont-unwrapped = mkwebfont-common pkgs "-p mkwebfont";
            mkwebfont-unwrapped-nonet = mkwebfont-common pkgs "-p mkwebfont --no-default-features --features appimage,binary,bundled-harfbuzz";

            mkwebfont-no_data = pkgs.runCommand "mkwebfont-no_data" {
              inherit (mkwebfont-unwrapped) pname version meta;
              nativeBuildInputs = [ pkgs.makeBinaryWrapper ];
            } ''
                mkdir -p $out/bin
                makeBinaryWrapper ${mkwebfont-unwrapped}/bin/mkwebfont $out/bin/mkwebfont-no_data
            '';

            mkwebfont = pkgs.runCommand "mkwebfont" {
              inherit (mkwebfont-unwrapped) pname version meta;
              nativeBuildInputs = [ pkgs.makeBinaryWrapper ];
            } ''
                mkdir -p $out/bin
                makeBinaryWrapper ${mkwebfont-unwrapped-nonet}/bin/mkwebfont $out/bin/mkwebfont \
                    --set MKWEBFONT_APPIMAGE_DATA ${mkwebfont-data}/share/mkwebfont-data
            '';

            rust-shell = pkgs.buildPackages.rust-bin.stable.latest.default.override {
                targets = [ "x86_64-pc-windows-gnu" ];
            };
        in rec {
            packages = {
                mkwebfont = mkwebfont;
                mkwebfont-no_data = mkwebfont-no_data;
            };

            devShells.default = pkgs.mkShell {
                buildInputs = [ rust-shell ];
            };
        }
    );
}
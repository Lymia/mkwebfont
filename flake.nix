{
    description = "mkwebfont flake";

    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
        flake-utils.url = "github:numtide/flake-utils";
        nixpkgs-mozilla.url = "github:mozilla/nixpkgs-mozilla";
    };

    outputs = inputs: with inputs; flake-utils.lib.eachDefaultSystem (system:
        let
            pkgs = import nixpkgs {
                system = "x86_64-linux";
                overlays = [ nixpkgs-mozilla.overlay ];
            };
            nightlyRustPlatform = pkgs.makeRustPlatform {
                rustc = pkgs.latest.rustChannels.nightly.rust;
                cargo = pkgs.latest.rustChannels.nightly.rust;
            };
            mkwebfont = nightlyRustPlatform.buildRustPackage {
                pname = "mkwebfont";
                version = "0.1.0";
                src = ./.;
                cargoBuildFlags = "-p mkwebfont --no-default-features --features binary";

                doCheck = false;

                cargoLock = {
                    lockFile = ./Cargo.lock;
                };

                buildInputs = [
                    pkgs.harfbuzz
                ];
                nativeBuildInputs = [
                    pkgs.pkg-config
                    nightlyRustPlatform.bindgenHook
                ];
            };
        in rec {
            packages = {
                mkwebfont = mkwebfont;
                default = mkwebfont;
            };
        }
    );
}
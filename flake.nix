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

            rust-platform = pkgs.makeRustPlatform {
                cargo = pkgs.buildPackages.rust-bin.beta.latest.default;
                rustc = pkgs.buildPackages.rust-bin.beta.latest.default;
            };

            mkwebfont-common = (pkgs: flags: rust-platform.buildRustPackage {
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
            mkwebfont = mkwebfont-common pkgs "-p mkwebfont";

            rust-shell = pkgs.buildPackages.rust-bin.beta.latest.default.override {
                targets = [ "x86_64-pc-windows-gnu" ];
            };
        in rec {
            packages = {
                mkwebfont = mkwebfont;
            };

            devShells.default = pkgs.mkShell {
                buildInputs = [ rust-shell pkgs.zig pkgs.cargo-zigbuild pkgs.rustPlatform.bindgenHook ];
            };
        }
    );
}
{
    description = "mkwebfont flake";

    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
        flake-utils.url = "github:numtide/flake-utils";
    };

    outputs = inputs: with inputs; flake-utils.lib.eachDefaultSystem (system:
        let
            pkgs = nixpkgs.legacyPackages.${system};
            mkwebfont = pkgs.rustPlatform.buildRustPackage {
                pname = "mkwebfont";
                version = "0.1.0";
                src = ./.;
                cargoBuildFlags = "-p mkwebfont";

                cargoLock = {
                    lockFile = ./Cargo.lock;
                };

                nativeBuildInputs = [ ];

                LIBCLANG_PATH="${pkgs.llvmPackages.libclang}/lib";
            };
        in rec {
            packages = {
                mkwebfont = mkwebfont;
                default = mkwebfont;
            };
        }
    );
}
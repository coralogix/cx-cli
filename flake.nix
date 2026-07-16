{
  description = "cx - Coralogix CLI";

  # Unstable channel: tracks the rustc version pinned in rust-toolchain.toml
  # (1.96.1). Stable channels lag behind and would mismatch the toolchain file.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in {
      packages = forAllSystems (system:
        let pkgs = pkgsFor system; in {
          skills = pkgs.runCommandLocal "cx-cli-skills" { } ''
            cp -r ${./skills} $out
          '';

          default = pkgs.rustPlatform.buildRustPackage {
            pname = "coralogix-cli";
            version = (nixpkgs.lib.importTOML ./Cargo.toml).package.version;
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = nixpkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.dbus ];

            # Tests need network / real Coralogix endpoints.
            doCheck = false;

            meta = {
              description = "Coralogix CLI - the observability backbone for AI agents and engineering teams";
              homepage = "https://github.com/coralogix/cx-cli";
              license = nixpkgs.lib.licenses.asl20;
              mainProgram = "cx";
            };
          };
        });

      devShells = forAllSystems (system:
        let pkgs = pkgsFor system; in {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.default ];
            packages = with pkgs; [ rustc cargo clippy rustfmt rust-analyzer ];
          };
        });
    };
}

{
  description = "OpenSpine kernel, shell, and development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs:
        let
          openspine = pkgs.rustPlatform.buildRustPackage {
            pname = "openspine";
            version = "0.1.0";

            src = self;
            cargoLock = {
              lockFile = ./Cargo.lock;
            };
            cargoBuildFlags = [ "--workspace" ];

            # A transitive dependency (native-tls via hyper-tls) links OpenSSL on
            # Linux; reqwest itself uses rustls. pkg-config finds it at build time.
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.openssl ];

            # `scripts/check.sh` is the test gate (it needs a built shell binary,
            # network access, and Docker). The Nix build only produces binaries.
            doCheck = false;

            meta = {
              description = "OpenSpine governed AI assistant kernel and shell";
              license = with pkgs.lib.licenses; [ mit asl20 ];
              mainProgram = "openspine";
            };
          };
        in
        {
          default = openspine;
          openspine = openspine;
        });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            rustc
            cargo
            clippy
            rustfmt
            nodejs_22
            docker-client
          ];

          shellHook = ''
            echo "openspine dev shell: rustc $(rustc --version), node $(node --version)"
            echo "openspec CLI: run 'npm install -g @openspec/cli' if not already on PATH"
          '';
        };
      });

      nixosModules.default = import ./nixos-module.nix { inherit self; };
    };
}

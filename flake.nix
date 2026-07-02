{
  description = "OpenSpine dev shell (convenience only — Docker + rustup remain the supported path)";

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
    };
}

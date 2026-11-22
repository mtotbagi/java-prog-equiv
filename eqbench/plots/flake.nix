{
  description = "Plots of eqbench results";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/f4a03c008967841f2a0b3cb7c436fc6fc31671f6";
    flake-utils.url = "github:numtide/flake-utils/b7a79d3fea1e214f6d668d521cecec0c01e26544";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = nixpkgs.legacyPackages.${system};
        python = pkgs.python3.withPackages (ps: [ps.matplotlib]);
        plot = pkgs.writeShellApplication {
          name = "eqbench-plot";
          runtimeInputs = [python];
          text = ''
            exec python3 -I ${./plot.py} "$@"
          '';
        };
      in {
        packages.default = plot;
        apps.default = flake-utils.lib.mkApp {drv = plot;};
      }
    );
}

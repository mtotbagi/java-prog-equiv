{
  description = "KLEE as an eqbench analysis tool for C programs";

  # KLEE only builds/runs on x86_64-linux, and its bitcode must be produced by
  # the same LLVM version nixpkgs builds it against (llvmPackages_19).
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils/b7a79d3fea1e214f6d668d521cecec0c01e26544";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
  }:
    flake-utils.lib.eachSystem ["x86_64-linux"] (
      system: let
        pkgs = import nixpkgs { inherit system; };
        llvm = pkgs.llvmPackages_19.llvm;
        clang = pkgs.llvmPackages_19.clang;

        adapter = pkgs.writeShellApplication {
          name = "klee-eqbench";
          runtimeInputs = [pkgs.klee clang llvm pkgs.python3];
          text = ''
            exec python3 -I ${./adapter.py} "$@"
          '';
        };
      in {
        packages = {
          default = adapter;
          klee = pkgs.klee;
        };
        apps.default = flake-utils.lib.mkApp {drv = adapter;};
        devShells.default = pkgs.mkShell {
          buildInputs = [pkgs.klee clang llvm pkgs.python3];
        };
      }
    );
}

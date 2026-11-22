{
  description = "Java Ranger (Symbolic PathFinder with path merging) as an eqbench analysis tool";

  inputs = {
    # nixpkgs dropped x86_64-darwin in 26.11, this one (PASDA's) still has it.
    nixpkgs.url = "github:NixOS/nixpkgs/ca77296380960cd497a765102eeb1356eb80fed0";
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

        # The SV-COMP 2027 build of Java Ranger and jpf-core, compiled for Java 8.
        javaRanger = pkgs.fetchzip {
          url = "https://zenodo.org/records/24322917/files/java-ranger.zip";
          hash = "sha256-MogDZhgnYBhlIxanWeUEa1k1wHXdZwdOA2pBwxf8bsg=";
        };

        # It ships Z3 for x86-64 only: Apple Silicon runs it with an x86-64 JDK
        # through Rosetta (requires `extra-platforms = x86_64-darwin`).
        jdkSystem =
          if system == "aarch64-darwin"
          then "x86_64-darwin"
          else system;
        jdk = nixpkgs.legacyPackages.${jdkSystem}.jdk8;
      in {
        packages = {
          default = javaRanger;
          java-ranger = javaRanger;
        };
      }
    );
}

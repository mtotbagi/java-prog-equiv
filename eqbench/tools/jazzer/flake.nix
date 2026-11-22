{
  description = "Jazzer (coverage-guided fuzzing for the JVM) as an eqbench analysis tool";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/f4a03c008967841f2a0b3cb7c436fc6fc31671f6";
    flake-utils.url = "github:numtide/flake-utils/b7a79d3fea1e214f6d668d521cecec0c01e26544";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
  }:
    flake-utils.lib.eachSystem ["aarch64-darwin" "x86_64-darwin"] (
      system: let
        pkgs = nixpkgs.legacyPackages.${system};
        jdk = pkgs.jdk17;

        # The release for macOS, a universal launcher and the agent jar.
        jazzer = pkgs.fetchzip {
          url = "https://github.com/CodeIntelligenceTesting/jazzer/releases/download/v0.30.0/jazzer-macos.tar.gz";
          hash = "sha256-/6JBMV4UEeho6iZAHbFxgnCBAOkMKLtpECi0Tr87hH4=";
          stripRoot = false;
        };

        # The fuzz target shared by every miter.
        target = pkgs.stdenvNoCC.mkDerivation {
          name = "eqbench-fuzz-target";
          src = ./EqbenchFuzzTarget.java;
          dontUnpack = true;
          nativeBuildInputs = [jdk];
          buildPhase = ''
            mkdir classes
            cp $src EqbenchFuzzTarget.java
            javac --release 8 -Xlint:-options -cp ${jazzer}/jazzer_standalone.jar -d classes EqbenchFuzzTarget.java
          '';
          installPhase = ''
            mkdir -p $out/share/java
            jar cf $out/share/java/eqbench-fuzz-target.jar -C classes .
          '';
        };

        adapter = pkgs.writeShellApplication {
          name = "jazzer-eqbench";
          runtimeInputs = [pkgs.python3];
          text = ''
            export JAVA_HOME=${jdk.home}
            export PATH=${jazzer}:$PATH
            exec python3 -I ${./adapter.py} --target ${target}/share/java/eqbench-fuzz-target.jar "$@"
          '';
        };
      in {
        packages = {
          default = adapter;
          inherit jazzer target;
        };
        apps.default = flake-utils.lib.mkApp {drv = adapter;};
      }
    );
}

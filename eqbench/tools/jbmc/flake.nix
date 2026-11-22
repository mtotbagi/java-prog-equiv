{
  description = "JBMC (CBMC's bounded model checker for Java bytecode) as an eqbench analysis tool";

  inputs = {
    # The one of the root flake, with cbmc 6.11.0.
    nixpkgs.url = "github:NixOS/nixpkgs/f4a03c008967841f2a0b3cb7c436fc6fc31671f6";
    flake-utils.url = "github:numtide/flake-utils/b7a79d3fea1e214f6d668d521cecec0c01e26544";
    # The jbmc/lib/java-models-library submodule of cbmc-6.11.0.
    java-models = {
      url = "github:diffblue/java-models-library/c7835345cf635279cf59f4a286c05538f8026bd8";
      flake = false;
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    java-models,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = nixpkgs.legacyPackages.${system};
        inherit (pkgs) lib;

        # Models of the Java library (core-models.jar) and the CProver API.
        models = pkgs.maven.buildMavenPackage {
          pname = "java-models-library";
          version = "0-unstable-${builtins.substring 0 8 java-models.lastModifiedDate}";
          src = java-models;
          mvnJdk = pkgs.jdk17;
          mvnHash = "sha256-OW7C6CbYntjdj2c3eGJNXcuo4UuH2zfid0zqhwo5BjU=";
          mvnParameters = "-Dmaven.test.skip=true -Dmaven.javadoc.skip=true";
          doCheck = false;
          installPhase = ''
            runHook preInstall
            mkdir -p $out/share/java
            cp target/core-models.jar target/cprover-api.jar $out/share/java/
            runHook postInstall
          '';
        };

        # nixpkgs builds cbmc without JBMC, whose CMake runs Maven for the
        # models: they are built above, and Maven replaced by a no-op.
        noMaven = pkgs.writeShellScript "mvn" "exit 0";
        jbmc = pkgs.cbmc.overrideAttrs (old: {
          pname = "jbmc";
          cmakeFlags =
            lib.remove "-DWITH_JBMC=OFF" old.cmakeFlags
            ++ ["-DWITH_JBMC=ON" "-DMAVEN_PROGRAM=${noMaven}"];
          # One `chmod` per file takes a quarter of an hour on macOS, and the
          # macOS 10.15 that CBMC targets is too old for nixpkgs' libc++.
          postPatch =
            builtins.replaceStrings ["find -exec chmod +w {} \\;"] ["chmod -R u+w ."] old.postPatch
            + ''
              substituteInPlace CMakeLists.txt --replace-fail "set(CMAKE_OSX_DEPLOYMENT_TARGET 10.15)" ""
            ''
            + ''
              cp -r ${java-models}/. jbmc/lib/java-models-library/
              chmod -R u+w jbmc/lib/java-models-library
              mkdir -p jbmc/lib/java-models-library/target
              cp ${models}/share/java/*.jar jbmc/lib/java-models-library/target/
            '';
          # Only JBMC, not the whole of CBMC.
          buildFlags = ["jbmc"];
          installPhase = ''
            runHook preInstall
            install -Dm755 bin/jbmc $out/bin/jbmc
            mkdir -p $out/share/java
            cp ${models}/share/java/*.jar $out/share/java/
            runHook postInstall
          '';
          postInstall = "";
          versionCheckProgram = "${placeholder "out"}/bin/jbmc";
        });

        adapter = pkgs.writeShellApplication {
          name = "jbmc-eqbench";
          runtimeInputs = [jbmc pkgs.python3];
          text = ''
            exec python3 -I ${./adapter.py} --models ${jbmc}/share/java/core-models.jar "$@"
          '';
        };
      in {
        packages = {
          default = adapter;
          inherit jbmc models;
        };
        apps.default = flake-utils.lib.mkApp {drv = adapter;};
      }
    );
}

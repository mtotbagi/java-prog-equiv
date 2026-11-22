{
  description = "PASDA (Glock, Pichler, Pinzger, JSS 2024) as an eqbench analysis tool";

  # deps.json must match the build.gradle of this exact PASDA commit. PASDA
  # builds with Gradle 7, removed from nixpkgs in 2026-01, hence an older
  # nixpkgs (the one jvm2json uses).
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/ca77296380960cd497a765102eeb1356eb80fed0";
    flake-utils.url = "github:numtide/flake-utils/b7a79d3fea1e214f6d668d521cecec0c01e26544";
    pasda = {
      url = "github:glockyco/PASDA/8e20d6c4f2d942759aba256390dd2f06c91b3898";
      flake = false;
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    pasda,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = nixpkgs.legacyPackages.${system};
        # JPF and Symbolic PathFinder only run on Java 8.
        jdk = pkgs.jdk8;
        gradle = pkgs.gradle_7.override {java = jdk;};

        # The two runners of PASDA (instrumentation and differencing) and the
        # files they read from their working directory at run time.
        pasdaRunners = pkgs.stdenv.mkDerivation (finalAttrs: {
          pname = "pasda";
          version = "0-unstable-${builtins.substring 0 8 pasda.lastModifiedDate}";
          src = "${pasda}/Implementation";

          # git: JPF writes its revision in build.properties (empty here).
          nativeBuildInputs = [gradle pkgs.git];
          # Regenerate from the root of the repository with
          # `nix build ./eqbench/tools/pasda#pasda.mitmCache.updateScript && ./result`.
          mitmCache = gradle.fetchDeps {
            pkg = finalAttrs.finalPackage;
            data = ./deps.json;
          };
          gradleUpdateTask = "nixDownloadDeps shadowJar";

          # On macOS, Gradle does not trust the certificate of the mitm-cache
          # proxy, so the recorded files are used as local Maven repositories
          # instead (the update script keeps its own init script).
          gradleInitScript = pkgs.writeText "init-local-repositories.gradle" ''
            gradle.projectsLoaded {
              rootProject.allprojects {
                tasks.withType(AbstractArchiveTask) {
                  preserveFileTimestamps = false
                  reproducibleFileOrder = true
                }
              }
            }

            def cache = new File(System.getenv("mitmCache"), "https")
            def local = { URI uri -> new File(cache, uri.host + uri.path).toURI() }
            def redirect = { repo ->
              if (repo instanceof MavenArtifactRepository && repo.url.scheme == "https") {
                repo.url = local(repo.url)
              }
            }

            settingsEvaluated { settings ->
              settings.pluginManagement.repositories {
                maven { url = local(new URI("https://plugins.gradle.org/m2")) }
              }
            }
            allprojects {
              buildscript.repositories.configureEach(redirect)
              repositories.configureEach(redirect)
            }
          '';
          gradleFlags = ["--offline"];

          buildPhase = ''
            runHook preBuild
            gradle -PmainClass=Runner.Runner shadowJar
            gradle -PmainClass=differencing.DifferencingRunner shadowJar
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            share=$out/share/pasda
            mkdir -p $share/build $share/src/main/resources $share/analysis $share/target
            cp -r build/libs $share/build/
            cp -r jpf-git site.properties $share/
            cp -r src/main/resources/templates $share/src/main/resources/
            cp analysis/create-tables.sql $share/analysis/
            cp -r target/classes $share/target/
            runHook postInstall
          '';
        });

        adapter = pkgs.writeShellApplication {
          name = "pasda-eqbench";
          runtimeInputs = [jdk pkgs.z3 pkgs.python3];
          text = ''
            exec python3 -I ${./adapter.py} --pasda ${pasdaRunners}/share/pasda "$@"
          '';
        };
      in {
        packages = {
          default = adapter;
          pasda = pasdaRunners;
        };
        apps.default = flake-utils.lib.mkApp {drv = adapter;};
      }
    );
}

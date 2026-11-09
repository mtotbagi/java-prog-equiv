{
  description = "nadar — explorateur d'un corpus impressionniste";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    # Not following our nixpkgs: its own pin is the one with binary caches.
    jvm2json.url = "github:kalhauge/jvm2json";
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
    jvm2json,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = ["rust-src" "rust-analyzer" "clippy" "rustfmt"];
        };

        # jvm2json is only packaged for x86_64, Apple Silicon runs it through Rosetta
        # (requires `extra-platforms = x86_64-darwin` in nix.conf).
        jvm2jsonSystem =
          if system == "aarch64-darwin"
          then "x86_64-darwin"
          else system;
        jvm2jsonPkg = jvm2json.packages.${jvm2jsonSystem}.default or null;
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            cargo-machete
            cargo-nextest

            just
            alejandra
            git
            curl
            jq
            tree

            jdk21
          ]
          ++ pkgs.lib.optional (jvm2jsonPkg != null) jvm2jsonPkg;

          shellHook = ''
            git config core.hooksPath .githooks

            echo "  rust   :$(rustc --version 2>/dev/null | cut -d' ' -f2 || echo 'missing')"
            echo "  javac  :$(javac -version 2>&1 | cut -d' ' -f2 || echo 'missing')"
            echo "  jvm2json:$(command -v jvm2json >/dev/null && echo 'ok' || echo 'missing')"
            echo ""
            echo "Run 'just' for available commands."
          '';

          RUST_BACKTRACE = "1";
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };
      }
    );
}

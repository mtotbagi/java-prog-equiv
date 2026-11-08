{
  description = "nadar — explorateur d'un corpus impressionniste";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
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
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            cargo-machete
            cargo-nextest
            z3

            just
            alejandra
            git
            curl
            jq
            tree
          ];

          shellHook = ''
            git config core.hooksPath .githooks

            echo "  rust   :$(rustc --version 2>/dev/null | cut -d' ' -f2 || echo 'missing')"
            echo ""
            echo "Run 'just' for available commands."
          '';

          RUST_BACKTRACE = "1";
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };
      }
    );
}

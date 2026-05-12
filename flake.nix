{
  description = "object-storage-proxy - Rust + Python (maturin) development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
        };

        pythonEnv = pkgs.python312.withPackages (ps: with ps; [
          pip
          python-dotenv
          maturin
          pytest
          requests
        ]);
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            # Rust
            rustToolchain

            # Python
            pythonEnv
            pkgs.uv

            # Build dependencies for aws-lc-rs / ring / rustls
            pkgs.cmake
            pkgs.perl
            pkgs.pkg-config
            pkgs.openssl
            pkgs.zlib

            # Tooling
            pkgs.maturin
            pkgs.cargo-watch
            pkgs.cargo-nextest
          ];

          env = {
            RUST_LOG = "debug";
            RUST_BACKTRACE = "1";
            # Ensure openssl is found by build scripts
            PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
            OPENSSL_DIR = "${pkgs.openssl.dev}";
            OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
            OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
          };

          shellHook = ''
            echo "🦀 Rust $(rustc --version)"
            echo "🐍 Python $(python --version)"
            echo ""
            echo "Commands:"
            echo "  cargo build          - build the Rust library"
            echo "  cargo test           - run Rust tests"
            echo "  maturin develop      - build and install Python extension"
            echo "  maturin build        - build Python wheel"
            echo "  uv run pytest        - run Python tests"
          '';
        };

        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "object-storage-proxy";
          version = "0.4.3";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [
            pkgs.cmake
            pkgs.perl
            pkgs.pkg-config
          ];

          buildInputs = [
            pkgs.openssl
            pkgs.zlib
          ];
        };
      }
    );
}

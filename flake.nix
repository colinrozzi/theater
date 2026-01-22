{
  description = "Theater - A WebAssembly actor runtime for reproducible, isolated, and observable programs";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-utils.url = "github:numtide/flake-utils";

    crane.url = "github:ipetkov/crane";

    # TODO: When ready to distribute, uncomment this and remove the local path assumption:
    # composite = {
    #   url = "github:colinrozzi/composite";  # or your actual repo
    #   inputs.nixpkgs.follows = "nixpkgs";
    #   inputs.rust-overlay.follows = "rust-overlay";
    # };
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Rust toolchain matching workspace rust-version
        rustToolchain = pkgs.rust-bin.stable."1.83.0".default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "clippy"
            "rustfmt"
          ];
          targets = [ "wasm32-unknown-unknown" "wasm32-wasip1" ];
        };

        # Setup crane with our toolchain
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Source filtering - include Cargo files, Rust sources, and WIT files
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            (pkgs.lib.hasSuffix ".wit" path) ||
            (pkgs.lib.hasSuffix ".toml" path) ||
            (craneLib.filterCargoSources path type);
        };

        # Common build arguments
        commonArgs = {
          inherit src;
          strictDeps = true;

          buildInputs = with pkgs; [
            openssl
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            pkgs.libiconv
          ];

          nativeBuildInputs = with pkgs; [
            pkg-config
            llvmPackages.llvm
            llvmPackages.clang
            cmake
          ];

          # Required for wasmtime build
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        };

        # Build dependencies only (for caching)
        cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
          pname = "theater-deps";
          version = "0.2.1";
          # Note: This requires ../composite to exist for dependency resolution
          # In CI, you may need to set up the composite dependency first
        });

        # Build the workspace
        theaterPackage = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "theater";
          version = "0.2.1";

          # Build all workspace members
          cargoExtraArgs = "--workspace";
        });

        # Clippy check
        theaterClippy = craneLib.cargoClippy (commonArgs // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--workspace -- --deny warnings";
        });

        # Format check
        theaterFmt = craneLib.cargoFmt {
          inherit src;
        };

        # Tests
        theaterTests = craneLib.cargoTest (commonArgs // {
          inherit cargoArtifacts;
          cargoTestExtraArgs = "--workspace";
        });

        # Doc check
        theaterDoc = craneLib.cargoDoc (commonArgs // {
          inherit cargoArtifacts;
          cargoDocExtraArgs = "--workspace --no-deps";
        });

      in
      {
        # Checks run by `nix flake check`
        checks = {
          inherit theaterPackage theaterClippy theaterFmt theaterTests theaterDoc;
        };

        # Packages
        packages = {
          default = theaterPackage;
          theater = theaterPackage;
        };

        # Development shell
        devShells.default = craneLib.devShell {
          # Include checks so their dependencies are available
          checks = self.checks.${system};

          # Additional development tools
          packages = with pkgs; [
            # Rust tools
            cargo-watch
            cargo-edit
            cargo-audit
            cargo-expand
            cargo-udeps
            cargo-nextest

            # WASM tooling
            wasmtime
            wasm-tools
            wit-bindgen

            # Build dependencies
            pkg-config
            openssl
            llvmPackages.llvm
            llvmPackages.clang
            cmake

            # Documentation
            mdbook

            # Debugging
            lldb
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            pkgs.libiconv
          ];

          # Environment variables
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          RUST_BACKTRACE = "1";

          shellHook = ''
            echo ""
            echo "========================================"
            echo "  Theater Development Environment"
            echo "========================================"
            echo ""
            echo "Rust:      $(rustc --version)"
            echo "Cargo:     $(cargo --version)"
            echo "wasmtime:  $(wasmtime --version)"
            echo ""
            echo "Available WASM targets:"
            echo "  - wasm32-unknown-unknown"
            echo "  - wasm32-wasip1"
            echo ""
            echo "Commands:"
            echo "  cargo build              Build the workspace"
            echo "  cargo test               Run tests"
            echo "  cargo clippy             Run linter"
            echo "  cargo fmt                Format code"
            echo "  cargo watch -x check     Watch for changes"
            echo "  nix flake check          Run all checks"
            echo ""
            echo "Note: Requires ../composite to be present"
            echo "========================================"
            echo ""
          '';
        };
      });
}

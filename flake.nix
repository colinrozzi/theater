{
  description = "Theater - A WebAssembly Component Model Runtime and Actor System";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    
    # Add crane for better Rust handling
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Get a specific rust toolchain with component-model support
        rustToolchain = pkgs.rust-bin.stable."1.75.0".default.override {
          extensions = [ 
            "rust-src"
            "rust-analyzer"
            "clippy"
            "rustfmt"
          ];
        };

        # Setup crane lib
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Common arguments can be set here to avoid repetition
        commonArgs = {
          src = craneLib.cleanCargoSource (craneLib.path ./.);
          buildInputs = with pkgs; [
            pkg-config
            openssl
          ];
        };

        # Build the cargo artifacts
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      in
      {
        checks = {
          # Build the crate as part of `nix flake check`
          build = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
          });
          
          # Run clippy (and deny all warnings) on the crate source,
          # again, resuing the dependency artifacts from above.
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "-- --deny warnings";
          });

          # Check formatting
          fmt = craneLib.cargoFmt {
            src = ./.;
          };
        };

        packages.default = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });

        apps.default = flake-utils.lib.mkApp {
          drv = self.packages.${system}.default;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = builtins.attrValues self.checks.${system};

          # Additional development tools
          buildInputs = with pkgs; [
            rustToolchain
            pkg-config
            openssl
            curl

            # Development tools
            cargo-watch
            cargo-edit
            cargo-audit
            cargo-expand
            cargo-udeps
            
            # Helpful for documentation
            mdbook
          ];

          # Environment variables
          shellHook = ''
            export RUST_SRC_PATH="${rustToolchain}/lib/rustlib/src/rust/library"
            
            # Print some helpful information
            echo "🎭 Theater Development Environment"
            echo "Rust toolchain: $(rustc --version)"
            echo "Cargo: $(cargo --version)"
            echo ""
            echo "Available commands:"
            echo "  cargo build     - Build the project"
            echo "  cargo test      - Run tests"
            echo "  cargo clippy    - Run linter"
            echo "  cargo fmt       - Format code"
            echo "  cargo watch     - Watch for changes"
            echo ""
          '';
        };
      });
}

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

    # Pack runtime dependency
    pack = {
      url = "github:colinrozzi/pack/v0.2.0";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, crane, pack, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Rust toolchain matching workspace rust-version
        rustToolchain = pkgs.rust-bin.stable."1.85.0".default.override {
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

        # Source filtering - include Cargo files, Rust sources, WIT files, and Pact files
        theaterSrc = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            (pkgs.lib.hasSuffix ".wit" path) ||
            (pkgs.lib.hasSuffix ".pact" path) ||
            (pkgs.lib.hasSuffix ".toml" path) ||
            (craneLib.filterCargoSources path type);
        };

        # Use theater source directly, pack will be added during build
        src = theaterSrc;

        # Common build arguments
        commonArgs = {
          inherit src;
          strictDeps = true;

          # Set up pack as sibling directory so ../pack paths resolve
          postUnpack = ''
            cp -rL ${pack} pack
          '';

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
          # Note: pack is added via postUnpack from the flake input
        });

        # Build the workspace
        theaterPackage = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "theater";
          version = "0.2.1";

          # Build all workspace members
          cargoExtraArgs = "--workspace";

          # Skip tests - they require pre-built WASM test actors
          doCheck = false;
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

          # Script to build all WASM test actors
          build-test-actors = pkgs.writeShellScriptBin "build-test-actors" ''
            set -e
            echo "Building test actors..."
            for dir in test-actors/*/; do
              if [ -f "$dir/Cargo.toml" ]; then
                name=$(basename "$dir")
                echo "  Building $name..."
                (cd "$dir" && cargo build --target wasm32-unknown-unknown --release 2>&1 | tail -1)
              fi
            done
            echo "All test actors built."
          '';

          # Script to build test actors then run tests
          test = pkgs.writeShellScriptBin "theater-test" ''
            set -e
            echo "=== Building test actors ==="
            nix run .#build-test-actors
            echo ""
            echo "=== Running tests ==="
            cargo test --lib "''${@}"
            echo ""
            echo "=== Running integration tests ==="
            cargo test --test golden_chain_test --test composite_integration_test --test shutdown_timing_test "''${@}"
          '';

          # Update pack dependency to a new version tag
          update-pack = pkgs.writeShellScriptBin "update-pack" ''
            set -e
            VERSION="''${1:?Usage: nix run .#update-pack <version> (e.g. v0.2.1)}"

            echo "Updating pack to $VERSION..."

            # Update Cargo.toml git tags
            ${pkgs.gnused}/bin/sed -i \
              "s|colinrozzi/pack\.git\", tag = \"[^\"]*\"|colinrozzi/pack.git\", tag = \"$VERSION\"|g" \
              Cargo.toml
            echo "  Updated Cargo.toml"

            # Update flake.nix URL (only the inputs.pack.url line)
            ${pkgs.python3}/bin/python3 -c "
import re, sys
with open('flake.nix', 'r') as f:
    content = f.read()
content = re.sub(
    r'(url = \"github:colinrozzi/pack)/[^\"]*',
    r'\1/' + sys.argv[1],
    content,
    count=1
)
with open('flake.nix', 'w') as f:
    f.write(content)
            " "$VERSION"
            echo "  Updated flake.nix"

            # Update flake lock
            nix flake update pack
            echo "  Updated flake.lock"

            echo ""
            echo "Pack updated to $VERSION. Changes:"
            git diff --stat
          '';

          # Create a PR from the current jj revision
          pr = pkgs.writeShellScriptBin "theater-pr" ''
            set -e
            DESCRIPTION=$(jj log -r @ --no-graph -T 'description' 2>/dev/null)
            if [ -z "$DESCRIPTION" ] || [ "$DESCRIPTION" = "(no description set)" ]; then
              echo "Error: Current revision has no description. Run: jj describe -m 'your change'"
              exit 1
            fi

            # Extract first line as branch name
            TITLE=$(echo "$DESCRIPTION" | head -1)
            BRANCH=$(echo "$TITLE" | tr '[:upper:]' '[:lower:]' | tr ' ' '-' | tr -cd 'a-z0-9-' | head -c 50)

            echo "Creating PR: $TITLE"
            echo "Branch: $BRANCH"
            echo ""

            # Create/move bookmark and push
            jj bookmark create "$BRANCH" -r @ 2>/dev/null || jj bookmark set "$BRANCH" -r @
            jj git push --bookmark "$BRANCH"

            # Create PR via gh
            ${pkgs.gh}/bin/gh pr create \
              --title "$TITLE" \
              --body "$DESCRIPTION" \
              --base main \
              --head "$BRANCH"
          '';
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

            # Debugging
            lldb

            # GitHub CLI
            gh
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
            echo "  cargo build                    Build the workspace"
            echo "  cargo test                     Run tests"
            echo "  cargo clippy                   Run linter"
            echo "  cargo fmt                      Format code"
            echo "  cargo watch -x check           Watch for changes"
            echo "  nix flake check                Run all checks"
            echo "  nix run .#build-test-actors    Build WASM test actors"
            echo "  nix run .#test                 Build actors + run all tests"
            echo ""
            echo "Note: Requires ../pack to be present"
            echo "========================================"
            echo ""
          '';
        };
      });
}

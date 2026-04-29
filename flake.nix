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
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Rust toolchain matching workspace rust-version
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
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

        src = theaterSrc;

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
          version = "0.3.6";
        });

        # Build the workspace
        theaterPackage = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "theater";
          version = "0.3.6";

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
            cargo test -p theater --test golden_chain_test --test composite_integration_test "''${@}"
            echo ""
            echo "=== Running handler integration tests ==="
            cargo test -p theater-tests "''${@}"
          '';

          # Dry-run publish all crates in dependency order
          publish-dry-run = pkgs.writeShellScriptBin "theater-publish-dry-run" ''
            set -e
            CRATES=(
              theater-chain
              theater
              theater-handler-assembler
              theater-handler-loop
              theater-handler-message-server
              theater-handler-rpc
              theater-handler-runtime
              theater-handler-store
              theater-handler-supervisor
              theater-handler-tcp
              theater-handler-terminal
              theater-handler-timer
              theater-server
              theater-cli
              theater-client
              theater-server-cli
            )
            for crate in "''${CRATES[@]}"; do
              echo "=== $crate ==="
              if ! cargo publish -p "$crate" --dry-run 2>&1 | tail -5; then
                echo "FAILED: $crate"
                exit 1
              fi
              echo ""
            done
            echo "All crates pass dry-run!"
          '';

          # Release: bump version and create a PR
          # After merge, CI publishes to crates.io and creates the git tag
          release = pkgs.writeShellScriptBin "theater-release" ''
            set -e

            BUMP="''${1:-patch}"
            CURRENT=$(${pkgs.gnugrep}/bin/grep -m1 '^version = ' Cargo.toml | ${pkgs.gnused}/bin/sed 's/version = "\(.*\)"/\1/')

            IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

            case "$BUMP" in
              patch) PATCH=$((PATCH + 1)) ;;
              minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
              major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
              [0-9]*.[0-9]*.[0-9]*) IFS='.' read -r MAJOR MINOR PATCH <<< "$BUMP" ;;
              *) echo "Usage: nix run .#release -- [patch|minor|major|X.Y.Z]"; exit 1 ;;
            esac

            NEW="$MAJOR.$MINOR.$PATCH"
            echo "Bumping $CURRENT -> $NEW"

            # Update workspace version
            ${pkgs.gnused}/bin/sed -i "0,/^version = \"$CURRENT\"/s//version = \"$NEW\"/" Cargo.toml

            # Update workspace dependency versions
            ${pkgs.gnused}/bin/sed -i "s/version = \"$CURRENT\", path/version = \"$NEW\", path/g" Cargo.toml

            # Update flake.nix version
            ${pkgs.gnused}/bin/sed -i "s/version = \"$CURRENT\"/version = \"$NEW\"/g" flake.nix

            # Update Cargo.lock
            cargo update --workspace 2>/dev/null || true

            echo ""
            echo "Updated to v$NEW"
            echo ""

            BRANCH="release-v$NEW"

            if command -v jj &>/dev/null; then
              jj describe -m "release v$NEW"
              jj bookmark create "$BRANCH" -r @ 2>/dev/null || jj bookmark set "$BRANCH" -r @
              jj git push --bookmark "$BRANCH" --allow-new
            else
              git checkout -b "$BRANCH"
              git add -A
              git commit -m "release v$NEW"
              git push -u origin "$BRANCH"
            fi

            ${pkgs.gh}/bin/gh pr create \
              --title "release v$NEW" \
              --body "Bump version to v$NEW. Merging will publish to crates.io and create a GitHub release." \
              --base main \
              --head "$BRANCH"

            echo ""
            echo "PR created. Merge to publish v$NEW to crates.io."
          '';

          # Create a PR from the current jj revision
          pr = pkgs.writeShellScriptBin "theater-pr" ''
            set -e
            DESCRIPTION=$(jj log -r @ --no-graph -T 'description' 2>/dev/null)
            if [ -z "$DESCRIPTION" ] || [ "$DESCRIPTION" = "(no description set)" ]; then
              echo "Error: Current revision has no description. Run: jj describe -m 'your change'"
              exit 1
            fi

            TITLE=$(echo "$DESCRIPTION" | head -1)
            BRANCH=$(echo "$TITLE" | tr '[:upper:]' '[:lower:]' | tr ' ' '-' | tr -cd 'a-z0-9-' | ${pkgs.gnused}/bin/sed 's/--*/-/g; s/^-//; s/-$//' | head -c 50)

            echo "Creating PR: $TITLE"
            echo "Branch: $BRANCH"
            echo ""

            jj bookmark create "$BRANCH" -r @ 2>/dev/null || jj bookmark set "$BRANCH" -r @
            jj git push --bookmark "$BRANCH" --allow-new

            ${pkgs.gh}/bin/gh pr create \
              --title "$TITLE" \
              --body "$DESCRIPTION" \
              --base main \
              --head "$BRANCH"
          '';
        };

        # Development shell
        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          packages = with pkgs; [
            cargo-watch
            cargo-edit
            cargo-audit
            cargo-expand
            cargo-udeps
            cargo-nextest

            wasmtime
            wasm-tools
            wit-bindgen

            pkg-config
            openssl
            llvmPackages.llvm
            llvmPackages.clang
            cmake

            lldb

            gh
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            pkgs.libiconv
          ];

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          RUST_BACKTRACE = "1";

          shellHook = ''
            echo ""
            echo "Theater Development Environment"
            echo "Rust: $(rustc --version)"
            echo ""
            echo "Commands:"
            echo "  cargo build                    Build the workspace"
            echo "  cargo test                     Run tests"
            echo "  cargo clippy                   Run linter"
            echo "  nix run .#build-test-actors    Build WASM test actors"
            echo "  nix run .#test                 Build actors + run all tests"
            echo "  nix run .#pr                   Create PR from jj revision"
            echo "  nix run .#release -- patch     Bump version, create release PR"
            echo "  nix run .#publish-dry-run      Verify all crates can publish"
            echo ""
          '';
        };
      });
}

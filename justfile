set positional-arguments

# Curated repo orientation
help:
    #!/usr/bin/env bash
    cat <<'EOF'
    nori cli — AI-powered coding assistant (Rust TUI)

    Primary entrypoints:
      just dev [-- args]          Run the nori binary
      just dev exec [-- args]     Run nori in headless mode

    Standard targets:
      just dev [-- args]
      just test [scope] [-- args]
      just doctor

    Test scopes:
      just test                   Workspace tests (default)
      just test tui               TUI crate tests
      just test acp               ACP backend tests
      just test core              Core library tests
      just test protocol          Protocol crate tests
      just test e2e               E2E tests (builds binary first)
      just test all               Full workspace with all features

    Repo-specific targets:
      just fmt                    Format code
      just fix [-- args]          Auto-fix clippy lints
      just clippy                 Run clippy
      just nextest                Run tests with cargo-nextest
      just install                Fetch toolchain and dependencies
    EOF

# Run the nori binary (primary dev entrypoint)
dev *args:
    cd codex-rs && cargo run --bin nori -- "$@"

# Run tests for a given scope (default: workspace)
test *args:
    #!/usr/bin/env bash
    set -euo pipefail
    scope="${1:-}"
    shift 2>/dev/null || true
    case "$scope" in
      tui)
        cd codex-rs && cargo test -p nori-tui "$@"
        ;;
      acp)
        cd codex-rs && cargo test -p codex-acp "$@"
        ;;
      core)
        cd codex-rs && cargo test -p codex-core "$@"
        ;;
      protocol)
        cd codex-rs && cargo test -p codex-protocol "$@"
        ;;
      e2e)
        cd codex-rs && cargo build --bin nori && cargo test -p tui-pty-e2e "$@"
        ;;
      all)
        cd codex-rs && cargo test --all-features "$@"
        ;;
      *)
        cd codex-rs && cargo test ${scope:+"$scope"} "$@"
        ;;
    esac

# Verify local toolchain and prerequisites
doctor:
    #!/usr/bin/env bash
    set -euo pipefail
    required_ok=0
    required_total=0
    ok=0
    total=0

    check_required() {
      local name="$1"
      local cmd="$2"
      total=$((total + 1))
      required_total=$((required_total + 1))
      if command -v "$cmd" > /dev/null 2>&1; then
        version=$("$cmd" --version 2>/dev/null | head -1)
        echo "  ok   $name ($version)"
        ok=$((ok + 1))
        required_ok=$((required_ok + 1))
      else
        echo "  MISS $name (command '$cmd' not found)"
      fi
    }

    check_optional() {
      local name="$1"
      local cmd="$2"
      total=$((total + 1))
      if command -v "$cmd" > /dev/null 2>&1; then
        version=$("$cmd" --version 2>/dev/null | head -1)
        echo "  ok   $name ($version)"
        ok=$((ok + 1))
      else
        echo "  MISS $name (command '$cmd' not found)"
      fi
    }

    echo "nori cli — doctor"
    echo ""
    echo "Required:"
    check_required "cargo" "cargo"
    check_required "rustup" "rustup"
    check_required "just" "just"

    echo ""
    echo "Recommended:"
    check_optional "cargo-nextest" "cargo-nextest"
    check_optional "cargo-insta" "cargo-insta"

    echo ""
    echo "Toolchain:"
    total=$((total + 1))
    if [ -f codex-rs/rust-toolchain.toml ]; then
      expected=$(grep 'channel' codex-rs/rust-toolchain.toml | head -1 | sed 's/.*= *"\(.*\)"/\1/')
      active=$(rustup show active-toolchain 2>/dev/null | awk '{print $1}')
      if echo "$active" | grep -q "$expected"; then
        echo "  ok   rust toolchain ($active)"
        ok=$((ok + 1))
      else
        echo "  WARN rust toolchain (expected $expected, active $active)"
      fi
    else
      echo "  SKIP rust-toolchain.toml not found"
    fi

    echo ""
    echo "$ok/$total checks passed."
    if [ "$required_ok" -lt "$required_total" ]; then
      echo ""
      echo "Install missing required tools before working in this repo."
      exit 1
    fi

# Forward existing codex-rs targets

# Format code
fmt:
    cd codex-rs && cargo fmt -- --config imports_granularity=Item

# Auto-fix clippy lints
fix *args:
    cd codex-rs && cargo clippy --fix --all-features --tests --allow-dirty "$@"

# Run clippy
clippy:
    cd codex-rs && cargo clippy --all-features --tests

# Fetch toolchain and dependencies
install:
    cd codex-rs && rustup show active-toolchain && cargo fetch

# Run tests with cargo-nextest
nextest:
    cd codex-rs && cargo nextest run --no-fail-fast

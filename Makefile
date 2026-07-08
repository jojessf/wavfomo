# Convenience targets. The build itself is plain `cargo build --release`.

.PHONY: licenses check-licenses deny

# Regenerate the bundled third-party license attribution from the resolved
# dependency tree. Run after adding/updating/removing a dependency.
# Requires: cargo install cargo-about --features cli
licenses:
	cargo about generate about.hbs > THIRD-PARTY-LICENSES.html

# Fail if any dependency's license falls outside the accepted set in about.toml
# (a lightweight policy gate; `generate` errors on unaccepted licenses too).
check-licenses:
	cargo about generate about.hbs > /dev/null

# Enforce the dependency license policy in deny.toml (the CI guardrail).
# Requires: cargo install cargo-deny --locked
deny:
	cargo deny check licenses

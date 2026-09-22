# Contributing to TeeLens

TeeLens is deliberately advisory: contributions must not mutate Kubernetes resources,
collect attestation evidence, or expose workload secrets by default.

Before opening a pull request, run:

```bash
cargo fmt --check
cargo test --locked
```

Keep schema changes additive within a version. New execution environments or devices
must include an eligible and an ineligible fixture so that rejection reasons remain
deterministic.

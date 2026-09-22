# TeeLens

TeeLens is a local preflight checker for confidential Kubernetes workloads. It combines a Pod manifest, Kata configuration, and an explicit node-capabilities document to explain confidential-execution and migration readiness without collecting attestation evidence, secrets, guest memory, or workload environment values.

## Quick start

```bash
cargo run -- check examples/pod.yaml \
  --runtime-class kata-qemu-coco \
  --node-capabilities examples/node-capabilities.json \
  --kata-config examples/configuration-qemu.toml
```

`NodeCapabilities` is deliberately an input contract in v1. The future collector must produce this shape from node-local checks; TeeLens does not probe a host implicitly.

## Heterogeneous placement planning

`plan` evaluates accelerator requests in the Pod against a node inventory and explains every rejection:

```bash
cargo run -- plan examples/pod.yaml --runtime-class kata-qemu-coco \
  --node-inventory examples/node-inventory.json \
  --kata-config examples/configuration-qemu.toml
```

It is advisory by design: TeeLens does not replace Kubernetes scheduling.

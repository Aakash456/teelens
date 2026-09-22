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

## Dynamic Resource Allocation (DRA)

`dra` turns the accelerator requests in a Pod into a Kubernetes
`resource.k8s.io/v1` `ResourceClaimTemplate`:

```bash
cargo run -- dra examples/pod.yaml \
  --device-class nvidia.com/gpu=production-gpu
```

The DeviceClass mapping is required explicitly: DeviceClasses and their driver
selectors are controlled by each cluster. Review the output and connect the
template to the Pod's `resourceClaims` before applying it. TeeLens does not
create DeviceClasses, infer driver selectors, use `adminAccess`, or apply
objects to a cluster.

## kubectl plugin

Install the read-only plugin with `cargo install --path . --bin kubectl-teelens`.
Then run the same commands as `kubectl teelens plan ...`. See
[plugin documentation](docs/kubectl-plugin.md).

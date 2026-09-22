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

For a reviewable two-document manifest (claim template and patched Pod), use:

```bash
cargo run -- dra-bundle examples/pod.yaml \
  --device-class nvidia.com/gpu=production-gpu \
  --container app
```

Every container requesting an accelerator must be listed explicitly. The
patched Pod removes the converted extended-resource request and grants only
those named containers access to the generated claim.

## v0.2 trust inputs

Collect a non-secret local capability document (KVM, detected TEE indicators,
architecture, and NUMA nodes):

```bash
cargo run -- collect --name node-a > node-capabilities.json
```

The collector deliberately does not collect attestation evidence, private
keys, measurements, GPU inventory, or infer installed VMM/WASM runtimes. Add
those through a reviewed node agent or CI inventory source.

Validate a versioned trust policy and make a conservative migration preflight:

```bash
cargo run -- policy-check --policy examples/trust-policy.yaml \
  --node-capabilities examples/node-capabilities.json

cargo run -- migrate-check --source examples/node-capabilities.json \
  --destination examples/node-capabilities.json \
  --runtime-class kata-qemu-coco --kata-config examples/configuration-qemu.toml
```

## kubectl plugin

Install the read-only plugin with `cargo install --path . --bin kubectl-teelens`.
Then run the same commands as `kubectl teelens plan ...`. See
[plugin documentation](docs/kubectl-plugin.md).

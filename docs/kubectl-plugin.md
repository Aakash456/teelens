# kubectl plugin

TeeLens ships a `kubectl-teelens` binary, which Kubernetes discovers as the
`kubectl teelens` plugin. It is intentionally read-only: v0.1 does not load a
kubeconfig, query the API server, mutate workloads, or require RBAC.

Install from a local checkout:

```bash
cargo install --path . --bin kubectl-teelens
```

Then use the familiar kubectl plugin form:

```bash
kubectl teelens plan examples/pod.yaml \
  --runtime-class kata-qemu-coco \
  --node-inventory examples/node-inventory.json \
  --kata-config examples/configuration-qemu.toml
```

The next integration milestone is an explicitly opt-in cluster inventory reader.
It will require only read access to Nodes, RuntimeClasses, ResourceSlices, and
ResourceClaims, and will retain `--offline` as the default mode.

# Dynamic Resource Allocation compiler

TeeLens compiles accelerator resource requests from a Pod into a standard
Kubernetes `resource.k8s.io/v1` `ResourceClaimTemplate`.

```bash
cargo run -- dra examples/pod.yaml \
  --device-class nvidia.com/gpu=production-gpu \
  --name protected-accelerators
```

Each `--device-class` value has the form `RESOURCE=DEVICE_CLASS`. Repeat the
flag for each accelerator resource requested by the Pod.

The compiler uses `exactly.allocationMode: ExactCount` and sets the requested
count from the Pod. It deliberately does not emit CEL selectors because driver
attribute schemas are cluster-specific. The target DeviceClass must already
exist and be managed by the cluster administrator or DRA driver.

The generated template is not applied automatically. Reference it from the
Pod's `spec.resourceClaims` and grant containers access via
`resources.claims` according to the Kubernetes DRA documentation. Do not add
`adminAccess` for normal workloads.

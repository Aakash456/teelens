# Five-minute TeeLens demo

```bash
cargo run -- plan examples/pod.yaml \
  --runtime-class kata-qemu-coco \
  --node-inventory examples/node-inventory.json \
  --kata-config examples/configuration-qemu.toml \
  --output json

cargo run -- compile examples/pod.yaml \
  --runtime-class kata-qemu-coco \
  --node-inventory examples/node-inventory.json \
  --kata-config examples/configuration-qemu.toml
```

Expected result: `snp-gpu-1` is eligible while `standard-1` is rejected because it
lacks a verified confidential-computing path. The compiled manifest records the QEMU
target, GPU requirement, attestation-policy reference, and conservative migration
posture.

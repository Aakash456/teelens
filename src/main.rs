use std::{collections::BTreeMap, fs, path::PathBuf, process::Command as ProcessCommand};

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Parser, Debug)]
#[command(about = "Preflight checks for confidential Kubernetes workloads")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Check a Pod against a Kata configuration and node capabilities.
    Check {
        /// Kubernetes Pod manifest (YAML or JSON).
        pod: PathBuf,
        /// RuntimeClass name used for this workload.
        #[arg(long)]
        runtime_class: String,
        /// TeeLens NodeCapabilities JSON document.
        #[arg(long)]
        node_capabilities: PathBuf,
        /// Kata runtime configuration TOML.
        #[arg(long)]
        kata_config: PathBuf,
        #[arg(long, value_enum, default_value_t = Output::Text)]
        output: Output,
    },
    /// Plan placement across a heterogeneous node inventory.
    Plan {
        pod: PathBuf,
        #[arg(long)]
        runtime_class: String,
        /// A TeeLens NodeInventory JSON document.
        #[arg(long)]
        node_inventory: PathBuf,
        #[arg(long)]
        kata_config: PathBuf,
        #[arg(long, value_enum, default_value_t = Output::Text)]
        output: Output,
    },
    /// Compile a trust-aware placement manifest.
    Compile {
        pod: PathBuf,
        #[arg(long)]
        runtime_class: String,
        #[arg(long)]
        node_inventory: PathBuf,
        #[arg(long)]
        kata_config: PathBuf,
    },
    /// Compile accelerator requests into a Kubernetes DRA ResourceClaimTemplate.
    Dra {
        /// Kubernetes Pod manifest (YAML or JSON).
        pod: PathBuf,
        /// Resource-to-DeviceClass mapping, for example nvidia.com/gpu=production-gpu.
        #[arg(
            long = "device-class",
            value_name = "RESOURCE=DEVICE_CLASS",
            required = true
        )]
        device_classes: Vec<String>,
        /// Name for the generated ResourceClaimTemplate. Defaults to <pod-name>-devices.
        #[arg(long)]
        name: Option<String>,
    },
    /// Emit a DRA ResourceClaimTemplate and a Pod patched to consume it.
    DraBundle {
        /// Kubernetes Pod manifest (YAML or JSON).
        pod: PathBuf,
        /// Resource-to-DeviceClass mapping, for example nvidia.com/gpu=production-gpu.
        #[arg(
            long = "device-class",
            value_name = "RESOURCE=DEVICE_CLASS",
            required = true
        )]
        device_classes: Vec<String>,
        /// Container granted access to the generated claim. Repeat for each requesting container.
        #[arg(long = "container", required = true)]
        containers: Vec<String>,
        /// Name for the generated ResourceClaimTemplate. Defaults to <pod-name>-devices.
        #[arg(long)]
        name: Option<String>,
    },
    /// Collect non-secret, local node capabilities into a versioned document.
    Collect {
        #[arg(long)]
        name: Option<String>,
    },
    /// Validate a node against a TeeLens trust policy document.
    PolicyCheck {
        #[arg(long)]
        policy: PathBuf,
        #[arg(long)]
        node_capabilities: PathBuf,
    },
    /// Evaluate whether a conservative live migration path is compatible.
    MigrateCheck {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        destination: PathBuf,
        #[arg(long)]
        runtime_class: String,
        #[arg(long)]
        kata_config: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Output {
    Text,
    Json,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("cannot parse {path}: {details}")]
    Parse { path: String, details: String },
    #[error("invalid input: {0}")]
    Invalid(String),
}

#[derive(Debug, Deserialize)]
struct Pod {
    metadata: Option<Metadata>,
    spec: Option<PodSpec>,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    name: Option<String>,
    namespace: Option<String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct PodSpec {
    #[serde(rename = "runtimeClassName")]
    runtime_class_name: Option<String>,
    #[serde(default)]
    containers: Vec<Container>,
}

#[derive(Debug, Deserialize)]
struct Container {
    name: Option<String>,
    resources: Option<Resources>,
}

#[derive(Debug, Deserialize)]
struct Resources {
    requests: Option<BTreeMap<String, String>>,
}

/// Versioned inventory produced by a node agent or supplied by CI.
#[derive(Debug, Deserialize, Serialize)]
struct NodeCapabilities {
    #[serde(default)]
    name: String,
    api_version: String,
    architecture: String,
    kvm: bool,
    tee: Vec<Tee>,
    hypervisors: Vec<String>,
    #[serde(default)]
    accelerators: BTreeMap<String, u32>,
    #[serde(default)]
    wasm_runtimes: Vec<String>,
    #[serde(default)]
    numa_nodes: Vec<u32>,
    #[serde(default)]
    kernel: String,
    #[serde(default)]
    cpu_model: String,
    #[serde(default)]
    cpu_features: Vec<String>,
    #[serde(default)]
    iommu: bool,
    #[serde(default)]
    pci_devices: u32,
    #[serde(default)]
    gpu_pci_devices: Vec<String>,
    #[serde(default)]
    network_interfaces: Vec<String>,
    #[serde(default)]
    software_versions: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct NodeInventory {
    api_version: String,
    nodes: Vec<NodeCapabilities>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "kebab-case")]
enum Tee {
    SevSnp,
    Tdx,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrustPolicy {
    api_version: String,
    #[serde(default)]
    allowed_tee: Vec<Tee>,
    #[serde(default)]
    requires_kvm: bool,
    #[serde(default)]
    allowed_hypervisors: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PolicyReport {
    api_version: &'static str,
    allowed: bool,
    violations: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MigrationReport {
    api_version: &'static str,
    compatible: bool,
    blockers: Vec<String>,
    assumptions: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    api_version: &'static str,
    workload: Workload,
    node_architecture: String,
    selected_hypervisor: Option<String>,
    confidential_readiness: Readiness,
    migration_readiness: Readiness,
    findings: Vec<Finding>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Workload {
    name: String,
    namespace: String,
    runtime_class: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Readiness {
    Supported,
    Blocked,
    Unknown,
}

#[derive(Debug, Serialize)]
struct Finding {
    severity: Severity,
    code: &'static str,
    message: String,
    remediation: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanReport {
    api_version: &'static str,
    execution_target: String,
    required_accelerators: BTreeMap<String, u32>,
    eligible_nodes: Vec<String>,
    rejected_nodes: Vec<NodeRejection>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeRejection {
    name: String,
    reasons: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlacementManifest {
    api_version: &'static str,
    execution_target: String,
    eligible_nodes: Vec<String>,
    required_accelerators: BTreeMap<String, u32>,
    attestation_policy: Option<String>,
    migration: &'static str,
}

/// The stable Kubernetes DRA API object. DeviceClasses are intentionally
/// supplied by the caller because they are cluster and driver specific.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceClaimTemplate {
    api_version: &'static str,
    kind: &'static str,
    metadata: DraMetadata,
    spec: ResourceClaimTemplateSpec,
}

#[derive(Debug, Serialize)]
struct DraMetadata {
    name: String,
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct ResourceClaimTemplateSpec {
    spec: DeviceClaim,
}

#[derive(Debug, Serialize)]
struct DeviceClaim {
    devices: DeviceClaimDevices,
}

#[derive(Debug, Serialize)]
struct DeviceClaimDevices {
    requests: Vec<DeviceRequest>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceRequest {
    name: String,
    exactly: ExactDeviceRequest,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExactDeviceRequest {
    device_class_name: String,
    allocation_mode: &'static str,
    count: u32,
}

fn read(path: &PathBuf) -> Result<String, AppError> {
    fs::read_to_string(path).map_err(|source| AppError::Read {
        path: path.display().to_string(),
        source,
    })
}

fn selected_hypervisor(config: &toml::Value) -> Option<String> {
    let hypervisor = config.get("hypervisor")?.as_table()?;
    ["qemu", "clh", "cloud-hypervisor"]
        .into_iter()
        .find(|name| hypervisor.contains_key(*name))
        .map(str::to_owned)
}

fn collect_local(name: Option<String>) -> NodeCapabilities {
    let has_path = |path: &str| std::path::Path::new(path).exists();
    let mut tee = Vec::new();
    // CCP is a prerequisite driver on many AMD systems, but its presence does
    // not prove that this host exposes the SEV guest interface or SEV-SNP.
    if has_path("/dev/sev") {
        tee.push(Tee::SevSnp);
    }
    if has_path("/sys/firmware/tdx_guest") || has_path("/dev/tdx-guest") {
        tee.push(Tee::Tdx);
    }
    let numa_nodes = fs::read_dir("/sys/devices/system/node")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix("node"))
                .and_then(|number| number.parse().ok())
        })
        .collect();
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let cpu_value = |key: &str| {
        cpuinfo.lines().find_map(|line| {
            line.split_once(':')
                .and_then(|(name, value)| (name.trim() == key).then(|| value.trim().to_owned()))
        })
    };
    let cpu_features = cpu_value("flags")
        .or_else(|| cpu_value("Features"))
        .map(|flags| {
            ["vmx", "svm", "aes", "avx", "avx2", "avx512f"]
                .into_iter()
                .filter(|flag| flags.split_whitespace().any(|value| value == *flag))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let pci_entries: Vec<_> = fs::read_dir("/sys/bus/pci/devices")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .collect();
    let gpu_pci_devices = pci_entries
        .iter()
        .filter_map(|entry| {
            let class = fs::read_to_string(entry.path().join("class")).ok()?;
            class
                .trim_start()
                .starts_with("0x03")
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect();
    let network_interfaces = fs::read_dir("/sys/class/net")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|interface| interface != "lo")
        .collect();
    let command_version = |binary: &str| {
        ProcessCommand::new(binary)
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|output| output.lines().next().map(str::to_owned))
    };
    let mut software_versions = BTreeMap::new();
    for (binary, label) in [
        ("qemu-system-x86_64", "qemu"),
        ("cloud-hypervisor", "cloud-hypervisor"),
        ("wasmtime", "wasmtime"),
        ("wasmedge", "wasmedge"),
    ] {
        if let Some(version) = command_version(binary) {
            software_versions.insert(label.into(), version);
        }
    }
    let hypervisors = ["qemu", "cloud-hypervisor"]
        .into_iter()
        .filter(|name| software_versions.contains_key(*name))
        .map(str::to_owned)
        .collect();
    let wasm_runtimes = ["wasmtime", "wasmedge"]
        .into_iter()
        .filter(|name| software_versions.contains_key(*name))
        .map(str::to_owned)
        .collect();
    NodeCapabilities {
        name: name
            .or_else(|| std::env::var("HOSTNAME").ok())
            .unwrap_or_else(|| "localhost".into()),
        api_version: "teelens.io/node-capabilities/v1".into(),
        architecture: std::env::consts::ARCH.into(),
        kvm: has_path("/dev/kvm"),
        tee,
        hypervisors,
        accelerators: BTreeMap::new(),
        wasm_runtimes,
        numa_nodes,
        kernel: ProcessCommand::new("uname")
            .arg("-r")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|output| output.trim().to_owned())
            .unwrap_or_else(|| std::env::consts::OS.into()),
        cpu_model: cpu_value("model name").unwrap_or_default(),
        cpu_features,
        iommu: fs::read_dir("/sys/kernel/iommu_groups").is_ok(),
        pci_devices: pci_entries.len() as u32,
        gpu_pci_devices,
        network_interfaces,
        software_versions,
    }
}

fn policy_check(policy: TrustPolicy, node: NodeCapabilities) -> PolicyReport {
    let mut violations = Vec::new();
    if policy.api_version != "teelens.io/trust-policy/v1" {
        violations.push("unsupported trust policy API version".into());
    }
    if policy.requires_kvm && !node.kvm {
        violations.push("policy requires KVM".into());
    }
    if !policy.allowed_tee.is_empty()
        && !node.tee.iter().any(|tee| policy.allowed_tee.contains(tee))
    {
        violations.push("node has no policy-approved TEE".into());
    }
    if !policy.allowed_hypervisors.is_empty()
        && !node
            .hypervisors
            .iter()
            .any(|h| policy.allowed_hypervisors.contains(h))
    {
        violations.push("node has no policy-approved hypervisor".into());
    }
    PolicyReport {
        api_version: "teelens.io/policy-report/v1",
        allowed: violations.is_empty(),
        violations,
    }
}

fn migration_check(
    source: NodeCapabilities,
    destination: NodeCapabilities,
    runtime_class: &str,
    config: toml::Value,
) -> MigrationReport {
    let mut blockers = Vec::new();
    let hypervisor = selected_hypervisor(&config);
    if source.architecture != destination.architecture {
        blockers.push("source and destination architectures differ".into());
    }
    if !source.kvm || !destination.kvm {
        blockers.push("both nodes must have KVM".into());
    }
    if source.tee != destination.tee {
        blockers.push("TEE capabilities differ".into());
    }
    if !source
        .hypervisors
        .iter()
        .any(|h| Some(h) == hypervisor.as_ref())
        || !destination
            .hypervisors
            .iter()
            .any(|h| Some(h) == hypervisor.as_ref())
    {
        blockers.push("selected hypervisor is unavailable on source or destination".into());
    }
    if runtime_class.contains("coco") && (source.tee.is_empty() || destination.tee.is_empty()) {
        blockers.push("confidential runtime requires a TEE on both nodes".into());
    }
    MigrationReport {
        api_version: "teelens.io/migration-report/v1",
        compatible: blockers.is_empty(),
        blockers,
        assumptions: vec![
            "guest memory, device state, CPU feature flags, and attestation continuity are not measured",
            "a compatible result is a preflight signal, not authorization to migrate",
        ],
    }
}

fn check(pod: Pod, runtime_class: String, node: NodeCapabilities, config: toml::Value) -> Report {
    let selected_hypervisor = selected_hypervisor(&config);
    let workload = Workload {
        name: pod
            .metadata
            .as_ref()
            .and_then(|m| m.name.clone())
            .unwrap_or_else(|| "unknown".into()),
        namespace: pod
            .metadata
            .as_ref()
            .and_then(|m| m.namespace.clone())
            .unwrap_or_else(|| "default".into()),
        runtime_class: runtime_class.clone(),
    };
    let mut findings = Vec::new();

    if pod.spec.and_then(|s| s.runtime_class_name).as_deref() != Some(runtime_class.as_str()) {
        findings.push(Finding {
            severity: Severity::Warning,
            code: "runtime-class-mismatch",
            message: "The manifest RuntimeClassName does not match --runtime-class.".into(),
            remediation: "Use the RuntimeClass selected by the deployment.".into(),
        });
    }
    if node.api_version != "teelens.io/node-capabilities/v1" {
        findings.push(Finding {
            severity: Severity::Warning,
            code: "unknown-capabilities-version",
            message: format!(
                "{} is not the supported NodeCapabilities version.",
                node.api_version
            ),
            remediation: "Regenerate node capabilities using a v1-compatible collector.".into(),
        });
    }
    if !node.kvm {
        findings.push(Finding {
            severity: Severity::Error,
            code: "kvm-unavailable",
            message: "KVM is unavailable on the selected node.".into(),
            remediation: "Schedule onto a node with usable KVM.".into(),
        });
    }
    if !node
        .hypervisors
        .iter()
        .any(|h| Some(h) == selected_hypervisor.as_ref())
    {
        findings.push(Finding {
            severity: Severity::Error,
            code: "hypervisor-unavailable",
            message: "The Kata-selected hypervisor is not advertised by the node.".into(),
            remediation:
                "Install the selected hypervisor or choose a compatible Kata configuration.".into(),
        });
    }

    let qemu = selected_hypervisor.as_deref() == Some("qemu");
    let confidential_supported = node.kvm && qemu && !node.tee.is_empty();
    if qemu && !node.tee.is_empty() {
        findings.push(Finding {
            severity: Severity::Info,
            code: "tee-available",
            message: format!("Node supports confidential execution via {:?}.", node.tee),
            remediation: "Verify attestation policy and measurement references before deployment."
                .into(),
        });
    } else if selected_hypervisor.is_some() {
        findings.push(Finding { severity: Severity::Warning, code: "tee-path-unverified", message: "This hypervisor and node combination has no verified confidential-execution path in TeeLens v1.".into(), remediation: "Use a supported QEMU TDX/SEV-SNP configuration or contribute a validated adapter.".into() });
    }

    Report {
        api_version: "teelens.io/report/v1",
        workload,
        node_architecture: node.architecture,
        selected_hypervisor,
        confidential_readiness: if confidential_supported {
            Readiness::Supported
        } else {
            Readiness::Blocked
        },
        migration_readiness: Readiness::Unknown,
        findings,
    }
}

fn requested_accelerators(pod: &Pod) -> BTreeMap<String, u32> {
    let mut requested = BTreeMap::new();
    for container in pod.spec.as_ref().into_iter().flat_map(|s| &s.containers) {
        for (resource, quantity) in container
            .resources
            .as_ref()
            .and_then(|r| r.requests.as_ref())
            .into_iter()
            .flatten()
        {
            if resource.contains("gpu") || resource.contains("accelerator") {
                if let Ok(quantity) = quantity.parse::<u32>() {
                    *requested.entry(resource.clone()).or_default() += quantity;
                }
            }
        }
    }
    requested
}

fn dns_label(input: &str) -> String {
    let mut label: String = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    label = label.trim_matches('-').to_owned();
    if label.is_empty() {
        "device".into()
    } else {
        label.chars().take(63).collect()
    }
}

fn parse_device_classes(entries: &[String]) -> Result<BTreeMap<String, String>, AppError> {
    let mut classes = BTreeMap::new();
    for entry in entries {
        let (resource, class) = entry.split_once('=').ok_or_else(|| {
            AppError::Invalid(format!(
                "--device-class must use RESOURCE=DEVICE_CLASS, got {entry:?}"
            ))
        })?;
        if resource.is_empty() || class.is_empty() {
            return Err(AppError::Invalid(format!(
                "--device-class must use non-empty RESOURCE=DEVICE_CLASS, got {entry:?}"
            )));
        }
        if classes
            .insert(resource.to_owned(), class.to_owned())
            .is_some()
        {
            return Err(AppError::Invalid(format!(
                "duplicate DeviceClass mapping for resource {resource:?}"
            )));
        }
    }
    Ok(classes)
}

fn resource_claim_template(
    pod: &Pod,
    device_classes: BTreeMap<String, String>,
    name: Option<String>,
) -> Result<ResourceClaimTemplate, AppError> {
    let requested = requested_accelerators(pod);
    if requested.is_empty() {
        return Err(AppError::Invalid(
            "the Pod has no accelerator resource requests to compile into DRA".into(),
        ));
    }
    let mut requests = Vec::new();
    for (resource, count) in requested {
        let device_class_name = device_classes.get(&resource).ok_or_else(|| {
            AppError::Invalid(format!(
                "missing --device-class mapping for requested resource {resource:?}"
            ))
        })?;
        requests.push(DeviceRequest {
            name: dns_label(&resource),
            exactly: ExactDeviceRequest {
                device_class_name: device_class_name.clone(),
                allocation_mode: "ExactCount",
                count,
            },
        });
    }
    let pod_name = pod
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.name.as_deref())
        .unwrap_or("workload");
    let mut annotations = BTreeMap::new();
    annotations.insert("teelens.io/compiler".into(), "dra/v1".into());
    annotations.insert(
        "teelens.io/notice".into(),
        "DeviceClasses are cluster-managed; verify driver selectors before applying.".into(),
    );
    Ok(ResourceClaimTemplate {
        api_version: "resource.k8s.io/v1",
        kind: "ResourceClaimTemplate",
        metadata: DraMetadata {
            name: name.unwrap_or_else(|| format!("{}-devices", dns_label(pod_name))),
            annotations,
        },
        spec: ResourceClaimTemplateSpec {
            spec: DeviceClaim {
                devices: DeviceClaimDevices { requests },
            },
        },
    })
}

fn accelerator_requesting_containers(pod: &Pod) -> Vec<String> {
    pod.spec
        .as_ref()
        .into_iter()
        .flat_map(|spec| &spec.containers)
        .filter(|container| {
            container
                .resources
                .as_ref()
                .and_then(|resources| resources.requests.as_ref())
                .is_some_and(|requests| {
                    requests.keys().any(|resource| {
                        resource.contains("gpu") || resource.contains("accelerator")
                    })
                })
        })
        .filter_map(|container| container.name.clone())
        .collect()
}

fn mapping(value: &mut serde_yaml::Value) -> Result<&mut serde_yaml::Mapping, AppError> {
    value
        .as_mapping_mut()
        .ok_or_else(|| AppError::Invalid("Pod manifest must be a YAML object".into()))
}

fn value_key(name: &str) -> serde_yaml::Value {
    serde_yaml::Value::String(name.into())
}

fn patch_pod_for_dra(
    mut pod: serde_yaml::Value,
    template_name: &str,
    requested_resources: &BTreeMap<String, u32>,
    containers: &[String],
) -> Result<serde_yaml::Value, AppError> {
    let selected: std::collections::BTreeSet<_> = containers.iter().cloned().collect();
    if selected.len() != containers.len() {
        return Err(AppError::Invalid(
            "--container was repeated for the same container".into(),
        ));
    }
    let spec = mapping(&mut pod)?
        .get_mut(value_key("spec"))
        .ok_or_else(|| AppError::Invalid("Pod manifest is missing spec".into()))?;
    let spec = mapping(spec)?;
    if spec.contains_key(value_key("resourceClaims")) {
        return Err(AppError::Invalid(
            "Pod already has spec.resourceClaims; merge it manually to avoid overwriting claims"
                .into(),
        ));
    }
    let claim_name = "teelens-devices";
    let mut claim = serde_yaml::Mapping::new();
    claim.insert(value_key("name"), value_key(claim_name));
    claim.insert(
        value_key("resourceClaimTemplateName"),
        value_key(template_name),
    );
    spec.insert(
        value_key("resourceClaims"),
        serde_yaml::Value::Sequence(vec![serde_yaml::Value::Mapping(claim)]),
    );
    let containers_value = spec
        .get_mut(value_key("containers"))
        .and_then(serde_yaml::Value::as_sequence_mut)
        .ok_or_else(|| AppError::Invalid("Pod spec must contain a containers list".into()))?;
    let mut found = std::collections::BTreeSet::new();
    for container in containers_value {
        let container = mapping(container)?;
        let name = container
            .get(value_key("name"))
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if !selected.contains(&name) {
            continue;
        }
        found.insert(name.clone());
        let resources = container
            .entry(value_key("resources"))
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        let resources = mapping(resources)?;
        if resources.contains_key(value_key("claims")) {
            return Err(AppError::Invalid(format!(
                "container {name:?} already has resources.claims; merge it manually"
            )));
        }
        let mut claim_access = serde_yaml::Mapping::new();
        claim_access.insert(value_key("name"), value_key(claim_name));
        resources.insert(
            value_key("claims"),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::Mapping(claim_access)]),
        );
        for field in ["requests", "limits"] {
            let empty = if let Some(values) = resources
                .get_mut(value_key(field))
                .and_then(serde_yaml::Value::as_mapping_mut)
            {
                for resource in requested_resources.keys() {
                    values.remove(value_key(resource));
                }
                values.is_empty()
            } else {
                false
            };
            if empty {
                resources.remove(value_key(field));
            }
        }
    }
    if found != selected {
        let missing: Vec<_> = selected.difference(&found).cloned().collect();
        return Err(AppError::Invalid(format!(
            "--container names not found in Pod: {}",
            missing.join(", ")
        )));
    }
    Ok(pod)
}

fn dra_bundle(
    pod: &Pod,
    pod_yaml: serde_yaml::Value,
    device_classes: BTreeMap<String, String>,
    name: Option<String>,
    containers: &[String],
) -> Result<(ResourceClaimTemplate, serde_yaml::Value), AppError> {
    let requesters: std::collections::BTreeSet<_> =
        accelerator_requesting_containers(pod).into_iter().collect();
    let selected: std::collections::BTreeSet<_> = containers.iter().cloned().collect();
    if requesters.is_empty() {
        return Err(AppError::Invalid(
            "DRA bundle requires named containers with accelerator requests".into(),
        ));
    }
    if !requesters.is_subset(&selected) {
        let omitted: Vec<_> = requesters.difference(&selected).cloned().collect();
        return Err(AppError::Invalid(format!(
            "--container must include every accelerator-requesting container: {}",
            omitted.join(", ")
        )));
    }
    let requested_resources = requested_accelerators(pod);
    let template = resource_claim_template(pod, device_classes, name)?;
    let patched = patch_pod_for_dra(
        pod_yaml,
        &template.metadata.name,
        &requested_resources,
        containers,
    )?;
    Ok((template, patched))
}

fn plan(
    pod: Pod,
    runtime_class: &str,
    inventory: NodeInventory,
    config: toml::Value,
) -> PlanReport {
    let required_accelerators = requested_accelerators(&pod);
    let selected_hypervisor = selected_hypervisor(&config);
    let wasm_runtime = pod
        .metadata
        .as_ref()
        .and_then(|m| m.annotations.get("teelens.io/wasm-runtime"))
        .cloned();
    let confidential = runtime_class.contains("coco");
    let mut eligible_nodes = Vec::new();
    let mut rejected_nodes = Vec::new();
    for node in inventory.nodes {
        let mut reasons = Vec::new();
        if wasm_runtime.is_none() && !node.kvm {
            reasons.push("KVM is unavailable".into());
        }
        if wasm_runtime.is_none()
            && !node
                .hypervisors
                .iter()
                .any(|h| Some(h) == selected_hypervisor.as_ref())
        {
            reasons.push("selected Kata hypervisor is unavailable".into());
        }
        if let Some(runtime) = &wasm_runtime {
            if !node.wasm_runtimes.contains(runtime) {
                reasons.push(format!("WASM runtime {runtime} is unavailable"));
            }
            if confidential {
                reasons.push("confidential WASM execution is not yet modeled".into());
            }
        } else if confidential
            && (selected_hypervisor.as_deref() != Some("qemu") || node.tee.is_empty())
        {
            reasons.push("no verified confidential-computing path".into());
        }
        for (resource, needed) in &required_accelerators {
            if node.accelerators.get(resource).copied().unwrap_or_default() < *needed {
                reasons.push(format!(
                    "requires {needed} {resource}, node has {}",
                    node.accelerators.get(resource).copied().unwrap_or_default()
                ));
            }
        }
        if reasons.is_empty() {
            eligible_nodes.push(node.name);
        } else {
            rejected_nodes.push(NodeRejection {
                name: node.name,
                reasons,
            });
        }
    }
    if inventory.api_version != "teelens.io/node-inventory/v1" {
        rejected_nodes.push(NodeRejection {
            name: "inventory".into(),
            reasons: vec!["unknown inventory API version".into()],
        });
    }
    PlanReport {
        api_version: "teelens.io/plan/v1",
        execution_target: wasm_runtime
            .map(|runtime| format!("wasm:{runtime}"))
            .unwrap_or_else(|| {
                format!(
                    "vmm:{}",
                    selected_hypervisor.unwrap_or_else(|| "unknown".into())
                )
            }),
        required_accelerators,
        eligible_nodes,
        rejected_nodes,
    }
}

pub fn run() -> Result<(), AppError> {
    let cli = Cli::parse();
    match cli.command {
        Command::Check {
            pod,
            runtime_class,
            node_capabilities,
            kata_config,
            output,
        } => {
            let pod: Pod = serde_yaml::from_str(&read(&pod)?).map_err(|e| AppError::Parse {
                path: pod.display().to_string(),
                details: e.to_string(),
            })?;
            let node =
                serde_json::from_str(&read(&node_capabilities)?).map_err(|e| AppError::Parse {
                    path: node_capabilities.display().to_string(),
                    details: e.to_string(),
                })?;
            let config = toml::from_str(&read(&kata_config)?).map_err(|e| AppError::Parse {
                path: kata_config.display().to_string(),
                details: e.to_string(),
            })?;
            let report = check(pod, runtime_class, node, config);
            match output {
                Output::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serializable report")
                ),
                Output::Text => {
                    println!("{} / {}", report.workload.namespace, report.workload.name);
                    println!(
                        "confidential execution: {:?}",
                        report.confidential_readiness
                    );
                    println!("live migration: {:?}", report.migration_readiness);
                    for finding in report.findings {
                        println!(
                            "{:?} [{}] {}",
                            finding.severity, finding.code, finding.message
                        );
                    }
                }
            }
        }
        Command::Plan {
            pod,
            runtime_class,
            node_inventory,
            kata_config,
            output,
        } => {
            let pod = serde_yaml::from_str(&read(&pod)?).map_err(|e| AppError::Parse {
                path: pod.display().to_string(),
                details: e.to_string(),
            })?;
            let inventory =
                serde_json::from_str(&read(&node_inventory)?).map_err(|e| AppError::Parse {
                    path: node_inventory.display().to_string(),
                    details: e.to_string(),
                })?;
            let config = toml::from_str(&read(&kata_config)?).map_err(|e| AppError::Parse {
                path: kata_config.display().to_string(),
                details: e.to_string(),
            })?;
            let report = plan(pod, &runtime_class, inventory, config);
            match output {
                Output::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serializable plan")
                ),
                Output::Text => {
                    println!("eligible nodes: {}", report.eligible_nodes.join(", "));
                    for node in report.rejected_nodes {
                        println!("{}: {}", node.name, node.reasons.join("; "));
                    }
                }
            }
        }
        Command::Compile {
            pod,
            runtime_class,
            node_inventory,
            kata_config,
        } => {
            let pod: Pod = serde_yaml::from_str(&read(&pod)?).map_err(|e| AppError::Parse {
                path: pod.display().to_string(),
                details: e.to_string(),
            })?;
            let policy = pod
                .metadata
                .as_ref()
                .and_then(|m| m.annotations.get("teelens.io/attestation-policy"))
                .cloned();
            let inventory =
                serde_json::from_str(&read(&node_inventory)?).map_err(|e| AppError::Parse {
                    path: node_inventory.display().to_string(),
                    details: e.to_string(),
                })?;
            let config = toml::from_str(&read(&kata_config)?).map_err(|e| AppError::Parse {
                path: kata_config.display().to_string(),
                details: e.to_string(),
            })?;
            let plan = plan(pod, &runtime_class, inventory, config);
            let manifest = PlacementManifest {
                api_version: "teelens.io/placement-manifest/v1",
                execution_target: plan.execution_target,
                eligible_nodes: plan.eligible_nodes,
                required_accelerators: plan.required_accelerators,
                attestation_policy: policy,
                migration: "deny-until-validated",
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&manifest).expect("serializable placement manifest")
            );
        }
        Command::Dra {
            pod,
            device_classes,
            name,
        } => {
            let pod: Pod = serde_yaml::from_str(&read(&pod)?).map_err(|e| AppError::Parse {
                path: pod.display().to_string(),
                details: e.to_string(),
            })?;
            let template =
                resource_claim_template(&pod, parse_device_classes(&device_classes)?, name)?;
            println!(
                "{}",
                serde_yaml::to_string(&template).expect("serializable DRA template")
            );
        }
        Command::DraBundle {
            pod,
            device_classes,
            containers,
            name,
        } => {
            let raw_pod = read(&pod)?;
            let typed_pod: Pod = serde_yaml::from_str(&raw_pod).map_err(|e| AppError::Parse {
                path: pod.display().to_string(),
                details: e.to_string(),
            })?;
            let yaml_pod = serde_yaml::from_str(&raw_pod).map_err(|e| AppError::Parse {
                path: pod.display().to_string(),
                details: e.to_string(),
            })?;
            let (template, patched_pod) = dra_bundle(
                &typed_pod,
                yaml_pod,
                parse_device_classes(&device_classes)?,
                name,
                &containers,
            )?;
            println!(
                "---\n{}---\n{}",
                serde_yaml::to_string(&template).expect("serializable DRA template"),
                serde_yaml::to_string(&patched_pod).expect("serializable patched Pod")
            );
        }
        Command::Collect { name } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&collect_local(name))
                    .expect("serializable node capabilities")
            );
        }
        Command::PolicyCheck {
            policy,
            node_capabilities,
        } => {
            let policy: TrustPolicy =
                serde_yaml::from_str(&read(&policy)?).map_err(|e| AppError::Parse {
                    path: policy.display().to_string(),
                    details: e.to_string(),
                })?;
            let node: NodeCapabilities =
                serde_json::from_str(&read(&node_capabilities)?).map_err(|e| AppError::Parse {
                    path: node_capabilities.display().to_string(),
                    details: e.to_string(),
                })?;
            println!(
                "{}",
                serde_json::to_string_pretty(&policy_check(policy, node))
                    .expect("serializable policy report")
            );
        }
        Command::MigrateCheck {
            source,
            destination,
            runtime_class,
            kata_config,
        } => {
            let source: NodeCapabilities =
                serde_json::from_str(&read(&source)?).map_err(|e| AppError::Parse {
                    path: source.display().to_string(),
                    details: e.to_string(),
                })?;
            let destination: NodeCapabilities = serde_json::from_str(&read(&destination)?)
                .map_err(|e| AppError::Parse {
                    path: destination.display().to_string(),
                    details: e.to_string(),
                })?;
            let config = toml::from_str(&read(&kata_config)?).map_err(|e| AppError::Parse {
                path: kata_config.display().to_string(),
                details: e.to_string(),
            })?;
            println!(
                "{}",
                serde_json::to_string_pretty(&migration_check(
                    source,
                    destination,
                    &runtime_class,
                    config
                ))
                .expect("serializable migration report")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> toml::Value {
        toml::from_str("[hypervisor.qemu]\npath = 'qemu'").unwrap()
    }

    fn node(name: &str, tee: Vec<Tee>, gpus: u32) -> NodeCapabilities {
        NodeCapabilities {
            name: name.into(),
            api_version: "teelens.io/node-capabilities/v1".into(),
            architecture: "x86_64".into(),
            kvm: true,
            tee,
            hypervisors: vec!["qemu".into()],
            accelerators: BTreeMap::from([("nvidia.com/gpu".into(), gpus)]),
            wasm_runtimes: vec!["wasmtime".into()],
            numa_nodes: vec![0],
            kernel: "test".into(),
            cpu_model: "test".into(),
            cpu_features: vec!["svm".into()],
            iommu: true,
            pci_devices: 0,
            gpu_pci_devices: Vec::new(),
            network_interfaces: Vec::new(),
            software_versions: BTreeMap::new(),
        }
    }

    #[test]
    fn confidential_gpu_workload_rejects_untrusted_node() {
        let pod: Pod = serde_yaml::from_str("metadata:\n  name: protected\nspec:\n  containers:\n  - resources:\n      requests:\n        nvidia.com/gpu: '1'\n").unwrap();
        let inventory = NodeInventory {
            api_version: "teelens.io/node-inventory/v1".into(),
            nodes: vec![node("snp", vec![Tee::SevSnp], 1), node("plain", vec![], 1)],
        };
        let report = plan(pod, "kata-qemu-coco", inventory, config());
        assert_eq!(report.eligible_nodes, vec!["snp"]);
        assert_eq!(report.rejected_nodes[0].name, "plain");
    }

    #[test]
    fn wasm_workload_does_not_require_kvm() {
        let pod: Pod = serde_yaml::from_str(
            "metadata:\n  annotations:\n    teelens.io/wasm-runtime: wasmtime\n",
        )
        .unwrap();
        let mut wasm = node("wasm", vec![], 0);
        wasm.kvm = false;
        let inventory = NodeInventory {
            api_version: "teelens.io/node-inventory/v1".into(),
            nodes: vec![wasm],
        };
        let report = plan(pod, "wasmtime", inventory, config());
        assert_eq!(report.eligible_nodes, vec!["wasm"]);
    }

    #[test]
    fn dra_compiles_accelerator_request_with_explicit_device_class() {
        let pod: Pod = serde_yaml::from_str(
            "metadata:\n  name: protected\nspec:\n  containers:\n  - resources:\n      requests:\n        nvidia.com/gpu: '2'\n",
        )
        .unwrap();
        let template = resource_claim_template(
            &pod,
            BTreeMap::from([("nvidia.com/gpu".into(), "production-gpu".into())]),
            None,
        )
        .unwrap();
        assert_eq!(template.api_version, "resource.k8s.io/v1");
        assert_eq!(template.metadata.name, "protected-devices");
        assert_eq!(
            template.spec.spec.devices.requests[0].name,
            "nvidia-com-gpu"
        );
        assert_eq!(template.spec.spec.devices.requests[0].exactly.count, 2);
        assert_eq!(
            template.spec.spec.devices.requests[0]
                .exactly
                .device_class_name,
            "production-gpu"
        );
    }

    #[test]
    fn dra_rejects_missing_device_class_mapping() {
        let pod: Pod = serde_yaml::from_str(
            "spec:\n  containers:\n  - resources:\n      requests:\n        nvidia.com/gpu: '1'\n",
        )
        .unwrap();
        let error = resource_claim_template(&pod, BTreeMap::new(), None).unwrap_err();
        assert!(error.to_string().contains("missing --device-class mapping"));
    }

    #[test]
    fn dra_bundle_adds_claim_access_and_removes_extended_resource() {
        let raw = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: protected\nspec:\n  containers:\n  - name: app\n    image: example.invalid/app\n    resources:\n      requests:\n        nvidia.com/gpu: '1'\n        cpu: '1'\n";
        let pod: Pod = serde_yaml::from_str(raw).unwrap();
        let yaml: serde_yaml::Value = serde_yaml::from_str(raw).unwrap();
        let (template, patched) = dra_bundle(
            &pod,
            yaml,
            BTreeMap::from([("nvidia.com/gpu".into(), "production-gpu".into())]),
            None,
            &["app".into()],
        )
        .unwrap();
        assert_eq!(template.metadata.name, "protected-devices");
        let text = serde_yaml::to_string(&patched).unwrap();
        assert!(text.contains("resourceClaimTemplateName: protected-devices"));
        assert!(text.contains("claims:"));
        assert!(text.contains("cpu: '1'"));
        assert!(!text.contains("nvidia.com/gpu"));
    }

    #[test]
    fn policy_rejects_plain_node_when_tee_is_required() {
        let policy = TrustPolicy {
            api_version: "teelens.io/trust-policy/v1".into(),
            allowed_tee: vec![Tee::SevSnp],
            requires_kvm: true,
            allowed_hypervisors: vec!["qemu".into()],
        };
        let report = policy_check(policy, node("plain", vec![], 0));
        assert!(!report.allowed);
        assert!(report.violations[0].contains("policy-approved TEE"));
    }

    #[test]
    fn migration_rejects_different_tee_capabilities() {
        let report = migration_check(
            node("snp", vec![Tee::SevSnp], 0),
            node("tdx", vec![Tee::Tdx], 0),
            "kata-qemu-coco",
            config(),
        );
        assert!(!report.compatible);
        assert!(
            report
                .blockers
                .iter()
                .any(|blocker| blocker.contains("TEE capabilities"))
        );
    }
}

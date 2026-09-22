use std::{collections::BTreeMap, fs, path::PathBuf};

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
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Output {
    Text,
    Json,
}

#[derive(Debug, Error)]
enum AppError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("cannot parse {path}: {details}")]
    Parse { path: String, details: String },
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
    resources: Option<Resources>,
}

#[derive(Debug, Deserialize)]
struct Resources {
    requests: Option<BTreeMap<String, String>>,
}

/// Versioned inventory produced by a node agent or supplied by CI.
#[derive(Debug, Deserialize)]
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
}

#[derive(Debug, Deserialize)]
struct NodeInventory {
    api_version: String,
    nodes: Vec<NodeCapabilities>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Tee {
    SevSnp,
    Tdx,
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

fn main() -> Result<(), AppError> {
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
}

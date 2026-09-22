use std::{fs, path::PathBuf};

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
}

#[derive(Debug, Deserialize)]
struct PodSpec {
    #[serde(rename = "runtimeClassName")]
    runtime_class_name: Option<String>,
}

/// Versioned inventory produced by a node agent or supplied by CI.
#[derive(Debug, Deserialize)]
struct NodeCapabilities {
    api_version: String,
    architecture: String,
    kvm: bool,
    tee: Vec<Tee>,
    hypervisors: Vec<String>,
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
            let pod = serde_yaml::from_str(&read(&pod)?).map_err(|e| AppError::Parse {
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
    }
    Ok(())
}

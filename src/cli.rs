use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::model::{GenKind, RbacRule};
use crate::util;

#[derive(Parser, Debug)]
#[command(
    name = "kute",
    version,
    about = "A holistic Kubernetes helper: manifests, kustomize scaffolding, RBAC and fuzzy kubectl search",
    long_about = None,
    propagate_version = true,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Generate a Kubernetes manifest
    Gen(Box<GenArgs>),
    /// Scaffold a kustomize base + overlays tree
    Scaffold(ScaffoldArgs),
    /// Build RBAC objects (ServiceAccount, Role, ClusterRole, bindings)
    Rbac(RbacArgs),
    /// Fuzzy-search kubectl commands from examples and shell history
    Search(SearchArgs),
    /// Inspect or switch kubectl context and namespace
    Ctx(CtxArgs),
    /// Launch the interactive TUI
    Tui(TuiArgs),
}

#[derive(Args, Debug)]
pub struct GenArgs {
    /// Resource kind to generate; omit to list the supported kinds
    #[arg(value_enum)]
    pub kind: Option<GenKind>,

    /// Name of the object
    pub name: Option<String>,

    /// List the supported kinds and exit
    #[arg(long)]
    pub list: bool,

    /// Write to a file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Overwrite the output file if it exists
    #[arg(long)]
    pub force: bool,

    /// Namespace for the object
    #[arg(short = 'n', long)]
    pub namespace: Option<String>,

    /// Extra label, may be repeated (KEY=VALUE)
    #[arg(short = 'l', long = "label", value_parser = util::parse_kv)]
    pub labels: Vec<(String, String)>,

    /// Annotation, may be repeated (KEY=VALUE)
    #[arg(long = "annotation", value_parser = util::parse_kv)]
    pub annotations: Vec<(String, String)>,

    /// Container image
    #[arg(short = 'i', long, default_value = "nginx:1.27")]
    pub image: String,

    /// Image pull policy
    #[arg(long, default_value = "IfNotPresent")]
    pub image_pull_policy: String,

    /// Replica count for scalable workloads
    #[arg(short = 'r', long, default_value_t = 1)]
    pub replicas: u32,

    /// Container port
    #[arg(short = 'p', long, default_value_t = 80)]
    pub port: u16,

    /// Name of the container port
    #[arg(long, default_value = "http")]
    pub port_name: String,

    /// Service port (defaults to --port)
    #[arg(long)]
    pub service_port: Option<u16>,

    /// Service targetPort (defaults to --port)
    #[arg(long)]
    pub target_port: Option<u16>,

    /// Service type
    #[arg(long, default_value = "ClusterIP", value_parser = ["ClusterIP", "NodePort", "LoadBalancer", "ExternalName"])]
    pub service_type: String,

    /// serviceAccountName for the pod spec
    #[arg(long)]
    pub service_account: Option<String>,

    /// CPU request
    #[arg(long, default_value = "100m")]
    pub cpu_request: String,

    /// CPU limit
    #[arg(long, default_value = "500m")]
    pub cpu_limit: String,

    /// Memory request
    #[arg(long, default_value = "128Mi")]
    pub mem_request: String,

    /// Memory limit
    #[arg(long, default_value = "512Mi")]
    pub mem_limit: String,

    /// Container command argument, may be repeated
    #[arg(long = "command", allow_hyphen_values = true)]
    pub command: Vec<String>,

    /// Container args argument, may be repeated
    #[arg(long = "args", allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Literal environment variable (KEY=VALUE)
    #[arg(short = 'e', long = "env", value_parser = util::parse_kv)]
    pub env: Vec<(String, String)>,

    /// Env var from a ConfigMap key (VAR_NAME=CONFIGMAP)
    #[arg(long = "env-from-configmap", value_parser = util::parse_kv)]
    pub env_from_configmap: Vec<(String, String)>,

    /// Env var from a Secret key (VAR_NAME=SECRET)
    #[arg(long = "env-from-secret", value_parser = util::parse_kv)]
    pub env_from_secret: Vec<(String, String)>,

    /// ConfigMap/Secret data entry (KEY=VALUE)
    #[arg(short = 'd', long = "data", value_parser = util::parse_kv)]
    pub data: Vec<(String, String)>,

    /// Secret type
    #[arg(long)]
    pub secret_type: Option<String>,

    /// Mount a ConfigMap (NAME:/mount/path), may be repeated
    #[arg(long = "mount-configmap", value_parser = util::parse_mount)]
    pub mount_configmap: Vec<(String, String)>,

    /// Mount a Secret read-only (NAME:/mount/path), may be repeated
    #[arg(long = "mount-secret", value_parser = util::parse_mount)]
    pub mount_secret: Vec<(String, String)>,

    /// Ingress host
    #[arg(long)]
    pub host: Option<String>,

    /// Ingress path
    #[arg(long, default_value = "/")]
    pub path: String,

    /// Ingress pathType
    #[arg(long, default_value = "Prefix", value_parser = ["Prefix", "Exact", "ImplementationSpecific"])]
    pub path_type: String,

    /// Ingress class name
    #[arg(long)]
    pub ingress_class: Option<String>,

    /// TLS secret name for the ingress
    #[arg(long)]
    pub tls_secret: Option<String>,

    /// Cron schedule, e.g. "*/5 * * * *"
    #[arg(long)]
    pub schedule: Option<String>,

    /// Pod restart policy for Job/CronJob
    #[arg(long, default_value = "OnFailure", value_parser = ["OnFailure", "Never", "Always"])]
    pub restart_policy: String,

    /// backoffLimit for Job/CronJob
    #[arg(long, default_value_t = 6)]
    pub backoff_limit: i32,

    /// Job completions
    #[arg(long)]
    pub completions: Option<i32>,

    /// Job parallelism
    #[arg(long)]
    pub parallelism: Option<i32>,

    /// Create the Job/CronJob suspended
    #[arg(long)]
    pub suspend: bool,

    /// PVC storage request
    #[arg(long, default_value = "1Gi")]
    pub storage: String,

    /// PVC access mode
    #[arg(long, default_value = "ReadWriteOnce", value_parser = ["ReadWriteOnce", "ReadOnlyMany", "ReadWriteMany", "ReadWriteOncePod"])]
    pub access_mode: String,

    /// PVC storageClassName
    #[arg(long)]
    pub storage_class: Option<String>,

    /// HPA minimum replicas
    #[arg(long, default_value_t = 2)]
    pub min_replicas: u32,

    /// HPA maximum replicas
    #[arg(long, default_value_t = 10)]
    pub max_replicas: u32,

    /// HPA target average CPU utilisation
    #[arg(long, default_value_t = 80)]
    pub target_cpu: u32,

    /// NetworkPolicy ingress port, may be repeated
    #[arg(long = "netpol-port")]
    pub netpol_port: Vec<u16>,

    /// Do not automount the service account token
    #[arg(long)]
    pub no_automount: bool,

    /// imagePullSecret name, may be repeated
    #[arg(long = "image-pull-secret")]
    pub image_pull_secrets: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ScaffoldArgs {
    /// Application name; also the directory created
    pub name: String,

    /// Parent directory for the generated tree
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,

    /// Namespace applied by each overlay
    #[arg(short = 'n', long, default_value = "default")]
    pub namespace: String,

    /// Container image in the base
    #[arg(short = 'i', long, default_value = "nginx:1.27")]
    pub image: String,

    /// Service port
    #[arg(short = 'p', long, default_value_t = 80)]
    pub port: u16,

    /// Overlay environments, comma separated
    #[arg(long, value_delimiter = ',', default_value = "dev,prod")]
    pub envs: Vec<String>,

    /// Replica count in the base
    #[arg(short = 'r', long, default_value_t = 1)]
    pub replicas: u32,

    /// Image tag used by the generated overlays (defaults to the tag in --image)
    #[arg(long)]
    pub tag: Option<String>,

    /// Print the tree instead of writing it
    #[arg(long)]
    pub dry_run: bool,

    /// Overwrite existing files
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct RbacArgs {
    #[command(subcommand)]
    pub command: RbacCommand,
}

#[derive(Subcommand, Debug)]
pub enum RbacCommand {
    /// Generate a ServiceAccount
    #[command(name = "serviceaccount")]
    ServiceAccount(RbacServiceAccountArgs),
    /// Generate a namespaced Role
    #[command(name = "role")]
    Role(RbacRoleArgs),
    /// Generate a ClusterRole
    #[command(name = "clusterrole")]
    ClusterRole(RbacRoleArgs),
    /// Bind a Role to subjects
    #[command(name = "rolebinding")]
    RoleBinding(RbacBindingArgs),
    /// Bind a ClusterRole to subjects
    #[command(name = "clusterrolebinding")]
    ClusterRoleBinding(RbacBindingArgs),
    /// Generate a ServiceAccount plus Role/ClusterRole and binding in one file
    #[command(name = "bundle")]
    Bundle(RbacBundleArgs),
}

#[derive(Args, Debug)]
pub struct RbacServiceAccountArgs {
    pub name: String,

    #[arg(short = 'n', long)]
    pub namespace: Option<String>,

    /// Do not automount the service account token
    #[arg(long)]
    pub no_automount: bool,

    /// imagePullSecret name, may be repeated
    #[arg(long = "image-pull-secret")]
    pub image_pull_secrets: Vec<String>,

    #[arg(short, long)]
    pub output: Option<PathBuf>,

    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct RbacRoleArgs {
    pub name: String,

    #[arg(short = 'n', long)]
    pub namespace: Option<String>,

    /// Rule as VERBS:RESOURCES[:APIGROUPS], may be repeated
    #[arg(short = 'r', long = "rule", value_parser = parse_rule)]
    pub rules: Vec<RbacRule>,

    /// Restrict the rule to these resource names
    #[arg(long = "resource-name")]
    pub resource_names: Vec<String>,

    #[arg(short, long)]
    pub output: Option<PathBuf>,

    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct RbacBindingArgs {
    pub name: String,

    #[arg(short = 'n', long)]
    pub namespace: Option<String>,

    /// Name of the Role/ClusterRole to bind
    #[arg(long)]
    pub role: String,

    /// ServiceAccount to bind as ([NAMESPACE/]NAME), may be repeated
    #[arg(long = "service-account", value_parser = parse_subject)]
    pub service_accounts: Vec<SubjectArg>,

    /// Namespace for RoleBinding output
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct RbacBundleArgs {
    /// Base name for the generated objects
    pub name: String,

    /// Namespace for the ServiceAccount and namespaced Role
    #[arg(short = 'n', long, default_value = "default")]
    pub namespace: String,

    /// Rule as VERBS:RESOURCES[:APIGROUPS], may be repeated
    #[arg(short = 'r', long = "rule", value_parser = parse_rule)]
    pub rules: Vec<RbacRule>,

    /// Restrict the rule to these resource names
    #[arg(long = "resource-name")]
    pub resource_names: Vec<String>,

    /// Emit a ClusterRole + ClusterRoleBinding instead of Role + RoleBinding
    #[arg(long)]
    pub cluster_wide: bool,

    /// Do not automount the service account token
    #[arg(long)]
    pub no_automount: bool,

    #[arg(short, long)]
    pub output: Option<PathBuf>,

    #[arg(long)]
    pub force: bool,
}

/// Clap adapter that turns the shared `VERBS:RESOURCES[:APIGROUPS]` parser into
/// an [`RbacRule`].
fn parse_rule(input: &str) -> Result<RbacRule, String> {
    let raw = util::parse_rule(input)?;
    Ok(RbacRule {
        verbs: raw.verbs,
        resources: raw.resources,
        api_groups: raw.api_groups,
        resource_names: Vec::new(),
    })
}

/// A ServiceAccount reference, optionally qualified with its namespace.
#[derive(Clone, Debug)]
pub struct SubjectArg {
    pub namespace: Option<String>,
    pub name: String,
}

/// Parse `[NAMESPACE/]NAME` into a [`SubjectArg`].
fn parse_subject(input: &str) -> Result<SubjectArg, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("expected [NAMESPACE/]NAME".to_string());
    }

    match trimmed.split_once('/') {
        Some((namespace, name)) if !namespace.is_empty() && !name.is_empty() => Ok(SubjectArg {
            namespace: Some(namespace.to_string()),
            name: name.to_string(),
        }),
        Some(_) => Err(format!("expected [NAMESPACE/]NAME, got `{input}`")),
        None => Ok(SubjectArg {
            namespace: None,
            name: trimmed.to_string(),
        }),
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum SearchSource {
    /// Search the built-in example corpus and shell history
    All,
    /// Search only the built-in example corpus
    Examples,
    /// Search only shell history
    History,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Words to match; omit to list everything (or use --interactive)
    #[arg()]
    pub query: Vec<String>,

    /// Maximum number of results
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: usize,

    /// Where to look for candidates
    #[arg(long, value_enum, default_value_t = SearchSource::All)]
    pub source: SearchSource,

    /// Additional history file to read
    #[arg(long = "history-file")]
    pub history_file: Vec<PathBuf>,

    /// Print only the best matching command, with no decoration
    #[arg(long)]
    pub print: bool,

    /// Print the full match instead of a truncated command
    #[arg(long)]
    pub full: bool,

    /// Open the interactive fuzzy picker
    #[arg(short = 'i', long)]
    pub interactive: bool,

    /// Emit results as JSON
    #[arg(long)]
    pub json: bool,

    /// Disable ANSI colour
    #[arg(long)]
    pub no_color: bool,
}

#[derive(Args, Debug)]
pub struct CtxArgs {
    /// Context to switch to
    pub set: Option<String>,

    /// Set the namespace on the current (or new) context
    #[arg(short = 'n', long)]
    pub namespace: Option<String>,

    /// Print only the current context name
    #[arg(long)]
    pub current: bool,
}

#[derive(Args, Debug)]
pub struct TuiArgs {
    /// Additional history file to read
    #[arg(long = "history-file")]
    pub history_file: Vec<PathBuf>,
}

pub mod form;
mod ui;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::cli::{RbacBundleArgs, ScaffoldArgs, SearchArgs, SearchSource, TuiArgs};
use crate::ctx::{self, ContextEntry};
use crate::generate::{self, sanitize_volume_name};
use crate::model::{EnvVar, GenContext, GenKind, RbacRule, Volume, VolumeMount};
use crate::rbac;
use crate::scaffold;
use crate::search::{self, Candidate, Hit};
use crate::util;

use form::{Field, Form, Section};

pub const MENU: [(&str, &str); 5] = [
    (
        "Generate manifest",
        "pick a kind, toggle the optional sections you want",
    ),
    (
        "Scaffold kustomize",
        "base + overlays tree, with optional ingress and HPA",
    ),
    (
        "Build RBAC",
        "ServiceAccount, Role/ClusterRole and binding in one file",
    ),
    (
        "Search commands",
        "fuzzy-find kubectl commands from examples and your history",
    ),
    ("Contexts", "inspect and switch kubectl contexts"),
];

const WORKLOAD_KINDS: &[GenKind] = &[
    GenKind::Deployment,
    GenKind::StatefulSet,
    GenKind::DaemonSet,
    GenKind::Job,
    GenKind::CronJob,
];

const SERVICE_KINDS: &[GenKind] = &[GenKind::Service, GenKind::Ingress];
const BATCH_KINDS: &[GenKind] = &[GenKind::Job, GenKind::CronJob];
const DATA_KINDS: &[GenKind] = &[GenKind::ConfigMap, GenKind::Secret];

// ── value palettes ──────────────────────────────────────────────────────────
//
// Offered as a dropdown next to free-form entry, so the common cases are one
// keystroke away without ever restricting what can be typed.

/// RBAC verbs, roughly most-granted first.
const COMMON_VERBS: &[&str] = &[
    "get",
    "list",
    "watch",
    "create",
    "update",
    "patch",
    "delete",
    "deletecollection",
    "use",
    "bind",
    "escalate",
    "impersonate",
    "approve",
    "sign",
];

const COMMON_RESOURCES: &[&str] = &[
    "pods",
    "pods/log",
    "pods/exec",
    "pods/portforward",
    "services",
    "endpoints",
    "configmaps",
    "secrets",
    "serviceaccounts",
    "events",
    "deployments",
    "statefulsets",
    "daemonsets",
    "replicasets",
    "jobs",
    "cronjobs",
    "ingresses",
    "networkpolicies",
    "persistentvolumeclaims",
    "roles",
    "rolebindings",
    "clusterroles",
    "clusterrolebindings",
    "nodes",
    "namespaces",
];

/// The core group is the empty string, which the rule parser already infers
/// when this list is left alone -- so it is deliberately not offered here.
const COMMON_API_GROUPS: &[&str] = &[
    "apps",
    "batch",
    "networking.k8s.io",
    "rbac.authorization.k8s.io",
    "autoscaling",
    "policy",
    "storage.k8s.io",
    "apiextensions.k8s.io",
    "coordination.k8s.io",
    "certificates.k8s.io",
];

const COMMON_ENVIRONMENTS: &[&str] = &["dev", "staging", "prod", "qa", "test", "canary", "sandbox"];

const COMMON_INGRESS_CLASSES: &[&str] = &["nginx", "traefik", "alb", "istio", "kong", "haproxy"];

const COMMON_CRON_SCHEDULES: &[&str] = &[
    "*/5 * * * *",
    "*/15 * * * *",
    "0 * * * *",
    "0 2 * * *",
    "0 3 * * 0",
    "0 9 * * 1-5",
    "@hourly",
    "@daily",
];

const COMMON_SECRET_TYPES: &[&str] = &[
    "Opaque",
    "kubernetes.io/tls",
    "kubernetes.io/dockerconfigjson",
    "kubernetes.io/basic-auth",
    "kubernetes.io/ssh-auth",
    "kubernetes.io/service-account-token",
];

const CPU_QUANTITIES: &[&str] = &["50m", "100m", "250m", "500m", "1", "2", "4"];
const MEMORY_QUANTITIES: &[&str] = &["64Mi", "128Mi", "256Mi", "512Mi", "1Gi", "2Gi", "4Gi"];
const STORAGE_SIZES: &[&str] = &["1Gi", "5Gi", "10Gi", "50Gi", "100Gi", "1Ti"];
const COMMON_STORAGE_CLASSES: &[&str] = &[
    "standard",
    "gp2",
    "gp3",
    "premium-rwo",
    "standard-rwo",
    "local-path",
];
const COMMON_PORTS: &[&str] = &["80", "443", "3000", "5000", "8080", "8443", "9090"];
const COMMON_PORT_NAMES: &[&str] = &["http", "https", "grpc", "metrics", "admin", "web"];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Menu,
    Search,
    Generate,
    Scaffold,
    Rbac,
    Contexts,
}

// ── search ──────────────────────────────────────────────────────────────────

pub struct SearchState {
    pub input: String,
    pub candidates: Vec<Candidate>,
    pub hits: Vec<Hit>,
    pub selected: usize,
    pub picked: Option<String>,
    pub limit: usize,
}

impl SearchState {
    pub fn refresh(&mut self) {
        self.hits = search::search(&self.candidates, &self.input, self.limit);
        if self.selected >= self.hits.len() {
            self.selected = self.hits.len().saturating_sub(1);
        }
    }

    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.hits
            .get(self.selected)
            .map(|hit| &self.candidates[hit.index])
    }

    fn move_selection(&mut self, delta: isize) {
        if self.hits.is_empty() {
            return;
        }
        let len = self.hits.len() as isize;
        self.selected = ((self.selected as isize + delta).rem_euclid(len)) as usize;
    }
}

// ── generate ────────────────────────────────────────────────────────────────

fn generate_form(kind: GenKind) -> Form {
    Form::new(vec![
        Section::required(
            "Basics",
            "always required",
            vec![
                Field::text("name", "name", "my-app", "the object's name"),
                Field::text(
                    "namespace",
                    "namespace",
                    "default",
                    "leave blank for cluster-scoped",
                ),
                Field::text(
                    "output",
                    "output file",
                    "",
                    "blank prints to the terminal only",
                ),
            ],
        ),
        Section::required(
            "Container",
            "the pod spec",
            vec![
                Field::text("image", "image", "nginx:1.27", ""),
                Field::choice(
                    "pull_policy",
                    "pull policy",
                    &["IfNotPresent", "Always", "Never"],
                    "IfNotPresent",
                    "",
                ),
                Field::number("replicas", "replicas", 1, ""),
                Field::number("port", "port", 80, "").suggesting(COMMON_PORTS),
                Field::text("port_name", "port name", "http", "").suggesting(COMMON_PORT_NAMES),
                Field::text(
                    "service_account",
                    "service account",
                    "",
                    "serviceAccountName",
                ),
                Field::list("command", "command", "press a to add, d to remove"),
                Field::list("args", "args", "press a to add, d to remove"),
            ],
        )
        .for_kinds(WORKLOAD_KINDS),
        Section::optional(
            "Service",
            "expose the workload",
            vec![
                Field::choice(
                    "service_type",
                    "type",
                    &["ClusterIP", "NodePort", "LoadBalancer"],
                    "ClusterIP",
                    "",
                ),
                Field::number(
                    "service_port",
                    "service port",
                    80,
                    "defaults to the container port",
                ),
                Field::number(
                    "target_port",
                    "target port",
                    80,
                    "defaults to the container port",
                ),
            ],
        )
        .for_kinds(SERVICE_KINDS),
        Section::optional(
            "Environment",
            "environment variables",
            vec![
                Field::list("env", "literal env", "KEY=VALUE, one per entry"),
                Field::list("env_from_configmap", "from configmap", "VAR=CONFIGMAP"),
                Field::list("env_from_secret", "from secret", "VAR=SECRET"),
            ],
        )
        .for_kinds(WORKLOAD_KINDS),
        Section::optional(
            "Resources",
            "requests and limits",
            vec![
                Field::text("cpu_request", "cpu request", "100m", "").suggesting(CPU_QUANTITIES),
                Field::text("cpu_limit", "cpu limit", "500m", "").suggesting(CPU_QUANTITIES),
                Field::text("mem_request", "memory request", "128Mi", "")
                    .suggesting(MEMORY_QUANTITIES),
                Field::text("mem_limit", "memory limit", "512Mi", "").suggesting(MEMORY_QUANTITIES),
            ],
        )
        .for_kinds(WORKLOAD_KINDS),
        Section::optional(
            "Volumes",
            "mount configmaps and secrets",
            vec![
                Field::list("mount_configmap", "mount configmap", "NAME:/mount/path"),
                Field::list(
                    "mount_secret",
                    "mount secret",
                    "NAME:/mount/path (read-only)",
                ),
            ],
        )
        .for_kinds(WORKLOAD_KINDS),
        Section::optional(
            "Ingress",
            "route HTTP traffic in",
            vec![
                Field::text("host", "host", "", "required when this section is on"),
                Field::text("path", "path", "/", ""),
                Field::choice(
                    "path_type",
                    "path type",
                    &["Prefix", "Exact", "ImplementationSpecific"],
                    "Prefix",
                    "",
                ),
                Field::text("ingress_class", "class", "", "ingressClassName")
                    .suggesting(COMMON_INGRESS_CLASSES),
                Field::text(
                    "tls_secret",
                    "tls secret",
                    "",
                    "secretName holding the cert",
                ),
            ],
        )
        .for_kinds(&[GenKind::Ingress]),
        Section::optional(
            "Autoscaling",
            "scale on CPU utilisation",
            vec![
                Field::number("min_replicas", "min replicas", 2, ""),
                Field::number("max_replicas", "max replicas", 10, ""),
                Field::number("target_cpu", "target cpu %", 80, ""),
            ],
        )
        .for_kinds(&[GenKind::Hpa]),
        Section::optional(
            "Job",
            "batch scheduling",
            vec![
                Field::text(
                    "schedule",
                    "schedule",
                    "0 2 * * *",
                    "cron expression, CronJob only",
                )
                .suggesting(COMMON_CRON_SCHEDULES),
                Field::choice(
                    "restart_policy",
                    "restart policy",
                    &["OnFailure", "Never", "Always"],
                    "OnFailure",
                    "",
                ),
                Field::number("backoff_limit", "backoff limit", 6, ""),
                Field::number("completions", "completions", 1, ""),
                Field::number("parallelism", "parallelism", 1, ""),
                Field::toggle("suspend", "suspend", false, "create it suspended"),
            ],
        )
        .for_kinds(BATCH_KINDS),
        Section::optional(
            "Storage",
            "persistent volume claim",
            vec![
                Field::text("storage", "storage", "1Gi", "").suggesting(STORAGE_SIZES),
                Field::choice(
                    "access_mode",
                    "access mode",
                    &[
                        "ReadWriteOnce",
                        "ReadOnlyMany",
                        "ReadWriteMany",
                        "ReadWriteOncePod",
                    ],
                    "ReadWriteOnce",
                    "",
                ),
                Field::text(
                    "storage_class",
                    "storage class",
                    "",
                    "blank uses the default",
                )
                .suggesting(COMMON_STORAGE_CLASSES),
            ],
        )
        .for_kinds(&[GenKind::Pvc]),
        Section::optional(
            "Data",
            "configmap and secret entries",
            vec![
                Field::list("data", "data", "KEY=VALUE, one per entry"),
                Field::text("secret_type", "secret type", "Opaque", "")
                    .suggesting(COMMON_SECRET_TYPES),
            ],
        )
        .for_kinds(DATA_KINDS),
        Section::optional(
            "Network",
            "network policy ingress ports",
            vec![
                Field::list(
                    "netpol_ports",
                    "allow ports",
                    "one port per entry; empty allows all ports",
                )
                .suggesting(COMMON_PORTS),
            ],
        )
        .for_kinds(&[GenKind::NetworkPolicy]),
        Section::optional(
            "Identity",
            "service account token and pull secrets",
            vec![
                Field::toggle(
                    "automount",
                    "automount token",
                    true,
                    "automountServiceAccountToken",
                ),
                Field::list(
                    "image_pull_secrets",
                    "pull secrets",
                    "one secret name per entry",
                ),
            ],
        )
        .for_kinds(&[GenKind::ServiceAccount]),
    ])
    .with_kind(kind)
}

fn split_kv(entry: &str) -> Option<(String, String)> {
    let (key, value) = entry.split_once('=')?;
    let (key, value) = (key.trim(), value.trim());
    if key.is_empty() || value.is_empty() {
        return None;
    }
    Some((key.to_string(), value.to_string()))
}

fn build_env(form: &Form) -> Vec<EnvVar> {
    let mut env: Vec<EnvVar> = form
        .list("env")
        .iter()
        .filter_map(|entry| split_kv(entry))
        .map(|(key, value)| EnvVar::literal(key, value))
        .collect();

    for (key, source) in form
        .list("env_from_configmap")
        .iter()
        .filter_map(|entry| split_kv(entry))
    {
        env.push(EnvVar {
            name: key,
            value: None,
            config_map_ref: Some(source),
            secret_ref: None,
        });
    }

    for (key, source) in form
        .list("env_from_secret")
        .iter()
        .filter_map(|entry| split_kv(entry))
    {
        env.push(EnvVar {
            name: key,
            value: None,
            config_map_ref: None,
            secret_ref: Some(source),
        });
    }

    env
}

fn add_mounts(ctx: &mut GenContext, entries: &[String], kind: &str, read_only: bool) {
    for entry in entries {
        let Some((source, path)) = entry.split_once(':') else {
            continue;
        };
        let (source, path) = (source.trim(), path.trim());
        if source.is_empty() || path.is_empty() {
            continue;
        }

        let volume_name = sanitize_volume_name(source);
        ctx.volumes.push(Volume {
            name: volume_name.clone(),
            kind: kind.to_string(),
            source: source.to_string(),
        });
        ctx.mounts.push(VolumeMount {
            name: volume_name,
            mount_path: path.to_string(),
            read_only,
        });
    }
}

pub struct GenerateState {
    pub kind_index: usize,
    pub choosing_kind: bool,
    pub form: Form,
    pub preview: String,
    pub scroll: u16,
    pub message: String,
}

impl GenerateState {
    pub fn kind(&self) -> GenKind {
        GenKind::ALL[self.kind_index]
    }

    pub fn select_kind(&mut self, index: usize) {
        self.kind_index = index;
        self.form.set_kind(self.kind());
    }

    /// Rebuild the preview from the form, honouring which optional sections the
    /// user has switched on.
    pub fn refresh(&mut self) {
        let kind = self.kind();
        let name = self.form.text("name");

        if name.is_empty() {
            self.preview = "name is required\n".to_string();
            return;
        }

        let mut ctx = GenContext::new(&name);

        if kind != GenKind::Namespace {
            let namespace = self.form.text("namespace");
            ctx.namespace = (!namespace.is_empty()).then_some(namespace);
        }

        // Container
        ctx.image = self.form.text_or("image", "nginx:1.27");
        ctx.image_pull_policy = self.form.text_or("pull_policy", "IfNotPresent");
        ctx.replicas = self.form.number_or("replicas", 1);
        ctx.port = self.form.number_u16("port").unwrap_or(80);
        ctx.port_name = self.form.text_or("port_name", "http");
        ctx.service_port = ctx.port;
        ctx.target_port = ctx.port;

        let service_account = self.form.text("service_account");
        ctx.service_account = (!service_account.is_empty()).then_some(service_account);
        ctx.command = self.form.list("command");
        ctx.args = self.form.list("args");

        if self.form.section_enabled("Service") {
            ctx.service_type = self.form.text_or("service_type", "ClusterIP");
            ctx.service_port = self.form.number_u16("service_port").unwrap_or(ctx.port);
            ctx.target_port = self.form.number_u16("target_port").unwrap_or(ctx.port);
        }

        if self.form.section_enabled("Environment") {
            ctx.env = build_env(&self.form);
        }

        if self.form.section_enabled("Resources") {
            ctx.cpu_request = self.form.text_or("cpu_request", "100m");
            ctx.cpu_limit = self.form.text_or("cpu_limit", "500m");
            ctx.mem_request = self.form.text_or("mem_request", "128Mi");
            ctx.mem_limit = self.form.text_or("mem_limit", "512Mi");
        }

        if self.form.section_enabled("Volumes") {
            add_mounts(
                &mut ctx,
                &self.form.list("mount_configmap"),
                "configMap",
                false,
            );
            add_mounts(&mut ctx, &self.form.list("mount_secret"), "secret", true);
        }

        if self.form.section_enabled("Ingress") {
            ctx.host = self.form.text("host");
            ctx.path = self.form.text_or("path", "/");
            ctx.path_type = self.form.text_or("path_type", "Prefix");
            let class = self.form.text("ingress_class");
            ctx.ingress_class = (!class.is_empty()).then_some(class);
            let tls = self.form.text("tls_secret");
            ctx.tls_secret = (!tls.is_empty()).then_some(tls);
        }

        if self.form.section_enabled("Autoscaling") {
            ctx.min_replicas = self.form.number_or("min_replicas", 2);
            ctx.max_replicas = self.form.number_or("max_replicas", 10);
            ctx.target_cpu = self.form.number_or("target_cpu", 80);
        }

        if self.form.section_enabled("Job") {
            ctx.schedule = self.form.text_or("schedule", "0 2 * * *");
            ctx.restart_policy = self.form.text_or("restart_policy", "OnFailure");
            ctx.backoff_limit = self.form.number_or("backoff_limit", 6) as i32;
            ctx.completions = self.form.number("completions").map(|value| value as i32);
            ctx.parallelism = self.form.number("parallelism").map(|value| value as i32);
            ctx.suspend = self.form.toggle_value("suspend");
        }

        if self.form.section_enabled("Storage") {
            ctx.storage = self.form.text_or("storage", "1Gi");
            ctx.access_mode = self.form.text_or("access_mode", "ReadWriteOnce");
            let class = self.form.text("storage_class");
            ctx.storage_class = (!class.is_empty()).then_some(class);
        }

        if self.form.section_enabled("Data") {
            ctx.data = self
                .form
                .list("data")
                .iter()
                .filter_map(|entry| split_kv(entry))
                .map(|(key, value)| crate::model::Label::new(key, value))
                .collect();
            ctx.secret_type = self.form.text_or("secret_type", "Opaque");
        }

        if self.form.section_enabled("Network") {
            ctx.netpol_ports = self
                .form
                .list("netpol_ports")
                .iter()
                .filter_map(|port| port.parse().ok())
                .collect();
        }

        if self.form.section_enabled("Identity") {
            ctx.automount = self.form.toggle_value("automount");
            ctx.image_pull_secrets = self.form.list("image_pull_secrets");
        }

        self.preview = match generate::render(kind, &ctx) {
            Ok(yaml) => yaml,
            Err(error) => format!("error: {error}\n"),
        };
    }

    fn output_path(&self) -> PathBuf {
        let explicit = self.form.text("output");
        if explicit.is_empty() {
            PathBuf::from(format!("{}.yaml", self.form.text("name")))
        } else {
            PathBuf::from(explicit)
        }
    }
}

// ── scaffold ────────────────────────────────────────────────────────────────

fn scaffold_form() -> Form {
    Form::new(vec![
        Section::required(
            "Basics",
            "always required",
            vec![
                Field::text("name", "name", "my-app", "also the directory created"),
                Field::text("directory", "directory", ".", "where to write the tree"),
                Field::text(
                    "namespace",
                    "namespace",
                    "default",
                    "applied by every overlay",
                ),
            ],
        ),
        Section::required(
            "Workload",
            "the base deployment and service",
            vec![
                Field::text("image", "image", "nginx:1.27", ""),
                Field::number("port", "port", 80, ""),
                Field::number("replicas", "replicas", 2, ""),
                Field::text("tag", "image tag", "", "blank reuses the tag in the image"),
            ],
        ),
        Section::required(
            "Environments",
            "one overlay directory per entry",
            vec![
                Field::list(
                    "environments",
                    "environments",
                    "press a to add, d to remove",
                )
                .suggesting(COMMON_ENVIRONMENTS),
            ],
        ),
        Section::optional(
            "Ingress",
            "adds base/ingress.yaml",
            vec![
                Field::text("host", "host", "", "required when this section is on"),
                Field::text("ingress_class", "class", "", "ingressClassName")
                    .suggesting(COMMON_INGRESS_CLASSES),
                Field::text(
                    "tls_secret",
                    "tls secret",
                    "",
                    "secretName holding the cert",
                ),
            ],
        ),
        Section::optional(
            "Autoscaling",
            "adds base/hpa.yaml",
            vec![
                Field::number("hpa_min", "min replicas", 2, ""),
                Field::number("hpa_max", "max replicas", 10, ""),
                Field::number("hpa_cpu", "target cpu %", 80, ""),
            ],
        ),
    ])
}

pub struct ScaffoldState {
    pub form: Form,
    pub preview: String,
    pub scroll: u16,
    pub message: String,
}

impl ScaffoldState {
    fn to_args(&self) -> ScaffoldArgs {
        let ingress_host = self
            .form
            .section_enabled("Ingress")
            .then(|| self.form.text("host"))
            .filter(|host| !host.is_empty());

        ScaffoldArgs {
            name: self.form.text("name"),
            dir: PathBuf::from(self.form.text_or("directory", ".")),
            namespace: self.form.text_or("namespace", "default"),
            image: self.form.text_or("image", "nginx:1.27"),
            port: self.form.number_u16("port").unwrap_or(80),
            envs: self.form.list("environments"),
            replicas: self.form.number_or("replicas", 1),
            tag: (!self.form.text("tag").is_empty()).then(|| self.form.text("tag")),
            ingress_host,
            ingress_class: (!self.form.text("ingress_class").is_empty())
                .then(|| self.form.text("ingress_class")),
            ingress_tls_secret: (!self.form.text("tls_secret").is_empty())
                .then(|| self.form.text("tls_secret")),
            hpa: self.form.section_enabled("Autoscaling"),
            hpa_min: self.form.number_or("hpa_min", 2),
            hpa_max: self.form.number_or("hpa_max", 10),
            hpa_cpu: self.form.number_or("hpa_cpu", 80),
            dry_run: false,
            force: false,
        }
    }

    /// Warn rather than silently drop an enabled-but-incomplete section.
    fn warnings(&self) -> Vec<&'static str> {
        let mut warnings = Vec::new();

        if self.form.section_enabled("Ingress") && self.form.text("host").is_empty() {
            warnings.push("Ingress is on but has no host, so no ingress.yaml will be written");
        }

        warnings
    }

    pub fn refresh(&mut self) {
        let mut preview = match scaffold::render_tree(&self.to_args()) {
            Ok(tree) => tree,
            Err(error) => format!("error: {error}\n"),
        };

        for warning in self.warnings() {
            preview = format!("!! {warning}\n\n{preview}");
        }

        self.preview = preview;
    }
}

// ── rbac ────────────────────────────────────────────────────────────────────

fn rbac_form() -> Form {
    Form::new(vec![
        Section::required(
            "Basics",
            "always required",
            vec![
                Field::text(
                    "name",
                    "name",
                    "my-app",
                    "used for every object in the bundle",
                ),
                Field::text("namespace", "namespace", "default", ""),
                Field::choice(
                    "scope",
                    "scope",
                    &["namespaced", "cluster-wide"],
                    "namespaced",
                    "cluster-wide emits a ClusterRole instead of a Role",
                ),
                Field::text(
                    "output",
                    "output file",
                    "",
                    "blank prints to the terminal only",
                ),
            ],
        ),
        Section::required(
            "Rule",
            "what the role is allowed to do",
            vec![
                Field::list("verbs", "verbs", "get, list, watch, create...")
                    .suggesting(COMMON_VERBS),
                Field::list("resources", "resources", "pods, services, deployments...")
                    .suggesting(COMMON_RESOURCES),
                Field::list("api_groups", "api groups", "blank means the core group")
                    .suggesting(COMMON_API_GROUPS),
                Field::list(
                    "resource_names",
                    "resource names",
                    "restrict to named objects",
                ),
            ],
        ),
        Section::optional(
            "Service account",
            "token mounting and pull secrets",
            vec![
                Field::toggle(
                    "automount",
                    "automount token",
                    true,
                    "automountServiceAccountToken",
                ),
                Field::list(
                    "image_pull_secrets",
                    "pull secrets",
                    "one secret name per entry",
                ),
            ],
        ),
    ])
}

pub struct RbacState {
    pub form: Form,
    pub preview: String,
    pub scroll: u16,
    pub message: String,
}

impl RbacState {
    fn to_args(&self) -> RbacBundleArgs {
        let verbs = self.form.list("verbs");
        let resources = self.form.list("resources");
        let api_groups = self.form.list("api_groups");
        let identity = self.form.section_enabled("Service account");

        let rules = if verbs.is_empty() || resources.is_empty() {
            Vec::new()
        } else {
            vec![RbacRule {
                verbs,
                resources,
                api_groups: if api_groups.is_empty() {
                    vec![String::new()]
                } else {
                    api_groups
                },
                resource_names: Vec::new(),
            }]
        };

        RbacBundleArgs {
            name: self.form.text("name"),
            namespace: self.form.text_or("namespace", "default"),
            rules,
            resource_names: self.form.list("resource_names"),
            cluster_wide: self.form.text("scope") == "cluster-wide",
            no_automount: identity && !self.form.toggle_value("automount"),
            image_pull_secrets: if identity {
                self.form.list("image_pull_secrets")
            } else {
                Vec::new()
            },
            output: None,
            force: false,
        }
    }

    fn output_path(&self) -> PathBuf {
        let explicit = self.form.text("output");
        if explicit.is_empty() {
            PathBuf::from(format!("{}-rbac.yaml", self.form.text("name")))
        } else {
            PathBuf::from(explicit)
        }
    }

    pub fn refresh(&mut self) {
        self.preview = match rbac::render_bundle(&self.to_args()) {
            Ok(yaml) => yaml,
            Err(error) => format!("error: {error}\n"),
        };
    }
}

// ── app ─────────────────────────────────────────────────────────────────────

pub struct App {
    pub screen: Screen,
    pub menu_index: usize,
    pub quit: bool,
    pub standalone: bool,
    pub status: String,
    pub search: SearchState,
    pub generate: GenerateState,
    pub scaffold: ScaffoldState,
    pub rbac: RbacState,
    pub contexts: Vec<ContextEntry>,
    pub context_index: usize,
    pub context_label: String,
    pub namespace_label: String,
}

impl App {
    pub fn new(extra_history: Vec<PathBuf>) -> Result<Self> {
        let candidates = search::build_candidates(SearchSource::All, &extra_history);

        let mut search = SearchState {
            input: String::new(),
            candidates,
            hits: Vec::new(),
            selected: 0,
            picked: None,
            limit: 500,
        };
        search.refresh();

        let mut generate = GenerateState {
            kind_index: 0,
            choosing_kind: false,
            form: generate_form(GenKind::Deployment),
            preview: String::new(),
            scroll: 0,
            message: String::new(),
        };
        generate.refresh();

        let mut scaffold = ScaffoldState {
            form: scaffold_form(),
            preview: String::new(),
            scroll: 0,
            message: String::new(),
        };
        scaffold.refresh();

        let mut rbac = RbacState {
            form: rbac_form(),
            preview: String::new(),
            scroll: 0,
            message: String::new(),
        };
        rbac.refresh();

        let contexts = ctx::list_contexts().unwrap_or_default();
        let context_index = contexts.iter().position(|entry| entry.current).unwrap_or(0);

        Ok(Self {
            screen: Screen::Menu,
            menu_index: 0,
            quit: false,
            standalone: false,
            status: String::new(),
            search,
            generate,
            scaffold,
            rbac,
            contexts,
            context_index,
            context_label: ctx::current_context().unwrap_or_else(|| "no context".to_string()),
            namespace_label: ctx::current_namespace().unwrap_or_else(|| "default".to_string()),
        })
    }

    pub fn launch(mut self) -> Result<()> {
        let mut terminal = ratatui::try_init()?;
        let result = self.event_loop(&mut terminal);
        ratatui::restore();
        result?;

        // Printing after restore keeps the picked command usable in a shell.
        if let Some(command) = self.search.picked {
            println!("{command}");
        }

        Ok(())
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|frame| ui::draw(frame, self))?;

            if event::poll(Duration::from_millis(250))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.handle_key(key.code);
            }
        }
        Ok(())
    }

    fn go_back(&mut self) {
        if self.standalone {
            self.quit = true;
        } else {
            self.screen = Screen::Menu;
            self.status.clear();
        }
    }

    fn handle_key(&mut self, key: KeyCode) {
        match self.screen {
            Screen::Menu => self.handle_menu_key(key),
            Screen::Search => self.handle_search_key(key),
            Screen::Generate => self.handle_generate_key(key),
            Screen::Scaffold => self.handle_scaffold_key(key),
            Screen::Rbac => self.handle_rbac_key(key),
            Screen::Contexts => self.handle_contexts_key(key),
        }
    }

    fn handle_menu_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                let len = MENU.len() as isize;
                self.menu_index = ((self.menu_index as isize - 1).rem_euclid(len)) as usize;
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                let len = MENU.len() as isize;
                self.menu_index = ((self.menu_index as isize + 1).rem_euclid(len)) as usize;
            }
            KeyCode::Enter => {
                self.screen = match self.menu_index {
                    0 => Screen::Generate,
                    1 => Screen::Scaffold,
                    2 => Screen::Rbac,
                    3 => Screen::Search,
                    _ => Screen::Contexts,
                };
                self.status.clear();
            }
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc => self.go_back(),
            KeyCode::Enter => {
                if let Some(candidate) = self.search.selected_candidate() {
                    self.search.picked = Some(candidate.cmd.clone());
                    self.quit = true;
                }
            }
            KeyCode::Up => self.search.move_selection(-1),
            KeyCode::Down => self.search.move_selection(1),
            KeyCode::PageDown => self.search.move_selection(10),
            KeyCode::PageUp => self.search.move_selection(-10),
            KeyCode::Backspace => {
                self.search.input.pop();
                self.search.refresh();
            }
            KeyCode::Char(character) => {
                self.search.input.push(character);
                self.search.refresh();
            }
            _ => {}
        }
    }

    /// Screen-level keys are only honoured when the form is not capturing text.
    fn handle_generate_key(&mut self, key: KeyCode) {
        if self.generate.choosing_kind {
            match key {
                KeyCode::Esc => self.generate.choosing_kind = false,
                KeyCode::Up | KeyCode::Char('k') => {
                    let len = GenKind::ALL.len() as isize;
                    let index = ((self.generate.kind_index as isize - 1).rem_euclid(len)) as usize;
                    self.generate.select_kind(index);
                    self.generate.refresh();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let len = GenKind::ALL.len() as isize;
                    let index = ((self.generate.kind_index as isize + 1).rem_euclid(len)) as usize;
                    self.generate.select_kind(index);
                    self.generate.refresh();
                }
                KeyCode::Enter => {
                    self.generate.choosing_kind = false;
                    self.generate.refresh();
                }
                _ => {}
            }
            return;
        }

        if !self.generate.form.editing {
            match key {
                KeyCode::Esc => return self.go_back(),
                KeyCode::Char('k') => {
                    self.generate.choosing_kind = true;
                    return;
                }
                KeyCode::Char('r') => {
                    self.generate.refresh();
                    self.generate.message.clear();
                    return;
                }
                KeyCode::Char('s') => {
                    self.generate.message = self.save_generate();
                    return;
                }
                KeyCode::PageDown => {
                    self.generate.scroll = self.generate.scroll.saturating_add(10);
                    return;
                }
                KeyCode::PageUp => {
                    self.generate.scroll = self.generate.scroll.saturating_sub(10);
                    return;
                }
                _ => {}
            }
        }

        if self.generate.form.handle_key(key) {
            self.generate.refresh();
        }
    }

    fn handle_scaffold_key(&mut self, key: KeyCode) {
        if !self.scaffold.form.editing {
            match key {
                KeyCode::Esc => return self.go_back(),
                KeyCode::Char('r') => {
                    self.scaffold.refresh();
                    self.scaffold.message.clear();
                    return;
                }
                KeyCode::Char('s') => {
                    self.scaffold.message = self.save_scaffold();
                    return;
                }
                KeyCode::PageDown => {
                    self.scaffold.scroll = self.scaffold.scroll.saturating_add(10);
                    return;
                }
                KeyCode::PageUp => {
                    self.scaffold.scroll = self.scaffold.scroll.saturating_sub(10);
                    return;
                }
                _ => {}
            }
        }

        if self.scaffold.form.handle_key(key) {
            self.scaffold.refresh();
        }
    }

    fn handle_rbac_key(&mut self, key: KeyCode) {
        if !self.rbac.form.editing {
            match key {
                KeyCode::Esc => return self.go_back(),
                KeyCode::Char('r') => {
                    self.rbac.refresh();
                    self.rbac.message.clear();
                    return;
                }
                KeyCode::Char('s') => {
                    self.rbac.message = self.save_rbac();
                    return;
                }
                KeyCode::PageDown => {
                    self.rbac.scroll = self.rbac.scroll.saturating_add(10);
                    return;
                }
                KeyCode::PageUp => {
                    self.rbac.scroll = self.rbac.scroll.saturating_sub(10);
                    return;
                }
                _ => {}
            }
        }

        if self.rbac.form.handle_key(key) {
            self.rbac.refresh();
        }
    }

    fn handle_contexts_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => self.go_back(),
            KeyCode::Char('r') => {
                self.contexts = ctx::list_contexts().unwrap_or_default();
                self.context_index = self
                    .contexts
                    .iter()
                    .position(|entry| entry.current)
                    .unwrap_or(0);
                self.context_label =
                    ctx::current_context().unwrap_or_else(|| "no context".to_string());
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.contexts.is_empty() {
                    let len = self.contexts.len() as isize;
                    self.context_index =
                        ((self.context_index as isize - 1).rem_euclid(len)) as usize;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.contexts.is_empty() {
                    let len = self.contexts.len() as isize;
                    self.context_index =
                        ((self.context_index as isize + 1).rem_euclid(len)) as usize;
                }
            }
            KeyCode::Enter => {
                if let Some(entry) = self.contexts.get(self.context_index) {
                    let name = entry.name.clone();
                    match ctx::use_context(&name) {
                        Ok(()) => {
                            self.context_label = name.clone();
                            self.namespace_label =
                                ctx::current_namespace().unwrap_or_else(|| "default".to_string());
                            self.contexts = ctx::list_contexts().unwrap_or_default();
                            self.status = format!("switched to context {name}");
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
            _ => {}
        }
    }

    fn save_generate(&mut self) -> String {
        let path = self.generate.output_path();
        match util::write_file(&path, &self.generate.preview, true) {
            Ok(()) => format!("wrote {}", path.display()),
            Err(error) => error.to_string(),
        }
    }

    fn save_rbac(&mut self) -> String {
        let path = self.rbac.output_path();
        match util::write_file(&path, &self.rbac.preview, true) {
            Ok(()) => format!("wrote {}", path.display()),
            Err(error) => error.to_string(),
        }
    }

    fn save_scaffold(&mut self) -> String {
        match scaffold::write_tree(&self.scaffold.to_args()) {
            Ok(paths) => format!("created {} files", paths.len()),
            Err(error) => error.to_string(),
        }
    }
}

pub fn run(args: TuiArgs) -> Result<()> {
    App::new(args.history_file)?.launch()
}

pub fn run_search(args: &SearchArgs) -> Result<()> {
    let mut app = App::new(args.history_file.clone())?;
    app.standalone = true;
    app.screen = Screen::Search;
    app.search.limit = args.limit.max(500);
    app.search.input = args.query.join(" ");
    app.search.refresh();
    app.launch()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section_mut<'a>(form: &'a mut Form, title: &str) -> &'a mut Section {
        form.sections
            .iter_mut()
            .find(|section| section.title == title)
            .unwrap_or_else(|| panic!("no section named {title}"))
    }

    fn field_mut<'a>(form: &'a mut Form, key: &str) -> &'a mut Field {
        form.sections
            .iter_mut()
            .flat_map(|section| section.fields.iter_mut())
            .find(|field| field.key == key)
            .unwrap_or_else(|| panic!("no field named {key}"))
    }

    fn generate(kind: GenKind) -> GenerateState {
        let mut state = GenerateState {
            kind_index: GenKind::ALL
                .iter()
                .position(|candidate| *candidate == kind)
                .unwrap(),
            choosing_kind: false,
            form: generate_form(kind),
            preview: String::new(),
            scroll: 0,
            message: String::new(),
        };
        state.refresh();
        state
    }

    fn scaffold_state() -> ScaffoldState {
        let mut state = ScaffoldState {
            form: scaffold_form(),
            preview: String::new(),
            scroll: 0,
            message: String::new(),
        };
        state.refresh();
        state
    }

    fn rbac_state() -> RbacState {
        let mut state = RbacState {
            form: rbac_form(),
            preview: String::new(),
            scroll: 0,
            message: String::new(),
        };
        state.refresh();
        state
    }

    // ── generate: skip vs include ────────────────────────────────────────────

    #[test]
    fn the_environment_section_is_skipped_until_switched_on() {
        let mut state = generate(GenKind::Deployment);
        assert!(
            !state.preview.contains("env:"),
            "an untouched optional section must not reach the manifest"
        );

        section_mut(&mut state.form, "Environment").enabled = true;
        field_mut(&mut state.form, "env")
            .items
            .push("LOG_LEVEL=debug".into());
        field_mut(&mut state.form, "env_from_secret")
            .items
            .push("DB_PASS=db-creds".into());
        state.refresh();

        assert!(state.preview.contains("value: \"debug\""));
        assert!(state.preview.contains("secretKeyRef"));
    }

    #[test]
    fn switching_a_section_back_off_removes_it_again() {
        let mut state = generate(GenKind::Deployment);

        section_mut(&mut state.form, "Volumes").enabled = true;
        field_mut(&mut state.form, "mount_configmap")
            .items
            .push("app-config:/etc/app".into());
        state.refresh();
        assert!(state.preview.contains("mountPath: /etc/app"));
        assert!(state.preview.contains("volumeMounts"));

        section_mut(&mut state.form, "Volumes").enabled = false;
        state.refresh();
        assert!(!state.preview.contains("volumeMounts"));
    }

    #[test]
    fn a_malformed_list_entry_is_ignored_rather_than_emitted() {
        let mut state = generate(GenKind::Deployment);

        section_mut(&mut state.form, "Volumes").enabled = true;
        field_mut(&mut state.form, "mount_configmap")
            .items
            .push("no-colon-here".into());
        field_mut(&mut state.form, "mount_configmap")
            .items
            .push("good:/etc/good".into());
        state.refresh();

        assert!(state.preview.contains("mountPath: /etc/good"));
        assert!(!state.preview.contains("no-colon-here"));
    }

    #[test]
    fn the_data_section_drives_configmap_entries() {
        let mut state = generate(GenKind::ConfigMap);
        // Note: "metadata:" contains "data:", so match on a line start.
        assert!(!state.preview.contains("\ndata:"));

        section_mut(&mut state.form, "Data").enabled = true;
        field_mut(&mut state.form, "data")
            .items
            .push("LOG_LEVEL=debug".into());
        state.refresh();

        assert!(state.preview.contains("\ndata:"));
        assert!(state.preview.contains("LOG_LEVEL: \"debug\""));
    }

    #[test]
    fn the_job_section_drives_the_schedule() {
        let mut state = generate(GenKind::CronJob);
        assert!(
            state.preview.contains("0 2 * * *"),
            "default schedule expected"
        );

        section_mut(&mut state.form, "Job").enabled = true;
        field_mut(&mut state.form, "schedule").value = "*/5 * * * *".into();
        state.refresh();

        assert!(state.preview.contains("*/5 * * * *"));
        assert!(!state.preview.contains("0 2 * * *"));
    }

    #[test]
    fn the_identity_section_drives_service_account_fields() {
        let mut state = generate(GenKind::ServiceAccount);
        assert!(state.preview.contains("automountServiceAccountToken: true"));

        section_mut(&mut state.form, "Identity").enabled = true;
        field_mut(&mut state.form, "automount").value = "no".into();
        field_mut(&mut state.form, "image_pull_secrets")
            .items
            .push("regcred".into());
        state.refresh();

        assert!(
            state
                .preview
                .contains("automountServiceAccountToken: false")
        );
        assert!(state.preview.contains("regcred"));
    }

    #[test]
    fn switching_kind_swaps_the_available_sections() {
        let mut state = generate(GenKind::Deployment);
        assert!(state.form.section_enabled("Container"));
        assert!(
            !state.form.section_applies(
                state
                    .form
                    .sections
                    .iter()
                    .find(|s| s.title == "Ingress")
                    .unwrap()
            )
        );

        let ingress = state
            .form
            .sections
            .iter()
            .position(|s| s.title == "Ingress")
            .unwrap();
        state.select_kind(
            GenKind::ALL
                .iter()
                .position(|k| *k == GenKind::Ingress)
                .unwrap(),
        );
        state.refresh();

        assert!(state.form.section_applies(&state.form.sections[ingress]));

        // The ingress section starts off, so the preview explains itself.
        assert!(state.preview.contains("host"));
    }

    // ── scaffold ─────────────────────────────────────────────────────────────

    #[test]
    fn the_scaffold_ingress_needs_the_section_and_a_host() {
        let mut state = scaffold_state();
        assert!(!state.preview.contains("base/ingress.yaml"));

        // Switched on but incomplete: warned, not silently dropped.
        section_mut(&mut state.form, "Ingress").enabled = true;
        state.refresh();
        assert!(!state.preview.contains("base/ingress.yaml"));
        assert!(
            state.preview.contains("!!"),
            "an incomplete section should be surfaced: {}",
            state.preview
        );

        field_mut(&mut state.form, "host").value = "app.example.com".into();
        state.refresh();
        assert!(state.preview.contains("base/ingress.yaml"));
        assert!(state.preview.contains("app.example.com"));
        assert!(!state.preview.contains("!!"));
    }

    #[test]
    fn the_scaffold_hpa_is_opt_in_and_configurable() {
        let mut state = scaffold_state();
        assert!(!state.preview.contains("base/hpa.yaml"));

        section_mut(&mut state.form, "Autoscaling").enabled = true;
        field_mut(&mut state.form, "hpa_max").value = "25".into();
        state.refresh();

        assert!(state.preview.contains("base/hpa.yaml"));
        assert!(state.preview.contains("maxReplicas: 25"));
    }

    #[test]
    fn the_scaffold_env_list_drives_the_overlay_directories() {
        let mut state = scaffold_state();
        assert_eq!(state.form.list("environments").len(), 0);

        field_mut(&mut state.form, "environments")
            .items
            .push("staging".into());
        state.refresh();

        assert!(
            state
                .preview
                .contains("overlays/staging/kustomization.yaml")
        );
    }

    // ── rbac ─────────────────────────────────────────────────────────────────

    /// The bundle refuses to render without a rule, so most tests seed one.
    fn seed_rule(form: &mut Form) {
        field_mut(form, "verbs").items.push("get".into());
        field_mut(form, "resources").items.push("pods".into());
    }

    #[test]
    fn the_rbac_identity_section_gates_token_and_pull_secrets() {
        let mut state = rbac_state();
        seed_rule(&mut state.form);
        state.refresh();

        assert!(state.preview.contains("automountServiceAccountToken: true"));
        assert!(!state.preview.contains("imagePullSecrets"));

        section_mut(&mut state.form, "Service account").enabled = true;
        field_mut(&mut state.form, "automount").value = "no".into();
        field_mut(&mut state.form, "image_pull_secrets")
            .items
            .push("regcred".into());
        state.refresh();

        assert!(
            state
                .preview
                .contains("automountServiceAccountToken: false")
        );
        assert!(state.preview.contains("regcred"));

        // Switching it back off returns the ServiceAccount to its defaults.
        section_mut(&mut state.form, "Service account").enabled = false;
        state.refresh();
        assert!(state.preview.contains("automountServiceAccountToken: true"));
        assert!(!state.preview.contains("imagePullSecrets"));
    }

    #[test]
    fn the_rbac_rules_come_from_the_list_fields() {
        let mut state = rbac_state();
        assert!(
            state.preview.contains("error:"),
            "an empty rule set should explain itself, got: {}",
            state.preview
        );

        for verb in ["get", "list"] {
            field_mut(&mut state.form, "verbs").items.push(verb.into());
        }
        field_mut(&mut state.form, "resources")
            .items
            .push("pods".into());
        state.refresh();

        assert!(!state.preview.contains("error:"));
        assert!(state.preview.contains("- \"get\""));
        assert!(state.preview.contains("- \"pods\""));
    }
}

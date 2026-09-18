use clap::ValueEnum;

/// A key/value pair used for labels, annotations and ConfigMap/Secret entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub key: String,
    pub value: String,
}

impl Label {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// An environment variable on a container. Exactly one of `value`,
/// `config_map_ref` or `secret_ref` should be set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvVar {
    pub name: String,
    pub value: Option<String>,
    pub config_map_ref: Option<String>,
    pub secret_ref: Option<String>,
}

impl EnvVar {
    pub fn literal(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Some(value.into()),
            config_map_ref: None,
            secret_ref: None,
        }
    }
}

/// A volume backed by a ConfigMap or Secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub name: String,
    /// Either `configMap` or `secret`.
    pub kind: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeMount {
    pub name: String,
    pub mount_path: String,
    pub read_only: bool,
}

/// Every resource kind `kute gen` can render.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum GenKind {
    #[value(name = "deployment")]
    Deployment,
    #[value(name = "statefulset")]
    StatefulSet,
    #[value(name = "daemonset")]
    DaemonSet,
    #[value(name = "job")]
    Job,
    #[value(name = "cronjob")]
    CronJob,
    #[value(name = "service")]
    Service,
    #[value(name = "configmap")]
    ConfigMap,
    #[value(name = "secret")]
    Secret,
    #[value(name = "ingress")]
    Ingress,
    #[value(name = "pvc")]
    Pvc,
    #[value(name = "namespace")]
    Namespace,
    #[value(name = "hpa")]
    Hpa,
    #[value(name = "networkpolicy")]
    NetworkPolicy,
    #[value(name = "serviceaccount")]
    ServiceAccount,
}

impl GenKind {
    /// All kinds, in the order shown by `kute gen list`.
    pub const ALL: [GenKind; 14] = [
        GenKind::Deployment,
        GenKind::StatefulSet,
        GenKind::DaemonSet,
        GenKind::Job,
        GenKind::CronJob,
        GenKind::Service,
        GenKind::ConfigMap,
        GenKind::Secret,
        GenKind::Ingress,
        GenKind::Pvc,
        GenKind::Namespace,
        GenKind::Hpa,
        GenKind::NetworkPolicy,
        GenKind::ServiceAccount,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            GenKind::Deployment => "deployment",
            GenKind::StatefulSet => "statefulset",
            GenKind::DaemonSet => "daemonset",
            GenKind::Job => "job",
            GenKind::CronJob => "cronjob",
            GenKind::Service => "service",
            GenKind::ConfigMap => "configmap",
            GenKind::Secret => "secret",
            GenKind::Ingress => "ingress",
            GenKind::Pvc => "pvc",
            GenKind::Namespace => "namespace",
            GenKind::Hpa => "hpa",
            GenKind::NetworkPolicy => "networkpolicy",
            GenKind::ServiceAccount => "serviceaccount",
        }
    }

    /// The Kubernetes `kind:` value written into the manifest.
    pub fn k8s_kind(self) -> &'static str {
        match self {
            GenKind::Deployment => "Deployment",
            GenKind::StatefulSet => "StatefulSet",
            GenKind::DaemonSet => "DaemonSet",
            GenKind::Job => "Job",
            GenKind::CronJob => "CronJob",
            GenKind::Service => "Service",
            GenKind::ConfigMap => "ConfigMap",
            GenKind::Secret => "Secret",
            GenKind::Ingress => "Ingress",
            GenKind::Pvc => "PersistentVolumeClaim",
            GenKind::Namespace => "Namespace",
            GenKind::Hpa => "HorizontalPodAutoscaler",
            GenKind::NetworkPolicy => "NetworkPolicy",
            GenKind::ServiceAccount => "ServiceAccount",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            GenKind::Deployment => "Stateless workload with rolling updates",
            GenKind::StatefulSet => "Stable-identity workload with ordered rollout",
            GenKind::DaemonSet => "One pod per node (agents, log shippers)",
            GenKind::Job => "Run-to-completion batch task",
            GenKind::CronJob => "Scheduled batch task",
            GenKind::Service => "Stable virtual IP in front of pods",
            GenKind::ConfigMap => "Non-confidential key/value configuration",
            GenKind::Secret => "Confidential key/value data",
            GenKind::Ingress => "HTTP(S) routing into the cluster",
            GenKind::Pvc => "PersistentVolumeClaim for storage",
            GenKind::Namespace => "Namespace with standard labels",
            GenKind::Hpa => "HorizontalPodAutoscaler on CPU utilisation",
            GenKind::NetworkPolicy => "Allow same-namespace ingress only",
            GenKind::ServiceAccount => "Identity for pods, with pull secrets",
        }
    }
}

/// The full template context. Every manifest template reads from this single
/// struct, so adding a flag never requires touching more than one type.
#[derive(Debug, Clone)]
pub struct GenContext {
    pub name: String,
    pub namespace: Option<String>,
    pub labels: Vec<Label>,
    pub annotations: Vec<Label>,

    pub selector_key: String,
    pub selector_value: String,

    // Workload / pod spec
    pub replicas: u32,
    pub image: String,
    pub image_pull_policy: String,
    pub container_name: String,
    pub port: u16,
    pub port_name: String,
    pub service_account: Option<String>,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub env: Vec<EnvVar>,
    pub cpu_request: String,
    pub mem_request: String,
    pub cpu_limit: String,
    pub mem_limit: String,
    pub volumes: Vec<Volume>,
    pub mounts: Vec<VolumeMount>,

    // Service
    pub service_type: String,
    pub service_port: u16,
    pub target_port: u16,

    // ConfigMap / Secret
    pub data: Vec<Label>,
    pub secret_type: String,

    // Ingress
    pub host: String,
    pub path: String,
    pub path_type: String,
    pub ingress_class: Option<String>,
    pub tls_secret: Option<String>,
    pub backend_service: String,

    // Job / CronJob
    pub schedule: String,
    pub restart_policy: String,
    pub backoff_limit: i32,
    pub completions: Option<i32>,
    pub parallelism: Option<i32>,
    pub suspend: bool,

    // PersistentVolumeClaim
    pub storage: String,
    pub access_mode: String,
    pub storage_class: Option<String>,

    // HorizontalPodAutoscaler
    pub min_replicas: u32,
    pub max_replicas: u32,
    pub target_cpu: u32,

    // NetworkPolicy
    pub netpol_ports: Vec<u16>,

    // ServiceAccount
    pub automount: bool,
    pub image_pull_secrets: Vec<String>,
}

impl Default for GenContext {
    fn default() -> Self {
        Self {
            name: String::new(),
            namespace: None,
            labels: Vec::new(),
            annotations: Vec::new(),

            selector_key: "app.kubernetes.io/name".into(),
            selector_value: String::new(),

            replicas: 1,
            image: "nginx:1.27".into(),
            image_pull_policy: "IfNotPresent".into(),
            container_name: String::new(),
            port: 80,
            port_name: "http".into(),
            service_account: None,
            command: Vec::new(),
            args: Vec::new(),
            env: Vec::new(),
            cpu_request: "100m".into(),
            mem_request: "128Mi".into(),
            cpu_limit: "500m".into(),
            mem_limit: "512Mi".into(),
            volumes: Vec::new(),
            mounts: Vec::new(),

            service_type: "ClusterIP".into(),
            service_port: 80,
            target_port: 80,

            data: Vec::new(),
            secret_type: "Opaque".into(),

            host: String::new(),
            path: "/".into(),
            path_type: "Prefix".into(),
            ingress_class: None,
            tls_secret: None,
            backend_service: String::new(),

            schedule: "0 2 * * *".into(),
            restart_policy: "OnFailure".into(),
            backoff_limit: 6,
            completions: None,
            parallelism: None,
            suspend: false,

            storage: "1Gi".into(),
            access_mode: "ReadWriteOnce".into(),
            storage_class: None,

            min_replicas: 2,
            max_replicas: 10,
            target_cpu: 80,

            netpol_ports: Vec::new(),

            automount: true,
            image_pull_secrets: Vec::new(),
        }
    }
}

impl GenContext {
    /// A context pre-populated with conventional labels and selector values.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            labels: vec![
                Label::new("app.kubernetes.io/name", name.clone()),
                Label::new("app.kubernetes.io/managed-by", "kute"),
            ],
            selector_value: name.clone(),
            container_name: name.clone(),
            backend_service: name.clone(),
            name,
            ..Self::default()
        }
    }
}

/// A rule inside a Role or ClusterRole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RbacRule {
    pub verbs: Vec<String>,
    pub resources: Vec<String>,
    pub api_groups: Vec<String>,
    pub resource_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RbacContext {
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
    pub labels: Vec<Label>,
    pub annotations: Vec<Label>,
    pub rules: Vec<RbacRule>,
}

#[derive(Debug, Clone)]
pub struct Subject {
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BindingContext {
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
    pub labels: Vec<Label>,
    pub annotations: Vec<Label>,
    pub role_ref_kind: String,
    pub role_ref_name: String,
    pub subjects: Vec<Subject>,
}

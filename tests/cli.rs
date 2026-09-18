//! End-to-end tests that drive the real binary, so argument parsing, template
//! rendering and file writing are all exercised exactly as a user would.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn kute(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kute"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run kute")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

/// Split a YAML stream on `---` separators.
///
/// Splitting by hand (rather than using a multi-document parser) means the
/// tests also assert that our own document separators are well formed.
fn split_documents(text: &str) -> Vec<String> {
    let mut chunks = vec![String::new()];

    for line in text.lines() {
        if line.trim_end() == "---" {
            chunks.push(String::new());
        } else {
            let chunk = chunks.last_mut().expect("at least one chunk");
            chunk.push_str(line);
            chunk.push('\n');
        }
    }

    chunks
}

/// Parse every YAML document in `text`, asserting the whole stream is valid.
fn parse_documents(text: &str) -> Vec<serde_yaml_ng::Value> {
    let mut documents = Vec::new();

    for chunk in split_documents(text) {
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&chunk)
            .unwrap_or_else(|error| panic!("generated YAML does not parse: {error}\n---\n{text}"));
        if !value.is_null() {
            documents.push(value);
        }
    }

    documents
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kute-it-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ── gen ─────────────────────────────────────────────────────────────────────

const GENERATED_KINDS: &[(&str, &[&str])] = &[
    ("deployment", &["my-app"]),
    ("statefulset", &["my-app"]),
    ("daemonset", &["my-app"]),
    ("job", &["my-app"]),
    ("cronjob", &["my-app"]),
    ("service", &["my-app"]),
    ("configmap", &["my-app"]),
    ("secret", &["my-app"]),
    ("pvc", &["my-app"]),
    ("namespace", &["my-app"]),
    ("hpa", &["my-app"]),
    ("networkpolicy", &["my-app"]),
    ("serviceaccount", &["my-app"]),
];

#[test]
fn every_kind_generates_parseable_yaml() {
    for (kind, args) in GENERATED_KINDS {
        let mut argv = vec!["gen", kind];
        argv.extend_from_slice(args);

        let output = kute(&argv);
        assert!(
            output.status.success(),
            "`kute gen {kind}` failed: {}",
            stderr(&output)
        );

        let documents = parse_documents(&stdout(&output));
        assert_eq!(
            documents.len(),
            1,
            "`kute gen {kind}` should emit one document"
        );

        let document = &documents[0];
        assert_eq!(
            document["metadata"]["name"].as_str(),
            Some("my-app"),
            "`kute gen {kind}` wrote the wrong name"
        );
    }
}

#[test]
fn ingress_requires_a_host_before_rendering() {
    let output = kute(&["gen", "ingress", "web"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("host"));

    let output = kute(&["gen", "ingress", "web", "--host", "app.example.com"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let documents = parse_documents(&stdout(&output));
    assert_eq!(documents[0]["spec"]["rules"][0]["host"], "app.example.com");
}

#[test]
fn deployment_carries_env_resources_and_mounts() {
    let output = kute(&[
        "gen",
        "deployment",
        "api",
        "--namespace",
        "prod",
        "--image",
        "registry.local/api:2.1",
        "--replicas",
        "4",
        "--port",
        "8080",
        "--env",
        "LOG=info",
        "--env-from-secret",
        "DB_PASS=db-creds",
        "--mount-configmap",
        "app-config:/etc/app",
        "--mount-secret",
        "tls:/etc/tls",
        "--label",
        "team=platform",
        "--service-account",
        "api-sa",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let documents = parse_documents(&stdout(&output));
    let deployment = &documents[0];
    let pod = &deployment["spec"]["template"]["spec"];
    let container = &pod["containers"][0];

    assert_eq!(deployment["metadata"]["namespace"], "prod");
    assert_eq!(deployment["metadata"]["labels"]["team"], "platform");
    assert_eq!(deployment["spec"]["replicas"], 4);
    assert_eq!(container["image"], "registry.local/api:2.1");
    assert_eq!(container["ports"][0]["containerPort"], 8080);
    assert_eq!(pod["serviceAccountName"], "api-sa");
    assert_eq!(container["env"][0]["value"], "info");
    assert_eq!(
        container["env"][1]["valueFrom"]["secretKeyRef"]["name"],
        "db-creds"
    );
    assert_eq!(container["volumeMounts"][0]["mountPath"], "/etc/app");
    assert_eq!(pod["volumes"][0]["configMap"]["name"], "app-config");
    assert_eq!(pod["volumes"][1]["secret"]["secretName"], "tls");
}

#[test]
fn a_custom_label_overrides_the_generated_one_consistently() {
    let output = kute(&[
        "gen",
        "service",
        "api",
        "-l",
        "app.kubernetes.io/name=renamed",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let documents = parse_documents(&stdout(&output));

    // The selector must follow the label, otherwise the Service selects nothing.
    assert_eq!(
        documents[0]["spec"]["selector"]["app.kubernetes.io/name"],
        "renamed"
    );
    assert_eq!(
        documents[0]["metadata"]["labels"]["app.kubernetes.io/name"],
        "renamed"
    );
}

#[test]
fn gen_without_a_kind_lists_every_kind() {
    let output = kute(&["gen"]);
    assert!(output.status.success());

    let listing = stdout(&output);
    for (kind, _) in GENERATED_KINDS {
        assert!(listing.contains(kind), "`{kind}` missing from the listing");
    }
}

#[test]
fn gen_without_a_name_is_an_error() {
    let output = kute(&["gen", "deployment"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("NAME"));
}

#[test]
fn output_file_is_written_once_and_then_protected() {
    let dir = scratch("output");
    let path = dir.join("nested/app.yaml");
    let path_arg = path.to_str().unwrap();

    let output = kute(&["gen", "configmap", "app", "-o", path_arg]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(path.exists(), "nested directories should be created");

    // Without --force the existing file must be preserved.
    let output = kute(&["gen", "configmap", "app", "-o", path_arg]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("--force"));

    let output = kute(&["gen", "configmap", "app", "-o", path_arg, "--force"]);
    assert!(output.status.success(), "{}", stderr(&output));
}

// ── rbac ────────────────────────────────────────────────────────────────────

#[test]
fn rbac_bundle_emits_a_linked_triple() {
    let output = kute(&[
        "rbac",
        "bundle",
        "reader",
        "-n",
        "prod",
        "-r",
        "get,list,watch:pods,services",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let documents = parse_documents(&stdout(&output));
    let kinds: Vec<&str> = documents
        .iter()
        .map(|document| document["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["ServiceAccount", "Role", "RoleBinding"]);

    // The binding must point at the role and subject the bundle just created.
    let binding = &documents[2];
    assert_eq!(binding["roleRef"]["kind"], "Role");
    assert_eq!(binding["roleRef"]["name"], "reader");
    assert_eq!(binding["subjects"][0]["name"], "reader");
    assert_eq!(binding["subjects"][0]["namespace"], "prod");

    let rule = &documents[1]["rules"][0];
    assert_eq!(rule["verbs"][0], "get");
    assert_eq!(rule["resources"][1], "services");
    assert_eq!(rule["apiGroups"][0], "");
}

#[test]
fn rbac_bundle_cluster_wide_switches_the_kinds() {
    let output = kute(&[
        "rbac",
        "bundle",
        "reader",
        "-n",
        "prod",
        "-r",
        "get:nodes",
        "--cluster-wide",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let documents = parse_documents(&stdout(&output));
    let kinds: Vec<&str> = documents
        .iter()
        .map(|document| document["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec!["ServiceAccount", "ClusterRole", "ClusterRoleBinding"]
    );
}

#[test]
fn cluster_roles_are_never_namespaced() {
    let output = kute(&[
        "rbac",
        "clusterrole",
        "reader",
        "-n",
        "prod",
        "-r",
        "get:nodes",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let documents = parse_documents(&stdout(&output));
    assert!(
        documents[0]["metadata"]["namespace"].is_null(),
        "a ClusterRole must not carry a namespace"
    );
}

#[test]
fn rbac_role_requires_at_least_one_rule() {
    let output = kute(&["rbac", "role", "reader"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("--rule"));
}

#[test]
fn a_bad_rule_is_rejected_with_a_helpful_message() {
    let output = kute(&["rbac", "role", "reader", "-r", "get"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("VERBS:RESOURCES"));
}

#[test]
fn resource_names_are_applied_to_the_rule() {
    let output = kute(&[
        "rbac",
        "role",
        "reader",
        "-r",
        "get:pods",
        "--resource-name",
        "web-0",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let documents = parse_documents(&stdout(&output));
    assert_eq!(documents[0]["rules"][0]["resourceNames"][0], "web-0");
}

// ── scaffold ────────────────────────────────────────────────────────────────

#[test]
fn scaffold_writes_a_kustomize_tree() {
    let dir = scratch("scaffold");

    let output = kute(&[
        "scaffold",
        "web",
        "--dir",
        dir.to_str().unwrap(),
        "--namespace",
        "prod",
        "--image",
        "ghcr.io/acme/web:1.2.3",
        "--envs",
        "dev,prod",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let root = dir.join("web");
    for relative in [
        "base/deployment.yaml",
        "base/service.yaml",
        "base/kustomization.yaml",
        "overlays/dev/kustomization.yaml",
        "overlays/prod/kustomization.yaml",
    ] {
        let path = root.join(relative);
        assert!(path.exists(), "missing {relative}");
        parse_documents(&fs::read_to_string(&path).unwrap());
    }

    // The overlay must build on the base and pin the image tag.
    let overlay = fs::read_to_string(root.join("overlays/prod/kustomization.yaml")).unwrap();
    let documents = parse_documents(&overlay);
    assert_eq!(documents[0]["namespace"], "prod");
    assert_eq!(documents[0]["resources"][0], "../../base");
    assert_eq!(documents[0]["images"][0]["name"], "ghcr.io/acme/web");
    assert_eq!(documents[0]["images"][0]["newTag"], "1.2.3");

    // Running again must refuse to clobber without --force.
    let again = kute(&["scaffold", "web", "--dir", dir.to_str().unwrap()]);
    assert!(!again.status.success());
    assert!(stderr(&again).contains("--force"));
}

#[test]
fn scaffold_dry_run_writes_nothing() {
    let dir = scratch("scaffold-dry");

    let output = kute(&[
        "scaffold",
        "web",
        "--dir",
        dir.to_str().unwrap(),
        "--dry-run",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("kustomization.yaml"));
    assert!(
        !dir.join("web").exists(),
        "--dry-run must not touch the filesystem"
    );
}

#[test]
fn scaffold_rejects_invalid_names() {
    let dir = scratch("scaffold-bad");
    let output = kute(&["scaffold", "Bad_Name", "--dir", dir.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("RFC 1123"));
}

// ── search ──────────────────────────────────────────────────────────────────

#[test]
fn search_finds_a_curated_example() {
    let output = kute(&[
        "search",
        "--source",
        "examples",
        "--no-color",
        "rollout",
        "status",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let results = stdout(&output);
    assert!(
        results.contains("kubectl rollout status deploy/NAME -n NS"),
        "unexpected results:\n{results}"
    );
}

#[test]
fn search_print_emits_a_bare_command() {
    let output = kute(&[
        "search",
        "--source",
        "examples",
        "--no-color",
        "--print",
        "port-forward",
        "service",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let printed = stdout(&output);
    let printed = printed.trim();
    assert!(
        printed.starts_with("kubectl "),
        "expected a bare command, got `{printed}`"
    );
    assert!(
        printed.contains("port-forward"),
        "expected a port-forward command, got `{printed}`"
    );
    assert!(
        !printed.contains('\n'),
        "expected exactly one line, got `{printed}`"
    );
}

#[test]
fn search_json_is_machine_readable() {
    let output = kute(&["search", "--source", "examples", "--json", "logs", "tail"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let parsed: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    let results = parsed.as_array().unwrap();
    assert!(!results.is_empty());
    assert!(
        results[0]["command"]
            .as_str()
            .unwrap()
            .starts_with("kubectl ")
    );
    assert_eq!(results[0]["source"], "examples");
}

#[test]
fn search_reads_a_custom_history_file() {
    let dir = scratch("history");
    let history = dir.join("history");
    fs::write(
        &history,
        "kubectl get pods -n kube-system\nls -la\nkubectl drain node-1 --ignore-daemonsets\n",
    )
    .unwrap();

    let output = kute(&[
        "search",
        "--source",
        "history",
        "--no-color",
        "--history-file",
        history.to_str().unwrap(),
        "drain",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let results = stdout(&output);
    assert!(
        results.contains("kubectl drain node-1"),
        "unexpected results:\n{results}"
    );
    assert!(!results.contains("ls -la"), "non-kubectl history leaked in");
}

#[test]
fn search_with_no_matches_is_not_an_error() {
    let output = kute(&["search", "--source", "examples", "--no-color", "zzzqqqxxx"]);
    assert!(output.status.success());
    assert_eq!(stdout(&output).trim(), "");
}

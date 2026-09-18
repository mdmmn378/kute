# kute

A holistic Kubernetes helper in one binary: generate manifests, scaffold kustomize
trees, build RBAC objects, and fuzzy-find kubectl commands from a built-in corpus
plus your own shell history.

Both a scriptable CLI and an interactive TUI over the same code.

```
kute
├── gen         generate a manifest for any common kind
├── scaffold    base + overlays kustomize tree
├── rbac        ServiceAccount / Role / ClusterRole / bindings / bundle
├── search      fuzzy kubectl command finder (examples + shell history)
├── ctx         inspect and switch kubectl contexts
└── tui         everything above, interactively
```

## Install

```sh
git clone <this repo> && cd kute
cargo install --path .
```

## Interactive mode

`kute tui` opens a menu over all four generators, with a live preview pane:

- **Generate** — pick a kind, edit fields, watch the YAML update as you type, `s` to save.
- **Scaffold** — fill in the app details, preview the whole file tree, `s` to write it.
- **Build RBAC** — describe a rule, get ServiceAccount + Role + RoleBinding, `s` to save.
- **Search** — type to fuzzy-filter; `enter` prints the chosen command on exit.
- **Contexts** — see every context, `enter` to switch.

`kute search -i` jumps straight into the search screen, so:

```sh
kute search -i          # browse, then hit enter; the command lands on stdout
```

## Generating manifests

```sh
kute gen                                # list every supported kind
kute gen deployment api -n prod --image registry.local/api:2.1 -r 4 -p 8080
kute gen cronjob nightly --schedule '*/5 * * * *' -i busybox \
    --command sh --command -c --command 'echo hi'
kute gen ingress web --host app.example.com --tls-secret web-tls --ingress-class nginx
```

Supported kinds: `deployment`, `statefulset`, `daemonset`, `job`, `cronjob`,
`service`, `configmap`, `secret`, `ingress`, `pvc`, `namespace`, `hpa`,
`networkpolicy`, `serviceaccount`.

Manifests go to stdout by default, or to a file with `-o`. Existing files are
never clobbered without `--force`.

Useful flags (see `kute gen <KIND> --help` for the full set):

| Flag | Purpose |
| --- | --- |
| `-n, --namespace` | namespace for the object |
| `-l, --label` | extra label, repeatable (`KEY=VALUE`) |
| `--annotation` | annotation, repeatable |
| `-e, --env` | literal env var (`KEY=VALUE`) |
| `--env-from-configmap` | `VAR=CONFIGMAP` → `configMapKeyRef` |
| `--env-from-secret` | `VAR=SECRET` → `secretKeyRef` |
| `-d, --data` | ConfigMap/Secret entry |
| `--mount-configmap` | `NAME:/path`, repeatable |
| `--mount-secret` | `NAME:/path`, repeatable (mounted read-only) |
| `--cpu-request/--cpu-limit/--mem-request/--mem-limit` | resources |
| `--service-account` | `serviceAccountName` |

Every generated object gets `app.kubernetes.io/name` and
`app.kubernetes.io/managed-by: kute` labels. Overriding the name label also
updates the selector, so Services keep matching their pods:

```sh
kute gen service api -l app.kubernetes.io/name=renamed
```

## Scaffolding kustomize

```sh
kute scaffold web --namespace prod --image ghcr.io/acme/web:1.2.3 --envs dev,staging,prod
```

produces a tree that `kubectl kustomize` already understands:

```
web/
├── base/
│   ├── deployment.yaml
│   ├── service.yaml
│   └── kustomization.yaml     # shared labels + resources
└── overlays/
    ├── dev/kustomization.yaml
    ├── staging/kustomization.yaml
    └── prod/kustomization.yaml
```

Each overlay sets its namespace, pins the image tag, sets replicas, and adds a
`configMapGenerator` for the environment name. `--tag` overrides the tag taken
from `--image`; `--dry-run` prints the tree without writing anything.

## RBAC

```sh
kute rbac serviceaccount runner -n prod --image-pull-secret regcred
kute rbac role reader -n prod -r get,list,watch:pods,services
kute rbac clusterrole cluster-reader -r get,list:nodes
kute rbac rolebinding rb -n prod --role reader --service-account prod/my-app
kute rbac clusterrolebinding crb --role cluster-reader --service-account my-app
```

Rules use `VERBS:RESOURCES[:APIGROUPS]` and may be repeated. `--resource-name`
narrows a rule (`--resource-name web-0`). `--service-account` accepts
`NAME` or `NAMESPACE/NAME` and may be repeated.

The one-shot helper emits a complete, already-wired triple:

```sh
kute rbac bundle reader -n prod -r get,list,watch:pods,services
# → ServiceAccount reader, Role reader, RoleBinding reader (binding the two)
```

Add `--cluster-wide` to get a ClusterRole + ClusterRoleBinding instead.
ClusterRoles never carry a namespace.

## Finding kubectl commands

`kute search` fuzzy-matches against two sources:

- a **built-in corpus** of 100+ curated commands in `data/kubectl.toml`,
  compiled into the binary and editable in place;
- your **shell history** — `~/.zsh_history`, `~/.bash_history`, `~/.histfile`,
  `$HISTFILE` and fish's history, parsed including zsh's extended format and
  line continuations, and filtered to kubectl invocations (including `kgp`,
  `kl`, `k`, `kc` style aliases).

```sh
kute search rollout status
kute search --source history drain
kute search -n 50 port-forward service
kute search --print get pods pending        # just the command, for scripting
kute search --json logs tail                # machine-readable
kute search -i                              # interactive picker
```

Matching is **token-based**: every word you type must match somewhere, in any
order, across command, description and tags. So `service port-forward` and
`port-forward service` both work, and `kubectl lgs app` still finds
`kubectl logs -f app`.

`--print` composes with the shell:

```sh
$(kute search --print pods pending)
```

## Contexts

```sh
kute ctx                       # list contexts, current one marked
kute ctx --current             # just the current context name
kute ctx staging               # switch context
kute ctx -n kube-system        # set namespace on the current context
kute ctx staging -n staging    # switch and set namespace together
```

## How it is built

| Concern | Choice |
| --- | --- |
| CLI | `clap` derive |
| Manifest templating | `askama`, compile-time checked |
| TUI | `ratatui` + `crossterm` |
| Fuzzy matching | `fuzzy-matcher` (Skim) |
| Corpus format | `toml`, embedded via `include_str!` |

Manifests are real templates in `templates/*.yaml`, not string concatenation, so
they stay readable and diffable. Every template renders from a single
`GenContext`, which means adding a flag never requires touching more than one
struct.

## Development

```sh
cargo test          # unit + end-to-end CLI tests
cargo clippy --all-targets
cargo fmt
```

The integration tests in `tests/cli.rs` run the real binary and parse its output
with a YAML parser, so a malformed template fails the build rather than your
cluster.

To add commands to the search corpus, append to `data/kubectl.toml` and rebuild —
`cargo test` verifies every entry is a genuine `kubectl` command with a
description and category.

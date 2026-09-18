# Taste
- Prefers Rust for building CLI/TUI tools. Confidence: 0.6
- Prefers `clap` for CLI argument parsing in Rust projects. Confidence: 0.7
- Prefers `askama` for templating (e.g. rendering YAML/manifest files) when a Rust project needs templates. Confidence: 0.6
- Prefers shell completions generated from the CLI definition itself (e.g. `clap_complete`) and shipped as a built-in subcommand, rather than hand-written completion scripts. Confidence: 0.8
- Prefers interactive TUIs where every optional configuration block can be individually enabled or skipped, with optional sections off by default instead of pre-filled. Confidence: 0.75
- Wants generated service scaffolds to support ingress and autoscaling (HPA) as opt-in add-ons that get wired into the surrounding config (e.g. the base kustomization) automatically. Confidence: 0.6
- For text inputs that take enumerated domain values (e.g. RBAC verbs, resources, API groups), wants a selectable list of frequently used presets offered alongside free-form custom input, rather than typing values from scratch. Confidence: 0.7
- Wants UX patterns like preset pickers applied consistently across all relevant screens/forms, not just the one where the need was first noticed. Confidence: 0.6

pub mod form;
mod ui;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::cli::{RbacBundleArgs, ScaffoldArgs, SearchArgs, SearchSource, TuiArgs};
use crate::ctx::{self, ContextEntry};
use crate::generate;
use crate::model::{GenContext, GenKind, RbacRule};
use crate::rbac;
use crate::scaffold;
use crate::search::{self, Candidate, Hit};
use crate::util;

use form::{Field, Form};

pub const MENU: [(&str, &str); 5] = [
    (
        "Generate manifest",
        "scaffold a single workload, service, config or RBAC-free object",
    ),
    (
        "Scaffold kustomize",
        "base + overlays tree ready for kubectl apply -k",
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Menu,
    Search,
    Generate,
    Scaffold,
    Rbac,
    Contexts,
}

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

    pub fn refresh(&mut self) {
        let kind = self.kind();
        let name = self.form.trimmed("name");

        if name.is_empty() {
            self.preview = "name is required\n".to_string();
            return;
        }

        let mut ctx = GenContext::new(&name);
        if kind != GenKind::Namespace {
            let namespace = self.form.trimmed("namespace");
            ctx.namespace = (!namespace.is_empty()).then_some(namespace);
        }

        let image = self.form.trimmed("image");
        if !image.is_empty() {
            ctx.image = image;
        }

        ctx.replicas = self.form.number("replicas").unwrap_or(1);
        ctx.port = self.form.number_u16("port").unwrap_or(80);
        ctx.service_port = ctx.port;
        ctx.target_port = ctx.port;
        ctx.service_type = self.form.trimmed("service type");

        let host = self.form.trimmed("ingress host");
        if !host.is_empty() {
            ctx.host = host;
        }

        self.preview = match generate::render(kind, &ctx) {
            Ok(yaml) => yaml,
            Err(error) => format!("{error}\n"),
        };
    }

    fn output_path(&self) -> PathBuf {
        let explicit = self.form.trimmed("output file");
        if explicit.is_empty() {
            PathBuf::from(format!("{}.yaml", self.form.trimmed("name")))
        } else {
            PathBuf::from(explicit)
        }
    }
}

pub struct ScaffoldState {
    pub form: Form,
    pub preview: String,
    pub scroll: u16,
    pub message: String,
}

impl ScaffoldState {
    fn to_args(&self) -> ScaffoldArgs {
        ScaffoldArgs {
            name: self.form.trimmed("name"),
            dir: PathBuf::from(if self.form.trimmed("directory").is_empty() {
                ".".to_string()
            } else {
                self.form.trimmed("directory")
            }),
            namespace: self.form.trimmed("namespace"),
            image: self.form.trimmed("image"),
            port: self.form.number_u16("port").unwrap_or(80),
            envs: util::split_list(&self.form.trimmed("environments")),
            replicas: self.form.number("replicas").unwrap_or(1),
            tag: (!self.form.trimmed("image tag").is_empty())
                .then(|| self.form.trimmed("image tag")),
            dry_run: false,
            force: false,
        }
    }

    pub fn refresh(&mut self) {
        self.preview = match scaffold::render_tree(&self.to_args()) {
            Ok(tree) => tree,
            Err(error) => format!("{error}\n"),
        };
    }
}

pub struct RbacState {
    pub form: Form,
    pub preview: String,
    pub scroll: u16,
    pub message: String,
}

impl RbacState {
    fn to_args(&self) -> RbacBundleArgs {
        let verbs = util::split_list(&self.form.trimmed("verbs"));
        let resources = util::split_list(&self.form.trimmed("resources"));
        let api_groups = util::split_list(&self.form.trimmed("api groups"));

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
            name: self.form.trimmed("name"),
            namespace: self.form.trimmed("namespace"),
            rules,
            resource_names: util::split_list(&self.form.trimmed("resource names")),
            cluster_wide: self.form.trimmed("scope") == "cluster-wide",
            no_automount: false,
            output: None,
            force: false,
        }
    }

    fn output_path(&self) -> PathBuf {
        let explicit = self.form.trimmed("output file");
        if explicit.is_empty() {
            PathBuf::from(format!("{}-rbac.yaml", self.form.trimmed("name")))
        } else {
            PathBuf::from(explicit)
        }
    }

    pub fn refresh(&mut self) {
        self.preview = match rbac::render_bundle(&self.to_args()) {
            Ok(yaml) => yaml,
            Err(error) => format!("{error}\n"),
        };
    }
}

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
            form: Form::new(vec![
                Field::text("name", "my-app"),
                Field::text("namespace", "default"),
                Field::text("image", "nginx:1.27"),
                Field::number("replicas", "1"),
                Field::number("port", "80"),
                Field::choice(
                    "service type",
                    &["ClusterIP", "NodePort", "LoadBalancer"],
                    "ClusterIP",
                ),
                Field::text("ingress host", ""),
                Field::text("output file", ""),
            ]),
            preview: String::new(),
            scroll: 0,
            message: String::new(),
        };
        generate.refresh();

        let mut scaffold = ScaffoldState {
            form: Form::new(vec![
                Field::text("name", "my-app"),
                Field::text("directory", "."),
                Field::text("namespace", "default"),
                Field::text("image", "nginx:1.27"),
                Field::number("port", "80"),
                Field::number("replicas", "2"),
                Field::text("environments", "dev,staging,prod"),
                Field::text("image tag", ""),
            ]),
            preview: String::new(),
            scroll: 0,
            message: String::new(),
        };
        scaffold.refresh();

        let mut rbac = RbacState {
            form: Form::new(vec![
                Field::text("name", "my-app"),
                Field::text("namespace", "default"),
                Field::text("verbs", "get,list,watch"),
                Field::text("resources", "pods,services"),
                Field::text("api groups", ""),
                Field::choice("scope", &["namespaced", "cluster-wide"], "namespaced"),
                Field::text("resource names", ""),
                Field::text("output file", ""),
            ]),
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
            KeyCode::Char('u') if self.search.input.is_empty() => {}
            KeyCode::Char(character) => {
                self.search.input.push(character);
                self.search.refresh();
            }
            _ => {}
        }
    }

    fn handle_generate_key(&mut self, key: KeyCode) {
        if self.generate.choosing_kind {
            match key {
                KeyCode::Esc => self.generate.choosing_kind = false,
                KeyCode::Up | KeyCode::Char('k') => {
                    let len = GenKind::ALL.len() as isize;
                    self.generate.kind_index =
                        ((self.generate.kind_index as isize - 1).rem_euclid(len)) as usize;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let len = GenKind::ALL.len() as isize;
                    self.generate.kind_index =
                        ((self.generate.kind_index as isize + 1).rem_euclid(len)) as usize;
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
            self.generate.scroll = 0;
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
            self.scaffold.scroll = 0;
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
            self.rbac.scroll = 0;
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

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use ratatui::crossterm::event::KeyCode;

use crate::model::GenKind;

/// How many suggestions the dropdown shows at once.
pub const PICKER_WINDOW: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Number,
    /// A fixed set of values cycled with `←`/`→`.
    Choice,
    /// A `[x]` / `[ ]` switch flipped with `space`.
    Toggle,
    /// A repeatable set of strings, added with `a` and removed with `d`.
    List,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub kind: FieldKind,
    pub value: String,
    pub choices: Vec<String>,
    pub items: Vec<String>,
    pub item_index: usize,
    /// Frequently used values offered as a dropdown, alongside free text entry.
    pub suggestions: Vec<String>,
}

impl Field {
    fn new(key: &'static str, label: &'static str, help: &'static str, kind: FieldKind) -> Self {
        Self {
            key,
            label,
            help,
            kind,
            value: String::new(),
            choices: Vec::new(),
            items: Vec::new(),
            item_index: 0,
            suggestions: Vec::new(),
        }
    }

    /// Attach a palette of common values, reachable with `i`.
    pub fn suggesting(mut self, values: &[&str]) -> Self {
        self.suggestions = values.iter().map(|value| value.to_string()).collect();
        self
    }

    pub fn has_suggestions(&self) -> bool {
        !self.suggestions.is_empty()
    }

    pub fn text(
        key: &'static str,
        label: &'static str,
        value: impl Into<String>,
        help: &'static str,
    ) -> Self {
        let mut field = Self::new(key, label, help, FieldKind::Text);
        field.value = value.into();
        field
    }

    pub fn number(
        key: &'static str,
        label: &'static str,
        value: impl ToString,
        help: &'static str,
    ) -> Self {
        let mut field = Self::new(key, label, help, FieldKind::Number);
        field.value = value.to_string();
        field
    }

    pub fn choice(
        key: &'static str,
        label: &'static str,
        choices: &[&str],
        value: &str,
        help: &'static str,
    ) -> Self {
        let mut field = Self::new(key, label, help, FieldKind::Choice);
        field.choices = choices.iter().map(|choice| choice.to_string()).collect();
        field.value = value.to_string();
        field
    }

    pub fn toggle(key: &'static str, label: &'static str, value: bool, help: &'static str) -> Self {
        let mut field = Self::new(key, label, help, FieldKind::Toggle);
        field.value = bool_word(value).to_string();
        field
    }

    pub fn list(key: &'static str, label: &'static str, help: &'static str) -> Self {
        Self::new(key, label, help, FieldKind::List)
    }

    pub fn is_on(&self) -> bool {
        self.value == "yes"
    }
}

fn bool_word(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// A group of related fields. Optional sections start disabled so the user
/// explicitly opts in to the parts they care about.
#[derive(Debug, Clone)]
pub struct Section {
    pub title: &'static str,
    pub help: &'static str,
    pub optional: bool,
    pub enabled: bool,
    pub expanded: bool,
    /// Kinds this section is relevant to; empty means "all kinds".
    pub applies_to: &'static [GenKind],
    pub fields: Vec<Field>,
}

impl Section {
    pub fn required(title: &'static str, help: &'static str, fields: Vec<Field>) -> Self {
        Self {
            title,
            help,
            optional: false,
            enabled: true,
            expanded: true,
            applies_to: &[],
            fields,
        }
    }

    pub fn optional(title: &'static str, help: &'static str, fields: Vec<Field>) -> Self {
        Self {
            title,
            help,
            optional: true,
            enabled: false,
            expanded: true,
            applies_to: &[],
            fields,
        }
    }

    pub fn for_kinds(mut self, kinds: &'static [GenKind]) -> Self {
        self.applies_to = kinds;
        self
    }
}

/// A navigable position inside a form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowRef {
    /// A section header; toggles the section on/off (or collapses it).
    Header(usize),
    /// A field within a section.
    Field(usize, usize),
}

/// The dropdown of common values for a list field. It filters as you type and
/// inserts the highlighted value into the target field -- free-form entry via
/// `a` stays available the whole time.
#[derive(Debug, Clone)]
pub struct Picker {
    pub field_key: &'static str,
    pub query: String,
    /// Indices into the field's suggestion list, best first.
    pub matches: Vec<usize>,
    pub selected: usize,
}

impl Picker {
    fn new(field_key: &'static str, suggestion_count: usize) -> Self {
        Self {
            field_key,
            query: String::new(),
            matches: (0..suggestion_count).collect(),
            selected: 0,
        }
    }

    fn refilter(&mut self, suggestions: &[String]) {
        let query = self.query.trim();

        if query.is_empty() {
            self.matches = (0..suggestions.len()).collect();
        } else {
            let matcher = SkimMatcherV2::default();
            let mut scored: Vec<(i64, usize)> = suggestions
                .iter()
                .enumerate()
                .filter_map(|(index, value)| {
                    matcher
                        .fuzzy_match(value, query)
                        .map(|score| (score, index))
                })
                .collect();
            scored.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| suggestions[a.1].cmp(&suggestions[b.1]))
            });
            self.matches = scored.into_iter().map(|(_, index)| index).collect();
        }

        self.selected = 0;
    }

    fn move_selection(&mut self, delta: isize) {
        if self.matches.is_empty() {
            return;
        }
        let len = self.matches.len() as isize;
        self.selected = ((self.selected as isize + delta).rem_euclid(len)) as usize;
    }
}

/// A vertical, sectioned form driven entirely by key presses.
#[derive(Debug, Clone)]
pub struct Form {
    pub sections: Vec<Section>,
    pub row: usize,
    pub editing: bool,
    /// Open value dropdown, if any.
    pub picker: Option<Picker>,
    kind_filter: Option<GenKind>,
}

impl Form {
    pub fn new(sections: Vec<Section>) -> Self {
        let mut form = Self {
            sections,
            row: 0,
            editing: false,
            picker: None,
            kind_filter: None,
        };
        form.clamp();
        form
    }

    /// Restrict the form to the sections that apply to `kind`.
    pub fn with_kind(mut self, kind: GenKind) -> Self {
        self.kind_filter = Some(kind);
        self.clamp();
        self
    }

    pub fn set_kind(&mut self, kind: GenKind) {
        self.kind_filter = Some(kind);
        self.clamp();
    }

    /// Whether a section is relevant to the form's current kind filter.
    pub fn section_applies(&self, section: &Section) -> bool {
        match self.kind_filter {
            Some(kind) => section.applies_to.is_empty() || section.applies_to.contains(&kind),
            None => true,
        }
    }

    /// Every row the user can currently reach. Collapsed and disabled sections
    /// contribute only their header, which is what makes skipping cheap.
    pub fn rows(&self) -> Vec<RowRef> {
        let mut rows = Vec::new();

        for (section_index, section) in self.sections.iter().enumerate() {
            if !self.section_applies(section) {
                continue;
            }

            rows.push(RowRef::Header(section_index));

            if section.enabled && section.expanded {
                for field_index in 0..section.fields.len() {
                    rows.push(RowRef::Field(section_index, field_index));
                }
            }
        }

        rows
    }

    pub fn clamp(&mut self) {
        let len = self.rows().len();
        self.row = if len == 0 { 0 } else { self.row.min(len - 1) };
    }

    pub fn current_row(&self) -> Option<RowRef> {
        self.rows().get(self.row).copied()
    }

    fn header_row(&self, section_index: usize) -> Option<usize> {
        self.rows()
            .iter()
            .position(|row| *row == RowRef::Header(section_index))
    }

    // ── value access ────────────────────────────────────────────────────────

    pub fn find(&self, key: &str) -> Option<&Field> {
        self.sections
            .iter()
            .flat_map(|section| section.fields.iter())
            .find(|field| field.key == key)
    }

    fn find_mut(&mut self, key: &str) -> Option<&mut Field> {
        self.sections
            .iter_mut()
            .flat_map(|section| section.fields.iter_mut())
            .find(|field| field.key == key)
    }

    pub fn text(&self, key: &str) -> String {
        self.find(key)
            .map(|field| field.value.trim().to_string())
            .unwrap_or_default()
    }

    pub fn text_or(&self, key: &str, fallback: &str) -> String {
        let value = self.text(key);
        if value.is_empty() {
            fallback.to_string()
        } else {
            value
        }
    }

    pub fn number(&self, key: &str) -> Option<u32> {
        self.text(key).parse().ok()
    }

    pub fn number_u16(&self, key: &str) -> Option<u16> {
        self.text(key).parse().ok()
    }

    pub fn number_or(&self, key: &str, fallback: u32) -> u32 {
        self.number(key).unwrap_or(fallback)
    }

    pub fn toggle_value(&self, key: &str) -> bool {
        self.find(key).map(Field::is_on).unwrap_or(false)
    }

    /// List items, trimmed and with blanks dropped -- so an abandoned `a`
    /// keypress never produces an empty entry.
    pub fn list(&self, key: &str) -> Vec<String> {
        self.find(key)
            .map(|field| {
                field
                    .items
                    .iter()
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn section_enabled(&self, title: &str) -> bool {
        self.sections
            .iter()
            .find(|section| section.title == title)
            .map(|section| section.enabled)
            .unwrap_or(false)
    }

    // ── key handling ────────────────────────────────────────────────────────

    /// Returns true when the form state changed and the caller should re-render.
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        if self.picker.is_some() {
            return self.handle_picker_key(key);
        }

        if self.editing {
            return self.handle_edit_key(key);
        }

        self.clamp();
        let Some(current) = self.current_row() else {
            return false;
        };

        match key {
            KeyCode::Up | KeyCode::BackTab => self.move_row(-1),
            KeyCode::Down | KeyCode::Tab => self.move_row(1),
            KeyCode::Char(' ') => self.flip_row(current),
            KeyCode::Enter => self.activate(current),
            KeyCode::Left => self.nudge(current, -1),
            KeyCode::Right => self.nudge(current, 1),
            KeyCode::Char('a') => self.list_push(current),
            KeyCode::Char('d') => self.list_pop(current),
            KeyCode::Char('i') => self.open_picker(current),
            _ => false,
        }
    }

    /// Keys while the value dropdown is open: typing filters, enter inserts.
    fn handle_picker_key(&mut self, key: KeyCode) -> bool {
        let Some(field_key) = self.picker.as_ref().map(|picker| picker.field_key) else {
            return false;
        };

        match key {
            KeyCode::Esc => {
                self.picker = None;
                true
            }
            KeyCode::Enter => self.accept_picker(),
            KeyCode::Up | KeyCode::BackTab => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.move_selection(-1);
                }
                true
            }
            KeyCode::Down | KeyCode::Tab => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.move_selection(1);
                }
                true
            }
            KeyCode::Backspace | KeyCode::Char(_) => {
                let suggestions = self.suggestions_of(field_key);

                if let Some(picker) = self.picker.as_mut() {
                    match key {
                        KeyCode::Backspace => {
                            picker.query.pop();
                        }
                        KeyCode::Char(character) => picker.query.push(character),
                        _ => {}
                    }
                    picker.refilter(&suggestions);
                }
                true
            }
            _ => false,
        }
    }

    fn suggestions_of(&self, field_key: &str) -> Vec<String> {
        self.find(field_key)
            .map(|field| field.suggestions.clone())
            .unwrap_or_default()
    }

    fn open_picker(&mut self, row: RowRef) -> bool {
        let RowRef::Field(section, field) = row else {
            return false;
        };

        let target = &self.sections[section].fields[field];
        // Choices already cycle and toggles are boolean, so only free-text
        // fields benefit from a palette.
        let acceptable = matches!(
            target.kind,
            FieldKind::List | FieldKind::Text | FieldKind::Number
        );
        if !acceptable || !target.has_suggestions() {
            return false;
        }

        self.picker = Some(Picker::new(target.key, target.suggestions.len()));
        true
    }

    /// Apply the highlighted suggestion. For a list field it appends (or jumps
    /// to an existing entry); for a text field it replaces the value.
    fn accept_picker(&mut self) -> bool {
        let Some(picker) = self.picker.take() else {
            return false;
        };
        let Some(&suggestion_index) = picker.matches.get(picker.selected) else {
            return true;
        };
        let Some(target) = self.find_mut(picker.field_key) else {
            return true;
        };
        let Some(value) = target.suggestions.get(suggestion_index).cloned() else {
            return true;
        };

        if target.kind != FieldKind::List {
            target.value = value;
            return true;
        }

        match target.items.iter().position(|item| *item == value) {
            Some(existing) => target.item_index = existing,
            None => {
                target.items.push(value);
                target.item_index = target.items.len() - 1;
            }
        }

        true
    }

    /// The dropdown's visible slice: a window of [`PICKER_WINDOW`] entries
    /// centred on the selection, so long palettes stay readable.
    pub fn picker_window(&self) -> Vec<(String, bool)> {
        let Some(picker) = self.picker.as_ref() else {
            return Vec::new();
        };
        let suggestions = self.suggestions_of(picker.field_key);

        if picker.matches.is_empty() {
            return Vec::new();
        }

        let start = picker
            .selected
            .saturating_sub(PICKER_WINDOW / 2)
            .min(picker.matches.len().saturating_sub(PICKER_WINDOW));

        picker.matches[start..]
            .iter()
            .take(PICKER_WINDOW)
            .enumerate()
            .map(|(offset, index)| {
                let is_selected = start + offset == picker.selected;
                (
                    suggestions.get(*index).cloned().unwrap_or_default(),
                    is_selected,
                )
            })
            .collect()
    }

    fn move_row(&mut self, delta: isize) -> bool {
        let len = self.rows().len() as isize;
        if len == 0 {
            return false;
        }
        self.row = ((self.row as isize + delta).clamp(0, len - 1)) as usize;
        true
    }

    /// `space` only acts on things that are genuinely two-state.
    fn flip_row(&mut self, row: RowRef) -> bool {
        match row {
            RowRef::Header(section) if self.sections[section].optional => {
                self.toggle_section(section)
            }
            RowRef::Field(section, field)
                if self.sections[section].fields[field].kind == FieldKind::Toggle =>
            {
                self.flip(section, field)
            }
            _ => false,
        }
    }

    fn activate(&mut self, row: RowRef) -> bool {
        match row {
            RowRef::Header(section) => self.toggle_section(section),
            RowRef::Field(section, field) => match self.sections[section].fields[field].kind {
                FieldKind::Choice => self.cycle(section, field, 1),
                FieldKind::Toggle => self.flip(section, field),
                FieldKind::List => {
                    if self.sections[section].fields[field].items.is_empty() {
                        self.sections[section].fields[field]
                            .items
                            .push(String::new());
                    }
                    self.editing = true;
                    true
                }
                FieldKind::Text | FieldKind::Number => {
                    self.editing = true;
                    true
                }
            },
        }
    }

    fn nudge(&mut self, row: RowRef, delta: isize) -> bool {
        let RowRef::Field(section, field) = row else {
            return false;
        };

        match self.sections[section].fields[field].kind {
            FieldKind::Choice => self.cycle(section, field, delta),
            FieldKind::List => {
                let target = &mut self.sections[section].fields[field];
                if target.items.is_empty() {
                    return false;
                }
                let len = target.items.len() as isize;
                target.item_index = ((target.item_index as isize + delta).rem_euclid(len)) as usize;
                true
            }
            _ => false,
        }
    }

    fn toggle_section(&mut self, index: usize) -> bool {
        let section = &mut self.sections[index];
        if section.optional {
            section.enabled = !section.enabled;
            if section.enabled {
                section.expanded = true;
            }
        } else {
            section.expanded = !section.expanded;
        }

        // Keep the cursor parked on the header we just acted on.
        if let Some(position) = self.header_row(index) {
            self.row = position;
        }
        true
    }

    fn cycle(&mut self, section: usize, field: usize, delta: isize) -> bool {
        let target = &mut self.sections[section].fields[field];
        if target.choices.is_empty() {
            return false;
        }

        let current = target.value.clone();
        let len = target.choices.len() as isize;
        let index = target
            .choices
            .iter()
            .position(|choice| *choice == current)
            .unwrap_or(0);
        let next = ((index as isize + delta).rem_euclid(len)) as usize;
        target.value = target.choices[next].clone();
        true
    }

    fn flip(&mut self, section: usize, field: usize) -> bool {
        let target = &mut self.sections[section].fields[field];
        target.value = bool_word(!target.is_on()).to_string();
        true
    }

    fn list_push(&mut self, row: RowRef) -> bool {
        let RowRef::Field(section, field) = row else {
            return false;
        };
        if self.sections[section].fields[field].kind != FieldKind::List {
            return false;
        }

        let target = &mut self.sections[section].fields[field];
        target.items.push(String::new());
        target.item_index = target.items.len() - 1;
        self.editing = true;
        true
    }

    fn list_pop(&mut self, row: RowRef) -> bool {
        let RowRef::Field(section, field) = row else {
            return false;
        };

        let target = &mut self.sections[section].fields[field];
        if target.kind != FieldKind::List || target.items.is_empty() {
            return false;
        }

        target.items.remove(target.item_index);
        if target.item_index >= target.items.len() {
            target.item_index = target.items.len().saturating_sub(1);
        }
        true
    }

    fn handle_edit_key(&mut self, key: KeyCode) -> bool {
        if matches!(key, KeyCode::Esc | KeyCode::Enter) {
            self.editing = false;
            return true;
        }

        let Some(current) = self.current_row() else {
            self.editing = false;
            return true;
        };
        let RowRef::Field(section, field) = current else {
            self.editing = false;
            return true;
        };

        let target = &mut self.sections[section].fields[field];
        let number_only = target.kind == FieldKind::Number;
        let is_list = target.kind == FieldKind::List;

        // Lists edit the highlighted item; everything else edits `value`.
        let editing_slot = if is_list {
            target.items.get_mut(target.item_index)
        } else {
            Some(&mut target.value)
        };
        let Some(slot) = editing_slot else {
            return false;
        };

        match key {
            KeyCode::Backspace => {
                slot.pop();
                true
            }
            KeyCode::Char(character) if !number_only || character.is_ascii_digit() => {
                slot.push(character);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Form {
        Form::new(vec![
            Section::required(
                "Basics",
                "always required",
                vec![
                    Field::text("name", "name", "app", ""),
                    Field::number("replicas", "replicas", 3, ""),
                ],
            ),
            Section::optional(
                "Ingress",
                "opt-in",
                vec![
                    Field::text("host", "host", "", ""),
                    Field::choice("path type", "path type", &["Prefix", "Exact"], "Prefix", ""),
                ],
            ),
        ])
    }

    /// Index into `rows()` of the field with this key.
    fn field_row(form: &Form, key: &str) -> usize {
        form.rows()
            .into_iter()
            .position(|row| match row {
                RowRef::Field(section, field) => form.sections[section].fields[field].key == key,
                RowRef::Header(_) => false,
            })
            .expect("field should be reachable")
    }

    #[test]
    fn optional_sections_start_skipped() {
        let form = sample();
        assert!(form.section_enabled("Basics"));
        assert!(!form.section_enabled("Ingress"));
    }

    #[test]
    fn skipped_sections_hide_their_fields() {
        let mut form = sample();
        let rows = form.rows();

        // Basics (header + 2 fields) + Ingress header only.
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3], RowRef::Header(1));

        // Enabling the section makes its fields reachable.
        form.row = 3;
        assert!(form.handle_key(KeyCode::Enter));
        assert_eq!(form.rows().len(), 6);
    }

    #[test]
    fn space_toggles_a_section_and_keeps_the_cursor_on_it() {
        let mut form = sample();
        form.row = 3;
        form.handle_key(KeyCode::Char(' '));

        assert!(form.section_enabled("Ingress"));
        assert_eq!(form.current_row(), Some(RowRef::Header(1)));
    }

    #[test]
    fn a_disabled_section_keeps_its_values_but_is_skipped() {
        let mut form = sample();

        // Enable, fill in a value, then disable again.
        form.row = 3;
        form.handle_key(KeyCode::Enter);
        form.row = field_row(&form, "host");
        form.handle_key(KeyCode::Enter);
        for character in "app.example.com".chars() {
            form.handle_key(KeyCode::Char(character));
        }
        form.handle_key(KeyCode::Enter);

        assert_eq!(form.text("host"), "app.example.com");

        form.row = 3;
        form.handle_key(KeyCode::Char(' '));

        assert!(!form.section_enabled("Ingress"));
        assert_eq!(
            form.text("host"),
            "app.example.com",
            "disabling should not discard what was typed"
        );
    }

    #[test]
    fn toggle_fields_flip_with_space() {
        let mut form = Form::new(vec![Section::required(
            "Scheduling",
            "",
            vec![Field::toggle("automount", "automount", true, "")],
        )]);

        form.row = 1;
        assert!(form.toggle_value("automount"));
        form.handle_key(KeyCode::Char(' '));
        assert!(!form.toggle_value("automount"));
    }

    #[test]
    fn lists_can_grow_and_shrink() {
        let mut form = Form::new(vec![Section::required(
            "Environment",
            "",
            vec![Field::list("env", "env", "")],
        )]);

        form.row = 1;

        // `a` adds an entry and drops straight into editing it.
        form.handle_key(KeyCode::Char('a'));
        assert!(form.editing);
        for character in "LOG=debug".chars() {
            form.handle_key(KeyCode::Char(character));
        }
        form.handle_key(KeyCode::Enter);

        form.handle_key(KeyCode::Char('a'));
        for character in "DB=prod".chars() {
            form.handle_key(KeyCode::Char(character));
        }
        form.handle_key(KeyCode::Enter);

        assert_eq!(form.list("env"), vec!["LOG=debug", "DB=prod"]);

        form.handle_key(KeyCode::Char('d'));
        assert_eq!(form.list("env"), vec!["LOG=debug"]);
    }

    #[test]
    fn blank_list_entries_are_dropped() {
        let mut form = Form::new(vec![Section::required(
            "Environment",
            "",
            vec![Field::list("env", "env", "")],
        )]);

        form.row = 1;
        form.handle_key(KeyCode::Char('a'));
        form.handle_key(KeyCode::Esc);

        assert!(
            form.list("env").is_empty(),
            "an abandoned add should not leak an entry"
        );
    }

    #[test]
    fn list_items_are_navigable_with_arrows() {
        let mut form = Form::new(vec![Section::required(
            "Environment",
            "",
            vec![Field::list("env", "env", "")],
        )]);

        form.row = 1;
        for value in ["A=1", "B=2"] {
            form.handle_key(KeyCode::Char('a'));
            for character in value.chars() {
                form.handle_key(KeyCode::Char(character));
            }
            form.handle_key(KeyCode::Enter);
        }

        assert_eq!(form.sections[0].fields[0].item_index, 1);
        form.handle_key(KeyCode::Left);
        assert_eq!(form.sections[0].fields[0].item_index, 0);
        form.handle_key(KeyCode::Right);
        assert_eq!(form.sections[0].fields[0].item_index, 1);
    }

    #[test]
    fn number_fields_reject_letters() {
        let mut form = sample();
        form.row = field_row(&form, "replicas");
        form.handle_key(KeyCode::Enter);
        form.handle_key(KeyCode::Char('a'));
        assert_eq!(form.text("replicas"), "3");
        form.handle_key(KeyCode::Char('5'));
        assert_eq!(form.text("replicas"), "35");
    }

    #[test]
    fn choice_fields_cycle_without_opening_an_editor() {
        let mut form = sample();
        form.row = 3;
        form.handle_key(KeyCode::Enter); // enable Ingress

        form.row = field_row(&form, "path type");
        form.handle_key(KeyCode::Enter);
        assert!(!form.editing, "choices should not open a text editor");
        assert_eq!(form.text("path type"), "Exact");
        form.handle_key(KeyCode::Right);
        assert_eq!(form.text("path type"), "Prefix");
    }

    #[test]
    fn kind_filter_hides_irrelevant_sections() {
        fn build(kind: GenKind) -> Form {
            Form::new(vec![
                Section::required("Basics", "", vec![Field::text("name", "name", "app", "")]),
                Section::optional("Ingress", "", vec![Field::text("host", "host", "", "")])
                    .for_kinds(&[GenKind::Ingress]),
            ])
            .with_kind(kind)
        }

        // A Deployment never offers the Ingress section: its header is drawn
        // greyed out by the renderer, but it is not reachable.
        let deployment = build(GenKind::Deployment);
        assert_eq!(
            deployment.rows(),
            vec![RowRef::Header(0), RowRef::Field(0, 0)]
        );

        // For an Ingress the section applies, so its header is reachable and
        // can be switched on.
        let ingress = build(GenKind::Ingress);
        assert_eq!(
            ingress.rows(),
            vec![RowRef::Header(0), RowRef::Field(0, 0), RowRef::Header(1)]
        );
    }

    #[test]
    fn cursor_stays_in_bounds_when_rows_disappear() {
        let mut form = sample();
        form.row = 5;
        form.clamp();
        assert!(form.current_row().is_some());
    }

    // ── value dropdown ──────────────────────────────────────────────────────

    fn rule_form() -> Form {
        Form::new(vec![Section::required(
            "Rule",
            "",
            vec![
                Field::list("verbs", "verbs", "").suggesting(&["get", "list", "watch"]),
                Field::list("bare", "bare", ""),
                Field::text("storage", "storage", "1Gi", "").suggesting(&["1Gi", "10Gi"]),
            ],
        )])
    }

    fn picker_press(form: &mut Form, key: KeyCode) {
        form.handle_key(key);
    }

    #[test]
    fn i_opens_the_dropdown_on_a_field_with_suggestions() {
        let mut form = rule_form();
        form.row = 1; // verbs

        assert!(form.handle_key(KeyCode::Char('i')));
        assert!(form.picker.is_some());

        let window = form.picker_window();
        assert_eq!(window.len(), 3);
        assert_eq!(window[0].0, "get");
        assert!(window[0].1, "the first entry starts selected");
    }

    #[test]
    fn i_does_nothing_without_a_palette() {
        let mut form = rule_form();
        form.row = 2; // the "bare" field has no suggestions

        assert!(!form.handle_key(KeyCode::Char('i')));
        assert!(form.picker.is_none());
    }

    #[test]
    fn choosing_a_value_appends_it_without_typing() {
        let mut form = rule_form();
        form.row = 1;

        picker_press(&mut form, KeyCode::Char('i'));
        picker_press(&mut form, KeyCode::Down); // list
        picker_press(&mut form, KeyCode::Enter);

        assert!(form.picker.is_none(), "the dropdown closes after inserting");
        assert_eq!(form.list("verbs"), vec!["list"]);
    }

    #[test]
    fn typing_filters_the_dropdown() {
        let mut form = rule_form();
        form.row = 1;
        picker_press(&mut form, KeyCode::Char('i'));

        for character in "wat".chars() {
            form.handle_key(KeyCode::Char(character));
        }

        let window = form.picker_window();
        assert_eq!(window.len(), 1);
        assert_eq!(window[0].0, "watch");

        picker_press(&mut form, KeyCode::Enter);
        assert_eq!(form.list("verbs"), vec!["watch"]);
    }

    #[test]
    fn escape_leaves_the_dropdown_without_inserting() {
        let mut form = rule_form();
        form.row = 1;

        picker_press(&mut form, KeyCode::Char('i'));
        picker_press(&mut form, KeyCode::Esc);

        assert!(form.picker.is_none());
        assert!(form.list("verbs").is_empty());
        // ...and the form is usable again straight away.
        assert!(form.handle_key(KeyCode::Down));
    }

    #[test]
    fn choosing_an_existing_value_does_not_duplicate_it() {
        let mut form = rule_form();
        form.row = 1;

        picker_press(&mut form, KeyCode::Char('i'));
        picker_press(&mut form, KeyCode::Enter); // "get"
        picker_press(&mut form, KeyCode::Char('i'));
        picker_press(&mut form, KeyCode::Enter); // "get" again

        assert_eq!(form.list("verbs"), vec!["get"]);
    }

    #[test]
    fn a_text_field_palette_replaces_the_value() {
        let mut form = rule_form();
        form.row = 3; // storage, currently "1Gi"

        picker_press(&mut form, KeyCode::Char('i'));
        picker_press(&mut form, KeyCode::Down);
        picker_press(&mut form, KeyCode::Enter);

        assert_eq!(form.text("storage"), "10Gi");
    }

    #[test]
    fn the_dropdown_windows_long_palettes_around_the_selection() {
        let long: Vec<&str> = vec![
            "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
        ];
        let mut form = Form::new(vec![Section::required(
            "S",
            "",
            vec![Field::list("big", "big", "").suggesting(&long)],
        )]);
        form.row = 1;
        picker_press(&mut form, KeyCode::Char('i'));

        assert_eq!(form.picker_window().len(), PICKER_WINDOW);

        // Walk to the far end; the window should follow.
        for _ in 0..15 {
            form.handle_key(KeyCode::Down);
        }
        let window = form.picker_window();
        assert_eq!(window.len(), PICKER_WINDOW);
        assert_eq!(window.last().unwrap().0, "p");
        assert!(window.last().unwrap().1, "the last entry is selected");
    }

    #[test]
    fn free_form_entry_still_works_alongside_the_dropdown() {
        let mut form = rule_form();
        form.row = 1;

        // Custom value via `a`, exactly as before the dropdown existed.
        form.handle_key(KeyCode::Char('a'));
        for character in "deletecollection".chars() {
            form.handle_key(KeyCode::Char(character));
        }
        form.handle_key(KeyCode::Enter);

        // Then top it up from the palette.
        picker_press(&mut form, KeyCode::Char('i'));
        picker_press(&mut form, KeyCode::Enter);

        assert_eq!(form.list("verbs"), vec!["deletecollection", "get"]);
    }
}

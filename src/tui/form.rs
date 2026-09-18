use ratatui::crossterm::event::KeyCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Number,
    /// A fixed set of values cycled with `←`/`→`.
    Choice,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub label: String,
    pub value: String,
    pub kind: FieldKind,
    pub choices: Vec<String>,
}

impl Field {
    pub fn text(label: &str, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            kind: FieldKind::Text,
            choices: Vec::new(),
        }
    }

    pub fn number(label: &str, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            kind: FieldKind::Number,
            choices: Vec::new(),
        }
    }

    pub fn choice(label: &str, choices: &[&str], value: &str) -> Self {
        Self {
            label: label.into(),
            value: value.to_string(),
            kind: FieldKind::Choice,
            choices: choices.iter().map(|choice| choice.to_string()).collect(),
        }
    }
}

/// A vertical list of editable fields driven entirely by key presses.
#[derive(Debug, Clone)]
pub struct Form {
    pub fields: Vec<Field>,
    pub selected: usize,
    pub editing: bool,
}

impl Form {
    pub fn new(fields: Vec<Field>) -> Self {
        Self {
            fields,
            selected: 0,
            editing: false,
        }
    }

    pub fn current(&self) -> &Field {
        &self.fields[self.selected]
    }

    fn current_mut(&mut self) -> &mut Field {
        &mut self.fields[self.selected]
    }

    pub fn value(&self, label: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| field.value.as_str())
    }

    pub fn trimmed(&self, label: &str) -> String {
        self.value(label).unwrap_or_default().trim().to_string()
    }

    pub fn number(&self, label: &str) -> Option<u32> {
        self.trimmed(label).parse().ok()
    }

    pub fn number_u16(&self, label: &str) -> Option<u16> {
        self.trimmed(label).parse().ok()
    }

    pub fn focus(&mut self, delta: isize) {
        let len = self.fields.len() as isize;
        if len == 0 {
            return;
        }
        self.selected = ((self.selected as isize + delta).rem_euclid(len)) as usize;
    }

    /// Returns true when the form state changed and the caller should re-render.
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        if self.editing {
            return self.handle_edit_key(key);
        }

        match key {
            KeyCode::Up | KeyCode::BackTab => {
                self.focus(-1);
                true
            }
            KeyCode::Down | KeyCode::Tab => {
                self.focus(1);
                true
            }
            KeyCode::Enter => {
                if self.current().kind == FieldKind::Choice {
                    self.cycle(1)
                } else {
                    self.editing = true;
                    true
                }
            }
            KeyCode::Left => self.current().kind == FieldKind::Choice && self.cycle(-1),
            KeyCode::Right => self.current().kind == FieldKind::Choice && self.cycle(1),
            _ => false,
        }
    }

    fn handle_edit_key(&mut self, key: KeyCode) -> bool {
        let number_only = self.current().kind == FieldKind::Number;

        match key {
            KeyCode::Esc | KeyCode::Enter => {
                self.editing = false;
                true
            }
            KeyCode::Backspace => {
                self.current_mut().value.pop();
                true
            }
            KeyCode::Char(character) if !number_only || character.is_ascii_digit() => {
                self.current_mut().value.push(character);
                true
            }
            _ => false,
        }
    }

    fn cycle(&mut self, delta: isize) -> bool {
        let field = &mut self.fields[self.selected];
        if field.choices.is_empty() {
            return false;
        }

        let current = field.value.clone();
        let len = field.choices.len() as isize;
        let index = field
            .choices
            .iter()
            .position(|choice| *choice == current)
            .unwrap_or(0);
        let next = ((index as isize + delta).rem_euclid(len)) as usize;
        field.value = field.choices[next].clone();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> Form {
        Form::new(vec![
            Field::text("name", "app"),
            Field::number("replicas", "3"),
            Field::choice("type", &["ClusterIP", "NodePort"], "ClusterIP"),
        ])
    }

    #[test]
    fn focus_wraps_around() {
        let mut form = form();
        form.focus(-1);
        assert_eq!(form.selected, 2);
        form.focus(1);
        assert_eq!(form.selected, 0);
    }

    #[test]
    fn typing_edits_the_selected_field() {
        let mut form = form();
        form.handle_key(KeyCode::Enter);
        assert!(form.editing);
        form.handle_key(KeyCode::Char('x'));
        form.handle_key(KeyCode::Enter);
        assert_eq!(form.trimmed("name"), "appx");
    }

    #[test]
    fn number_fields_reject_letters() {
        let mut form = form();
        form.focus(1);
        form.handle_key(KeyCode::Enter);
        form.handle_key(KeyCode::Char('a'));
        assert_eq!(form.trimmed("replicas"), "3");
        form.handle_key(KeyCode::Char('5'));
        assert_eq!(form.trimmed("replicas"), "35");
    }

    #[test]
    fn choice_fields_cycle_from_the_keyboard() {
        let mut form = form();
        form.focus(2);
        form.handle_key(KeyCode::Enter);
        assert!(!form.editing, "choices should not open a text editor");
        assert_eq!(form.trimmed("type"), "NodePort");
        form.handle_key(KeyCode::Right);
        assert_eq!(form.trimmed("type"), "ClusterIP");
    }

    #[test]
    fn escape_leaves_edit_mode() {
        let mut form = form();
        form.handle_key(KeyCode::Enter);
        form.handle_key(KeyCode::Esc);
        assert!(!form.editing);
    }
}

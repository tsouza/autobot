//! The parser of KERNEL §10, the only place the lifecycles are printed.
//!
//! This module is the single parser of that section; everything that needs the lifecycle
//! states (the design-set check, the lifecycle doc-sync test, the CRD enum test, the Quint
//! domain lint) calls [`parse`] instead of reading the section itself.
//!
//! # Grammar of the block
//!
//! §10 holds one fenced block. A line that starts in column 0 carries a machine name up to
//! the first run of two spaces, and the machine's first statement after it. A name ending
//! in `,` or `.` continues on the next line's name column (`ExternalOperation,` +
//! `ToolInvocation`, `Plan.status.` + `revisions[rev]`). An indented line adds statements to
//! the machine above it.
//!
//! A line holds statements separated by `;`. A statement is a chain of groups joined by `→`
//! (one direction) or `↔` (both directions); a group is states separated by `|`. Every state
//! of one group moves to every state of the next. A chain of a single group only lists
//! states. A statement may start with a field label, `field:`, which puts it on that field's
//! machine instead of the kind's own; a label with nothing after it applies to the more
//! indented lines below it. `field: as Kind` gives the field the machine of the same field
//! on `Kind`. A source may be `any` or `any non-terminal`; a target may be `field := STATE`,
//! which sets a sibling field instead of moving this one.
//!
//! A parenthesis directly after a state (one space) annotates the transitions into it. A
//! parenthesis at the start of a line or after two or more spaces is a note on the machine;
//! it may run over several lines. A note after two or more spaces that follows a statement on
//! its line (a *line note*) is also the line note of the transitions into that statement's last
//! group: the last arrow of its chain. The first state listed for a field is its initial state.
//!
//! A machine note *names* a transition when it holds `FROM → TO` for it, or when one of its
//! `;`-separated clauses starts with `TO:` ([`Machine::notes_naming`]).

use crate::{Error, Result, markdown};
use std::collections::BTreeSet;

/// The heading of KERNEL §10, as [`markdown::section`] matches it.
pub const SECTION: &str = "10. Lifecycles";

/// One printed machine: a name with the state fields its lines define.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    /// The name as printed, joined over continuation lines (`ExternalOperation, ToolInvocation`).
    pub name: String,
    /// The kinds or records the machine applies to: the name split at commas.
    pub subjects: Vec<String>,
    /// The state fields, in order of first appearance.
    pub fields: Vec<Field>,
    /// The notes printed on the machine, in order.
    pub notes: Vec<String>,
}

/// The states and transitions of one field of a machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The field label (`hold_state`, `fence_state`), or `None` for the machine's own states.
    pub name: Option<String>,
    /// The machine this field is printed as a copy of (`fence_state: as TaskRun`).
    pub same_as: Option<String>,
    /// Every state, in order of first appearance; the first one is the initial state.
    pub states: Vec<String>,
    /// Every transition, in printed order.
    pub transitions: Vec<Transition>,
}

/// Where a transition starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A named state.
    State(String),
    /// `any`: every state of the field.
    Any,
    /// `any non-terminal`: every state of the field that has an outgoing transition.
    AnyNonTerminal,
}

/// One arrow of a machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    /// The source state.
    pub from: Source,
    /// The target state.
    pub to: String,
    /// The sibling field the target belongs to, for `field := STATE` targets.
    pub assigns: Option<String>,
    /// The annotation printed after the target (`same snapshot`, `continuation`).
    pub note: Option<String>,
    /// The line note of the transition: a note that closes the line of the statement that
    /// printed it, when the transition is into that statement's last group (`human adjudication
    /// only`). It is also in [`Machine::notes`].
    pub line_note: Option<String>,
}

impl Machine {
    /// The field named `name`, or the machine's own states for `None`.
    #[must_use]
    pub fn field(&self, name: Option<&str>) -> Option<&Field> {
        self.fields.iter().find(|f| f.name.as_deref() == name)
    }

    /// The notes of the machine that name the transition `from → to`: those that hold
    /// `from → to` as whole states, or with a `;`-separated clause that starts with `to:`.
    #[must_use]
    pub fn notes_naming(&self, from: &str, to: &str) -> Vec<&str> {
        let arrow = format!("{from} → {to}");
        let label = format!("{to}:");
        self.notes
            .iter()
            .map(String::as_str)
            .filter(|n| {
                contains_whole(n, &arrow)
                    || n.split(';')
                        .any(|clause| clause.trim_start().starts_with(&label))
            })
            .collect()
    }
}

/// Whether `text` holds `needle` with no word character on either side of it, as a note holds a
/// state or an arrow.
#[must_use]
pub fn contains_whole(text: &str, needle: &str) -> bool {
    text.match_indices(needle).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + needle.len()..].chars().next();
        !before.is_some_and(crate::design::is_word_char)
            && !after.is_some_and(crate::design::is_word_char)
    })
}

impl Field {
    /// The initial state: the first one listed.
    #[must_use]
    pub fn initial(&self) -> Option<&str> {
        self.states.first().map(String::as_str)
    }
}

/// The machine that applies to `subject` (`TaskRun`, `Milestone`, `pending commit slot`).
#[must_use]
pub fn find<'a>(machines: &'a [Machine], subject: &str) -> Option<&'a Machine> {
    machines
        .iter()
        .find(|m| m.subjects.iter().any(|s| s == subject))
}

/// Every state of every field of every machine.
#[must_use]
pub fn all_states(machines: &[Machine]) -> BTreeSet<String> {
    machines
        .iter()
        .flat_map(|m| &m.fields)
        .flat_map(|f| {
            f.states.iter().cloned().chain(
                f.transitions
                    .iter()
                    .filter(|t| t.assigns.is_some())
                    .map(|t| t.to.clone()),
            )
        })
        .collect()
}

/// Parses the machines of §10 out of the whole KERNEL document.
///
/// # Errors
/// Fails if the document has no §10, the section has no fenced block, or the block does not
/// follow the grammar in the module documentation.
pub fn parse(kernel: &str) -> Result<Vec<Machine>> {
    let section = markdown::section(kernel, SECTION)
        .ok_or_else(|| Error::Parse(format!("KERNEL has no section `{SECTION}`")))?;
    let block = fenced_block(section)
        .ok_or_else(|| Error::Parse("KERNEL §10 has no fenced block".to_owned()))?;
    parse_block(&block)
}

/// Parses a §10 block: the text between the fences.
///
/// # Errors
/// Fails if the block does not follow the grammar in the module documentation.
pub fn parse_block(block: &str) -> Result<Vec<Machine>> {
    let mut p = Parser::default();
    for (i, line) in block.lines().enumerate() {
        p.line(line).map_err(|msg| {
            Error::Parse(format!("KERNEL §10 block line {}: {msg}: `{line}`", i + 1))
        })?;
    }
    if p.note.is_some() {
        return Err(Error::Parse(
            "KERNEL §10 block ends inside a note".to_owned(),
        ));
    }
    resolve_same_as(&mut p.machines)?;
    Ok(p.machines)
}

/// Prints machines in the canonical form of the block grammar, one statement per line.
///
/// [`parse_block`] of the result yields `machines` again.
#[must_use]
pub fn render(machines: &[Machine]) -> String {
    let mut out = String::new();
    for m in machines {
        let mut lines = Vec::new();
        for f in &m.fields {
            let label = f.name.as_ref().map_or(String::new(), |n| format!("{n}: "));
            if let Some(kind) = &f.same_as {
                lines.push(format!("{label}as {kind}"));
                continue;
            }
            lines.push(format!("{label}{}", f.states.join(" | ")));
            let mut rest = f.transitions.as_slice();
            while let Some(first) = rest.first() {
                let run = rest
                    .iter()
                    .take_while(|t| first.line_note.is_some() && t.line_note == first.line_note)
                    .count()
                    .max(1);
                let (group, tail) = rest.split_at(run);
                rest = tail;
                match (&first.line_note, cross_product(group)) {
                    (Some(ln), Some((froms, tos))) => {
                        let froms: Vec<&str> = froms.iter().map(|s| source(s)).collect();
                        let tos: Vec<String> = tos.iter().map(|t| target(t)).collect();
                        lines.push(format!(
                            "{label}{} → {}   ({ln})",
                            froms.join(" | "),
                            tos.join(" | ")
                        ));
                    }
                    _ => {
                        for t in group {
                            let ln = t
                                .line_note
                                .as_ref()
                                .map_or(String::new(), |n| format!("   ({n})"));
                            lines.push(format!("{label}{} → {}{ln}", source(&t.from), target(t)));
                        }
                    }
                }
            }
        }
        let line_notes: Vec<&str> = m
            .fields
            .iter()
            .flat_map(|f| &f.transitions)
            .filter_map(|t| t.line_note.as_deref())
            .collect();
        lines.extend(
            m.notes
                .iter()
                .filter(|n| !line_notes.contains(&n.as_str()))
                .map(|n| format!("({n})")),
        );
        let indent = " ".repeat(m.name.chars().count() + 2);
        for (i, line) in lines.iter().enumerate() {
            let lead = if i == 0 {
                format!("{}  ", m.name)
            } else {
                indent.clone()
            };
            out.push_str(&lead);
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// A source as the block prints it.
fn source(s: &Source) -> &str {
    match s {
        Source::State(s) => s,
        Source::Any => "any",
        Source::AnyNonTerminal => "any non-terminal",
    }
}

/// A transition's target as the block prints it, with its annotation.
fn target(t: &Transition) -> String {
    let assigns = t
        .assigns
        .as_ref()
        .map_or(String::new(), |a| format!("{a} := "));
    let note = t.note.as_ref().map_or(String::new(), |n| format!(" ({n})"));
    format!("{assigns}{}{note}", t.to)
}

/// `group` as the sources and targets of one arrow, when it is exactly every source to every
/// target, source by source, with the same target annotations for each source.
fn cross_product(group: &[Transition]) -> Option<(Vec<&Source>, Vec<&Transition>)> {
    let first = group.first()?;
    let tos: Vec<&Transition> = group.iter().take_while(|t| t.from == first.from).collect();
    let froms: Vec<&Source> = group.iter().step_by(tos.len()).map(|t| &t.from).collect();
    let exact = group.len() == froms.len() * tos.len()
        && group.chunks(tos.len()).zip(&froms).all(|(chunk, from)| {
            chunk.iter().zip(&tos).all(|(t, want)| {
                &t.from == *from
                    && t.to == want.to
                    && t.assigns == want.assigns
                    && t.note == want.note
            })
        });
    exact.then_some((froms, tos))
}

/// The body of the first fenced code block in `text`.
fn fenced_block(text: &str) -> Option<String> {
    let mut fence = markdown::Fence::default();
    let mut body: Option<String> = None;
    for line in text.lines() {
        let was_open = fence.is_open();
        fence.step(line);
        match (&mut body, was_open, fence.is_open()) {
            (None, false, true) => body = Some(String::new()),
            (Some(_), true, false) => return body,
            (Some(b), true, true) => {
                b.push_str(line);
                b.push('\n');
            }
            _ => {}
        }
    }
    None
}

/// The transitions of one field that a line note annotates: the field's index in its machine
/// and the transitions' indexes in the field.
type Hop = (usize, Vec<usize>);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Word(String),
    Arrow,
    Both,
    Bar,
    Semi,
    Colon,
    Assign,
    /// A note: `detached` after two or more spaces, `trailing` when it also follows a statement
    /// on its line, and `hop` the transitions it annotates when it opened on an earlier line.
    Note {
        text: String,
        detached: bool,
        trailing: bool,
        hop: Option<Hop>,
    },
}

/// A note still open at the end of a line.
#[derive(Debug)]
struct OpenNote {
    text: String,
    depth: usize,
    detached: bool,
    /// The note follows a statement on the line it opened on.
    trailing: bool,
    /// The last arrow of that statement, once its line is parsed.
    hop: Option<Hop>,
}

#[derive(Debug, Default)]
struct Parser {
    machines: Vec<Machine>,
    note: Option<OpenNote>,
    /// A field label with an empty chain, and the body column it was printed at.
    pending_field: Option<(String, usize)>,
    /// The transitions into the last group of the last statement parsed.
    last_hop: Option<Hop>,
}

type LineResult<T = ()> = std::result::Result<T, String>;

impl Parser {
    fn line(&mut self, line: &str) -> LineResult {
        if line.trim().is_empty() {
            return Ok(());
        }
        let indent = line.chars().take_while(|c| *c == ' ').count();
        let (body, column) = if indent == 0 && self.note.is_none() {
            let (name, rest) = line
                .split_once("  ")
                .ok_or("a machine name must be followed by two spaces and a statement")?;
            let rest_trimmed = rest.trim_start();
            let column = line.chars().count() - rest_trimmed.chars().count();
            self.name(name.trim());
            (rest_trimmed, column)
        } else {
            (line.trim_start(), indent)
        };
        let toks = self.tokenize(body)?;
        let pending = match self.pending_field.take() {
            Some((f, c)) if column > c => {
                self.pending_field = Some((f.clone(), c));
                Some(f)
            }
            _ => None,
        };
        let mut stmt = Vec::new();
        let mut line_notes = Vec::new();
        for tok in toks.into_iter().chain(std::iter::once(Tok::Semi)) {
            match tok {
                Tok::Note {
                    text,
                    detached: true,
                    trailing,
                    hop,
                } => {
                    self.machine()?.notes.push(text.clone());
                    if let Some(hop) = hop {
                        self.annotate(&hop, &text)?;
                    } else if trailing {
                        line_notes.push(text);
                    }
                }
                Tok::Semi => {
                    if !stmt.is_empty() {
                        self.statement(&std::mem::take(&mut stmt), pending.clone(), column)?;
                        if let Some(hop) = self.last_hop.clone() {
                            for text in line_notes.drain(..) {
                                self.annotate(&hop, &text)?;
                            }
                        }
                        line_notes.clear();
                    }
                }
                other => stmt.push(other),
            }
        }
        if let Some(n) = &mut self.note
            && n.trailing
        {
            n.trailing = false;
            n.hop = self.last_hop.clone();
        }
        Ok(())
    }

    /// Sets `text` as the line note of the transitions of `hop`.
    fn annotate(&mut self, hop: &Hop, text: &str) -> LineResult {
        let (fi, transitions) = hop;
        let field = self
            .machine()?
            .fields
            .get_mut(*fi)
            .ok_or("a line note on a missing field")?;
        for &ti in transitions {
            let t = field
                .transitions
                .get_mut(ti)
                .ok_or("a line note on a missing transition")?;
            t.line_note = Some(match t.line_note.take() {
                Some(earlier) => format!("{earlier}; {text}"),
                None => text.to_owned(),
            });
        }
        Ok(())
    }

    /// Starts a machine, or continues the previous name when it ends in `,` or `.`.
    fn name(&mut self, name: &str) {
        self.pending_field = None;
        if let Some(last) = self.machines.last_mut()
            && (last.name.ends_with(',') || last.name.ends_with('.'))
        {
            if last.name.ends_with(',') {
                last.name.push(' ');
            }
            last.name.push_str(name);
            last.subjects = subjects(&last.name);
            return;
        }
        self.machines.push(Machine {
            name: name.to_owned(),
            subjects: subjects(name),
            fields: Vec::new(),
            notes: Vec::new(),
        });
    }

    fn machine(&mut self) -> LineResult<&mut Machine> {
        self.machines
            .last_mut()
            .ok_or_else(|| "a statement before any machine name".to_owned())
    }

    fn field(&mut self, name: Option<String>) -> LineResult<&mut Field> {
        let m = self.machine()?;
        let pos = if let Some(p) = m.fields.iter().position(|f| f.name == name) {
            p
        } else {
            m.fields.push(Field {
                name,
                same_as: None,
                states: Vec::new(),
                transitions: Vec::new(),
            });
            m.fields.len() - 1
        };
        Ok(&mut m.fields[pos])
    }

    fn tokenize(&mut self, body: &str) -> LineResult<Vec<Tok>> {
        let mut toks = Vec::new();
        let mut word = String::new();
        let mut spaces = usize::MAX; // Start of the body counts as detached.
        if let Some(n) = &mut self.note
            && !n.text.ends_with(' ')
        {
            n.text.push(' ');
        }
        let mut chars = body.chars().peekable();
        while let Some(c) = chars.next() {
            if let Some(n) = &mut self.note {
                match c {
                    '(' => n.depth += 1,
                    ')' => n.depth -= 1,
                    _ => {}
                }
                if n.depth == 0 {
                    let n = self.note.take().ok_or("unreachable note state")?;
                    toks.push(Tok::Note {
                        text: n.text.trim().to_owned(),
                        detached: n.detached,
                        trailing: n.trailing,
                        hop: n.hop,
                    });
                } else {
                    n.text.push(c);
                }
                spaces = 0;
                continue;
            }
            if c.is_whitespace() {
                flush(&mut word, &mut toks);
                spaces = spaces.saturating_add(1);
                continue;
            }
            let run = std::mem::replace(&mut spaces, 0);
            match c {
                '(' => {
                    flush(&mut word, &mut toks);
                    let follows_statement = toks
                        .iter()
                        .rev()
                        .take_while(|t| !matches!(t, Tok::Semi))
                        .any(|t| !matches!(t, Tok::Note { .. }));
                    self.note = Some(OpenNote {
                        text: String::new(),
                        depth: 1,
                        detached: run >= 2,
                        trailing: run >= 2 && follows_statement,
                        hop: None,
                    });
                }
                ')' => return Err("unbalanced `)`".to_owned()),
                '→' | '↔' | '|' | ';' | ':' => {
                    flush(&mut word, &mut toks);
                    toks.push(match c {
                        '→' => Tok::Arrow,
                        '↔' => Tok::Both,
                        '|' => Tok::Bar,
                        ';' => Tok::Semi,
                        _ if chars.peek() == Some(&'=') => {
                            chars.next();
                            Tok::Assign
                        }
                        _ => Tok::Colon,
                    });
                }
                _ => word.push(c),
            }
        }
        flush(&mut word, &mut toks);
        Ok(toks)
    }

    fn statement(&mut self, toks: &[Tok], pending: Option<String>, column: usize) -> LineResult {
        self.last_hop = None;
        let (label, chain) = match toks {
            [Tok::Word(f), Tok::Colon, rest @ ..] => (Some(f.clone()), rest),
            [Tok::Word(f), Tok::Note { text, .. }, Tok::Colon, rest @ ..] => {
                self.machine()?.notes.push(text.clone());
                (Some(f.clone()), rest)
            }
            _ => (None, toks),
        };
        if let Some(f) = &label
            && chain.is_empty()
        {
            self.pending_field = Some((f.clone(), column));
            return Ok(());
        }
        let field_name = label.or(pending);
        if let [Tok::Word(as_), Tok::Word(kind)] = chain
            && as_ == "as"
        {
            let field = self.field(field_name)?;
            if !field.states.is_empty() || field.same_as.is_some() {
                return Err("`as` on a field that already has states".to_owned());
            }
            field.same_as = Some(kind.clone());
            return Ok(());
        }
        let groups = groups(chain)?;
        let field = self.field(field_name.clone())?;
        if field.same_as.is_some() {
            return Err("states on a field printed with `as`".to_owned());
        }
        let hop = add_chain(field, &groups)?;
        let fi = self
            .machine()?
            .fields
            .iter()
            .position(|f| f.name == field_name)
            .ok_or("unreachable field state")?;
        if !hop.is_empty() {
            self.last_hop = Some((fi, hop));
        }
        Ok(())
    }
}

fn flush(word: &mut String, toks: &mut Vec<Tok>) {
    if !word.is_empty() {
        toks.push(Tok::Word(std::mem::take(word)));
    }
}

fn subjects(name: &str) -> Vec<String> {
    name.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    State(String),
    Any,
    AnyNonTerminal,
    Assign(String, String),
}

#[derive(Debug)]
struct Group {
    /// How this group is reached from the previous one: `false` for `→`, `true` for `↔`.
    both: bool,
    items: Vec<(Item, Option<String>)>,
}

/// Whether `word` is shaped like a lifecycle state: an ASCII capital, then capitals, digits or `_`.
pub(crate) fn is_state(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_uppercase())
        && word
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn groups(chain: &[Tok]) -> LineResult<Vec<Group>> {
    let mut out = vec![Group {
        both: false,
        items: Vec::new(),
    }];
    let mut item: Vec<&Tok> = Vec::new();
    for tok in chain.iter().chain(std::iter::once(&Tok::Bar)) {
        match tok {
            Tok::Bar | Tok::Arrow | Tok::Both => {
                let last = out.last_mut().ok_or("no group")?;
                last.items.push(parse_item(&item)?);
                item.clear();
                if !matches!(tok, Tok::Bar) {
                    out.push(Group {
                        both: matches!(tok, Tok::Both),
                        items: Vec::new(),
                    });
                }
            }
            other => item.push(other),
        }
    }
    Ok(out)
}

fn parse_item(toks: &[&Tok]) -> LineResult<(Item, Option<String>)> {
    let (toks, note) = match toks {
        [rest @ .., Tok::Note { text, .. }] => (rest, Some(text.clone())),
        _ => (toks, None),
    };
    let item = match toks {
        [Tok::Word(s)] if s == "any" => Item::Any,
        [Tok::Word(a), Tok::Word(b)] if a == "any" && b == "non-terminal" => Item::AnyNonTerminal,
        [Tok::Word(s)] if is_state(s) => Item::State(s.clone()),
        [Tok::Word(f), Tok::Assign, Tok::Word(s)] if is_state(s) => {
            Item::Assign(f.clone(), s.clone())
        }
        [] => return Err("an empty state".to_owned()),
        _ => return Err(format!("not a state: {toks:?}")),
    };
    Ok((item, note))
}

fn push_state(field: &mut Field, s: &str) {
    if !field.states.iter().any(|x| x == s) {
        field.states.push(s.to_owned());
    }
}

/// Adds the states and transitions of a chain; returns the indexes of the transitions of its last
/// arrow, into its last group.
fn add_chain(field: &mut Field, groups: &[Group]) -> LineResult<Vec<usize>> {
    let mut hop = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        for (item, _) in &g.items {
            match item {
                Item::State(s) => push_state(field, s),
                Item::Any | Item::AnyNonTerminal if i == 0 && groups.len() > 1 => {}
                Item::Assign(..) if i == groups.len() - 1 && i > 0 => {}
                _ => return Err("`any` is only a source and `:=` only a final target".to_owned()),
            }
        }
    }
    let last = groups.len().saturating_sub(2);
    for (w, pair) in groups.windows(2).enumerate() {
        let [from, to] = pair else { continue };
        for (src, _) in &from.items {
            let source = match src {
                Item::State(s) => Source::State(s.clone()),
                Item::Any => Source::Any,
                Item::AnyNonTerminal => Source::AnyNonTerminal,
                Item::Assign(..) => return Err("`:=` as a source".to_owned()),
            };
            for (dst, note) in &to.items {
                let (assigns, target) = match dst {
                    Item::State(s) => (None, s.clone()),
                    Item::Assign(f, s) => (Some(f.clone()), s.clone()),
                    Item::Any | Item::AnyNonTerminal => return Err("`any` as a target".to_owned()),
                };
                let i = push_transition(
                    field,
                    Transition {
                        from: source.clone(),
                        to: target,
                        assigns,
                        note: note.clone(),
                        line_note: None,
                    },
                );
                if w == last {
                    hop.push(i);
                }
            }
        }
        if to.both {
            for (dst, _) in &to.items {
                for (src, note) in &from.items {
                    let (Item::State(d), Item::State(s)) = (dst, src) else {
                        return Err("`↔` joins plain states only".to_owned());
                    };
                    push_transition(
                        field,
                        Transition {
                            from: Source::State(d.clone()),
                            to: s.clone(),
                            assigns: None,
                            note: note.clone(),
                            line_note: None,
                        },
                    );
                }
            }
        }
    }
    Ok(hop)
}

/// Adds `t` unless the same arrow is already there (a `↔` may restate one); returns its index.
fn push_transition(field: &mut Field, t: Transition) -> usize {
    if let Some(i) = field
        .transitions
        .iter()
        .position(|x| x.from == t.from && x.to == t.to && x.assigns == t.assigns)
    {
        return i;
    }
    field.transitions.push(t);
    field.transitions.len() - 1
}

/// Copies the states of every `field: as Kind` from the same field of `Kind`.
fn resolve_same_as(machines: &mut [Machine]) -> Result<()> {
    let mut copies = Vec::new();
    for (mi, m) in machines.iter().enumerate() {
        for (fi, f) in m.fields.iter().enumerate() {
            let Some(kind) = &f.same_as else { continue };
            let source = find(machines, kind)
                .and_then(|src| src.field(f.name.as_deref()))
                .filter(|src| src.same_as.is_none())
                .ok_or_else(|| {
                    Error::Parse(format!(
                        "KERNEL §10: {} field {:?} is `as {kind}`, which prints no such field",
                        m.name, f.name
                    ))
                })?;
            copies.push((mi, fi, source.states.clone(), source.transitions.clone()));
        }
    }
    for (mi, fi, states, transitions) in copies {
        let f = &mut machines[mi].fields[fi];
        f.states = states;
        f.transitions = transitions;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kernel() -> String {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/design/AUTOBOT-KERNEL.md"
        );
        std::fs::read_to_string(path).unwrap()
    }

    fn sources(f: &Field, to: &str) -> Vec<Source> {
        f.transitions
            .iter()
            .filter(|t| t.to == to && t.assigns.is_none())
            .map(|t| t.from.clone())
            .collect()
    }

    fn st(s: &str) -> Source {
        Source::State(s.to_owned())
    }

    #[test]
    fn imported_kernel_round_trips_every_machine() {
        let machines = parse(&kernel()).unwrap();
        let names: Vec<&str> = machines.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "CommandReceipt",
                "AdmissionStamp",
                "ExternalOperation, ToolInvocation",
                "EffectIntent",
                "EffectReceipt",
                "pending commit slot",
                "control receipt",
                "reservation phase",
                "ledger entry",
                "expected record",
                "WorkContext",
                "Plan.phase",
                "Plan.status.revisions[rev]",
                "PlanSnapshot",
                "PlanProposal",
                "Intake",
                "WorkBrief",
                "Project, Repository",
                "Charter, ProjectCharter",
                "ManagerLease",
                "Task, Milestone",
                "TaskRun",
                "AgentRun",
                "AgentCheckpoint",
                "ScopeCapsule",
                "ExecutionIdentity",
                "CredentialGrant",
                "FenceSession",
                "Workspace",
                "CustodyPolicy",
                "CustodyCheckpoint",
                "ArtifactCommit",
                "Artifact",
                "WorkspaceConflict",
                "RestoreRequest",
                "IntegrationBasis",
                "VerificationRun",
                "EvidenceBundle",
                "Budget",
                "BudgetReservation",
                "UsageReceipt",
                "OutcomeRecord",
                "TelemetryGap",
                "Finding",
                "Decision",
                "Intervention",
                "gate",
                "ProjectionState",
            ]
        );
        let again = parse_block(&render(&machines)).unwrap();
        assert_eq!(again.len(), machines.len());
        for (a, b) in machines.iter().zip(&again) {
            assert_eq!(a, b, "machine `{}` does not round-trip", a.name);
        }
    }

    #[test]
    fn fenced_block_follows_commonmark_fences() {
        assert_eq!(
            fenced_block("x\n~~~\nA -> B\n~~~\n").as_deref(),
            Some("A -> B\n")
        );
        assert_eq!(
            fenced_block("````\n```\nA -> B\n```\n````\n").as_deref(),
            Some("```\nA -> B\n```\n")
        );
        assert_eq!(fenced_block("```\nA -> B\n"), None);
    }

    #[test]
    fn imported_kernel_loses_no_state_token() {
        // Every ALL-CAPS token of the block outside parentheses is a parsed state.
        let text = kernel();
        let block = fenced_block(markdown::section(&text, SECTION).unwrap()).unwrap();
        let states = all_states(&parse_block(&block).unwrap());
        let mut depth = 0usize;
        let mut outside = String::new();
        for c in block.chars() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ if depth == 0 => outside.push(c),
                _ => {}
            }
        }
        let tokens: BTreeSet<String> = outside
            .split(|c: char| !crate::design::is_word_char(c))
            .filter(|w| w.len() >= 3 && is_state(w))
            .map(str::to_owned)
            .collect();
        let missing: Vec<_> = tokens.difference(&states).collect();
        assert!(missing.is_empty(), "{missing:?}");
        assert!(states.contains("FENCE_PENDING") && states.contains("ACCEPTED_NOT_SENT"));
    }

    #[test]
    fn continued_names_join_into_one_machine() {
        let machines = parse(&kernel()).unwrap();
        let op = find(&machines, "ToolInvocation").unwrap();
        assert_eq!(op.name, "ExternalOperation, ToolInvocation");
        assert_eq!(op.subjects, ["ExternalOperation", "ToolInvocation"]);
        let own = op.field(None).unwrap();
        assert_eq!(own.initial(), Some("REQUESTED"));
        assert_eq!(
            sources(own, "REQUESTED"),
            [st("PERMITTED"), st("RECONCILING")]
        );
        assert_eq!(
            sources(own, "BLOCKED_UNSUPPORTED"),
            [st("REQUESTED"), st("PERMITTED")]
        );
        assert_eq!(
            sources(own, "RELEASED"),
            [st("UNRESOLVED"), st("REQUESTED")]
        );
        assert_eq!(
            sources(own, "UNRESOLVED"),
            [st("RECONCILING"), st("REQUESTED")]
        );
        assert_eq!(
            sources(own, "CONFIRMED"),
            [
                st("DISPATCHING"),
                st("RECONCILING"),
                st("UNRESOLVED"),
                st("REQUESTED")
            ]
        );
        assert!(
            !own.transitions.iter().any(|t| t.from == st("RELEASED")),
            "RELEASED is terminal"
        );
        assert!(
            op.notes
                .iter()
                .any(|n| n.contains("DISPATCHING always carries send_attempt"))
        );
        let rev = find(&machines, "Plan.status.revisions[rev]").unwrap();
        assert_eq!(
            rev.field(None).unwrap().states,
            [
                "PROPOSED",
                "VERIFIED",
                "ACTIVE",
                "QUIESCING",
                "SUPERSEDED",
                "ABANDONED"
            ]
        );
    }

    #[test]
    fn field_labels_split_a_kind_into_machines() {
        let machines = parse(&kernel()).unwrap();
        let wc = find(&machines, "WorkContext").unwrap();
        assert!(wc.field(None).is_none());
        let hold = wc.field(Some("hold_state")).unwrap();
        assert_eq!(hold.initial(), Some("RUNNING"));
        assert_eq!(hold.states.len(), 5);
        let phase = wc.field(Some("manager_authority[plan].phase")).unwrap();
        assert_eq!(phase.states, ["ACTIVE", "DRAINING"]);
        assert_eq!(phase.transitions[1].note.as_deref(), Some("new epoch"));

        let task_run = find(&machines, "TaskRun").unwrap();
        let fence = task_run.field(Some("fence_state")).unwrap();
        assert_eq!(
            fence.states,
            ["ACTIVE", "FENCE_PENDING", "FENCED", "FENCED_UNCERTAIN"]
        );
        assert!(
            !task_run
                .field(None)
                .unwrap()
                .states
                .contains(&"FENCED".to_owned())
        );
        assert_eq!(
            sources(task_run.field(None).unwrap(), "CANCELLED"),
            [Source::AnyNonTerminal]
        );

        let agent = find(&machines, "AgentRun").unwrap();
        let agent_fence = agent.field(Some("fence_state")).unwrap();
        assert_eq!(agent_fence.same_as.as_deref(), Some("TaskRun"));
        assert_eq!(agent_fence.transitions, fence.transitions);
        let own = agent.field(None).unwrap();
        assert!(!own.states.contains(&"FENCE_PENDING".to_owned()));
        assert!(
            own.transitions
                .iter()
                .any(|t| t.from == st("HEARTBEAT_LOST")
                    && t.assigns.as_deref() == Some("fence_state")
                    && t.to == "FENCE_PENDING")
        );
    }

    #[test]
    fn both_ways_arrows_and_any_sources() {
        let both = parse_block("Kind  OPEN → WAITING ↔ OPEN ; OPEN → DONE\n").unwrap();
        let both = find(&both, "Kind").unwrap().field(None).unwrap();
        assert_eq!(sources(both, "WAITING"), [st("OPEN")]);
        assert_eq!(sources(both, "OPEN"), [st("WAITING")]);
        assert_eq!(sources(both, "DONE"), [st("OPEN")]);

        let machines = parse(&kernel()).unwrap();
        let intake = find(&machines, "Intake").unwrap().field(None).unwrap();
        assert_eq!(sources(intake, "CAPTURED"), [st("PROPOSED")]);
        assert_eq!(
            sources(intake, "REJECTED"),
            [st("PROPOSED"), st("CAPTURED")]
        );
        let ws = find(&machines, "Workspace").unwrap().field(None).unwrap();
        assert_eq!(sources(ws, "CONFLICT"), [Source::AnyNonTerminal]);
        let receipt = find(&machines, "EffectReceipt").unwrap();
        assert_eq!(receipt.field(None).unwrap().states, ["RECORDED"]);
        assert!(receipt.field(None).unwrap().transitions.is_empty());
        assert_eq!(receipt.notes, ["immutable, one per attempt"]);
        let slot = find(&machines, "pending commit slot").unwrap();
        assert_eq!(slot.field(None).unwrap().initial(), Some("CLEARED"));
    }

    #[test]
    fn malformed_blocks_are_errors() {
        for (block, want) in [
            ("Kind  A → lower\n", "not a state"),
            ("Kind  A → B (open\n", "ends inside a note"),
            ("Kind  A → B)\n", "unbalanced"),
            ("  A → B\n", "before any machine"),
            ("Kind  any\n", "only a source"),
            ("Kind  f: as Missing\n", "prints no such field"),
            ("Kind  A → B\nOther  X\n", ""),
        ] {
            let got = parse_block(block);
            if want.is_empty() {
                assert!(got.is_ok(), "{block}");
            } else {
                let err = got.unwrap_err().to_string();
                assert!(err.contains(want), "{block}: {err}");
            }
        }
        assert!(parse("# KERNEL\n\n## 11. Scope\n").is_err());
    }

    fn line_note<'a>(f: &'a Field, from: &str, to: &str) -> Option<&'a str> {
        f.transitions
            .iter()
            .find(|t| t.from == st(from) && t.to == to && t.assigns.is_none())
            .and_then(|t| t.line_note.as_deref())
    }

    #[test]
    fn a_line_note_annotates_the_last_arrow_of_its_statement() {
        let block = "Kind  A → B → C | D   (only after x)\n      A → E ; E → F   (two\n       lines)\n      F → A\n      (loose)\nOther  X → Y (attached)\n";
        let machines = parse_block(block).unwrap();
        let kind = find(&machines, "Kind").unwrap();
        let own = kind.field(None).unwrap();
        assert_eq!(line_note(own, "A", "B"), None);
        assert_eq!(line_note(own, "B", "C"), Some("only after x"));
        assert_eq!(line_note(own, "B", "D"), Some("only after x"));
        assert_eq!(line_note(own, "A", "E"), None);
        assert_eq!(line_note(own, "E", "F"), Some("two lines"));
        // A note alone on its line follows no statement.
        assert_eq!(line_note(own, "F", "A"), None);
        assert_eq!(kind.notes, ["only after x", "two lines", "loose"]);
        let other = find(&machines, "Other").unwrap();
        let t = &other.field(None).unwrap().transitions[0];
        assert_eq!(
            (t.note.as_deref(), t.line_note.as_deref()),
            (Some("attached"), None)
        );
        assert!(other.notes.is_empty());
        assert_eq!(parse_block(&render(&machines)).unwrap(), machines);
    }

    #[test]
    fn imported_kernel_line_notes_sit_on_their_arrow() {
        let machines = parse(&kernel()).unwrap();
        let op = find(&machines, "ExternalOperation").unwrap();
        let own = op.field(None).unwrap();
        assert_eq!(
            line_note(own, "UNRESOLVED", "COMPENSATED"),
            Some("human adjudication only")
        );
        assert_eq!(line_note(own, "RECONCILING", "UNRESOLVED"), None);
        assert!(
            line_note(own, "RECONCILING", "REQUESTED")
                .is_some_and(|n| n.starts_with("non-application proven"))
        );
        assert!(op.notes.iter().any(|n| n == "human adjudication only"));
        let task = find(&machines, "Task").unwrap().field(None).unwrap();
        assert!(line_note(task, "BLOCKED", "READY").is_some());
        assert_eq!(line_note(task, "READY", "BLOCKED"), None);
        let expected = find(&machines, "expected record").unwrap();
        assert!(
            line_note(expected.field(None).unwrap(), "PENDING", "GAP")
                .is_some_and(|n| n.contains("GAP is final"))
        );
    }

    #[test]
    fn notes_name_an_arrow_or_a_labelled_target() {
        let machines = parse_block(
            "Kind  A → B → C ; A → C\n      (A → B only once; C: reached last)\n      (AA → B is not A → BB)\n",
        )
        .unwrap();
        let kind = find(&machines, "Kind").unwrap();
        assert_eq!(
            kind.notes_naming("A", "B"),
            ["A → B only once; C: reached last"]
        );
        assert_eq!(
            kind.notes_naming("B", "C"),
            ["A → B only once; C: reached last"]
        );
        assert_eq!(
            kind.notes_naming("A", "C"),
            ["A → B only once; C: reached last"]
        );
        assert!(kind.notes_naming("A", "BB").len() == 1 && kind.notes_naming("B", "A").is_empty());

        let real = parse(&kernel()).unwrap();
        let stamp = find(&real, "AdmissionStamp").unwrap();
        assert_eq!(
            stamp.notes_naming("BROKER_ACCEPTED", "INVALIDATED").len(),
            1
        );
        assert!(stamp.notes_naming("ISSUED", "INVALIDATED").is_empty());
        let checkpoint = find(&real, "AgentCheckpoint").unwrap();
        assert_eq!(checkpoint.notes_naming("CREATED", "QUARANTINED").len(), 1);
    }
}

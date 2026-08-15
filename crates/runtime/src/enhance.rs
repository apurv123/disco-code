//! Prompt-enhancement harness.
//!
//! A short, vague request produces a bad answer from any model, but the effect
//! is far worse on a 9B model running locally than on a frontier model. Large
//! models compensate for an underspecified prompt by silently doing the
//! clarifying, planning and checking *inside* one response. Small models do not
//! have the headroom for that; asked to do everything at once, they do all of
//! it badly.
//!
//! This module turns one vague request into a short sequence of narrow ones.
//! Each stage asks the model for exactly one kind of thinking, which is the
//! shape small models handle well.
//!
//! The design follows the workflow popularised by
//! [oh-my-codex](https://github.com/Yeachan-Heo/oh-my-codex) (MIT) —
//! clarify, plan, harden, execute, verify — reimplemented in Rust and retuned
//! for local models. Two deliberate departures:
//!
//! 1. **Triage is mandatory.** Running a five-stage interview for "fix this
//!    typo" would make the product unusable. Most requests get one stage.
//! 2. **Stage selection is deterministic.** Asking the model to decide how much
//!    process it needs is a judgement call, and judgement is the first thing to
//!    degrade at 9B. Heuristics are worse than a frontier model at this and
//!    better than a small one, and they cost no tokens.

use std::fmt;

/// One kind of thinking, asked for on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// Resolve ambiguity before any work happens. Costs a round trip, so it is
    /// only worth it when the request could reasonably mean different things.
    Clarify,
    /// Commit to an approach and to acceptance criteria before editing.
    Plan,
    /// Attack the plan: risks, blast radius, what it would break.
    Harden,
    /// Do the work.
    Execute,
    /// Check the work against the criteria set during planning.
    Verify,
}

impl Stage {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Clarify => "clarify",
            Self::Plan => "plan",
            Self::Harden => "harden",
            Self::Execute => "execute",
            Self::Verify => "verify",
        }
    }

    /// The instruction injected for this stage.
    ///
    /// Each one is deliberately narrow and ends by constraining the shape of
    /// the reply. Small models drift without a stated output format, and a
    /// drifting reply cannot be parsed by the next stage.
    #[must_use]
    pub fn directive(self) -> &'static str {
        match self {
            Self::Clarify => {
                "# Stage: clarify\n\
                 The request below is ambiguous. Do not write any code yet, and do not \
                 guess what was meant.\n\
                 Ask up to three questions, and only ones whose answers would change what \
                 you build. Skip anything you could determine yourself by reading the \
                 project.\n\
                 If, having thought about it, nothing genuinely blocks you, reply with \
                 exactly: NO QUESTIONS\n\
                 Output: a numbered list of questions, or NO QUESTIONS. Nothing else."
            }
            Self::Plan => {
                "# Stage: plan\n\
                 Do not write the final code yet. Decide the approach first.\n\
                 State the approach in two or three sentences, then list the concrete \
                 steps in order. Name the specific files you expect to change.\n\
                 Then list the acceptance criteria: the observable conditions that would \
                 show the work is correct. Write them so each one can be checked as true \
                 or false, not judged.\n\
                 Output:\n\
                 APPROACH: <text>\n\
                 STEPS:\n\
                 1. <step>\n\
                 CRITERIA:\n\
                 - <criterion>"
            }
            Self::Harden => {
                "# Stage: harden\n\
                 Attack the plan above before it is executed.\n\
                 Identify what it would break, what it assumes without checking, and which \
                 steps are hard to undo. Be concrete: name the failure, not the category.\n\
                 If a step is irreversible or touches state outside this repository, say so \
                 explicitly.\n\
                 Then give the corrected plan, or state that it stands unchanged.\n\
                 Output:\n\
                 RISKS:\n\
                 - <risk>\n\
                 REVISED PLAN: <text, or 'unchanged'>"
            }
            Self::Execute => {
                "# Stage: execute\n\
                 Carry out the work.\n\
                 Follow the plan. If you discover partway through that the plan is wrong, \
                 stop and say why rather than quietly doing something else.\n\
                 Keep changes scoped to what was asked."
            }
            Self::Verify => {
                "# Stage: verify\n\
                 Check the work against the acceptance criteria.\n\
                 Go through them one at a time. For each, state whether it is met and cite \
                 the specific evidence: command output, a test result, the changed line.\n\
                 Do not mark a criterion met because the code looks right. If you did not \
                 check it, say NOT CHECKED. Reporting an unverified criterion as met is \
                 worse than reporting it unchecked.\n\
                 Output, one line per criterion:\n\
                 <MET|NOT MET|NOT CHECKED>: <criterion> - <evidence>"
            }
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// How much process a request warrants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Complexity {
    /// Concrete, bounded, low blast radius. Extra stages would be pure overhead.
    Trivial,
    /// The common case: worth planning and checking, not worth interviewing.
    Standard,
    /// Broad, vague, or dangerous enough to be worth the full loop.
    Complex,
}

impl Complexity {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Trivial => "trivial",
            Self::Standard => "standard",
            Self::Complex => "complex",
        }
    }
}

/// Why triage reached its conclusion. Surfaced so the decision can be shown to
/// the user and argued with, rather than being an opaque verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Signal {
    /// Open-ended verb with no stated target: "clean this up".
    VagueScope,
    /// Refers to something the request never names: "fix it".
    UnresolvedReference,
    /// Spans an unbounded amount of the codebase: "everywhere", "all of".
    Unbounded,
    /// Hard or impossible to undo: deletes, migrations, deploys, credentials.
    Irreversible,
    /// Several distinct asks in one message.
    MultiPart,
    /// Names a concrete file, symbol, or path.
    ConcreteTarget,
    /// A named small edit: typo, rename, comment.
    NarrowEdit,
}

impl Signal {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::VagueScope => "vague scope",
            Self::UnresolvedReference => "unresolved reference",
            Self::Unbounded => "unbounded breadth",
            Self::Irreversible => "irreversible action",
            Self::MultiPart => "multiple requests",
            Self::ConcreteTarget => "concrete target",
            Self::NarrowEdit => "narrow edit",
        }
    }
}

/// The result of triage: which stages to run, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Triage {
    pub complexity: Complexity,
    pub stages: Vec<Stage>,
    pub signals: Vec<Signal>,
}

impl Triage {
    #[must_use]
    pub fn has(&self, signal: Signal) -> bool {
        self.signals.contains(&signal)
    }

    #[must_use]
    pub fn runs(&self, stage: Stage) -> bool {
        self.stages.contains(&stage)
    }

    /// A one-line explanation for the user.
    #[must_use]
    pub fn rationale(&self) -> String {
        if self.signals.is_empty() {
            return format!("{}: no strong signals", self.complexity.label());
        }
        let reasons: Vec<&str> = self.signals.iter().map(|s| s.label()).collect();
        format!("{}: {}", self.complexity.label(), reasons.join(", "))
    }
}

const VAGUE_VERBS: &[&str] = &[
    "improve",
    "optimize",
    "optimise",
    "refactor",
    "modernize",
    "modernise",
    "polish",
    "enhance",
    "revamp",
    "overhaul",
    "make it better",
    "make this better",
    "make it nicer",
];

/// Verb-particle pairs that routinely have a word wedged between them:
/// "clean **this** up", "tidy **the module** up". A plain substring list
/// misses every one of those, which are the most common vague phrasings there
/// are.
const PHRASAL_VAGUE: &[(&str, &str)] = &[
    ("clean", "up"),
    ("tidy", "up"),
    ("fix", "up"),
    ("sort", "out"),
    ("clear", "up"),
    ("smarten", "up"),
    ("straighten", "out"),
    ("tighten", "up"),
];

const UNBOUNDED: &[&str] = &[
    "everywhere",
    "every file",
    "all files",
    "the entire",
    "the whole",
    "across the codebase",
    "throughout",
    "all of the",
    "codebase-wide",
    "everything",
];

const IRREVERSIBLE: &[&str] = &[
    "delete",
    "remove all",
    "drop table",
    "drop the",
    "migrate",
    "migration",
    "deploy",
    "publish",
    "release to",
    "force push",
    "force-push",
    "rewrite history",
    "production",
    "prod database",
    "credential",
    "secret",
    "api key",
    "rotate key",
    "truncate",
    "wipe",
    "reset --hard",
];

const NARROW_EDITS: &[&str] = &[
    "typo",
    "spelling",
    "rename",
    "add a comment",
    "add comments",
    "formatting",
    "indentation",
    "whitespace",
    "bump the version",
    "changelog entry",
];

/// Words that point at something the request never introduces.
const DANGLING: &[&str] = &["fix it", "fix this", "update it", "change it", "do it", "make it work"];

/// Decide how much process a request warrants.
///
/// Heuristics, not a model call: this runs before the first token is spent, and
/// a wrong answer here costs at most one unnecessary stage.
#[must_use]
pub fn triage(request: &str) -> Triage {
    let text = request.to_lowercase();
    let trimmed = text.trim();
    let words = trimmed.split_whitespace().count();

    let mut signals = Vec::new();

    if (VAGUE_VERBS.iter().any(|v| trimmed.contains(v)) || has_phrasal_vague(trimmed))
        && !mentions_concrete_target(trimmed)
    {
        signals.push(Signal::VagueScope);
    }
    if DANGLING.iter().any(|d| trimmed.contains(d)) && !mentions_concrete_target(trimmed) {
        signals.push(Signal::UnresolvedReference);
    }
    if UNBOUNDED.iter().any(|u| trimmed.contains(u)) {
        signals.push(Signal::Unbounded);
    }
    if IRREVERSIBLE.iter().any(|i| trimmed.contains(i)) {
        signals.push(Signal::Irreversible);
    }
    if counts_as_multi_part(trimmed) {
        signals.push(Signal::MultiPart);
    }
    if mentions_concrete_target(trimmed) {
        signals.push(Signal::ConcreteTarget);
    }
    if NARROW_EDITS.iter().any(|n| trimmed.contains(n)) {
        signals.push(Signal::NarrowEdit);
    }

    signals.sort_unstable();
    signals.dedup();

    let complexity = classify(&signals, words);
    let stages = stages_for(complexity, &signals);

    Triage {
        complexity,
        stages,
        signals,
    }
}

fn classify(signals: &[Signal], words: usize) -> Complexity {
    let vague = signals.contains(&Signal::VagueScope)
        || signals.contains(&Signal::UnresolvedReference)
        || signals.contains(&Signal::Unbounded);
    let risky = signals.contains(&Signal::Irreversible);
    let narrow = signals.contains(&Signal::NarrowEdit);
    let concrete = signals.contains(&Signal::ConcreteTarget);

    // Risk dominates. A dangerous request is worth the full loop even when it
    // is precisely stated - especially then, since a precise instruction to do
    // something destructive is exactly what should be questioned.
    if risky {
        return Complexity::Complex;
    }
    if vague || signals.contains(&Signal::MultiPart) {
        return Complexity::Complex;
    }
    // A named small edit against a named target needs no ceremony.
    if narrow && concrete {
        return Complexity::Trivial;
    }
    if narrow && words <= 12 {
        return Complexity::Trivial;
    }
    Complexity::Standard
}

fn stages_for(complexity: Complexity, signals: &[Signal]) -> Vec<Stage> {
    let mut stages = match complexity {
        Complexity::Trivial => vec![Stage::Execute],
        Complexity::Standard => vec![Stage::Plan, Stage::Execute, Stage::Verify],
        Complexity::Complex => vec![
            Stage::Clarify,
            Stage::Plan,
            Stage::Harden,
            Stage::Execute,
            Stage::Verify,
        ],
    };

    // An irreversible action always gets hardened, even if it somehow triaged
    // low. This is the one stage whose absence can cost real data.
    if signals.contains(&Signal::Irreversible) && !stages.contains(&Stage::Harden) {
        let at = stages
            .iter()
            .position(|s| *s == Stage::Execute)
            .unwrap_or(stages.len());
        stages.insert(at, Stage::Harden);
    }

    // Clarifying is only worth a round trip when something is genuinely
    // ambiguous. Breadth or risk alone does not make a request unclear.
    let ambiguous = signals.contains(&Signal::VagueScope)
        || signals.contains(&Signal::UnresolvedReference);
    if !ambiguous {
        stages.retain(|s| *s != Stage::Clarify);
    }

    stages
}

/// Matches a verb-particle pair with up to three words between them, so
/// "clean up", "clean this up" and "clean the whole module up" all register.
fn has_phrasal_vague(text: &str) -> bool {
    let words: Vec<&str> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();

    PHRASAL_VAGUE.iter().any(|(verb, particle)| {
        words.iter().enumerate().any(|(i, w)| {
            *w == *verb
                && words
                    .iter()
                    .skip(i + 1)
                    .take(4)
                    .any(|later| *later == *particle)
        })
    })
}

fn mentions_concrete_target(text: &str) -> bool {    text.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
        .any(is_target_token)
}

fn is_target_token(token: &str) -> bool {
    let token = token.trim_matches(|c: char| c == '`' || c == '"' || c == '\'' || c == '.');
    if token.len() < 3 {
        return false;
    }
    // A path.
    if token.contains('/') || token.contains('\\') {
        return true;
    }
    // A file with a plausible extension: `main.rs`, `Cargo.toml`.
    if let Some((stem, ext)) = token.rsplit_once('.') {
        if !stem.is_empty()
            && (2..=5).contains(&ext.len())
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return true;
        }
    }
    // An identifier: `snake_case`, `CamelCase`, `foo()`.
    if token.contains('_') && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return true;
    }
    false
}

fn counts_as_multi_part(text: &str) -> bool {
    // " and then ", " also ", numbered lists, and semicolons each suggest more
    // than one ask. A single " and " does not - "read and update the file" is
    // one request.
    let markers = [" and then ", " then ", " also ", " as well as ", ";"];
    if markers.iter().any(|m| text.contains(m)) {
        return true;
    }
    text.matches(" and ").count() >= 2
}

/// A request with its stage sequence resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnhancedPrompt {
    pub request: String,
    pub triage: Triage,
}

impl EnhancedPrompt {
    #[must_use]
    pub fn new(request: impl Into<String>) -> Self {
        let request = request.into();
        let triage = triage(&request);
        Self { request, triage }
    }

    /// Render the message for one stage.
    ///
    /// `carry` is the useful output of previous stages - the plan, the answers
    /// to clarifying questions. Prior stages are passed forward explicitly
    /// rather than relying on conversation history, because history is the
    /// first thing a compaction pass discards.
    #[must_use]
    pub fn render_stage(&self, stage: Stage, carry: &[(Stage, String)]) -> String {
        let mut out = String::new();
        out.push_str(stage.directive());
        out.push_str("\n\n# Request\n");
        out.push_str(self.request.trim());

        for (from, text) in carry {
            if text.trim().is_empty() {
                continue;
            }
            out.push_str(&format!("\n\n# From the {from} stage\n"));
            out.push_str(text.trim());
        }

        out
    }

    /// The stage after `stage`, or `None` when the sequence is finished.
    #[must_use]
    pub fn next_after(&self, stage: Stage) -> Option<Stage> {
        let at = self.triage.stages.iter().position(|s| *s == stage)?;
        self.triage.stages.get(at + 1).copied()
    }

    #[must_use]
    pub fn first_stage(&self) -> Stage {
        self.triage.stages.first().copied().unwrap_or(Stage::Execute)
    }
}

/// A clarify reply, interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClarifyOutcome {
    /// The model had nothing genuinely blocking; go straight on.
    NoQuestions,
    /// Questions to put to the user.
    Questions(Vec<String>),
}

/// Read a clarify reply.
///
/// A model that has no questions is supposed to say `NO QUESTIONS`, but small
/// models often wrap it in a sentence, so the marker is matched anywhere in an
/// otherwise question-free reply.
#[must_use]
pub fn parse_clarify(reply: &str) -> ClarifyOutcome {
    let questions = extract_numbered(reply);
    if questions.is_empty() || reply.to_uppercase().contains("NO QUESTIONS") && questions.is_empty()
    {
        return ClarifyOutcome::NoQuestions;
    }
    ClarifyOutcome::Questions(questions)
}

/// Acceptance criteria lifted out of a plan reply.
///
/// These are the contract the verify stage checks against. Without them,
/// verification degrades into the model re-reading its own work and declaring
/// it good, which is exactly the failure this harness exists to prevent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub approach: Option<String>,
    pub steps: Vec<String>,
    pub criteria: Vec<String>,
}

impl Plan {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.approach.is_none() && self.steps.is_empty() && self.criteria.is_empty()
    }
}

/// Parse a plan reply.
///
/// Tolerant by necessity: a 9B model will not always honour the requested
/// layout. Headings are matched case-insensitively, with or without a colon,
/// and bullets may use `-`, `*`, or numbers.
#[must_use]
pub fn parse_plan(reply: &str) -> Plan {
    let mut plan = Plan::default();
    let mut section = None;

    for raw in reply.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        match heading_of(line) {
            Some(("approach", rest)) => {
                section = Some("approach");
                if !rest.is_empty() {
                    plan.approach = Some(rest.to_string());
                }
                continue;
            }
            Some(("steps", _)) => {
                section = Some("steps");
                continue;
            }
            Some(("criteria", _)) => {
                section = Some("criteria");
                continue;
            }
            _ => {}
        }

        match section {
            Some("approach") => {
                if let Some(existing) = &mut plan.approach {
                    existing.push(' ');
                    existing.push_str(line);
                } else {
                    plan.approach = Some(line.to_string());
                }
            }
            Some("steps") => {
                if let Some(item) = bullet_body(line) {
                    plan.steps.push(item);
                }
            }
            Some("criteria") => {
                if let Some(item) = bullet_body(line) {
                    plan.criteria.push(item);
                }
            }
            _ => {}
        }
    }

    plan
}

fn heading_of(line: &str) -> Option<(&'static str, &str)> {
    let lower = line.to_lowercase();
    let stripped = lower.trim_start_matches(['#', '*', ' ']);
    for name in ["approach", "steps", "criteria"] {
        if let Some(rest) = stripped.strip_prefix(name) {
            let rest = rest.trim_start_matches([':', ' ', '*', '#']);
            // Guard against a sentence that merely starts with the word.
            let consumed = line.len() - rest.len();
            if consumed <= name.len() + 6 {
                return Some((name, line[line.len() - rest.len()..].trim()));
            }
        }
    }
    None
}

fn bullet_body(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix(['-', '*', '+']) {
        let body = rest.trim();
        return (!body.is_empty()).then(|| body.to_string());
    }
    // "1. text" / "1) text"
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        let rest = trimmed[digits.len()..].trim_start_matches(['.', ')', ' ']);
        let body = rest.trim();
        return (!body.is_empty()).then(|| body.to_string());
    }
    None
}

fn extract_numbered(reply: &str) -> Vec<String> {
    reply
        .lines()
        .filter_map(|line| bullet_body(line))
        .filter(|line| line.contains('?'))
        .collect()
}

/// How a single criterion came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CriterionState {
    Met,
    NotMet,
    NotChecked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionResult {
    pub state: CriterionState,
    pub text: String,
}

/// Read a verify reply.
///
/// Anything that does not clearly claim `MET` is treated as not met. An
/// unparseable verification is not a passing one.
#[must_use]
pub fn parse_verification(reply: &str) -> Vec<CriterionResult> {
    let mut results = Vec::new();
    for raw in reply.lines() {
        let line = bullet_body(raw).unwrap_or_else(|| raw.trim().to_string());
        if line.is_empty() {
            continue;
        }
        let upper = line.to_uppercase();
        // Order matters: "NOT MET" contains "MET".
        let state = if upper.starts_with("NOT CHECKED") {
            CriterionState::NotChecked
        } else if upper.starts_with("NOT MET") {
            CriterionState::NotMet
        } else if upper.starts_with("MET") {
            CriterionState::Met
        } else {
            continue;
        };
        results.push(CriterionResult {
            state,
            text: line.to_string(),
        });
    }
    results
}

/// True when every criterion was positively confirmed.
#[must_use]
pub fn verification_passed(results: &[CriterionResult]) -> bool {
    !results.is_empty() && results.iter().all(|r| r.state == CriterionState::Met)
}

#[cfg(test)]
mod tests {
    use super::{
        parse_clarify, parse_plan, parse_verification, triage, ClarifyOutcome, Complexity,
        CriterionState, EnhancedPrompt, Signal, Stage, verification_passed,
    };

    #[test]
    fn a_narrow_concrete_edit_gets_no_ceremony() {
        let t = triage("fix the typo in README.md");
        assert_eq!(t.complexity, Complexity::Trivial);
        assert_eq!(t.stages, vec![Stage::Execute]);
    }

    #[test]
    fn an_ordinary_request_is_planned_and_verified_but_not_interviewed() {
        let t = triage("add a --json flag to the status command in main.rs");
        assert_eq!(t.complexity, Complexity::Standard);
        assert_eq!(t.stages, vec![Stage::Plan, Stage::Execute, Stage::Verify]);
        assert!(!t.runs(Stage::Clarify));
    }

    #[test]
    fn a_vague_request_earns_the_full_loop() {
        let t = triage("clean this up");
        assert_eq!(t.complexity, Complexity::Complex);
        assert!(t.has(Signal::VagueScope));
        assert_eq!(
            t.stages,
            vec![
                Stage::Clarify,
                Stage::Plan,
                Stage::Harden,
                Stage::Execute,
                Stage::Verify
            ]
        );
    }

    #[test]
    fn a_vague_verb_against_a_named_file_is_not_treated_as_vague() {
        // "refactor" alone is a blank cheque; "refactor crates/api/src/client.rs"
        // is a bounded request and should not trigger an interview.
        let t = triage("refactor crates/api/src/client.rs");
        assert!(!t.has(Signal::VagueScope));
        assert!(t.has(Signal::ConcreteTarget));
        assert!(!t.runs(Stage::Clarify));
    }

    #[test]
    fn a_dangling_reference_triggers_clarification() {
        let t = triage("fix it");
        assert!(t.has(Signal::UnresolvedReference));
        assert!(t.runs(Stage::Clarify));
    }

    #[test]
    fn irreversible_work_is_always_hardened_even_when_precisely_stated() {
        let t = triage("delete the users table in migrations/003_users.sql");
        assert!(t.has(Signal::Irreversible));
        assert!(t.runs(Stage::Harden));
        assert_eq!(t.complexity, Complexity::Complex);
    }

    #[test]
    fn a_precisely_stated_destructive_request_still_does_not_get_interviewed() {
        // It is dangerous, not unclear. Asking questions would be noise; the
        // value is in hardening.
        let t = triage("delete the users table in migrations/003_users.sql");
        assert!(!t.runs(Stage::Clarify));
        assert!(t.runs(Stage::Harden));
    }

    #[test]
    fn unbounded_breadth_is_complex_but_not_ambiguous() {
        let t = triage("add error handling everywhere");
        assert!(t.has(Signal::Unbounded));
        assert_eq!(t.complexity, Complexity::Complex);
    }

    #[test]
    fn several_asks_in_one_message_are_planned_carefully() {
        let t = triage("add a flag and then update the docs and also bump the version");
        assert!(t.has(Signal::MultiPart));
        assert_eq!(t.complexity, Complexity::Complex);
    }

    #[test]
    fn one_conjunction_is_not_multiple_requests() {
        let t = triage("read and update the config in settings.toml");
        assert!(!t.has(Signal::MultiPart));
    }

    #[test]
    fn triage_explains_itself() {
        let t = triage("clean this up");
        let rationale = t.rationale();
        assert!(rationale.contains("complex"), "{rationale}");
        assert!(rationale.contains("vague scope"), "{rationale}");
    }

    #[test]
    fn stages_run_in_order_and_terminate() {
        let prompt = EnhancedPrompt::new("clean this up");
        let mut stage = prompt.first_stage();
        let mut seen = vec![stage];
        while let Some(next) = prompt.next_after(stage) {
            stage = next;
            seen.push(stage);
        }
        assert_eq!(seen, prompt.triage.stages);
        assert_eq!(prompt.next_after(Stage::Verify), None);
    }

    #[test]
    fn a_rendered_stage_carries_prior_work_forward() {
        let prompt = EnhancedPrompt::new("add a --json flag to main.rs");
        let rendered = prompt.render_stage(
            Stage::Execute,
            &[(Stage::Plan, "APPROACH: add the flag".to_string())],
        );
        assert!(rendered.contains("# Stage: execute"));
        assert!(rendered.contains("add a --json flag to main.rs"));
        assert!(rendered.contains("# From the plan stage"));
        assert!(rendered.contains("APPROACH: add the flag"));
    }

    #[test]
    fn empty_carry_sections_are_omitted() {
        let prompt = EnhancedPrompt::new("add a flag to main.rs");
        let rendered = prompt.render_stage(Stage::Execute, &[(Stage::Plan, "   ".to_string())]);
        assert!(!rendered.contains("# From the plan stage"));
    }

    #[test]
    fn every_stage_directive_states_an_output_shape() {
        // Small models drift without one, and a drifting reply cannot be
        // parsed by the following stage.
        for stage in [
            Stage::Clarify,
            Stage::Plan,
            Stage::Harden,
            Stage::Execute,
            Stage::Verify,
        ] {
            let directive = stage.directive();
            assert!(
                directive.contains("# Stage:"),
                "{stage} directive lacks a header"
            );
            assert!(directive.len() > 80, "{stage} directive is too thin");
        }
    }

    #[test]
    fn split_verb_particle_phrasings_are_recognised() {
        // "clean up" is easy; "clean this up" is the phrasing people actually
        // use, and a plain substring list misses it entirely.
        for request in [
            "clean up",
            "clean this up",
            "tidy the auth module up",
            "sort this out",
        ] {
            let t = triage(request);
            assert!(
                t.has(Signal::VagueScope),
                "{request:?} should read as vague"
            );
        }
    }

    #[test]
    fn a_clarify_reply_with_no_questions_moves_straight_on() {
        assert_eq!(parse_clarify("NO QUESTIONS"), ClarifyOutcome::NoQuestions);
        assert_eq!(
            parse_clarify("Everything is clear, so: NO QUESTIONS"),
            ClarifyOutcome::NoQuestions
        );
    }

    #[test]
    fn a_clarify_reply_yields_its_questions() {
        let reply = "1. Which database should this target?\n2. Should the old API stay?";
        match parse_clarify(reply) {
            ClarifyOutcome::Questions(q) => {
                assert_eq!(q.len(), 2);
                assert!(q[0].contains("database"));
            }
            other => panic!("expected questions, got {other:?}"),
        }
    }

    #[test]
    fn non_question_bullets_are_not_mistaken_for_questions() {
        let reply = "1. I will add the flag\n2. Then update docs";
        assert_eq!(parse_clarify(reply), ClarifyOutcome::NoQuestions);
    }

    #[test]
    fn a_plan_reply_is_parsed_into_approach_steps_and_criteria() {
        let reply = "APPROACH: Add a flag and thread it through.\n\
                     STEPS:\n\
                     1. Add the CLI flag\n\
                     2. Thread it to the renderer\n\
                     CRITERIA:\n\
                     - `disco status --json` emits valid JSON\n\
                     - existing text output is unchanged";
        let plan = parse_plan(reply);
        assert_eq!(
            plan.approach.as_deref(),
            Some("Add a flag and thread it through.")
        );
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.criteria.len(), 2);
        assert!(plan.criteria[0].contains("valid JSON"));
    }

    #[test]
    fn plan_parsing_tolerates_the_layout_a_small_model_actually_produces() {
        // Markdown headings, mixed bullets, lowercase.
        let reply = "## Approach\nKeep it simple.\n\n## Steps\n* first thing\n* second thing\n\n## Criteria\n- it compiles";
        let plan = parse_plan(reply);
        assert_eq!(plan.approach.as_deref(), Some("Keep it simple."));
        assert_eq!(plan.steps, vec!["first thing", "second thing"]);
        assert_eq!(plan.criteria, vec!["it compiles"]);
    }

    #[test]
    fn a_reply_with_no_recognisable_plan_yields_an_empty_plan() {
        assert!(parse_plan("I'll just get started.").is_empty());
    }

    #[test]
    fn verification_distinguishes_not_met_from_met() {
        // "NOT MET" contains "MET"; a naive check would read every failure as
        // a pass, which is the worst possible direction to be wrong in.
        let reply = "MET: it compiles - cargo build exited 0\n\
                     NOT MET: tests pass - 2 failures in api\n\
                     NOT CHECKED: docs updated - did not look";
        let results = parse_verification(reply);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].state, CriterionState::Met);
        assert_eq!(results[1].state, CriterionState::NotMet);
        assert_eq!(results[2].state, CriterionState::NotChecked);
        assert!(!verification_passed(&results));
    }

    #[test]
    fn verification_passes_only_when_everything_is_confirmed() {
        let results = parse_verification("MET: it compiles - exit 0\nMET: tests pass - 41 passed");
        assert!(verification_passed(&results));
    }

    #[test]
    fn an_unparseable_verification_does_not_count_as_passing() {
        assert!(!verification_passed(&parse_verification("Looks good to me!")));
    }

    #[test]
    fn an_empty_request_does_not_panic_and_is_not_trivial_by_accident() {
        let t = triage("");
        assert!(!t.stages.is_empty());
    }
}

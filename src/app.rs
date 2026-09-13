use payment_routing::operations::{Dossier, ObservedRun, Operations, PaymentStatus, Preset};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    widgets::TableState,
};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Watch,
    Payments,
    Rails,
    Network,
    Optimizer,
    Compare,
}
impl View {
    #[cfg(test)]
    pub const ALL: [Self; 6] = [
        Self::Watch,
        Self::Payments,
        Self::Rails,
        Self::Network,
        Self::Optimizer,
        Self::Compare,
    ];
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Active,
    Queued,
    Failed,
    Finished,
}
impl Filter {
    const ALL: [Self; 5] = [
        Self::All,
        Self::Active,
        Self::Queued,
        Self::Failed,
        Self::Finished,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Active => "active",
            Self::Queued => "queues",
            Self::Failed => "SLA/overload",
            Self::Finished => "finished",
        }
    }
    fn matches(self, p: &Dossier) -> bool {
        match self {
            Self::All => true,
            Self::Active => !p.status.terminal(),
            Self::Queued => matches!(p.status, PaymentStatus::Queued | PaymentStatus::Waiting),
            Self::Failed => matches!(
                p.status,
                PaymentStatus::Draining
                    | PaymentStatus::Late
                    | PaymentStatus::Expired
                    | PaymentStatus::Rejected
            ),
            Self::Finished => p.status.terminal(),
        }
    }
}

pub struct App {
    pub ops: Operations,
    pub view: View,
    pub strategy: usize,
    pub tables: [TableState; 6],
    pub scroll: [u16; 6],
    pub running: bool,
    pub speed: usize,
    pub filter: Filter,
    pub query: String,
    pub editing: Option<String>,
    pub selected_payment: Option<u128>,
    pub detail: Option<Dossier>,
    pub rail_detail: bool,
    pub evidence: bool,
    pub follow_latest: bool,
    pub detail_scroll: u16,
    pub help: bool,
    pub scenario_choice: Option<usize>,
    pub help_scroll: u16,
    pub error: Option<String>,
    pub notice: String,
}
impl App {
    pub const SPEEDS: [u64; 5] = [1, 2, 5, 20, 100];
    pub fn new(
        preset: Preset,
        seed: u64,
    ) -> Result<Self, payment_routing::simulation::SimulationError> {
        Ok(Self {
            ops: Operations::new(preset, seed)?,
            view: View::Watch,
            strategy: 1,
            tables: Default::default(),
            scroll: [0; 6],
            running: false,
            speed: 1,
            filter: Filter::All,
            query: String::new(),
            editing: None,
            selected_payment: None,
            detail: None,
            rail_detail: false,
            evidence: false,
            follow_latest: true,
            detail_scroll: 0,
            help: false,
            scenario_choice: None,
            help_scroll: 0,
            error: None,
            notice: String::new(),
        })
    }
    pub fn run(&self) -> &ObservedRun {
        &self.ops.runs[self.strategy]
    }
    pub fn strategy_name(&self) -> &'static str {
        if self.strategy == 0 {
            "Static"
        } else {
            "Reserved"
        }
    }
    pub fn period(&self) -> Duration {
        Duration::from_millis(1000 / Self::SPEEDS[self.speed])
    }
    pub fn payment_ids(&self) -> Vec<u128> {
        if self.view == View::Compare {
            let mut ids: Vec<_> = self.ops.runs[0]
                .payments
                .keys()
                .chain(self.ops.runs[1].payments.keys())
                .copied()
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids.retain(|id| {
                match (
                    self.ops.runs[0].payments.get(id),
                    self.ops.runs[1].payments.get(id),
                ) {
                    (Some(a), Some(b)) => {
                        a.status != b.status
                            || a.route != b.route
                            || a.actual_fee_cents != b.actual_fee_cents
                    }
                    _ => true,
                }
            });
            ids.reverse();
            return ids;
        }
        let q = self.query.to_lowercase();
        self.run()
            .payments
            .iter()
            .rev()
            .filter(|(_, p)| {
                self.filter.matches(p)
                    && (q.is_empty()
                        || format!(
                            "{} {} {} {} {}",
                            p.payment.id,
                            p.payment.sender,
                            p.payment.receiver,
                            p.status.label(),
                            p.route
                                .as_ref()
                                .map(|r| r
                                    .hops
                                    .iter()
                                    .map(|h| h.rail_id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" "))
                                .unwrap_or_default()
                        )
                        .to_lowercase()
                        .contains(&q))
            })
            .map(|(&id, _)| id)
            .collect()
    }
    pub fn selected_dossier(&self, id: u128) -> Option<&Dossier> {
        self.run()
            .payments
            .get(&id)
            .or_else(|| self.ops.runs[1 - self.strategy].payments.get(&id))
    }
    pub fn sync_selection(&mut self) {
        let ids = self.payment_ids();
        let selected = self
            .selected_payment
            .filter(|_| !self.follow_latest)
            .and_then(|id| ids.iter().position(|&i| i == id))
            .or(if ids.is_empty() { None } else { Some(0) });
        self.tables[View::Payments as usize].select(selected);
        self.selected_payment = selected.map(|i| ids[i]);
        for view in [View::Watch, View::Rails, View::Network] {
            let count = self.row_count(view);
            let state = &mut self.tables[view as usize];
            state.select(if count == 0 {
                None
            } else {
                Some(state.selected().unwrap_or(0).min(count - 1))
            });
        }
    }
    pub fn step(&mut self) {
        if self.error.is_some() {
            self.running = false;
            return;
        }
        if let Err(e) = self.ops.step() {
            self.error = Some(e.to_string());
            self.running = false;
        }
        if self.error.is_none() {
            self.notice.clear();
        }
        self.sync_selection();
        if let Some(detail) = &self.detail {
            self.detail = self
                .run()
                .payments
                .get(&detail.sequence)
                .cloned()
                .or_else(|| self.detail.clone());
        }
    }
    fn restart(&mut self, seed: u64, preset: Preset) {
        self.running = false;
        match Operations::new(preset, seed) {
            Ok(ops) => {
                self.ops = ops;
                self.error = None;
                self.detail = None;
                self.rail_detail = false;
                self.evidence = false;
                self.follow_latest = true;
                self.selected_payment = None;
                self.tables = Default::default();
                self.scroll = [0; 6];
                self.notice = format!("Seed {seed}");
                self.sync_selection();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    pub fn watch_rails(&self) -> Vec<usize> {
        let rails = self.run().simulator.rail_states();
        let mut indices: Vec<_> = (0..rails.len()).collect();
        indices.sort_by_key(|&i| match rails[i].rail_id.as_str() {
            "RTP" => 0,
            "FEDNOW" => 1,
            "ACH" => 2,
            "FEDWIRE" => 3,
            _ => 4,
        });
        indices
    }
    pub fn selected_rail(&self) -> usize {
        if self.view == View::Watch {
            self.watch_rails()
                .get(self.tables[0].selected().unwrap_or(0))
                .copied()
                .unwrap_or(0)
        } else {
            self.tables[View::Rails as usize].selected().unwrap_or(0)
        }
    }
    pub fn row_count(&self, view: View) -> usize {
        match view {
            View::Payments | View::Compare => self.payment_ids().len(),
            View::Watch | View::Rails => self.run().simulator.rail_states().len(),
            View::Network => self.run().simulator.scenario().network.institutions.len(),
            _ => 0,
        }
    }
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return true;
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return false;
        }
        if let Some(input) = &mut self.editing {
            match key.code {
                KeyCode::Esc => self.editing = None,
                KeyCode::Enter => {
                    self.query = input.clone();
                    self.editing = None;
                    self.selected_payment = None;
                    self.sync_selection();
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) if input.len() < 64 => input.push(c),
                _ => {}
            }
            return false;
        }
        if key.code == KeyCode::Char('q') {
            return true;
        }
        if let Some(choice) = self.scenario_choice {
            match key.code {
                KeyCode::Esc => self.scenario_choice = None,
                KeyCode::Down | KeyCode::Char('j') => {
                    self.scenario_choice = Some((choice + 1) % Preset::ALL.len())
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.scenario_choice =
                        Some((choice + Preset::ALL.len() - 1) % Preset::ALL.len())
                }
                KeyCode::Enter => {
                    self.scenario_choice = None;
                    self.restart(self.run().simulator.seed(), Preset::ALL[choice]);
                }
                _ => {}
            }
            return false;
        }
        if key.code == KeyCode::Esc {
            if self.help {
                self.help = false;
            } else if self.evidence {
                self.evidence = false;
                self.detail_scroll = 0;
            } else if self.detail.is_some() || self.rail_detail {
                self.detail = None;
                self.rail_detail = false;
            } else if !matches!(self.view, View::Watch | View::Compare) {
                self.view = View::Watch;
            } else {
                return true;
            }
            return false;
        }
        if key.code == KeyCode::Char('?') {
            self.help = !self.help;
            self.running = false;
            return false;
        }
        if self.help {
            self.help_scroll = match key.code {
                KeyCode::Down | KeyCode::Char('j') => self.help_scroll.saturating_add(1),
                KeyCode::Up | KeyCode::Char('k') => self.help_scroll.saturating_sub(1),
                KeyCode::PageDown => self.help_scroll.saturating_add(8),
                KeyCode::PageUp => self.help_scroll.saturating_sub(8),
                KeyCode::Home => 0,
                KeyCode::End => u16::MAX,
                _ => self.help_scroll,
            };
            return false;
        }
        // Do not repeat destructive controls on keyboard auto-repeat.
        if key.kind == KeyEventKind::Repeat
            && matches!(key.code, KeyCode::Char('r' | 'n' | 'c' | ' '))
        {
            return false;
        }
        match key.code {
            KeyCode::Char(' ')
                if self.detail.is_none()
                    && !self.rail_detail
                    && !self.evidence
                    && self.error.is_none() =>
            {
                self.running = !self.running
            }
            KeyCode::Char('.') => {
                self.running = false;
                self.step();
            }
            KeyCode::Char('+' | '=') => self.speed = (self.speed + 1).min(Self::SPEEDS.len() - 1),
            KeyCode::Char('-') => self.speed = self.speed.saturating_sub(1),
            KeyCode::Char('r') => self.restart(self.run().simulator.seed(), self.ops.preset),
            KeyCode::Char('n') => {
                self.restart(self.run().simulator.seed().wrapping_add(1), self.ops.preset)
            }
            KeyCode::Char('c') => {
                self.running = false;
                self.scenario_choice = Some(self.ops.preset as usize);
            }
            KeyCode::Char('p') => {
                if self.rail_detail {
                    self.filter = Filter::All;
                    self.query = self.run().simulator.rail_states()[self.selected_rail()]
                        .rail_id
                        .clone();
                }
                self.view = View::Payments;
                self.rail_detail = false;
                self.detail = None;
                self.evidence = false;
                self.running = false;
                self.sync_selection();
            }
            KeyCode::Char('e')
                if self.detail.is_some() || self.rail_detail || self.view == View::Compare =>
            {
                self.evidence = !self.evidence;
                self.running = false;
                self.detail_scroll = 0;
            }
            KeyCode::Char('s') => {
                self.rail_detail = false;
                self.strategy = 1 - self.strategy;
                self.detail = None;
                self.evidence = false;
                self.sync_selection();
            }
            KeyCode::Char('/') => {
                self.rail_detail = false;
                self.view = View::Payments;
                self.detail = None;
                self.running = false;
                self.evidence = false;
                self.editing = Some(self.query.clone());
            }
            KeyCode::Char('f') if self.detail.is_none() && !self.rail_detail => {
                self.view = View::Payments;
                self.filter = Filter::ALL[(self.filter as usize + 1) % Filter::ALL.len()];
                self.selected_payment = None;
                self.sync_selection();
            }
            KeyCode::Enter
                if matches!(self.view, View::Watch | View::Rails)
                    && !self.rail_detail
                    && self.detail.is_none()
                    && !self.evidence =>
            {
                self.rail_detail = true;
                self.evidence = false;
                self.detail_scroll = 0;
                self.running = false;
            }
            KeyCode::Enter
                if matches!(self.view, View::Payments | View::Compare)
                    && self.detail.is_none()
                    && !self.evidence =>
            {
                self.sync_selection();
                if self
                    .selected_payment
                    .is_some_and(|id| !self.run().payments.contains_key(&id))
                {
                    self.strategy = 1 - self.strategy;
                }
                self.detail = self
                    .selected_payment
                    .and_then(|id| self.selected_dossier(id).cloned());
                self.evidence = false;
                self.detail_scroll = 0;
                self.running = false;
            }
            KeyCode::Down
            | KeyCode::Up
            | KeyCode::Char('j' | 'k')
            | KeyCode::PageDown
            | KeyCode::PageUp
            | KeyCode::Home
            | KeyCode::End => self.move_row(key.code),
            _ if self.detail.is_none() && !self.rail_detail && !self.evidence => {
                match key.code {
                    KeyCode::Tab
                    | KeyCode::BackTab
                    | KeyCode::Right
                    | KeyCode::Left
                    | KeyCode::Char('h' | 'l') => {
                        self.view = if self.view == View::Compare {
                            View::Watch
                        } else {
                            View::Compare
                        };
                    }
                    KeyCode::Char('1') => self.view = View::Watch,
                    KeyCode::Char('2') => self.view = View::Compare,
                    KeyCode::Char('3') => self.view = View::Rails,
                    KeyCode::Char('4') => self.view = View::Network,
                    KeyCode::Char('5') => self.view = View::Optimizer,
                    _ => {}
                }
                self.sync_selection();
            }
            _ => {}
        }
        false
    }
    fn move_row(&mut self, key: KeyCode) {
        let down = matches!(key, KeyCode::Down | KeyCode::Char('j') | KeyCode::PageDown);
        let amount = if matches!(key, KeyCode::PageDown | KeyCode::PageUp) {
            8
        } else {
            1
        };
        if self.detail.is_some()
            || self.rail_detail
            || self.evidence
            || self.row_count(self.view) == 0
        {
            let offset = if self.detail.is_some() || self.rail_detail || self.evidence {
                &mut self.detail_scroll
            } else {
                &mut self.scroll[self.view as usize]
            };
            *offset = match key {
                KeyCode::Home => 0,
                KeyCode::End => u16::MAX,
                _ if down => offset.saturating_add(amount),
                _ => offset.saturating_sub(amount),
            };
            return;
        }
        let count = self.row_count(self.view);
        let table_view = match self.view {
            View::Compare => View::Payments,
            view => view,
        };
        let state = &mut self.tables[table_view as usize];
        let current = state.selected().unwrap_or(0);
        let next = match key {
            KeyCode::Home => 0,
            KeyCode::End => count - 1,
            _ if down => current.saturating_add(amount as usize).min(count - 1),
            _ => current.saturating_sub(amount as usize),
        };
        state.select(Some(next));
        if matches!(self.view, View::Payments | View::Compare) {
            self.follow_latest = key == KeyCode::Home;
            self.selected_payment = self.payment_ids().get(next).copied();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(app: &mut App, code: KeyCode) -> bool {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    #[test]
    fn watch_compare_inspection_and_scenario_cancel_preserve_simulation() {
        let mut app = App::new(Preset::Balanced, 42).unwrap();
        for _ in 0..40 {
            app.step();
        }
        let expected = app.ops.clone();
        app.sync_selection();
        assert_eq!(
            app.run().simulator.rail_states()[app.selected_rail()].rail_id,
            "RTP"
        );
        key(&mut app, KeyCode::Down);
        assert_eq!(
            app.run().simulator.rail_states()[app.selected_rail()].rail_id,
            "FEDNOW"
        );
        app.running = true;
        key(&mut app, KeyCode::Enter);
        assert!(app.rail_detail && !app.running);
        key(&mut app, KeyCode::Char('e'));
        assert!(app.evidence);
        key(&mut app, KeyCode::Esc);
        assert!(app.rail_detail && !app.evidence);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.view, View::Compare);
        assert!(!app.payment_ids().is_empty());
        key(&mut app, KeyCode::Down);
        let id = app.selected_payment.unwrap();
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.detail.as_ref().unwrap().sequence, id);
        assert!(!app.evidence);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char('c'));
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Esc);
        assert!(app.scenario_choice.is_none());
        assert_eq!(app.ops, expected);
    }

    #[test]
    fn differences_include_fee_only_and_records_retained_by_either_strategy() {
        let mut app = App::new(Preset::Balanced, 42).unwrap();
        app.step();
        let dossier = app.ops.runs[0].payments[&1].clone();
        app.ops.runs[1].payments.insert(1, dossier);
        app.view = View::Compare;
        assert!(!app.payment_ids().contains(&1));
        app.ops.runs[1]
            .payments
            .get_mut(&1)
            .unwrap()
            .actual_fee_cents += 1;
        assert!(app.payment_ids().contains(&1));
        app.ops.runs[1].payments.remove(&1);
        app.selected_payment = Some(1);
        app.follow_latest = false;
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.detail.as_ref().unwrap().sequence, 1);
        assert_eq!(app.strategy, 0);
    }

    #[test]
    fn controls_replay_restart_and_keep_both_runs_at_one_minute() {
        let mut a = App::new(Preset::Balanced, 42).unwrap();
        assert!(!a.running);
        key(&mut a, KeyCode::Char(' '));
        assert!(a.running);
        for _ in 0..30 {
            a.step();
        }
        let expected = a.ops.clone();
        key(&mut a, KeyCode::Char('+'));
        key(&mut a, KeyCode::Char('s'));
        assert_eq!(a.ops, expected);
        assert_eq!(a.strategy, 0);
        key(&mut a, KeyCode::Char('r'));
        assert!(!a.running);
        assert!(a.ops.runs.iter().all(|r| r.simulator.next_minute() == 0));
        for _ in 0..30 {
            key(&mut a, KeyCode::Char('.'));
        }
        assert_eq!(a.ops, expected);
        key(&mut a, KeyCode::Char('n'));
        assert_eq!(a.run().simulator.seed(), 43);
        key(&mut a, KeyCode::Char('c'));
        key(&mut a, KeyCode::Down);
        key(&mut a, KeyCode::Enter);
        assert_eq!(a.ops.preset, Preset::Pressure);
        assert_eq!(a.run().simulator.seed(), 43);
        assert!(!a.running);
    }
    #[test]
    fn selection_tracks_payment_identity_and_inspection_search_pause() {
        let mut a = App::new(Preset::Balanced, 42).unwrap();
        for _ in 0..40 {
            a.step();
        }
        key(&mut a, KeyCode::Char('p'));
        key(&mut a, KeyCode::Down);
        let selected = a.selected_payment;
        a.step();
        assert_eq!(a.selected_payment, selected);
        a.running = true;
        key(&mut a, KeyCode::Enter);
        assert!(!a.running);
        assert_eq!(a.detail.as_ref().map(|p| p.sequence), selected);
        assert!(!key(&mut a, KeyCode::Esc));
        assert!(a.detail.is_none());
        a.running = true;
        key(&mut a, KeyCode::Char('/'));
        assert!(!a.running);
        for c in "SIM-1".chars() {
            key(&mut a, KeyCode::Char(c));
        }
        key(&mut a, KeyCode::Enter);
        assert!(
            a.payment_ids()
                .iter()
                .all(|id| format!("SIM-{id}").contains("SIM-1"))
        );
        key(&mut a, KeyCode::Char('/'));
        key(&mut a, KeyCode::Char('q'));
        assert!(a.editing.as_ref().unwrap().ends_with('q'));
        key(&mut a, KeyCode::Esc);
        assert_eq!(a.query, "SIM-1");
    }
    #[test]
    fn help_release_repeat_modifiers_and_empty_rows_are_safe() {
        let mut a = App::new(Preset::Balanced, u64::MAX).unwrap();
        key(&mut a, KeyCode::Char('p'));
        key(&mut a, KeyCode::End);
        key(&mut a, KeyCode::Enter);
        assert!(a.detail.is_none());
        key(&mut a, KeyCode::Char('?'));
        key(&mut a, KeyCode::PageDown);
        assert_eq!(a.help_scroll, 8);
        assert!(!key(&mut a, KeyCode::Esc));
        assert!(!a.help);
        let mut event = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        event.kind = KeyEventKind::Repeat;
        a.handle_key(event);
        assert_eq!(a.run().simulator.seed(), u64::MAX);
        event.kind = KeyEventKind::Release;
        a.handle_key(event);
        assert_eq!(a.run().simulator.seed(), u64::MAX);
        key(&mut a, KeyCode::Char('n'));
        assert_eq!(a.run().simulator.seed(), 0);
        assert!(a.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        for view in View::ALL {
            a.view = view;
            assert!(key(&mut a, KeyCode::Char('q')));
            assert_eq!(
                key(&mut a, KeyCode::Esc),
                matches!(view, View::Watch | View::Compare)
            );
        }
    }
}

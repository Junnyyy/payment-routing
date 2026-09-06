use payment_routing::network::Network;
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    widgets::TableState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Overview,
    Institutions,
    Rails,
    Payments,
}

impl View {
    const ALL: [Self; 4] = [
        Self::Overview,
        Self::Institutions,
        Self::Rails,
        Self::Payments,
    ];
}

pub struct App {
    pub network: Network,
    pub view: View,
    pub tables: [TableState; 4],
}

impl App {
    pub fn new(network: Network) -> Self {
        let mut app = Self {
            network,
            view: View::Overview,
            tables: Default::default(),
        };
        for view in View::ALL {
            if app.row_count(view) > 0 {
                app.tables[view as usize].select(Some(0));
            }
        }
        app
    }

    pub fn row_count(&self, view: View) -> usize {
        match view {
            View::Overview => 0,
            View::Institutions => self.network.institutions.len(),
            View::Rails => self.network.rails.len(),
            View::Payments => self.network.payments.len(),
        }
    }

    /// Returns true to exit. Key releases never repeat an action.
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
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                self.view = View::ALL[(self.view as usize + 1) % View::ALL.len()];
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                self.view = View::ALL[(self.view as usize + View::ALL.len() - 1) % View::ALL.len()];
            }
            KeyCode::Char('1') => self.view = View::Overview,
            KeyCode::Char('2') => self.view = View::Institutions,
            KeyCode::Char('3') => self.view = View::Rails,
            KeyCode::Char('4') => self.view = View::Payments,
            KeyCode::Down
            | KeyCode::Char('j')
            | KeyCode::Up
            | KeyCode::Char('k')
            | KeyCode::Home
            | KeyCode::End => self.move_row(key.code),
            _ => {}
        }
        false
    }

    fn move_row(&mut self, key: KeyCode) {
        let count = self.row_count(self.view);
        let state = &mut self.tables[self.view as usize];
        if count == 0 {
            state.select(None);
            return;
        }
        let current = state.selected().unwrap_or(0);
        let next = match key {
            KeyCode::Home => 0,
            KeyCode::End => count - 1,
            KeyCode::Down | KeyCode::Char('j') => current.saturating_add(1).min(count - 1),
            _ => current.saturating_sub(1),
        };
        state.select(Some(next));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use payment_routing::demo::demo_network;

    fn press(app: &mut App, code: KeyCode) -> bool {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn tabs_wrap_both_ways_and_shortcuts_select_each_view() {
        let mut app = App::new(demo_network());
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.view, View::Payments);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.view, View::Overview);
        for (number, view) in [
            ('1', View::Overview),
            ('2', View::Institutions),
            ('3', View::Rails),
            ('4', View::Payments),
        ] {
            press(&mut app, KeyCode::Char(number));
            assert_eq!(app.view, view);
        }
        press(&mut app, KeyCode::Right);
        assert_eq!(app.view, View::Overview);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.view, View::Payments);
    }

    #[test]
    fn rows_clamp_preserve_selection_per_view_and_never_change_network() {
        let original = demo_network();
        let mut app = App::new(original.clone());
        press(&mut app, KeyCode::Char('4'));
        press(&mut app, KeyCode::Up);
        assert_eq!(app.tables[3].selected(), Some(0));
        press(&mut app, KeyCode::End);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.tables[3].selected(), Some(11));
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.tables[3].selected(), Some(10));
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.tables[1].selected(), Some(0));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.tables[1].selected(), Some(1));
        press(&mut app, KeyCode::Char('4'));
        assert_eq!(app.tables[3].selected(), Some(10));
        press(&mut app, KeyCode::Home);
        assert_eq!(app.tables[3].selected(), Some(0));
        assert_eq!(app.network, original);
    }

    #[test]
    fn empty_collections_and_overview_have_no_selection() {
        let network = Network {
            name: "Empty".into(),
            institutions: vec![],
            rails: vec![],
            payments: vec![],
        };
        let mut app = App::new(network);
        for number in ['1', '2', '3', '4'] {
            press(&mut app, KeyCode::Char(number));
            for key in [KeyCode::Up, KeyCode::Down, KeyCode::Home, KeyCode::End] {
                press(&mut app, key);
                assert_eq!(app.tables[app.view as usize].selected(), None);
            }
        }
    }

    #[test]
    fn quit_keys_work_from_every_view_and_releases_are_ignored() {
        let mut app = App::new(demo_network());
        for view in View::ALL {
            app.view = view;
            assert!(press(&mut app, KeyCode::Char('q')));
            assert!(press(&mut app, KeyCode::Esc));
            assert!(app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
            let mut released = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
            released.kind = KeyEventKind::Release;
            assert!(!app.handle_key(released));
            assert!(!app.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)));
            assert_eq!(app.view, view);
        }
    }
}

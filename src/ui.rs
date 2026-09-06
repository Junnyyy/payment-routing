use payment_routing::network::Network;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph, Row, Table, Tabs},
};

use crate::app::{App, View};

const ACCENT: Color = Color::Cyan;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 80 || area.height < 18 {
        frame.render_widget(
            Paragraph::new("Resize to at least 80 x 18.\nq / Esc / Ctrl-C: quit")
                .block(Block::bordered().title("payment-routing")),
            area,
        );
        return;
    }
    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(2),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(format!("{}  |  SYNTHETIC  |  USD", app.network.name))
            .block(Block::bordered().title("payment-routing / network explorer"))
            .style(Style::new().fg(ACCENT)),
        header,
    );
    frame.render_widget(
        Tabs::new(["1 Overview", "2 Institutions", "3 Rails", "4 Payments"])
            .select(app.view as usize)
            .highlight_style(
                Style::new()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
        tabs,
    );
    if app.view == View::Overview {
        overview(frame, body, &app.network);
    } else {
        records(frame, body, app);
    }
    frame.render_widget(
        Paragraph::new("Tab / Shift-Tab / Left / Right: views   1-4: jump   q / Esc / Ctrl-C: quit\nUp / Down or j / k: rows   Home / End: first / last"),
        footer,
    );
}

fn overview(frame: &mut Frame, area: Rect, network: &Network) {
    let stats = network.statistics();
    let lines = vec![
        Line::from(format!(
            " Institutions {:>3}    Payment rails {:>3}    Payments {:>3}",
            stats.institution_count, stats.rail_count, stats.payment_count
        )),
        Line::from(""),
        Line::from(format!(
            " Opening liquidity     USD {}",
            money(stats.opening_balance_cents)
        )),
        Line::from(format!(
            " Payment volume        USD {}",
            money(stats.payment_volume_cents)
        )),
        Line::from(format!(
            " Largest payment       USD {}",
            money(u128::from(stats.largest_payment_cents))
        )),
        Line::from(""),
        Line::from(" All payments await routing. Opening balances are unchanged."),
        Line::from(" Rail membership, fees and settlement times are synthetic inputs."),
        Line::from(" No routes, fees incurred, or settlement outcomes have been computed."),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title("Scenario overview")),
        area,
    );
}

fn records(frame: &mut Frame, area: Rect, app: &mut App) {
    let (title, headers, widths, rows) = match app.view {
        View::Institutions => (
            "Institutions / opening balances in USD",
            vec!["ID", "Institution", "Opening balance"],
            vec![
                Constraint::Length(8),
                Constraint::Fill(1),
                Constraint::Length(20),
            ],
            app.network
                .institutions
                .iter()
                .map(|i| {
                    Row::new(vec![
                        i.id.clone(),
                        i.name.clone(),
                        money(u128::from(i.opening_balance_cents)),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
        View::Rails => (
            "Payment rails / synthetic inputs",
            vec!["ID", "Rail", "Members", "Fee USD", "Minutes"],
            vec![
                Constraint::Length(7),
                Constraint::Length(13),
                Constraint::Fill(1),
                Constraint::Length(8),
                Constraint::Length(7),
            ],
            app.network
                .rails
                .iter()
                .map(|r| {
                    Row::new(vec![
                        r.id.clone(),
                        r.name.clone(),
                        r.participants.join(" "),
                        money(u128::from(r.fee_cents)),
                        r.settlement_minutes.to_string(),
                    ])
                })
                .collect(),
        ),
        View::Payments => (
            "Payments / unassigned instructions",
            vec!["ID", "From", "To", "Amount USD", "Status"],
            vec![
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(18),
                Constraint::Fill(1),
            ],
            app.network
                .payments
                .iter()
                .map(|p| {
                    Row::new(vec![
                        p.id.clone(),
                        p.sender.clone(),
                        p.receiver.clone(),
                        money(u128::from(p.amount_cents)),
                        "Awaiting routing".into(),
                    ])
                })
                .collect(),
        ),
        View::Overview => return,
    };
    let count = rows.len();
    if count == 0 {
        frame.render_widget(
            Paragraph::new("No records in this scenario.").block(Block::bordered().title(title)),
            area,
        );
        return;
    }
    let state = &mut app.tables[app.view as usize];
    let selected = state.selected().map_or(0, |index| index + 1);
    let table = Table::new(rows, widths)
        .header(Row::new(headers).style(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)))
        .block(Block::bordered().title(format!("{title} / {selected} of {count}")))
        .row_highlight_style(Style::new().fg(Color::Black).bg(ACCENT))
        .highlight_symbol("> ")
        .column_spacing(1);
    frame.render_stateful_widget(table, area, state);
}

/// Exact, grouped decimal display. Currency belongs in the column or metric label.
fn money(cents: u128) -> String {
    let whole = (cents / 100).to_string();
    let mut result = String::new();
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    format!("{result}.{:02}", cents % 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use payment_routing::demo::demo_network;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    };

    fn screen(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn overview_shows_computed_totals_and_synthetic_scope_at_minimum_size() {
        let output = screen(&mut App::new(demo_network()), 80, 18);
        for expected in [
            "SYNTHETIC",
            "USD",
            "Institutions   6",
            "Payment rails   4",
            "Payments  12",
            "1,000,000.00",
            "225,001.50",
            "75,000.00",
            "await routing",
            "Rail membership, fees and settlement times are synthetic inputs.",
            "Ctrl-C: quit",
        ] {
            assert!(output.contains(expected), "missing {expected}: {output}");
        }
    }

    #[test]
    fn all_record_views_show_identifiers_and_complete_values() {
        let mut app = App::new(demo_network());
        for (view, expected) in [
            (
                View::Institutions,
                vec!["ALP", "Alpine Bank", "250,000.00", "Field Credit"],
            ),
            (
                View::Rails,
                vec![
                    "Payment rails / synthetic inputs",
                    "RTP",
                    "FEDNOW",
                    "FedNow",
                    "ACH",
                    "FEDWIRE",
                    "Fedwire",
                    "ALP BRK CDR DLT ELM FLD",
                    "0.25",
                    "0.05",
                    "15.00",
                    "1440",
                ],
            ),
            (
                View::Payments,
                vec!["P001", "P012", "12,500.00", "900.25", "Awaiting routing"],
            ),
        ] {
            app.view = view;
            let heights: &[u16] = if view == View::Rails {
                &[18, 24]
            } else {
                &[24]
            };
            for &height in heights {
                let output = screen(&mut app, 80, height);
                for value in &expected {
                    assert!(output.contains(value), "missing {value}: {output}");
                }
            }
        }
    }

    #[test]
    fn navigation_scrolls_to_last_payment_then_back_to_first() {
        let mut app = App::new(demo_network());
        app.view = View::Payments;
        app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        let output = screen(&mut app, 80, 18);
        assert!(output.contains("P012"));
        assert!(output.contains("12 of 12"));
        assert!(app.tables[3].offset() > 0);
        app.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        let output = screen(&mut app, 80, 18);
        assert!(output.contains("P001"));
        assert_eq!(app.tables[3].offset(), 0);
    }

    #[test]
    fn small_terminals_and_empty_collections_render_safely() {
        let mut app = App::new(demo_network());
        assert!(screen(&mut app, 60, 10).contains("Resize to at least 80 x 18"));
        for (width, height) in [(1, 1), (0, 0), (79, 17)] {
            screen(&mut app, width, height);
        }
        app.network.payments.clear();
        app.view = View::Payments;
        assert!(screen(&mut app, 80, 24).contains("No records in this scenario"));
    }

    #[test]
    fn money_formatting_preserves_cents_without_floating_point() {
        assert_eq!(money(0), "0.00");
        assert_eq!(money(1), "0.01");
        assert_eq!(money(99_999), "999.99");
        assert_eq!(money(100_000), "1,000.00");
        assert_eq!(money(u128::from(u64::MAX)), "184,467,440,737,095,516.15");
    }
}

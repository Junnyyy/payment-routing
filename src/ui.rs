use crate::app::{App, View};
use payment_routing::{
    operations::{Dossier, PaymentStatus, RECENT_PAYMENTS},
    routing::Route,
    simulation::{EventKind, RoutingStrategy},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph, Row, Table, Tabs},
};

const ACCENT: Color = Color::Cyan;
const HELP: &str = "CONTROLS\nSpace run/pause   . step   +/- speed   r restart   n next seed\nTab watch/compare   1 watch   2 compare   p payments\nc choose scenario\ns switch strategy   ↑↓ select   Enter inspect   Esc back   q quit\nf payment filter   / search   Home newest   End oldest\ne evidence / comparison totals   PgUp/PgDn scroll details\n\nDIAGNOSTICS\n3 rail metrics   4 network   5 optimizer\n\nREADING THE TABLES\nCapacity: principal departed this minute / that minute's budget.\nMoving: hops in flight. Instant transfers appear in the event line.\nUnassigned: waiting without a complete route; excluded from rail queues.\nMissed: deadline failures, including late or still-moving payments.\nRejected: admission limit reached. Active: all unfinished payments.\nFees: every actual departure, including failed work.\nComparison: identical demand; different completion cohorts.\nDifferences: retained payments with different status, route or paid fee.\nNot retained: record aged out; not an outcome.\nSearch misses are unresolved, not proof of infeasibility.\n\nRETENTION\nAll active + newest 128 terminal payments per strategy.\n32 events and 24 search entries per payment; omissions counted.\n\nSynthetic USD. Accelerated timing. Descriptive opening balances.\nNo live payments, liquidity constraints, netting or actual settlement.";

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 80 || area.height < 18 {
        frame.render_widget(Paragraph::new(format!("Resize to at least 80 x 18. Current {} x {}\nSpace pause/run | q / Esc / Ctrl-C quit", area.width, area.height)).block(Block::bordered().title("payment-routing")), area);
        return;
    }
    app.sync_selection();
    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(2),
    ])
    .areas(area);
    let sim = &app.ops.runs[app.strategy].simulator;
    let minute = sim
        .next_minute()
        .checked_sub(1)
        .map_or("Ready".into(), |v| format!("{v}m"));
    let state = if app.error.is_some() {
        "Error"
    } else if app.running {
        "Running"
    } else {
        "Paused"
    };
    frame.render_widget(
        Paragraph::new(format!(
            "Payment routing   {} · {}   {} · {}   {} tick/s",
            app.ops.preset.name(),
            app.strategy_name(),
            state,
            minute,
            App::SPEEDS[app.speed]
        )),
        header,
    );
    frame.render_widget(
        Tabs::new(["Watch", "Compare"])
            .select(if app.view == View::Compare {
                Some(1)
            } else if app.view == View::Watch {
                Some(0)
            } else {
                None
            })
            .padding("", "")
            .divider("   ")
            .highlight_style(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        tabs,
    );
    if let Some(choice) = app.scenario_choice {
        let descriptions = [
            "Normal demand",
            "More arrivals",
            "ACH unavailable",
            "Limited search",
            "Changing rails",
        ];
        let lines = payment_routing::operations::Preset::ALL
            .iter()
            .enumerate()
            .map(|(i, preset)| {
                Line::styled(
                    format!(
                        "{} {:<14} {}",
                        if i == choice { ">" } else { " " },
                        preset.name(),
                        descriptions[i]
                    ),
                    if i == choice {
                        Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    },
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title("Scenario")),
            body,
        );
    } else if app.help {
        let mut lines = vec![
            format!("Seed {} · Synthetic USD", sim.seed()),
            String::new(),
        ];
        lines.extend(HELP.lines().map(str::to_string));
        text_page(frame, body, "Help", lines, &mut app.help_scroll);
    } else if app.evidence && app.view == View::Compare && app.detail.is_none() {
        let lines = comparison_totals(app, body.width.saturating_sub(2) as usize);
        text_page(frame, body, "Totals", lines, &mut app.detail_scroll);
    } else if app.rail_detail {
        let lines = if app.evidence {
            rail_detail_lines(app)
        } else {
            rail_summary(app)
        };
        text_page(
            frame,
            body,
            if app.evidence {
                "Rail evidence"
            } else {
                "Rail"
            },
            lines,
            &mut app.detail_scroll,
        );
    } else if let Some(detail) = &app.detail {
        let lines = if app.evidence {
            detail_lines(app, detail)
        } else {
            payment_summary(app, detail, body.width.saturating_sub(2) as usize)
        };
        text_page(
            frame,
            body,
            if app.evidence {
                "Payment evidence"
            } else {
                "Payment"
            },
            lines,
            &mut app.detail_scroll,
        );
    } else {
        match app.view {
            View::Watch => watch(frame, body, app),
            View::Compare => comparison(frame, body, app),
            View::Payments => payments(frame, body, app),
            View::Rails => rails(frame, body, app),
            View::Network => network(frame, body, app),
            View::Optimizer => {
                let lines = optimizer(app);
                text_page(frame, body, "Diagnostics", lines, &mut app.scroll[4]);
            }
        }
    }
    let controls = if app.scenario_choice.is_some() {
        "↑↓ select   Enter restart   Esc cancel"
    } else if app.help {
        "↑↓ scroll   Esc back"
    } else if app.detail.is_some() || app.rail_detail || app.evidence {
        "↑↓ scroll   e evidence   p payments   Esc back   ? help"
    } else if matches!(app.view, View::Watch | View::Compare) {
        "Space run/pause   ↑↓ select   Enter inspect   Tab view   c scenario   ? help"
    } else {
        "↑↓ select   Enter inspect   / search   Esc back   ? help"
    };
    let message = if let Some(error) = &app.error {
        format!("Error: {error} · r restart")
    } else if let Some(input) = &app.editing {
        format!("Search: {input}_   Enter apply   Esc cancel")
    } else {
        app.notice.clone()
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(message, Style::new().fg(Color::LightRed)),
            Line::from(controls),
        ]),
        footer,
    );
}

// Explicit word wrapping gives deterministic line counts for clamped scroll at any size.
fn wrapped(lines: Vec<String>, width: usize) -> Vec<String> {
    let mut out = vec![];
    for line in lines {
        if line.chars().count() <= width {
            out.push(line);
            continue;
        }
        if line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut row = String::new();
        for word in line.split_whitespace() {
            if !row.is_empty() && row.chars().count() + word.chars().count() + 1 > width {
                out.push(std::mem::take(&mut row));
            }
            for c in word.chars() {
                if row.chars().count() == width {
                    out.push(std::mem::take(&mut row));
                }
                row.push(c);
            }
            row.push(' ');
        }
        out.push(row.trim_end().to_string());
    }
    out
}
fn text_page(frame: &mut Frame, area: Rect, title: &str, lines: Vec<String>, scroll: &mut u16) {
    let lines = wrapped(lines, area.width.saturating_sub(2).max(1) as usize);
    let visible = area.height.saturating_sub(2) as usize;
    let max = lines.len().saturating_sub(visible).min(u16::MAX as usize) as u16;
    *scroll = (*scroll).min(max);
    let count = lines.len();
    frame.render_widget(
        Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>())
            .scroll((*scroll, 0))
            .block(Block::bordered().title(format!(
                "{title} [{}/{}]",
                *scroll as usize + 1,
                count
            ))),
        area,
    );
}

fn watch(frame: &mut Frame, area: Rect, app: &mut App) {
    let [grid, event, totals] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(2),
        Constraint::Length(3),
    ])
    .areas(area);
    let sim = &app.ops.runs[app.strategy].simulator;
    let rows = app
        .watch_rails()
        .into_iter()
        .map(|i| {
            let rail = &sim.rail_states()[i];
            let service = &sim.effective_scenario().services[i];
            let queued = sim
                .active_payments()
                .iter()
                .filter(|p| {
                    p.in_flight_until.is_none()
                        && p.has_complete_plan()
                        && p.route
                            .as_ref()
                            .and_then(|r| r.hops.get(p.next_hop))
                            .is_some_and(|h| h.rail_id == rail.rail_id)
                })
                .count();
            let capacity = if sim.next_minute() == 0 {
                "--".into()
            } else {
                service
                    .capacity_per_minute_cents
                    .map_or("Unlimited".into(), |cap| {
                        if cap == 0 {
                            return "No capacity".into();
                        }
                        let filled =
                            ((rail.used_this_minute_cents * 10) / u128::from(cap)).min(10) as usize;
                        format!(
                            "{}{} {:>3}%",
                            "█".repeat(filled),
                            "░".repeat(10 - filled),
                            rail.used_this_minute_cents * 100 / u128::from(cap)
                        )
                    })
            };
            Row::new(vec![
                rail.rail_id.clone(),
                if sim.next_minute() == 0 {
                    "--"
                } else if rail.open {
                    "Open"
                } else {
                    "Closed"
                }
                .into(),
                capacity,
                format!("{queued:>6}"),
                format!("{:>6}", rail.departed_hops - rail.settled_hops),
            ])
        })
        .collect::<Vec<_>>();
    let (grid, preview) = if grid.height >= 14 {
        let [table, preview] =
            Layout::vertical([Constraint::Length(8), Constraint::Fill(1)]).areas(grid);
        (table, Some(preview))
    } else {
        (grid, None)
    };
    frame.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(20),
                Constraint::Length(8),
                Constraint::Fill(1),
            ],
        )
        .header(
            Row::new(["Rail", "Status", "Capacity used", "Queued", "Moving"])
                .bottom_margin(1)
                .style(Style::new().fg(ACCENT)),
        )
        .row_highlight_style(Style::new().bg(Color::DarkGray))
        .highlight_symbol("> "),
        grid,
        &mut app.tables[0],
    );
    if let Some(preview) = preview {
        let lines = rail_summary(app);
        frame.render_widget(
            Paragraph::new(wrapped(lines, preview.width as usize).join("\n")),
            preview,
        );
    }
    let caption = latest_activity(app);
    frame.render_widget(
        Paragraph::new(wrapped(vec![caption], event.width as usize).join("\n")),
        event,
    );
    let m = sim.metrics();
    let unassigned = sim
        .active_payments()
        .iter()
        .filter(|p| !p.has_complete_plan() && p.in_flight_until.is_none())
        .count();
    let summary = format!(
        "On time {}   Missed {}   Rejected {}   Active {}   Fees ${}\nUnassigned {}",
        m.completed - m.completed_late,
        m.sla_failures - m.rejected,
        m.rejected,
        sim.active_payments().len(),
        money(m.routing_cost_cents),
        unassigned
    );
    frame.render_widget(
        Paragraph::new(
            wrapped(
                summary.lines().map(str::to_string).collect(),
                totals.width as usize,
            )
            .join("\n"),
        ),
        totals,
    );
}

fn latest_activity(app: &App) -> String {
    let sim = &app.ops.runs[app.strategy].simulator;
    if sim.next_minute() == 0 {
        return "Space to start".into();
    }
    let now = sim.next_minute() - 1;
    let change = app.run().network_events.iter().rev().find_map(|e| {
        if now.saturating_sub(e.minute) <= 3
            && let EventKind::DisruptionApplied(c) = &e.kind
        {
            let mut changes = vec![];
            if c.before.available != c.after.available {
                changes.push(if c.after.available {
                    if sim
                        .rail_states()
                        .iter()
                        .any(|r| r.rail_id == c.rail_id && !r.open)
                    {
                        "enabled · window closed".into()
                    } else {
                        "enabled".into()
                    }
                } else {
                    "closed".into()
                });
            }
            if c.before.capacity_per_minute_cents != c.after.capacity_per_minute_cents {
                changes.push(format!(
                    "capacity {}",
                    c.after
                        .capacity_per_minute_cents
                        .map_or("unlimited".into(), |v| format!("${}/m", money(v as u128)))
                ));
            }
            Some(format!(
                "{}m · {} {}",
                e.minute,
                c.rail_id,
                changes.join(" · ")
            ))
        } else {
            None
        }
    });
    change
        .or_else(|| {
            sim.recent_events().iter().rev().find_map(|e| {
                if now.saturating_sub(e.minute) > 3 {
                    return None;
                }
                event_label(&e.kind).map(|label| format!("{}m · {label}", e.minute))
            })
        })
        .unwrap_or_else(|| "Waiting for activity".into())
}

fn event_label(kind: &EventKind) -> Option<String> {
    match kind {
        EventKind::Completed { sequence, late, .. } => Some(format!(
            "SIM-{sequence} delivered{}",
            if *late { " late" } else { "" }
        )),
        EventKind::HopDeparted { sequence, hop, .. } => Some(format!(
            "SIM-{sequence} · {} → {} · {}",
            hop.sender, hop.receiver, hop.rail_id
        )),
        EventKind::DeadlineMissed { sequence } => Some(format!("SIM-{sequence} missed deadline")),
        EventKind::Rejected { sequence } => Some(format!("SIM-{sequence} rejected")),
        _ => None,
    }
}

fn table(
    frame: &mut Frame,
    area: Rect,
    title: String,
    headers: Vec<&str>,
    widths: Vec<Constraint>,
    rows: Vec<Row<'static>>,
    state: &mut ratatui::widgets::TableState,
) {
    frame.render_stateful_widget(
        Table::new(rows, widths)
            .header(Row::new(headers).style(Style::new().fg(ACCENT)))
            .block(Block::bordered().title(title))
            .row_highlight_style(
                Style::new()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ")
            .column_spacing(1),
        area,
        state,
    );
}
fn payments(frame: &mut Frame, area: Rect, app: &mut App) {
    let ids = app.payment_ids();
    let (mut id_width, mut due_width) = ids.iter().fold((10, 6), |(id_width, due_width), id| {
        let p = &app.run().payments[id];
        (
            id_width.max(p.payment.id.len()),
            due_width.max(p.deadline.to_string().len()),
        )
    });
    // Keep borders (2), selection (2), gaps (6), the four fixed columns,
    // and at least one full rail ID (7). Generated IDs and deadlines are ASCII.
    let value_budget = usize::from(area.width) - (2 + 2 + 6 + 5 + 5 + 9 + 9 + 7);
    while id_width + due_width > value_budget {
        if id_width > due_width {
            id_width -= 1;
        } else {
            due_width -= 1;
        }
    }
    let rows = ids
        .iter()
        .map(|id| {
            let p = &app.run().payments[id];
            let id_lines = wrapped(vec![p.payment.id.clone()], id_width);
            let due_lines = wrapped(vec![p.deadline.to_string()], due_width);
            let height = id_lines.len().max(due_lines.len()) as u16;
            let route = p
                .route
                .as_ref()
                .map(route_short)
                .unwrap_or_else(|| "--".into());
            Row::new(vec![
                id_lines.join("\n"),
                p.payment.sender.clone(),
                p.payment.receiver.clone(),
                money(p.payment.amount_cents as u128),
                p.status.label().into(),
                due_lines.join("\n"),
                route,
            ])
            .height(height)
            .style(Style::new().fg(status_color(p.status)))
        })
        .collect();
    let selected = app.tables[1].selected().map_or(0, |i| i + 1);
    let title = format!(
        "Payments {} / {} ({}/{}) | /{} | {}",
        app.filter.label(),
        ids.len(),
        selected,
        ids.len(),
        app.query,
        if app.follow_latest {
            "FOLLOW newest"
        } else {
            "HOLD ID; Home follows"
        }
    );
    if ids.is_empty() {
        frame.render_widget(Paragraph::new("No matching payments. Space or . generates demand.\nf cycles filters; / then Enter clears search.\nAll active + newest 128 terminal payments are retained.").block(Block::bordered().title(title)), area);
        return;
    }
    table(
        frame,
        area,
        title,
        vec!["ID", "From", "To", "USD", "State", "Due", "Route"],
        vec![
            Constraint::Length(id_width as u16),
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(due_width as u16),
            Constraint::Fill(1),
        ],
        rows,
        &mut app.tables[1],
    );
}
fn rails(frame: &mut Frame, area: Rect, app: &mut App) {
    let [grid, detail] = Layout::vertical([Constraint::Fill(1), Constraint::Length(5)]).areas(area);
    let sim = &app.ops.runs[app.strategy].simulator;
    let rows = sim
        .rail_states()
        .iter()
        .zip(&sim.effective_scenario().services)
        .map(|(r, s)| {
            let queue = sim
                .active_payments()
                .iter()
                .filter(|p| {
                    p.in_flight_until.is_none()
                        && p.route.as_ref().is_some_and(|route| {
                            route
                                .hops
                                .get(p.next_hop)
                                .is_some_and(|h| h.rail_id == r.rail_id)
                        })
                })
                .count();
            let cap = s
                .capacity_per_minute_cents
                .map_or("unlimited".into(), |c| money(c as u128));
            Row::new(vec![
                r.rail_id.clone(),
                if sim.next_minute() == 0 {
                    "--"
                } else if r.open {
                    "OPEN"
                } else {
                    "SHUT"
                }
                .into(),
                money(r.used_this_minute_cents),
                cap,
                s.capacity_per_minute_cents.map_or("n/a".into(), |c| {
                    percent(r.used_this_minute_cents, c as u128)
                }),
                queue.to_string(),
                r.departed_hops.to_string(),
                money(r.routing_cost_cents),
            ])
        })
        .collect();
    let i = app.selected_rail();
    let r = &sim.effective_scenario().network.rails[i];
    let state = &sim.rail_states()[i];
    let s = &sim.effective_scenario().services[i];
    let mut reservations: BTreeSlots = Default::default();
    for p in sim.active_payments() {
        if let (Some(route), Some(times)) = (&p.route, &p.planned_departures) {
            for (hop, time) in route.hops.iter().zip(times) {
                if hop.rail_id == r.id && *time >= sim.next_minute() {
                    *reservations.entry(*time).or_default() += p.payment.amount_cents as u128;
                }
            }
        }
    }
    let reserved = reservations
        .iter()
        .take(5)
        .map(|(t, a)| format!("@{t} ${}", money(*a)))
        .collect::<Vec<_>>()
        .join(" ");
    let text = format!(
        "{} | {} | fee ${} | transit {}m\nWindow +{} open {}/{}m | enabled {} | in-flight USD {}\nReserved: {}  [Enter: all utilization / future slots]",
        r.id,
        r.participants.join(" "),
        money(r.fee_cents as u128),
        r.settlement_minutes,
        s.offset_minutes,
        s.open_minutes,
        s.period_minutes,
        r.available,
        money(state.departed_principal_cents - state.settled_principal_cents),
        if reserved.is_empty() {
            "none"
        } else {
            &reserved
        }
    );
    table(
        frame,
        grid,
        "Rails / last processed minute principal budget / j k select".into(),
        vec![
            "Rail", "Now", "Used USD", "Cap USD", "Use %", "Wait", "Hops", "Fee USD",
        ],
        vec![
            Constraint::Length(7),
            Constraint::Length(4),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(7),
            Constraint::Length(4),
            Constraint::Length(6),
            Constraint::Fill(1),
        ],
        rows,
        &mut app.tables[2],
    );
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title("Selected rail / reservations are future departures")),
        detail,
    );
}
fn rail_detail_lines(app: &App) -> Vec<String> {
    let sim = &app.ops.runs[app.strategy].simulator;
    let i = app.selected_rail();
    let r = &sim.effective_scenario().network.rails[i];
    let s = &sim.effective_scenario().services[i];
    let state = &sim.rail_states()[i];
    let mut slots: BTreeSlots = Default::default();
    let mut waits = vec![];
    for p in sim.active_payments() {
        if let Some(route) = &p.route {
            if p.in_flight_until.is_none()
                && let Some(hop) = route.hops.get(p.next_hop)
                && hop.rail_id == r.id
            {
                waits.push(format!(
                    "{} {} -> {} USD {} due {}",
                    p.payment.id,
                    hop.sender,
                    hop.receiver,
                    money(p.payment.amount_cents as u128),
                    p.deadline
                ));
            }
            if let Some(times) = &p.planned_departures {
                for (hop, t) in route.hops.iter().zip(times) {
                    if hop.rail_id == r.id && *t >= sim.next_minute() {
                        *slots.entry(*t).or_default() += p.payment.amount_cents as u128;
                    }
                }
            }
        }
    }
    let mut lines = vec![
        format!(
            "{} / {} | members {}",
            r.id,
            r.name,
            r.participants.join(" ")
        ),
        format!(
            "Enabled {} | fee USD {} | transit {}m | transaction ceiling {}",
            r.available,
            money(r.fee_cents as u128),
            r.settlement_minutes,
            r.max_amount_cents
                .map_or("none".into(), |c| money(c as u128))
        ),
        format!(
            "Service: offset {} / open {} / period {} minutes",
            s.offset_minutes, s.open_minutes, s.period_minutes
        ),
        format!(
            "Fresh principal budget per open minute: USD {}",
            s.capacity_per_minute_cents
                .map_or("unlimited".into(), |c| money(c as u128))
        ),
        format!(
            "Last tick open {} / used USD {}",
            state.open,
            money(state.used_this_minute_cents)
        ),
        format!(
            "CUMULATIVE hop principal departed USD {} / settled USD {}",
            money(state.departed_principal_cents),
            money(state.settled_principal_cents)
        ),
        format!(
            "IN FLIGHT principal USD {} / hops {}",
            money(state.departed_principal_cents - state.settled_principal_cents),
            state.departed_hops - state.settled_hops
        ),
        format!(
            "Actual fee USD {} / departed hops {} / settled hops {}",
            money(state.routing_cost_cents),
            state.departed_hops,
            state.settled_hops
        ),
        "Capacity usage is departure principal, not in-flight occupancy.".into(),
        "FUTURE RESERVATIONS (all active witnesses, aggregate per minute)".into(),
    ];
    for e in app.run().network_events.iter().rev() {
        if let EventKind::DisruptionApplied(c) = &e.kind
            && c.rail_id == r.id
        {
            lines.push(format!(
                "Change @{}: enabled {} -> {}; capacity {:?} -> {:?} cents",
                e.minute,
                c.before.available,
                c.after.available,
                c.before.capacity_per_minute_cents,
                c.after.capacity_per_minute_cents
            ));
        }
    }
    if slots.is_empty() {
        lines.push("None. Static strategy makes no reservations.".into());
    }
    for (minute, amount) in slots {
        lines.push(format!(
            "@{} USD {} / budget {}",
            minute,
            money(amount),
            s.capacity_per_minute_cents
                .map_or("unlimited".into(), |c| money(c as u128))
        ));
    }
    lines.push("WAITING FOR THIS RAIL (unrouted demand is not assigned to any rail)".into());
    if waits.is_empty() {
        lines.push("None".into());
    } else {
        lines.extend(waits);
    }
    lines
}
type BTreeSlots = std::collections::BTreeMap<u128, u128>;
fn network(frame: &mut Frame, area: Rect, app: &mut App) {
    let [grid, detail] = Layout::vertical([Constraint::Fill(1), Constraint::Length(4)]).areas(area);
    let sim = &app.ops.runs[app.strategy].simulator;
    let rows = sim
        .scenario()
        .network
        .institutions
        .iter()
        .map(|n| {
            let queued = sim
                .active_payments()
                .iter()
                .filter(|p| {
                    p.in_flight_until.is_none()
                        && p.route.as_ref().map_or(p.payment.sender == n.id, |r| {
                            r.hops.get(p.next_hop).map_or_else(
                                || r.hops.last().is_some_and(|h| h.receiver == n.id),
                                |h| h.sender == n.id,
                            )
                        })
                })
                .count();
            let flight = sim
                .active_payments()
                .iter()
                .filter(|p| {
                    p.in_flight_until.is_some()
                        && p.route
                            .as_ref()
                            .is_some_and(|r| r.hops[p.next_hop].receiver == n.id)
                })
                .count();
            let origin = sim
                .active_payments()
                .iter()
                .filter(|p| p.payment.sender == n.id)
                .count();
            let members = sim
                .scenario()
                .network
                .rails
                .iter()
                .filter(|r| r.participants.contains(&n.id))
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            Row::new(vec![
                n.id.clone(),
                queued.to_string(),
                flight.to_string(),
                origin.to_string(),
                money(n.opening_balance_cents as u128),
                members,
            ])
        })
        .collect();
    let n = &sim.effective_scenario().network.institutions[app.tables[3].selected().unwrap_or(0)];
    let text = format!(
        "{} / {}\nQueued = waiting at node; inbound = currently in-flight to node.\nRails join all their members. Opening USD is descriptive, never spent.",
        n.id, n.name
    );
    table(
        frame,
        grid,
        "Network / active demand location".into(),
        vec![
            "Node",
            "Queue",
            "Inbound",
            "Origin",
            "Opening USD",
            "Membership",
        ],
        vec![
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(13),
            Constraint::Fill(1),
        ],
        rows,
        &mut app.tables[3],
    );
    frame.render_widget(
        Paragraph::new(text).block(Block::bordered().title("Selected institution")),
        detail,
    );
}
fn optimizer(app: &App) -> Vec<String> {
    let sim = &app.ops.runs[app.strategy].simulator;
    let d = sim.routing_diagnostics();
    let mut lines = vec![format!(
        "POLICY {} | accepted routes {} | current reservations {}",
        app.strategy_name(),
        sim.metrics().accepted_routes,
        sim.reservation_entries()
    )];
    let a = sim.adaptation_metrics();
    lines.extend([
        format!(
            "DISRUPTIONS {} | repairs {} | assignment churn {}/{}",
            a.rail_changes, a.reoptimizations, a.changed_assignments, a.assignment_comparisons
        ),
        format!(
            "Churn: routes {} / timing-only {} / withdrawn {}",
            a.changed_routes, a.retimed_only, a.withdrawn
        ),
    ]);
    if let Some(r) = sim.last_reoptimization() {
        lines.push(format!(
            "LAST REPAIR @{} {:?} | same planned cohort {}",
            r.minute, r.selected, r.same_planned_cohort
        ));
        for (label, score) in [("Preserve", &r.preserve), ("Recompute", &r.recompute)] {
            lines.push(format!(
                "{}: planned {}/{} | fee {}c | time {}m | hops {} | churn {}/{}",
                label,
                score.planned,
                score.eligible_payments,
                score.remaining_fee_cents,
                score.remaining_elapsed_minutes,
                score.remaining_hops,
                score.changed_assignments,
                score.previously_planned
            ));
        }
        lines.push(
            if r.reserved {
                "Remaining fees/time only. Recompute is bounded, not an optimum certificate."
            } else {
                "Static time estimates omit future waiting; no complete SLA certificate."
            }
            .into(),
        );
        if !r.same_planned_cohort {
            lines
                .push("Different served IDs: fee/elapsed differences are not quality gaps.".into());
        }
    }
    lines.push(String::new());
    lines.push(format!(
        "Reoptimization policy: {:?}",
        sim.reoptimization_policy()
    ));
    for e in app.run().network_events.iter().rev().take(8) {
        if let EventKind::DisruptionApplied(c) = &e.kind {
            lines.push(format!(
                "@{} {}: enabled {} -> {}; capacity {:?} -> {:?} cents",
                e.minute,
                c.rail_id,
                c.before.available,
                c.after.available,
                c.before.capacity_per_minute_cents,
                c.after.capacity_per_minute_cents
            ));
        }
    }
    lines.push(String::new());
    match sim.scenario().strategy {
        RoutingStrategy::CheapestStatic => lines.extend([
            "Exact static simple-path search at each FIFO routing attempt.".into(),
            "Objective: fee, latency, hops, lexical hop sequence.".into(),
            "Uses currently open rails and remaining capacity/SLA; replans on disruption.".into(),
            "Does not reserve future capacity or predict future windows.".into(),
            "Static search counters: unavailable in this runtime view (not zero).".into(),
        ]),
        RoutingStrategy::Reserved { limits } => lines.extend([
            "Bounded calendar search; FIFO, deadline, amount, reverse + pair repairs.".into(),
            "Allocation: maximize served count, then fee / elapsed / hops / lexical.".into(),
            format!(
                "SEARCHES {} | expansions {} | candidates {}",
                d.searches, d.expansions, d.candidates
            ),
            format!(
                "TRUNCATED {} | unresolved trials {} | repairs {}",
                d.truncated_searches, d.unresolved, d.repair_trials
            ),
            format!(
                "LIMITS per-node {} labels {} expansions {} candidates {} repairs {}",
                limits.labels_per_node,
                limits.max_labels,
                limits.max_expansions,
                limits.max_candidates,
                limits.max_repairs
            ),
            "Counters include discarded order/repair trials, not unique payments.".into(),
            "Unresolved is not infeasible. Truncation may lose feasible/optimal paths.".into(),
        ]),
    }
    lines.extend([
        String::new(),
        "INVESTIGATION: 2 Payments > Enter > selected route / search evidence".into(),
        format!(
            "Retaining {} dossiers (all active + {} recent terminal), not full history.",
            app.run().payments.len(),
            RECENT_PAYMENTS
        ),
        "Evidence is capped at 24 entries per payment per decision tick.".into(),
        "Candidate routes may belong to discarded joint allocation trials.".into(),
        "A rejected prefix is not a complete alternative route or a proof.".into(),
        String::new(),
        "LATEST DECISIONS (search evidence retained, newest payments first)".into(),
    ]);
    for p in app
        .run()
        .payments
        .values()
        .rev()
        .filter(|p| p.decision_minute.is_some())
        .take(12)
    {
        lines.push(format!(
            "{} @{} {} | {} evidence / {} omitted",
            p.payment.id,
            p.decision_minute.unwrap(),
            p.route
                .as_ref()
                .map(route_short)
                .unwrap_or_else(|| "no route found".into()),
            p.evidence.entries.len(),
            p.evidence.omitted
        ));
    }
    lines
}
fn short_status(p: Option<&Dossier>) -> &'static str {
    match p.map(|p| p.status) {
        None => "Not retained",
        Some(PaymentStatus::Queued) => "Unassigned",
        Some(PaymentStatus::Waiting) => "Waiting",
        Some(PaymentStatus::InFlight) => "In transit",
        Some(PaymentStatus::Draining) => "Late · moving",
        Some(PaymentStatus::Completed) => "Delivered",
        Some(PaymentStatus::Late) => "Delivered late",
        Some(PaymentStatus::Expired) => "Expired",
        Some(PaymentStatus::Rejected) => "Rejected",
    }
}

fn comparison_values(app: &App) -> Vec<(&'static str, String, String)> {
    let a = &app.ops.runs[0].simulator;
    let b = &app.ops.runs[1].simulator;
    let x = a.metrics();
    let y = b.metrics();
    Vec::from([
        (
            "On time",
            (x.completed - x.completed_late).to_string(),
            (y.completed - y.completed_late).to_string(),
        ),
        (
            "Missed",
            (x.sla_failures - x.rejected).to_string(),
            (y.sla_failures - y.rejected).to_string(),
        ),
        ("Rejected", x.rejected.to_string(), y.rejected.to_string()),
        (
            "Active",
            a.active_payments().len().to_string(),
            b.active_payments().len().to_string(),
        ),
        (
            "Fees",
            format!("${}", money(x.routing_cost_cents)),
            format!("${}", money(y.routing_cost_cents)),
        ),
    ])
}

fn comparison_totals(app: &App, width: usize) -> Vec<String> {
    let mut lines = paired_line("", "Static", "Reserved", width);
    for (label, left, right) in comparison_values(app) {
        lines.extend(paired_line(label, &left, &right, width));
    }
    lines.push(String::new());
    lines.push("Same demand · In progress".into());
    lines
}

fn comparison(frame: &mut Frame, area: Rect, app: &mut App) {
    let [summary, grid] =
        Layout::vertical([Constraint::Length(9), Constraint::Fill(1)]).areas(area);
    let mut lines = vec![format!("{:<18}{:>22}{:>22}", "", "Static", "Reserved")];
    for (label, left, right) in comparison_values(app) {
        let fit = |value: String| {
            if value.len() > 22 {
                "See totals (e)".into()
            } else {
                value
            }
        };
        lines.push(format!("{label:<18}{:>22}{:>22}", fit(left), fit(right)));
    }
    lines.push(String::new());
    lines.push("Same demand · In progress · e totals".into());
    frame.render_widget(Paragraph::new(lines.join("\n")), summary);
    let ids = app.payment_ids();
    if ids.is_empty() {
        frame.render_widget(Paragraph::new("No payment differences yet"), grid);
        return;
    }
    let id_width = ids
        .iter()
        .filter_map(|id| app.selected_dossier(*id))
        .map(|p| p.payment.id.len())
        .max()
        .unwrap_or(10)
        .max(10)
        .min(area.width as usize - 42);
    let rows = ids
        .iter()
        .map(|id| {
            let p = app.selected_dossier(*id).unwrap();
            let id_lines = wrapped(vec![p.payment.id.clone()], id_width);
            Row::new(vec![
                id_lines.join("\n"),
                short_status(app.ops.runs[0].payments.get(id)).into(),
                short_status(app.ops.runs[1].payments.get(id)).into(),
                match (
                    app.ops.runs[0].payments.get(id),
                    app.ops.runs[1].payments.get(id),
                ) {
                    (Some(a), Some(b)) if a.status != b.status => "Status",
                    (Some(a), Some(b)) if a.route != b.route => "Route",
                    (Some(_), Some(_)) => "Fee",
                    _ => "Record",
                }
                .into(),
            ])
            .height(id_lines.len() as u16)
        })
        .collect::<Vec<_>>();
    frame.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(id_width as u16),
                Constraint::Fill(1),
                Constraint::Fill(1),
                Constraint::Length(7),
            ],
        )
        .header(
            Row::new(["Payment", "Static", "Reserved", "Change"]).style(Style::new().fg(ACCENT)),
        )
        .row_highlight_style(Style::new().bg(Color::DarkGray))
        .highlight_symbol("> "),
        grid,
        &mut app.tables[1],
    );
}

fn paired_line(label: &str, left: &str, right: &str, width: usize) -> Vec<String> {
    let label_width = 10;
    let col = width.saturating_sub(label_width + 4) / 2;
    let labels = wrapped(vec![label.into()], label_width);
    let a = wrapped(vec![left.into()], col.max(1));
    let b = wrapped(vec![right.into()], col.max(1));
    (0..a.len().max(b.len()).max(labels.len()))
        .map(|i| {
            format!(
                "{:<label_width$}  {:<col$}  {:<col$}",
                labels.get(i).map_or("", String::as_str),
                a.get(i).map_or("", String::as_str),
                b.get(i).map_or("", String::as_str)
            )
        })
        .collect()
}

fn payment_summary(app: &App, p: &Dossier, width: usize) -> Vec<String> {
    let left = app.ops.runs[0].payments.get(&p.sequence);
    let right = app.ops.runs[1].payments.get(&p.sequence);
    let mut lines = vec![
        format!(
            "{} · ${} · {} → {}",
            p.payment.id,
            money(p.payment.amount_cents as u128),
            p.payment.sender,
            p.payment.receiver
        ),
        format!("Due {}m", p.deadline),
        String::new(),
    ];
    lines.extend(paired_line("", "Static", "Reserved", width));
    lines.extend(paired_line(
        "Status",
        short_status(left),
        short_status(right),
        width,
    ));
    let route = |p: Option<&Dossier>| {
        p.and_then(|p| {
            p.route.as_ref().map(|r| {
                let suffix = if r
                    .hops
                    .last()
                    .is_some_and(|h| h.receiver != p.payment.receiver)
                {
                    " · partial"
                } else {
                    ""
                };
                format!("{}{suffix}", route_long(r))
            })
        })
        .unwrap_or_else(|| "—".into())
    };
    lines.extend(paired_line("Route", &route(left), &route(right), width));
    let fee =
        |p: Option<&Dossier>| p.map_or("—".into(), |p| format!("${}", money(p.actual_fee_cents)));
    lines.extend(paired_line("Fee paid", &fee(left), &fee(right), width));
    lines.push(String::new());
    let mut minutes = std::collections::BTreeSet::new();
    for p in [left, right].into_iter().flatten() {
        for e in &p.events {
            minutes.insert(e.minute);
        }
    }
    let events = |p: Option<&Dossier>, minute: u128| {
        p.map(|p| {
            p.events
                .iter()
                .filter(|e| e.minute == minute)
                .filter_map(|e| match &e.kind {
                    EventKind::Generated { .. } => Some("Arrived".into()),
                    EventKind::HopDeparted { hop, .. } => Some(format!("Departed {}", hop.rail_id)),
                    EventKind::HopSettled { hop, .. } => Some(format!("At {}", hop.receiver)),
                    EventKind::Completed { late, .. } => {
                        Some(if *late { "Delivered late" } else { "Delivered" }.into())
                    }
                    EventKind::DeadlineMissed { .. } => Some("Missed deadline".into()),
                    EventKind::Rejected { .. } => Some("Rejected".into()),
                    EventKind::Expired { .. } => Some("Expired".into()),
                    EventKind::PlanRevised { .. } => Some("Plan revised".into()),
                    _ => None,
                })
                .collect::<Vec<String>>()
                .join("; ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "—".into())
    };
    for minute in minutes {
        let a = events(left, minute);
        let b = events(right, minute);
        if a != "—" || b != "—" {
            lines.extend(paired_line(&format!("{minute}m"), &a, &b, width));
        }
    }
    let omitted = |p: Option<&Dossier>| p.map_or(0, |p| p.omitted_events);
    if omitted(left) > 0 || omitted(right) > 0 {
        lines.push(format!(
            "Earlier events omitted: Static {} · Reserved {}",
            omitted(left),
            omitted(right)
        ));
    }
    lines
}

fn rail_summary(app: &App) -> Vec<String> {
    let sim = &app.ops.runs[app.strategy].simulator;
    let i = app.selected_rail();
    let rail = &sim.effective_scenario().network.rails[i];
    let service = &sim.effective_scenario().services[i];
    let state = &sim.rail_states()[i];
    let mut lines = vec![
        format!("{} · {}", rail.id, rail.participants.join(" · ")),
        format!(
            "Fee ${} · Transit {}m",
            money(rail.fee_cents as u128),
            rail.settlement_minutes
        ),
        format!(
            "Used ${} / {} per minute",
            money(state.used_this_minute_cents),
            service
                .capacity_per_minute_cents
                .map_or("unlimited".into(), |v| format!("${}", money(v as u128)))
        ),
        String::new(),
    ];
    for p in sim.active_payments() {
        if let Some(hop) = p.route.as_ref().and_then(|r| r.hops.get(p.next_hop))
            && hop.rail_id == rail.id
        {
            lines.push(format!(
                "{} · ${} · {} → {} · {}",
                p.payment.id,
                money(p.payment.amount_cents as u128),
                hop.sender,
                hop.receiver,
                p.in_flight_until
                    .map_or("Waiting".into(), |v| format!("Arrives {v}m"))
            ));
        }
    }
    if lines.len() == 4 {
        lines.push("No active payments".into());
    }
    lines
}

fn detail_lines(app: &App, p: &Dossier) -> Vec<String> {
    let mut lines = vec![
        format!(
            "{} {} -> {} | USD {} | {}",
            p.payment.id,
            p.payment.sender,
            p.payment.receiver,
            money(p.payment.amount_cents as u128),
            p.status.label()
        ),
        format!(
            "Release {} | deadline {} inclusive | last routing attempt {}",
            p.arrived_at,
            p.deadline,
            p.decision_minute.map_or("--".into(), |t| t.to_string())
        ),
    ];
    if let Some(other) = app.ops.runs[1 - app.strategy].payments.get(&p.sequence) {
        lines.push(format!(
            "Other strategy: {} | {}",
            other.status.label(),
            other
                .route
                .as_ref()
                .map(route_short)
                .unwrap_or_else(|| "no accepted route".into())
        ));
    }
    lines.push(format!(
        "Executed fee USD {} / {} departures | completed elapsed {}m",
        money(p.actual_fee_cents),
        p.departed_hops,
        p.completed_elapsed_minutes
            .map_or("--".into(), |v| v.to_string())
    ));
    if p.route.as_ref().is_some_and(|r| {
        r.hops
            .last()
            .is_some_and(|h| h.receiver != p.payment.receiver)
    }) {
        lines
            .push("FIXED PREFIX ONLY; suffix unresolved after disruption. Awaiting repair.".into());
    } else {
        lines.push("SELECTED ROUTE (accepted plan; fees accrue only on actual departure)".into());
    }
    if let Some(route) = &p.route {
        lines.push(format!(
            "Planned fee USD {} | transit {}m (excludes waiting) | {} hops",
            money(route.total_fee_cents),
            route.total_settlement_minutes,
            route.hops.len()
        ));
        for (i, hop) in route.hops.iter().enumerate() {
            lines.push(format!(
                "{}. {} {} -> {}{}",
                i + 1,
                hop.rail_id,
                hop.sender,
                hop.receiver,
                p.planned_departures
                    .as_ref()
                    .map_or(String::new(), |times| format!(" reserved @{}", times[i]))
            ));
        }
    } else {
        lines.push(
            if p.status == PaymentStatus::Rejected {
                "No route attempted: active admission limit reached."
            } else {
                "No accepted route. Pending work retries until its inclusive deadline."
            }
            .into(),
        );
    }
    if let Some(active) = app
        .run()
        .simulator
        .active_payments()
        .iter()
        .find(|a| a.sequence == p.sequence)
    {
        lines.push(format!(
            "Settled hops {} | in-flight arrival {} | SLA failed {}",
            active.next_hop,
            active
                .in_flight_until
                .map_or("--".into(), |t| t.to_string()),
            active.sla_failed
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "SEARCH EVIDENCE @{} / {} entries / {} omitted",
        p.decision_minute.map_or("--".into(), |t| t.to_string()),
        p.evidence.entries.len(),
        p.evidence.omitted
    ));
    lines.push(
        "Actual search branches; candidates may be superseded or from discarded trials.".into(),
    );
    if p.evidence.entries.is_empty() {
        lines.push("No search evidence retained for this payment.".into());
    }
    for (i, e) in p.evidence.entries.iter().enumerate() {
        lines.push(format!("{}. {}", i + 1, e.reason));
        if let Some(r) = &e.route {
            lines.push(format!(
                "   {} | fee ${} transit {}m{}",
                route_long(r),
                money(r.total_fee_cents),
                r.total_settlement_minutes,
                if e.departures.is_empty() {
                    String::new()
                } else {
                    format!(" | dep {:?}", e.departures)
                }
            ));
        }
    }
    lines.push(String::new());
    lines.push(format!(
        "LIFECYCLE / {} retained events / {} omitted",
        p.events.len(),
        p.omitted_events
    ));
    for e in &p.events {
        let label = match &e.kind {
            EventKind::Generated { .. } => "generated".into(),
            EventKind::Rejected { .. } => "overload rejected (SLA failure)".into(),
            EventKind::RouteAccepted { .. } => "route accepted".into(),
            EventKind::PlanRevised { .. } => "plan revised after disruption/retry".into(),
            EventKind::HopDeparted {
                hop,
                fee_cents,
                arrival_minute,
                ..
            } => format!(
                "depart {} {} -> {} fee ${} arrival {}",
                hop.rail_id,
                hop.sender,
                hop.receiver,
                money(*fee_cents as u128),
                arrival_minute
            ),
            EventKind::HopSettled { hop, .. } => {
                format!("settled {} at {}", hop.rail_id, hop.receiver)
            }
            EventKind::DeadlineMissed { .. } => "SLA deadline missed".into(),
            EventKind::Expired { .. } => "expired".into(),
            EventKind::Completed {
                late,
                elapsed_minutes,
                ..
            } => format!("completed elapsed {}m late {}", elapsed_minutes, late),
            EventKind::RailTick { .. }
            | EventKind::DisruptionApplied(_)
            | EventKind::Reoptimized(_) => continue,
        };
        lines.push(format!("@{} #{:<5} {}", e.minute, e.sequence, label));
    }
    lines
}
fn status_color(status: PaymentStatus) -> Color {
    match status {
        PaymentStatus::Expired
        | PaymentStatus::Late
        | PaymentStatus::Rejected
        | PaymentStatus::Draining => Color::LightRed,
        PaymentStatus::Completed => Color::Green,
        PaymentStatus::Queued | PaymentStatus::Waiting => Color::Yellow,
        _ => Color::White,
    }
}
fn route_short(route: &Route) -> String {
    route
        .hops
        .iter()
        .map(|h| h.rail_id.as_str())
        .collect::<Vec<_>>()
        .join(">")
}
fn route_long(route: &Route) -> String {
    route
        .hops
        .iter()
        .map(|h| format!("{}:{}>{}", h.rail_id, h.sender, h.receiver))
        .collect::<Vec<_>>()
        .join(" ")
}
fn percent(n: u128, d: u128) -> String {
    if d == 0 {
        "n/a".into()
    } else {
        format!("{:.1}%", n as f64 / d as f64 * 100.0)
    }
}
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
    use payment_routing::operations::Preset;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    };
    fn key(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }
    fn screen(app: &mut App, w: u16, h: u16, name: &str) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .chunks(w as usize)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        if let Ok(dir) = std::env::var("CONSOLE_SNAPSHOT_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(format!("{dir}/{name}-{w}x{h}.txt"), &text).unwrap();
        }
        text
    }
    fn large_payment_fixture(sequences: &[u128], deadline: u128) -> App {
        let mut app = App::new(Preset::Balanced, 42).unwrap();
        app.step();
        let template = app.run().payments[&1].clone();
        let payments = &mut app.ops.runs[app.strategy].payments;
        payments.clear();
        for &sequence in sequences {
            let mut p = template.clone();
            p.sequence = sequence;
            p.payment.id = format!("SIM-{sequence}");
            p.deadline = deadline;
            payments.insert(sequence, p);
        }
        app.view = View::Payments;
        app.sync_selection();
        app
    }
    #[test]
    fn compact_watch_and_compare_render_actual_state_without_search_noise() {
        let mut app = App::new(Preset::Disruptions, 42).unwrap();
        for _ in 0..13 {
            app.step();
        }
        let before = app.ops.clone();
        let output = screen(&mut app, 80, 18, "watch-demo");
        assert!(output.find("RTP").unwrap() < output.find("FEDNOW").unwrap());
        assert!(output.find("FEDNOW").unwrap() < output.find("ACH").unwrap());
        assert!(output.contains("12m · ACH enabled"), "{output}");
        for absent in ["CONSERVATION", "SEARCH", "SLA", "twin", "1-6"] {
            assert!(!output.contains(absent), "{output}");
        }
        let _ = screen(&mut app, 120, 32, "watch-demo");
        assert_eq!(app.ops, before);
        for _ in 13..40 {
            app.step();
        }
        let before = app.ops.clone();
        app.view = View::Compare;
        let output = screen(&mut app, 80, 18, "compare-demo");
        for label in ["On time", "Missed", "Rejected", "Active", "Fees", "Change"] {
            assert!(output.contains(label), "{output}");
        }
        key(&mut app, KeyCode::Enter);
        let output = screen(&mut app, 80, 18, "inspect-demo");
        assert!(
            output.contains("Fee paid") && !output.contains("SEARCH EVIDENCE"),
            "{output}"
        );
        assert_eq!(app.ops, before);
    }

    #[test]
    fn paired_columns_preserve_wide_money_and_minutes() {
        let value = format!("${}", money(u128::MAX));
        let label = format!("{}m", u128::MAX);
        let rows = paired_line(&label, &value, &value, 78);
        let mut labels = String::new();
        let mut left = String::new();
        let mut right = String::new();
        for row in rows {
            assert!(row.len() <= 78);
            labels.push_str(row[..10].trim());
            left.push_str(row[12..44].trim());
            right.push_str(row[46..].trim());
        }
        assert_eq!(labels, label);
        assert_eq!(left, value);
        assert_eq!(right, value);
    }

    #[test]
    fn disruption_alternatives_and_events_render_together_at_minimum_size() {
        let mut app = App::new(Preset::Disruptions, 42).unwrap();
        for _ in 0..13 {
            app.step();
        }
        assert!(app.error.is_none());
        app.view = View::Optimizer;
        let output = screen(&mut app, 80, 18, "disruptions-comparison");
        for expected in ["DISRUPTIONS 3", "LAST REPAIR", "Preserve:", "Recompute:"] {
            assert!(output.contains(expected), "{output}");
        }
        for view in View::ALL {
            app.view = view;
            let _ = screen(&mut app, 80, 18, "disruptions-all-views");
        }
    }
    #[test]
    fn million_payment_ids_and_deadlines_remain_distinct_and_complete() {
        let sequences = [999_999, 1_000_000, 1_000_001];
        let mut app = large_payment_fixture(&sequences, 1_000_012);
        for width in [80, 120] {
            let output = screen(&mut app, width, 18, "million-payments");
            for sequence in sequences {
                assert!(output.contains(&format!("SIM-{sequence}")), "{output}");
            }
            assert_eq!(
                output.matches("1000012").count(),
                sequences.len(),
                "{output}"
            );
        }
    }
    #[test]
    fn wide_payment_values_wrap_without_losing_digits_or_selection() {
        let sequences: Vec<_> = (0..6).map(|i| u128::MAX - i).collect();
        let mut app = large_payment_fixture(&sequences, u128::MAX);
        for width in [80, 120, 180] {
            for (key_code, expected) in
                [(KeyCode::Home, sequences[0]), (KeyCode::End, sequences[5])]
            {
                key(&mut app, key_code);
                let output = screen(&mut app, width, 18, "wide-payments");
                // Read columns from the rendered headers, joining continuation
                // lines to verify the complete selected value is on screen.
                let mut rows = output.lines().filter_map(|line| line.strip_prefix('│'));
                let header = rows.next().unwrap();
                let id = header.find("ID").unwrap()..header.find("From").unwrap();
                let due = header.find("Due").unwrap()..header.find("Route").unwrap();
                let (mut ids, mut deadlines) = (String::new(), String::new());
                for row in rows {
                    ids.push_str(row[id.clone()].trim());
                    deadlines.push_str(row[due.clone()].trim());
                }
                assert!(ids.contains(&format!("SIM-{expected}")), "{output}");
                assert!(deadlines.contains(&u128::MAX.to_string()), "{output}");
                assert_eq!(app.selected_payment, Some(expected));
                key(&mut app, KeyCode::Enter);
                assert_eq!(app.detail.as_ref().unwrap().sequence, expected);
                key(&mut app, KeyCode::Esc);
            }
        }
    }
    #[test]
    fn deterministic_scenarios_render_all_views_at_minimum_and_large_sizes() {
        for preset in Preset::ALL {
            let mut app = App::new(preset, 42).unwrap();
            for _ in 0..80 {
                app.step();
            }
            assert!(app.error.is_none());
            for view in View::ALL {
                app.view = view;
                for (w, h) in [(80, 18), (120, 32)] {
                    let output = screen(&mut app, w, h, &format!("{}-{view:?}", preset.name()));
                    for expected in ["Payment routing", "79m", "? help"] {
                        assert!(
                            output.contains(expected),
                            "{preset:?}/{view:?}: missing {expected}\n{output}"
                        );
                    }
                    if view == View::Rails {
                        assert!(output.contains("FEDWIRE"));
                        assert!(output.contains("Reserved:"));
                    }
                    if view == View::Compare {
                        assert!(output.contains("Static"));
                        assert!(output.contains("Reserved"));
                    }
                }
            }
        }
    }
    #[test]
    fn details_scroll_to_rejected_alternatives_and_complete_lifecycle() {
        let mut app = App::new(Preset::Balanced, 42).unwrap();
        for _ in 0..40 {
            app.step();
        }
        app.strategy = 0;
        app.view = View::Payments;
        let id = app
            .run()
            .payments
            .values()
            .find(|p| {
                p.route.is_some()
                    && p.status.terminal()
                    && p.evidence
                        .entries
                        .iter()
                        .any(|e| e.reason.starts_with("Rejected"))
            })
            .unwrap()
            .sequence;
        app.selected_payment = Some(id);
        app.follow_latest = false;
        app.sync_selection();
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('e'));
        let top = screen(&mut app, 80, 18, "detail-top");
        assert!(top.contains("SELECTED ROUTE"));
        let mut all = top;
        for _ in 0..20 {
            key(&mut app, KeyCode::PageDown);
            all += &screen(&mut app, 80, 18, "detail-scroll");
        }
        assert!(all.contains("Rejected"));
        assert!(all.contains("LIFECYCLE"));
        assert!(all.contains("completed elapsed"));
        key(&mut app, KeyCode::End);
        let bottom = screen(&mut app, 80, 18, "detail-end");
        assert!(bottom.contains("completed elapsed"));
        assert!(!app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::End);
        screen(&mut app, 80, 18, "payments-end");
        assert!(app.tables[1].offset() > 0);
        key(&mut app, KeyCode::Home);
        screen(&mut app, 80, 18, "payments-home");
        assert_eq!(app.tables[1].offset(), 0);
    }
    #[test]
    fn same_tick_completed_payment_shows_its_reserved_departure() {
        let mut app = App::new(Preset::Balanced, 42).unwrap();
        app.step();
        let dossier = &app.run().payments[&1];
        assert_eq!(dossier.status, PaymentStatus::Completed);
        assert_eq!(dossier.completed_elapsed_minutes, Some(0));
        app.detail = Some(dossier.clone());
        app.evidence = true;
        let output = screen(&mut app, 80, 18, "same-tick-reservation");
        assert!(
            output.contains("reserved @0"),
            "accepted plan missing from investigation: {output}"
        );
    }

    #[test]
    fn in_flight_sla_failure_is_visible_with_zero_queue_and_late_fees() {
        use payment_routing::{
            network::{Institution, Network, Rail},
            simulation::{
                ArrivalProcess, PaymentFlow, RailService, RoutingStrategy, Scenario, Simulator,
            },
        };
        let network = Network {
            name: "drain".into(),
            institutions: ["A", "B", "C"]
                .map(|id| Institution {
                    id: id.into(),
                    name: id.into(),
                    opening_balance_cents: 0,
                })
                .to_vec(),
            rails: [
                Rail {
                    id: "AB".into(),
                    name: "AB".into(),
                    participants: vec!["A".into(), "B".into()],
                    fee_cents: 1,
                    settlement_minutes: 1,
                    available: true,
                    max_amount_cents: None,
                    batch_capacity_cents: None,
                },
                Rail {
                    id: "BC".into(),
                    name: "BC".into(),
                    participants: vec!["B".into(), "C".into()],
                    fee_cents: 1,
                    settlement_minutes: 2,
                    available: true,
                    max_amount_cents: None,
                    batch_capacity_cents: None,
                },
            ]
            .to_vec(),
            payments: vec![],
        };
        let scenario = Scenario {
            network,
            arrivals: ArrivalProcess {
                attempts_per_minute: 1,
                probability_per_million: 500_000,
                flows: vec![PaymentFlow {
                    sender: "A".into(),
                    receiver: "C".into(),
                }],
                min_amount_cents: 1,
                max_amount_cents: 1,
                min_sla_minutes: 3,
                max_sla_minutes: 3,
            },
            services: vec![
                RailService {
                    rail_id: "AB".into(),
                    period_minutes: 1,
                    offset_minutes: 0,
                    open_minutes: 1,
                    capacity_per_minute_cents: None,
                },
                RailService {
                    rail_id: "BC".into(),
                    period_minutes: 2,
                    offset_minutes: 0,
                    open_minutes: 1,
                    capacity_per_minute_cents: None,
                },
            ],
            strategy: RoutingStrategy::CheapestStatic,
            max_active_payments: 16,
            retained_events: 32,
            disruptions: vec![],
        };
        let mut app = App::new(Preset::Balanced, 42).unwrap();
        for run in &mut app.ops.runs {
            run.simulator = Simulator::new(scenario.clone(), 6).unwrap();
        }
        app.strategy = 0;
        for _ in 0..4 {
            app.step();
        }
        assert_eq!(app.run().payments[&1].status, PaymentStatus::Draining);
        let output = screen(&mut app, 80, 18, "in-flight-sla");
        assert!(output.contains("Missed 1"));
        assert!(output.contains("Unassigned 0"));
        let bc = output.lines().find(|line| line.contains("BC ")).unwrap();
        assert_eq!(bc.split_whitespace().last(), Some("1"), "{output}");
        assert_eq!(app.run().simulator.metrics().generated, 1);
        app.view = View::Payments;
        app.query = "SIM-1".into();
        assert!(screen(&mut app, 80, 18, "draining-payment").contains("DRAIN SLA"));
        app.step();
        let p = &app.run().payments[&1];
        assert_eq!(p.status, PaymentStatus::Late);
        assert_eq!(p.actual_fee_cents, 2);
        assert_eq!(p.completed_elapsed_minutes, Some(4));
        key(&mut app, KeyCode::Enter);
        let output = screen(&mut app, 80, 18, "late-payment");
        assert!(
            output.contains("$0.02") && output.contains("Delivered late"),
            "{output}"
        );
    }

    #[test]
    fn empty_filtered_small_help_and_zero_denominators_are_readable() {
        let mut app = App::new(Preset::Balanced, 42).unwrap();
        let before = screen(&mut app, 80, 18, "paused-empty");
        assert!(before.contains("Ready"));
        assert!(before.contains("Space to start"));
        app.view = View::Payments;
        assert!(screen(&mut app, 80, 18, "empty-payments").contains("No matching payments"));
        for _ in 0..8 {
            app.step();
        }
        app.query = "no-such-payment".into();
        assert!(screen(&mut app, 80, 18, "no-results").contains("No matching payments"));
        assert!(screen(&mut app, 40, 10, "small").contains("80 x 18"));
        key(&mut app, KeyCode::Char('?'));
        key(&mut app, KeyCode::End);
        assert!(screen(&mut app, 80, 18, "help-end").contains("No live payments"));
        assert_eq!(
            money(u128::MAX),
            "3,402,823,669,209,384,634,633,746,074,317,682,114.55"
        );
    }
}

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
const HELP: &str = "SIMULATION\nSpace start/pause   . one tick and pause   +/- speed (ticks/second)\nr restart current seed   n next seed (seed + 1, wrapping at u64 max)\nc cycle scenario and reset: balanced / pressure / outage / limited\ns inspect other strategy; both advance together on identical demand\n\nNAVIGATION\n1 overview  2 payments  3 rails  4 network  5 optimizer  6 compare\nTab / Shift-Tab or h/l / arrows change view\nj/k / arrows select or scroll; PgUp/PgDn page; Home/End first/last\nf cycle payment filter; / search ID, endpoints, status or route rail\nEnter opens payment and pauses; Esc closes detail/help, otherwise quits\n? help and pause; q / Ctrl-C quit; Esc cancels a search\n\nREADING THE CONSOLE\nMinute is the last completed tick; next is the next minute to execute.\nCapacity is shared principal per departure minute, never in-flight load.\nFees are actual departures, including failed work; planned fees are separate.\nSLA failure = overload rejection or deadline miss, counted once.\nQueued samples exclude in-flight work; zero queue does not mean zero failures.\nSearch evidence contains candidates and rejected prefixes from actual trials.\nReserved unresolved/truncated searches do not prove infeasibility.\nAll active + newest 128 terminal dossiers; 32 events and 24 evidence entries.\nComparison cohorts may differ; fees are not a certified optimality gap.\n\nSynthetic USD, accelerated timing. Opening balances are descriptive.\nNo live payments, liquidity constraints, netting or actual settlement.";

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
        Constraint::Length(3),
    ])
    .areas(area);
    let sim = &app.run().simulator;
    let minute = sim
        .next_minute()
        .checked_sub(1)
        .map_or("--".into(), |v| v.to_string());
    let state = if app.error.is_some() {
        "ERROR"
    } else if app.running {
        "RUNNING"
    } else {
        "PAUSED"
    };
    let title = format!(
        "PAYMENT OPS / {} / {} / {}   SYNTHETIC USD",
        app.ops.preset.name(),
        app.strategy_name(),
        state
    );
    let status = format!(
        "seed {} | minute {} next {} | {} tick/s | active {}/{} | twin {:.1}ms",
        sim.seed(),
        minute,
        sim.next_minute(),
        App::SPEEDS[app.speed],
        sim.active_payments().len(),
        sim.scenario().max_active_payments,
        app.last_step.as_secs_f64() * 1000.0
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(title, Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Line::from(status),
        ]),
        header,
    );
    frame.render_widget(
        Tabs::new([
            "1 Overview",
            "2 Payments",
            "3 Rails",
            "4 Network",
            "5 Optimizer",
            "6 Compare",
        ])
        .select(app.view as usize)
        .padding("", "")
        .divider(" | ")
        .highlight_style(Style::new().fg(ACCENT).add_modifier(Modifier::UNDERLINED)),
        tabs,
    );
    if app.help {
        text_page(
            frame,
            body,
            "Keyboard reference (? / Esc close)",
            HELP.lines().map(str::to_string).collect(),
            &mut app.help_scroll,
        );
    } else if let Some(detail) = &app.detail {
        let lines = detail_lines(app, detail);
        text_page(
            frame,
            body,
            &format!("{} / payment investigation / Esc back", detail.payment.id),
            lines,
            &mut app.detail_scroll,
        );
    } else {
        match app.view {
            View::Overview => {
                let lines = overview(app);
                text_page(
                    frame,
                    body,
                    "Live execution / j k scroll",
                    lines,
                    &mut app.scroll[0],
                );
            }
            View::Payments => payments(frame, body, app),
            View::Rails => rails(frame, body, app),
            View::Network => network(frame, body, app),
            View::Optimizer => {
                let lines = optimizer(app);
                text_page(
                    frame,
                    body,
                    "Routing behavior / j k scroll",
                    lines,
                    &mut app.scroll[4],
                );
            }
            View::Compare => {
                let lines = compare(app);
                text_page(
                    frame,
                    body,
                    "Same seed / same minute / different execution outcomes",
                    lines,
                    &mut app.scroll[5],
                );
            }
        }
    }
    let message = if let Some(input) = &app.editing {
        format!("Search: {input}_  [Enter apply / Esc cancel]")
    } else if let Some(error) = &app.error {
        format!("ERROR: {error} [paused; r resets]")
    } else if app.detail.is_some() {
        "Selected route + actual search evidence; j/k/PgDn scroll, Home/End; Esc back".into()
    } else {
        app.notice.clone()
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(
                "Space run/pause  . step  +/- speed  r restart  n seed  c scenario  s strategy",
            ),
            Line::from(
                "1-6 views  j/k rows  Enter inspect  f filter  / search  ? help  q/Esc quit",
            ),
            Line::styled(
                message,
                Style::new().fg(if app.error.is_some() {
                    Color::Red
                } else {
                    Color::Yellow
                }),
            ),
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

fn overview(app: &App) -> Vec<String> {
    let sim = &app.run().simulator;
    let m = sim.metrics();
    let active = sim.active_payments();
    let unrouted = active.iter().filter(|p| p.route.is_none()).count();
    let waiting = active
        .iter()
        .filter(|p| p.route.is_some() && p.in_flight_until.is_none())
        .count();
    let flight = active
        .iter()
        .filter(|p| p.in_flight_until.is_some())
        .count();
    let draining = active.iter().filter(|p| p.sla_failed).count();
    let oldest = active
        .iter()
        .filter(|p| p.in_flight_until.is_none())
        .map(|p| {
            sim.next_minute()
                .saturating_sub(1)
                .saturating_sub(p.arrived_at)
        })
        .max()
        .unwrap_or(0);
    let active_volume: u128 = active.iter().map(|p| p.payment.amount_cents as u128).sum();
    let ontime = m.completed - m.completed_late;
    let mut lines = vec![
        format!(
            "DEMAND {} generated / USD {} | accepted routes {}",
            m.generated,
            money(m.generated_volume_cents),
            m.accepted_routes
        ),
        format!(
            "ACTIVE {} | unrouted {} | wait slot {} | in flight {} | draining {}",
            active.len(),
            unrouted,
            waiting,
            flight,
            draining
        ),
        format!(
            "QUEUE oldest {}m | active principal USD {} | reserve slots {}",
            oldest,
            money(active_volume),
            sim.reservation_entries()
        ),
        format!(
            "DELIVERY {} completed ({} on time, {} late) | expired {} | overload {}",
            m.completed, ontime, m.completed_late, m.expired, m.rejected
        ),
        format!(
            "SLA failures {} / generated {} = {} | failed USD {}",
            m.sla_failures,
            m.generated,
            percent(m.sla_failures, m.generated),
            money(m.sla_failed_volume_cents)
        ),
        format!(
            "FEES actual USD {} | completed USD {} | mean elapsed {}m",
            money(m.routing_cost_cents),
            money(m.completed_volume_cents),
            ratio(m.completed_elapsed_minutes, m.completed)
        ),
        format!(
            "HOPS {} departed / {} settled | hop principal USD {}",
            m.departed_hops,
            m.settled_hops,
            money(m.departed_principal_cents)
        ),
        format!(
            "CONSERVATION {} = {} completed + {} expired + {} rejected + {} active",
            m.generated,
            m.completed,
            m.expired,
            m.rejected,
            active.len()
        ),
        String::new(),
        "QUEUE HISTORY (post-tick; in-flight SLA misses can occur at zero queue)".into(),
    ];
    let samples = &app.run().samples;
    let tail: Vec<_> = samples.iter().rev().take(48).collect();
    let peak = tail.iter().map(|s| s.queued).max().unwrap_or(0);
    let bars: String = tail
        .iter()
        .rev()
        .map(|s| {
            ['.', ':', '-', '=', '+', '*', '#', '@']
                [(s.queued * 7).checked_div(peak).unwrap_or(0).min(7)]
        })
        .collect();
    lines.push(format!(
        "{} | peak {} | latest {}",
        bars,
        peak,
        samples.back().map_or(0, |s| s.queued)
    ));
    lines.push("MINUTE / QUEUED / FLIGHT / COMPLETED total / SLA FAIL total".into());
    for s in samples.iter().rev().take(12) {
        lines.push(format!(
            "{} / {} / {} / {} / {}",
            s.minute, s.queued, s.in_flight, s.completed, s.sla_failures
        ));
    }
    lines.push(
        "All amounts USD. Fees include failed work. Completion latency includes waiting.".into(),
    );
    lines
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
    let rows = ids
        .iter()
        .map(|id| {
            let p = &app.run().payments[id];
            let route = p
                .route
                .as_ref()
                .map(route_short)
                .unwrap_or_else(|| "--".into());
            Row::new(vec![
                p.payment.id.clone(),
                format!(">{}", p.payment.receiver),
                p.payment.sender.clone(),
                money(p.payment.amount_cents as u128),
                p.status.label().into(),
                p.deadline.to_string(),
                route,
            ])
            .style(Style::new().fg(status_color(p.status)))
        })
        .collect();
    let selected = app.tables[1].selected().map_or(0, |i| i + 1);
    let title = format!(
        "Payments {} / {} ({}/{}) | /{} | Enter inspect",
        app.filter.label(),
        ids.len(),
        selected,
        ids.len(),
        app.query
    );
    if ids.is_empty() {
        frame.render_widget(Paragraph::new("No matching payments. Space or . generates demand.\nf cycles filters; / then Enter clears search.\nAll active + newest 128 terminal payments are retained.").block(Block::bordered().title(title)), area);
        return;
    }
    table(
        frame,
        area,
        title,
        vec!["ID", "To", "From", "USD", "State", "Due", "Route"],
        vec![
            Constraint::Length(10),
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(6),
            Constraint::Fill(1),
        ],
        rows,
        &mut app.tables[1],
    );
}
fn rails(frame: &mut Frame, area: Rect, app: &mut App) {
    let [grid, detail] = Layout::vertical([Constraint::Fill(1), Constraint::Length(5)]).areas(area);
    let sim = &app.run().simulator;
    let rows = sim
        .rail_states()
        .iter()
        .zip(&sim.scenario().services)
        .map(|(r, s)| {
            let queue = sim
                .active_payments()
                .iter()
                .filter(|p| {
                    p.in_flight_until.is_none()
                        && p.route
                            .as_ref()
                            .is_some_and(|route| route.hops[p.next_hop].rail_id == r.rail_id)
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
    let i = app.tables[2].selected().unwrap_or(0);
    let r = &sim.scenario().network.rails[i];
    let state = &sim.rail_states()[i];
    let s = &sim.scenario().services[i];
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
        "{} ({}) | members {} | fee ${} | latency {}m\nWindow +{} open {}/{}m enabled {} | hop USD out {} settled {} flight {}\nFuture reserved: {}",
        r.id,
        r.name,
        r.participants.join(" "),
        money(r.fee_cents as u128),
        r.settlement_minutes,
        s.offset_minutes,
        s.open_minutes,
        s.period_minutes,
        r.available,
        money(state.departed_principal_cents),
        money(state.settled_principal_cents),
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
            "Rail", "State", "Used USD", "Cap USD", "Use %", "Wait", "Hops", "Fee USD",
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
type BTreeSlots = std::collections::BTreeMap<u128, u128>;
fn network(frame: &mut Frame, area: Rect, app: &mut App) {
    let [grid, detail] = Layout::vertical([Constraint::Fill(1), Constraint::Length(4)]).areas(area);
    let sim = &app.run().simulator;
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
                            r.hops[p.next_hop].sender == n.id
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
    let n = &sim.scenario().network.institutions[app.tables[3].selected().unwrap_or(0)];
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
    let sim = &app.run().simulator;
    let d = sim.routing_diagnostics();
    let mut lines = vec![format!(
        "POLICY {} | accepted routes {} | current reservations {}",
        app.strategy_name(),
        sim.metrics().accepted_routes,
        sim.reservation_entries()
    )];
    match sim.scenario().strategy {
        RoutingStrategy::CheapestStatic => lines.extend([
            "Exact static simple-path search at each FIFO routing attempt.".into(),
            "Objective: fee, latency, hops, lexical hop sequence.".into(),
            "Uses currently open rails and remaining capacity/SLA. Pins the route.".into(),
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
fn compare(app: &App) -> Vec<String> {
    let a = &app.ops.runs[0].simulator;
    let b = &app.ops.runs[1].simulator;
    let x = a.metrics();
    let y = b.metrics();
    let mut lines = vec![
        format!(
            "Seed {} | {} | both next minute {}",
            a.seed(),
            app.ops.preset.name(),
            a.next_minute()
        ),
        "METRIC                         STATIC             RESERVED".into(),
    ];
    for (label, left, right) in [
        (
            "Generated",
            x.generated.to_string(),
            y.generated.to_string(),
        ),
        (
            "Completed",
            x.completed.to_string(),
            y.completed.to_string(),
        ),
        (
            "On-time completed",
            (x.completed - x.completed_late).to_string(),
            (y.completed - y.completed_late).to_string(),
        ),
        (
            "Late completions",
            x.completed_late.to_string(),
            y.completed_late.to_string(),
        ),
        ("Expired", x.expired.to_string(), y.expired.to_string()),
        (
            "Overload rejections",
            x.rejected.to_string(),
            y.rejected.to_string(),
        ),
        (
            "Active",
            a.active_payments().len().to_string(),
            b.active_payments().len().to_string(),
        ),
        (
            "SLA failures / generated",
            percent(x.sla_failures, x.generated),
            percent(y.sla_failures, y.generated),
        ),
        (
            "Completed USD",
            money(x.completed_volume_cents),
            money(y.completed_volume_cents),
        ),
        (
            "Actual fees USD",
            money(x.routing_cost_cents),
            money(y.routing_cost_cents),
        ),
        (
            "Mean completed elapsed m",
            ratio(x.completed_elapsed_minutes, x.completed),
            ratio(y.completed_elapsed_minutes, y.completed),
        ),
    ] {
        lines.push(format!("{label:<27} {left:>15} {right:>20}"));
    }
    lines.extend([
        String::new(),
        "Identical generated demand; completion cohorts and active work can differ.".into(),
        "Lower fees alone do not imply better routing. No global optimality gap.".into(),
        "s switches the inspected strategy without resetting either run.".into(),
        "MATCHED RETAINED PAYMENTS (same ID/amount, status and planned route fee)".into(),
    ]);
    for (id, p) in app.ops.runs[0].payments.iter().rev().take(16) {
        if let Some(q) = app.ops.runs[1].payments.get(id) {
            lines.push(format!(
                "{} | {} {} | {} {}",
                p.payment.id,
                p.status.label(),
                p.route
                    .as_ref()
                    .map_or("--".into(), |r| money(r.total_fee_cents)),
                q.status.label(),
                q.route
                    .as_ref()
                    .map_or("--".into(), |r| money(r.total_fee_cents))
            ));
        }
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
    lines.push("SELECTED ROUTE (accepted plan; fees accrue only on actual departure)".into());
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
            EventKind::RailTick { .. } => continue,
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
fn ratio(n: u128, d: u128) -> String {
    if d == 0 {
        "--".into()
    } else {
        format!("{:.2}", n as f64 / d as f64)
    }
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
                    for expected in ["SYNTHETIC USD", "minute 79 next 80", "q/Esc quit"] {
                        assert!(
                            output.contains(expected),
                            "{preset:?}/{view:?}: missing {expected}\n{output}"
                        );
                    }
                    if view == View::Rails {
                        assert!(output.contains("FEDWIRE"));
                        assert!(output.contains("Future reserved:"));
                    }
                    if view == View::Compare {
                        assert!(output.contains("STATIC"));
                        assert!(output.contains("RESERVED"));
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
        app.sync_selection();
        key(&mut app, KeyCode::Enter);
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
        key(&mut app, KeyCode::End);
        screen(&mut app, 80, 18, "payments-end");
        assert!(app.tables[1].offset() > 0);
        key(&mut app, KeyCode::Home);
        screen(&mut app, 80, 18, "payments-home");
        assert_eq!(app.tables[1].offset(), 0);
    }
    #[test]
    fn empty_filtered_small_help_and_zero_denominators_are_readable() {
        let mut app = App::new(Preset::Balanced, 42).unwrap();
        let before = screen(&mut app, 80, 18, "paused-empty");
        assert!(before.contains("minute -- next 0"));
        assert!(before.contains("n/a"));
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

use super::*;
use crate::routing::route_payment;

fn add(target: &mut u128, value: u128, field: &'static str) -> Result<(), SimulationError> {
    *target = target
        .checked_add(value)
        .ok_or(SimulationError::ArithmeticOverflow(field))?;
    Ok(())
}

fn require(condition: bool, message: &str) -> Result<(), SimulationError> {
    if condition {
        Ok(())
    } else {
        Err(SimulationError::Invariant(message.into()))
    }
}

impl State {
    fn emit(&mut self, events: &mut Vec<Event>, kind: EventKind) -> Result<(), SimulationError> {
        let sequence = self.next_event;
        add(&mut self.next_event, 1, "event sequence")?;
        events.push(Event {
            sequence,
            minute: self.next_minute,
            kind,
        });
        Ok(())
    }

    pub(super) fn process_tick(
        &mut self,
        scenario: &Scenario,
        router: &Router,
    ) -> Result<TickReport, SimulationError> {
        let minute = self.next_minute;
        let mut events = vec![];
        self.reservations
            .slots
            .retain(|(_, time), _| *time >= minute);
        for i in 0..self.rails.len() {
            let open = scenario.network.rails[i].available && scenario.services[i].is_open(minute);
            self.rails[i].open = open;
            self.rails[i].used_this_minute_cents = 0;
            self.emit(
                &mut events,
                EventKind::RailTick {
                    rail_id: self.rails[i].rail_id.clone(),
                    open,
                    capacity_cents: scenario.services[i].capacity_per_minute_cents,
                },
            )?;
        }

        // Settle all existing departures before admitting this minute's arrivals.
        for mut payment in std::mem::take(&mut self.active) {
            let terminal = payment.in_flight_until == Some(minute)
                && self.settle(&mut payment, &mut events)?;
            if !terminal {
                self.active.push(payment);
            }
        }
        self.generate(scenario, &mut events)?;
        if let RoutingStrategy::Reserved { limits } = scenario.strategy {
            for mut payment in std::mem::take(&mut self.active) {
                if payment.route.is_none() && !payment.sla_failed {
                    let (journey, diagnostics) = router.find(
                        &payment.payment,
                        minute,
                        payment.deadline,
                        &self.reservations,
                        limits,
                    );
                    self.routing_diagnostics.plus(diagnostics);
                    if let Some(journey) = journey {
                        router.reserve(
                            &mut self.reservations,
                            &journey,
                            payment.payment.amount_cents,
                        );
                        let route = router.route(&journey);
                        payment.planned_departures =
                            Some(journey.steps.iter().map(|s| s.departure).collect());
                        self.emit(
                            &mut events,
                            EventKind::RouteAccepted {
                                sequence: payment.sequence,
                                route: route.clone(),
                            },
                        )?;
                        add(&mut self.metrics.accepted_routes, 1, "accepted routes")?;
                        payment.route = Some(route);
                    }
                }
                self.active.push(payment);
            }
        }

        for mut payment in std::mem::take(&mut self.active) {
            if !self.drive(scenario, &mut payment, &mut events)? {
                self.active.push(payment);
            }
        }
        // Inclusive deadlines: every same-minute departure/settlement gets its
        // opportunity before this phase. An outstanding hop is allowed to drain.
        for mut payment in std::mem::take(&mut self.active) {
            if !payment.sla_failed && payment.deadline == minute {
                payment.sla_failed = true;
                add(&mut self.metrics.sla_failures, 1, "SLA failures")?;
                add(
                    &mut self.metrics.sla_failed_volume_cents,
                    u128::from(payment.payment.amount_cents),
                    "SLA volume",
                )?;
                self.emit(
                    &mut events,
                    EventKind::DeadlineMissed {
                        sequence: payment.sequence,
                    },
                )?;
                if payment.in_flight_until.is_none() {
                    self.expire(&payment, &mut events)?;
                    continue;
                }
            }
            self.active.push(payment);
        }
        add(&mut self.next_minute, 1, "simulated minute")?;
        Ok(TickReport { minute, events })
    }

    fn generate(
        &mut self,
        scenario: &Scenario,
        events: &mut Vec<Event>,
    ) -> Result<(), SimulationError> {
        let arrivals = &scenario.arrivals;
        for _ in 0..arrivals.attempts_per_minute {
            // All four samples precede probability/admission gates. Queue pressure
            // and routing outcomes cannot perturb the generated demand stream.
            let chance = self.random.inclusive(0, 999_999);
            let flow = &arrivals.flows
                [self.random.inclusive(0, (arrivals.flows.len() - 1) as u64) as usize];
            let amount = self
                .random
                .inclusive(arrivals.min_amount_cents, arrivals.max_amount_cents);
            let sla = self
                .random
                .inclusive(arrivals.min_sla_minutes, arrivals.max_sla_minutes);
            if chance >= u64::from(arrivals.probability_per_million) {
                continue;
            }
            let mut deadline = self.next_minute;
            add(&mut deadline, u128::from(sla), "payment deadline")?;
            add(&mut self.metrics.generated, 1, "generated count")?;
            add(
                &mut self.metrics.generated_volume_cents,
                u128::from(amount),
                "generated volume",
            )?;
            let sequence = self.metrics.generated;
            let payment = Payment {
                id: format!("SIM-{sequence}"),
                sender: flow.sender.clone(),
                receiver: flow.receiver.clone(),
                amount_cents: amount,
                max_delivery_minutes: Some(sla),
            };
            self.emit(
                events,
                EventKind::Generated {
                    sequence,
                    payment: payment.clone(),
                    deadline,
                },
            )?;
            if self.active.len() == scenario.max_active_payments {
                add(&mut self.metrics.rejected, 1, "rejected count")?;
                add(
                    &mut self.metrics.rejected_volume_cents,
                    u128::from(amount),
                    "rejected volume",
                )?;
                add(&mut self.metrics.sla_failures, 1, "SLA failures")?;
                add(
                    &mut self.metrics.sla_failed_volume_cents,
                    u128::from(amount),
                    "SLA volume",
                )?;
                self.emit(events, EventKind::Rejected { sequence })?;
            } else {
                self.active.push(ActivePayment {
                    sequence,
                    payment,
                    arrived_at: self.next_minute,
                    deadline,
                    route: None,
                    next_hop: 0,
                    planned_departures: None,
                    in_flight_until: None,
                    sla_failed: false,
                });
            }
        }
        Ok(())
    }

    fn can_depart(&self, scenario: &Scenario, rail: usize, amount: u64) -> bool {
        self.rails[rail].open
            && scenario.services[rail]
                .capacity_per_minute_cents
                .is_none_or(|limit| {
                    u128::from(amount)
                        <= u128::from(limit) - self.rails[rail].used_this_minute_cents
                })
    }

    fn drive(
        &mut self,
        scenario: &Scenario,
        payment: &mut ActivePayment,
        events: &mut Vec<Event>,
    ) -> Result<bool, SimulationError> {
        if payment.in_flight_until.is_some() || payment.sla_failed {
            return Ok(false);
        }
        if payment.route.is_none() {
            if matches!(scenario.strategy, RoutingStrategy::Reserved { .. }) {
                return Ok(false);
            }
            let mut view = scenario.network.clone();
            for (i, rail) in view.rails.iter_mut().enumerate() {
                rail.available = self.can_depart(scenario, i, payment.payment.amount_cents);
            }
            let mut instruction = payment.payment.clone();
            instruction.max_delivery_minutes = Some((payment.deadline - self.next_minute) as u64);
            let route = match scenario.strategy {
                RoutingStrategy::CheapestStatic => route_payment(&view, &instruction)?,
                RoutingStrategy::Reserved { .. } => unreachable!(),
            };
            let Some(route) = route else {
                return Ok(false);
            };
            self.emit(
                events,
                EventKind::RouteAccepted {
                    sequence: payment.sequence,
                    route: route.clone(),
                },
            )?;
            add(&mut self.metrics.accepted_routes, 1, "accepted routes")?;
            payment.route = Some(route);
        }
        loop {
            if let Some(times) = &payment.planned_departures {
                let departure = times[payment.next_hop];
                require(
                    departure >= self.next_minute,
                    "reserved departure was skipped",
                )?;
                if departure > self.next_minute {
                    return Ok(false);
                }
            }
            let hop = payment.route.as_ref().unwrap().hops[payment.next_hop].clone();
            let i = self.rail_index(&hop.rail_id);
            if !self.can_depart(scenario, i, payment.payment.amount_cents) {
                require(
                    payment.planned_departures.is_none(),
                    "reserved capacity unavailable",
                )?;
                return Ok(false);
            }
            let rail = &scenario.network.rails[i];
            let amount = u128::from(payment.payment.amount_cents);
            let mut arrival = self.next_minute;
            add(
                &mut arrival,
                u128::from(rail.settlement_minutes),
                "hop arrival",
            )?;
            require(
                payment.planned_departures.is_none() || arrival <= payment.deadline,
                "reserved arrival exceeds deadline",
            )?;
            let state = &mut self.rails[i];
            add(
                &mut state.used_this_minute_cents,
                amount,
                "minute capacity usage",
            )?;
            add(
                &mut state.departed_principal_cents,
                amount,
                "rail departed principal",
            )?;
            add(&mut state.departed_hops, 1, "rail departed hops")?;
            add(
                &mut state.routing_cost_cents,
                u128::from(rail.fee_cents),
                "rail fees",
            )?;
            add(
                &mut self.metrics.departed_principal_cents,
                amount,
                "departed principal",
            )?;
            add(&mut self.metrics.departed_hops, 1, "departed hops")?;
            add(
                &mut self.metrics.routing_cost_cents,
                u128::from(rail.fee_cents),
                "routing fees",
            )?;
            payment.in_flight_until = Some(arrival);
            self.emit(
                events,
                EventKind::HopDeparted {
                    sequence: payment.sequence,
                    hop,
                    amount_cents: payment.payment.amount_cents,
                    fee_cents: rail.fee_cents,
                    arrival_minute: arrival,
                },
            )?;
            if arrival != self.next_minute {
                return Ok(false);
            }
            if self.settle(payment, events)? {
                return Ok(true);
            }
        }
    }

    fn rail_index(&self, id: &str) -> usize {
        self.rails
            .iter()
            .position(|r| r.rail_id == id)
            .expect("validated route rail")
    }

    /// Returns true only when the payment reaches a terminal outcome.
    fn settle(
        &mut self,
        payment: &mut ActivePayment,
        events: &mut Vec<Event>,
    ) -> Result<bool, SimulationError> {
        let route = payment.route.as_ref().unwrap();
        let hop = route.hops[payment.next_hop].clone();
        let i = self.rail_index(&hop.rail_id);
        let amount = u128::from(payment.payment.amount_cents);
        add(
            &mut self.rails[i].settled_principal_cents,
            amount,
            "rail settled principal",
        )?;
        add(&mut self.rails[i].settled_hops, 1, "rail settled hops")?;
        add(
            &mut self.metrics.settled_principal_cents,
            amount,
            "settled principal",
        )?;
        add(&mut self.metrics.settled_hops, 1, "settled hops")?;
        self.emit(
            events,
            EventKind::HopSettled {
                sequence: payment.sequence,
                hop,
                amount_cents: payment.payment.amount_cents,
            },
        )?;
        payment.next_hop += 1;
        payment.in_flight_until = None;
        if payment.next_hop == route.hops.len() {
            let elapsed = self.next_minute - payment.arrived_at;
            add(&mut self.metrics.completed, 1, "completed count")?;
            add(
                &mut self.metrics.completed_volume_cents,
                amount,
                "completed volume",
            )?;
            add(
                &mut self.metrics.completed_elapsed_minutes,
                elapsed,
                "completed elapsed time",
            )?;
            if payment.sla_failed {
                add(&mut self.metrics.completed_late, 1, "late completed count")?;
                add(
                    &mut self.metrics.completed_late_volume_cents,
                    amount,
                    "late completed volume",
                )?;
            }
            self.emit(
                events,
                EventKind::Completed {
                    sequence: payment.sequence,
                    late: payment.sla_failed,
                    elapsed_minutes: elapsed,
                },
            )?;
            return Ok(true);
        }
        if payment.sla_failed {
            self.expire(payment, events)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn expire(
        &mut self,
        payment: &ActivePayment,
        events: &mut Vec<Event>,
    ) -> Result<(), SimulationError> {
        add(&mut self.metrics.expired, 1, "expired count")?;
        add(
            &mut self.metrics.expired_volume_cents,
            u128::from(payment.payment.amount_cents),
            "expired volume",
        )?;
        self.emit(
            events,
            EventKind::Expired {
                sequence: payment.sequence,
            },
        )
    }

    pub(super) fn check_invariants(&self, scenario: &Scenario) -> Result<(), SimulationError> {
        require(
            self.active.len() <= scenario.max_active_payments,
            "active storage bound",
        )?;
        let mut active_volume = 0;
        let mut overdue = 0;
        let mut overdue_volume = 0;
        let mut last = 0;
        let mut flight_volume = vec![0u128; self.rails.len()];
        let mut flight_hops = vec![0u128; self.rails.len()];
        for payment in &self.active {
            require(
                payment.sequence > last && payment.sequence <= self.metrics.generated,
                "active sequence order",
            )?;
            last = payment.sequence;
            require(
                payment.arrived_at < self.next_minute && payment.deadline >= payment.arrived_at,
                "active time order",
            )?;
            require(
                payment.sla_failed == (payment.deadline < self.next_minute),
                "deadline failure recorded exactly once",
            )?;
            add(
                &mut active_volume,
                u128::from(payment.payment.amount_cents),
                "active volume invariant",
            )?;
            if payment.sla_failed {
                require(
                    payment.in_flight_until.is_some(),
                    "overdue active work must be draining",
                )?;
                add(&mut overdue, 1, "overdue count invariant")?;
                add(
                    &mut overdue_volume,
                    u128::from(payment.payment.amount_cents),
                    "overdue volume invariant",
                )?;
            }
            if let Some(route) = &payment.route {
                require(
                    payment.next_hop < route.hops.len(),
                    "terminal route removed",
                )?;
            } else {
                require(
                    payment.next_hop == 0 && payment.in_flight_until.is_none(),
                    "unrouted work cannot execute",
                )?;
            }
            if let Some(arrival) = payment.in_flight_until {
                require(arrival >= self.next_minute, "due settlement not skipped")?;
                let route = payment.route.as_ref().unwrap();
                let i = self.rail_index(&route.hops[payment.next_hop].rail_id);
                add(
                    &mut flight_volume[i],
                    u128::from(payment.payment.amount_cents),
                    "in-flight volume invariant",
                )?;
                add(&mut flight_hops[i], 1, "in-flight count invariant")?;
            }
        }
        if matches!(scenario.strategy, RoutingStrategy::Reserved { .. }) {
            let minute = self.next_minute.saturating_sub(1);
            let mut expected = std::collections::BTreeMap::new();
            for (r, state) in self.rails.iter().enumerate() {
                if scenario.services[r].capacity_per_minute_cents.is_some()
                    && state.used_this_minute_cents > 0
                {
                    expected.insert((r, minute), state.used_this_minute_cents);
                }
            }
            for p in &self.active {
                if let Some(route) = &p.route {
                    let times = p.planned_departures.as_ref().ok_or_else(|| {
                        SimulationError::Invariant("reserved route without times".into())
                    })?;
                    require(times.len() == route.hops.len(), "reserved timestamp count")?;
                    require(!p.sla_failed, "accepted reservation missed deadline")?;
                    let mut ready = p.arrived_at;
                    for (h, &departure) in route.hops.iter().zip(times) {
                        let r = self.rail_index(&h.rail_id);
                        require(
                            departure >= ready && scenario.services[r].is_open(departure),
                            "reserved temporal feasibility",
                        )?;
                        ready = departure
                            .checked_add(u128::from(scenario.network.rails[r].settlement_minutes))
                            .ok_or(SimulationError::ArithmeticOverflow("reservation arrival"))?;
                        if departure > minute
                            && scenario.services[r].capacity_per_minute_cents.is_some()
                        {
                            *expected.entry((r, departure)).or_insert(0) +=
                                u128::from(p.payment.amount_cents);
                        }
                    }
                    require(ready <= p.deadline, "reserved final deadline")?;
                } else {
                    require(p.planned_departures.is_none(), "times without route")?;
                }
            }
            require(
                expected == self.reservations.slots,
                "reservation accounting",
            )?;
            for (&(r, _), &used) in &expected {
                require(
                    scenario.services[r]
                        .capacity_per_minute_cents
                        .is_none_or(|c| used <= u128::from(c)),
                    "future capacity exceeded",
                )?;
            }
        }
        let m = &self.metrics;
        let sum = |values: &[u128]| -> Result<u128, SimulationError> {
            values.iter().try_fold(0u128, |a, b| {
                a.checked_add(*b)
                    .ok_or(SimulationError::ArithmeticOverflow("invariant total"))
            })
        };
        require(
            m.generated
                == sum(&[
                    m.completed,
                    m.expired,
                    m.rejected,
                    self.active.len() as u128,
                ])?,
            "payment count conservation",
        )?;
        require(
            m.generated_volume_cents
                == sum(&[
                    m.completed_volume_cents,
                    m.expired_volume_cents,
                    m.rejected_volume_cents,
                    active_volume,
                ])?,
            "payment principal conservation",
        )?;
        require(
            m.sla_failures == sum(&[m.rejected, m.expired, m.completed_late, overdue])?,
            "SLA count conservation",
        )?;
        require(
            m.sla_failed_volume_cents
                == sum(&[
                    m.rejected_volume_cents,
                    m.expired_volume_cents,
                    m.completed_late_volume_cents,
                    overdue_volume,
                ])?,
            "SLA principal conservation",
        )?;
        require(
            m.completed_late <= m.completed
                && m.completed_late_volume_cents <= m.completed_volume_cents,
            "late completion subset",
        )?;
        require(
            m.accepted_routes <= m.generated - m.rejected && m.completed <= m.accepted_routes,
            "accepted route count",
        )?;
        let mut totals = [0u128; 5];
        for (i, rail) in self.rails.iter().enumerate() {
            require(
                rail.departed_principal_cents
                    == sum(&[rail.settled_principal_cents, flight_volume[i]])?,
                "rail principal conservation",
            )?;
            require(
                rail.departed_hops == sum(&[rail.settled_hops, flight_hops[i]])?,
                "rail hop conservation",
            )?;
            require(
                rail.open || rail.used_this_minute_cents == 0,
                "closed rail usage",
            )?;
            if let Some(limit) = scenario.services[i].capacity_per_minute_cents {
                require(
                    rail.used_this_minute_cents <= u128::from(limit),
                    "minute capacity exceeded",
                )?;
            }
            for (total, value) in totals.iter_mut().zip([
                rail.departed_principal_cents,
                rail.settled_principal_cents,
                rail.departed_hops,
                rail.settled_hops,
                rail.routing_cost_cents,
            ]) {
                add(total, value, "rail aggregate invariant")?;
            }
        }
        require(
            totals
                == [
                    m.departed_principal_cents,
                    m.settled_principal_cents,
                    m.departed_hops,
                    m.settled_hops,
                    m.routing_cost_cents,
                ],
            "rail/global aggregate equality",
        )
    }
}

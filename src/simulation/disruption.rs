//! Surprise service changes and explicit, unweighted stability policy.
use super::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailUpdate {
    pub rail_id: String,
    /// None leaves availability unchanged.
    pub available: Option<bool>,
    /// None leaves capacity unchanged; Some(None) makes it unlimited.
    pub capacity_per_minute_cents: Option<Option<u64>>,
}
impl RailUpdate {
    pub(super) fn validate(&self, scenario: &Scenario) -> Result<(), ValidationError> {
        if !scenario.network.rails.iter().any(|r| r.id == self.rail_id) {
            return Err(ValidationError(format!(
                "unknown disruption rail {}",
                self.rail_id
            )));
        }
        if self.available.is_none() && self.capacity_per_minute_cents.is_none() {
            return Err(ValidationError("empty rail update".into()));
        }
        Ok(())
    }
    fn merge(&mut self, other: &Self) {
        if other.available.is_some() {
            self.available = other.available;
        }
        if other.capacity_per_minute_cents.is_some() {
            self.capacity_per_minute_cents = other.capacity_per_minute_cents;
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disruption {
    pub minute: u128,
    pub update: RailUpdate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RailConditions {
    pub available: bool,
    pub capacity_per_minute_cents: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedDisruption {
    pub rail_id: String,
    pub before: RailConditions,
    pub after: RailConditions,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReoptimizationPolicy {
    Preserve,
    Recompute,
    Adaptive {
        max_extra_fee_cents: u128,
        max_extra_elapsed_minutes: u128,
    },
}
impl Default for ReoptimizationPolicy {
    fn default() -> Self {
        Self::Adaptive {
            max_extra_fee_cents: 0,
            max_extra_elapsed_minutes: 0,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReoptimizationChoice {
    Preserve,
    Recompute,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanAssessment {
    pub eligible_payments: usize,
    pub planned: usize,
    pub unplanned: usize,
    /// Denominator for changed_assignments / previously_planned.
    pub previously_planned: usize,
    pub changed_assignments: usize,
    pub changed_routes: usize,
    /// Same route, different absolute departure times.
    pub retimed_only: usize,
    pub withdrawn: usize,
    pub newly_planned: usize,
    pub remaining_fee_cents: u128,
    pub remaining_elapsed_minutes: u128,
    pub remaining_hops: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReoptimizationReport {
    pub minute: u128,
    pub policy: ReoptimizationPolicy,
    pub selected: ReoptimizationChoice,
    pub preserve: PlanAssessment,
    pub recompute: PlanAssessment,
    /// If false, fee/elapsed differences are not comparable cohort gaps.
    pub same_planned_cohort: bool,
    /// False for static estimates, which do not predict waiting/windows.
    pub reserved: bool,
    pub preserve_diagnostics: SearchDiagnostics,
    pub recompute_diagnostics: SearchDiagnostics,
}
impl ReoptimizationReport {
    pub fn selected_assessment(&self) -> &PlanAssessment {
        match self.selected {
            ReoptimizationChoice::Preserve => &self.preserve,
            ReoptimizationChoice::Recompute => &self.recompute,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdaptationMetrics {
    pub rail_changes: u128,
    pub reoptimizations: u128,
    pub assignment_comparisons: u128,
    pub changed_assignments: u128,
    pub changed_routes: u128,
    pub retimed_only: u128,
    pub withdrawn: u128,
}

impl Simulator {
    /// Current effective configuration; scenario() remains the restart input.
    pub fn effective_scenario(&self) -> &Scenario {
        &self.effective
    }
    pub fn adaptation_metrics(&self) -> &AdaptationMetrics {
        &self.state.adaptation_metrics
    }
    pub fn last_reoptimization(&self) -> Option<&ReoptimizationReport> {
        self.state.last_reoptimization.as_ref()
    }
    pub fn reoptimization_policy(&self) -> ReoptimizationPolicy {
        self.reoptimization_policy
    }
    /// Applies to future disruption decisions; does not advance time or reroute.
    /// Restart retains this explicit policy setting, but discards pending updates.
    pub fn set_reoptimization_policy(&mut self, policy: ReoptimizationPolicy) {
        self.reoptimization_policy = policy;
    }
    /// Stage a next-tick control. Repeated writes merge per field, last write wins.
    /// Storage is at most one record per rail; an invalid write changes nothing.
    pub fn queue_rail_update(&mut self, update: RailUpdate) -> Result<(), SimulationError> {
        update.validate(&self.scenario)?;
        if let Some(previous) = self.pending_updates.get_mut(&update.rail_id) {
            previous.merge(&update);
        } else {
            self.pending_updates.insert(update.rail_id.clone(), update);
        }
        Ok(())
    }
    pub fn pending_rail_updates(&self) -> usize {
        self.pending_updates.len()
    }

    pub(super) fn prepare_disruptions(&self) -> (Option<Scenario>, usize, Vec<AppliedDisruption>) {
        let mut index = self.next_disruption;
        let mut updates = BTreeMap::new();
        while let Some(event) = self.scenario.disruptions.get(index) {
            if event.minute != self.next_minute() {
                break;
            }
            updates.insert(event.update.rail_id.clone(), event.update.clone());
            index += 1;
        }
        for (id, update) in &self.pending_updates {
            if let Some(previous) = updates.get_mut(id) {
                previous.merge(update);
            } else {
                updates.insert(id.clone(), update.clone());
            }
        }
        if updates.is_empty() {
            return (None, index, vec![]);
        }
        let mut effective = self.effective.clone();
        let mut changes = vec![];
        for (id, update) in updates {
            let i = effective
                .network
                .rails
                .binary_search_by(|r| r.id.cmp(&id))
                .unwrap();
            let before = RailConditions {
                available: effective.network.rails[i].available,
                capacity_per_minute_cents: effective.services[i].capacity_per_minute_cents,
            };
            let after = RailConditions {
                available: update.available.unwrap_or(before.available),
                capacity_per_minute_cents: update
                    .capacity_per_minute_cents
                    .unwrap_or(before.capacity_per_minute_cents),
            };
            if before != after {
                effective.network.rails[i].available = after.available;
                effective.services[i].capacity_per_minute_cents = after.capacity_per_minute_cents;
                changes.push(AppliedDisruption {
                    rail_id: id,
                    before,
                    after,
                });
            }
        }
        ((!changes.is_empty()).then_some(effective), index, changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_tick_keeps_effective_network_controls_policy_and_event_cursor() {
        for field in 0..3 {
            let mut sim = super::super::tests::simulator();
            let update = RailUpdate {
                rail_id: "ACH".into(),
                available: Some(false),
                capacity_per_minute_cents: Some(Some(0)),
            };
            sim.scenario.disruptions.push(Disruption {
                minute: 0,
                update: update.clone(),
            });
            sim.queue_rail_update(update).unwrap();
            match field {
                0 => sim.state.next_event = u128::MAX - 5,
                1 => sim.state.adaptation_metrics.rail_changes = u128::MAX,
                _ => sim.state.adaptation_metrics.reoptimizations = u128::MAX,
            }
            let before = sim.clone();
            assert!(matches!(
                sim.step_observed(),
                Err(SimulationError::ArithmeticOverflow(_))
            ));
            assert_eq!(sim, before);
            assert_eq!(sim.pending_rail_updates(), 1);
            assert_eq!(sim.next_disruption, 0);
        }
    }
}

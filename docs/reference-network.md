# Static reference fixture

Institutions are fictional. The demo uses recognizable U.S. payment-rail names: **RTP, FedNow, ACH and Fedwire**. All rail membership, topology, fees and settlement times are synthetic scenario inputs, not verified real-world network data or operating rules. The demo sets all rails available, with no transaction ceilings or delivery deadlines. Demo batch capacities are unlimited. The static fixture has no timetable; the separate scheduled API accepts explicit synthetic departure opportunities. Actual network rules are not modeled. All money is USD, stored as integer cents. Loading the fixture always produces the same records in the same order.

| Statistic | Expected value |
| --- | ---: |
| Institutions | 6 |
| Payment rails | 4 |
| Payments awaiting routing | 12 |
| Opening liquidity | USD 1,000,000.00 |
| Payment volume | USD 225,001.50 |
| Largest payment | USD 75,000.00 |

These are the static fixture totals used by library tests and exact routing examples. The operations console generates its own payment stream and displays runtime metrics. Its network view preserves descriptive opening balances.

The rails appear in this stable order. **Every membership, fee and timing value below is synthetic.**

| ID | Display name | Synthetic members | Synthetic fee (USD) | Synthetic settlement minutes |
| --- | --- | --- | ---: | ---: |
| RTP | RTP | ALP, BRK, CDR, DLT | 0.25 | 0 |
| FEDNOW | FedNow | ALP, BRK, CDR, DLT | 0.25 | 0 |
| ACH | ACH | ALP, BRK, CDR, DLT, ELM, FLD | 0.05 | 1440 |
| FEDWIRE | Fedwire | ALP, DLT, ELM | 15.00 | 30 |

RTP, ACH and Fedwire retain the existing instant, batch and wire fixture inputs respectively. FedNow is a fourth rail that reuses the synthetic instant membership, fee and timing inputs. The shared values are a demo choice and do not imply that RTP and FedNow operate identically. Institutions, balances and payment instructions are unchanged.

Payments in the static fixture remain unassigned instructions. The library also exposes `routing::route_payment(&network, &payment)` for read-only, minimum-fee routing through shared rails. It returns `Ok(Some(route))`, `Ok(None)` when no route exists, or a validation error for malformed input. It never incurs fees, moves funds or settles payments. Scenario-file import, multiple currencies and actual transfer execution remain outside this foundation; no external optimization solver is used.

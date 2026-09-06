import unittest
from strategy_gaps import compare


class GapTests(unittest.TestCase):
    def rows(self, optimum, actual, status='feasible'):
        common = dict(scale=2, seed=42, ticks=1, solve_ms_median=1,
                      objective_minutes=0, objective_hops=1, result_digest='x')
        return [dict(common, family='case', status='optimal', fee_cents=optimum),
                dict(common, family='bounded-case', status=status, fee_cents=actual)]

    def test_zero_optimum_has_explicit_zero_or_infinite_gap(self):
        self.assertEqual(compare(self.rows(0, 0))[0]['gap_percent'], 0)
        r = compare(self.rows(0, 1))[0]
        self.assertEqual(r['gap_status'], 'infinite')
        self.assertIsNone(r['gap_percent'])

    def test_unserved_work_is_never_a_finite_gap(self):
        r = compare(self.rows(10, 0, 'unresolved'))[0]
        self.assertEqual(r['gap_status'], 'no full solution')
        self.assertIsNone(r['gap_percent'])
        self.assertEqual(r['optimum_fee'], 10)

    def test_gap_formula_and_exact_feasibility_disagreements(self):
        self.assertEqual(compare(self.rows(100, 125))[0]['gap_percent'], 25)
        rows = self.rows(100, 100)
        rows[0]['status'] = 'infeasible'
        with self.assertRaises(AssertionError):
            compare(rows)
        with self.assertRaises(AssertionError):
            compare(self.rows(100, 99))

    def test_censored_oracle_does_not_manufacture_an_optimum(self):
        rows = self.rows(100, 100)
        rows[0]['status'] = 'censored'
        r = compare(rows)[0]
        self.assertEqual(r['gap_status'], 'unknown optimum')
        self.assertIsNone(r['gap_percent'])

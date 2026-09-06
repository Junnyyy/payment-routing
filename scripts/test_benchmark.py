import sys
import unittest
import benchmark

class SupervisorTests(unittest.TestCase):
    def test_exit_resource_accounting_and_timeout_are_distinct(self):
        worker = [sys.executable, '-c', 'print(\'{"kind":"result","status":"infeasible"}\')']
        result = benchmark.supervise(worker, 2, 512)
        self.assertEqual(result['status'], 'infeasible')
        self.assertEqual(result['exit_code'], 0)
        self.assertGreater(result['peak_rss_bytes'], 0)
        result = benchmark.supervise([sys.executable, '-c', 'import time; time.sleep(5)'], .05, 512)
        self.assertEqual(result['status'], 'timeout')
        self.assertEqual(result['exit_code'], -9)
        self.assertEqual(result['result'], {})
        result = benchmark.supervise([sys.executable, '-c', 'raise RuntimeError("test")'], 2, 512)
        self.assertEqual(result['status'], 'error')

    def test_replay_check_ignores_timing_but_rejects_witness_changes(self):
        rows = [dict(status='optimal', mode=m, input={'input_digest':'a'},
                     result={'solve_ns':i, 'result_digest':'b'})
                for i,m in enumerate(['plain','plain','stats'])]
        benchmark.verify_rows(rows)
        rows[-1]['result']['result_digest']='c'
        with self.assertRaises(AssertionError):
            benchmark.verify_rows(rows)

if __name__ == '__main__':
    unittest.main()

import sys
import unittest
from unittest.mock import Mock, call, patch
import benchmark

class SupervisorTests(unittest.TestCase):
    def test_exit_between_poll_and_kill_preserves_limit_record_and_reaps_once(self):
        for reason, now in [('timeout', 2.1), ('rss_limit', 0.2)]:
            with self.subTest(reason=reason):
                child = Mock(pid=1234, returncode=None)
                usage = Mock(ru_utime=0.1, ru_stime=0.02, ru_maxrss=65536,
                             ru_minflt=3, ru_majflt=0, ru_nvcsw=2, ru_nivcsw=1)

                def start_worker(*args, **kwargs):
                    kwargs['stdout'].write(b'{"kind":"result","status":"optimal"}\n')
                    return child

                # Model the OS race explicitly, without depending on scheduler timing:
                # poll sees a live child; kill sees ESRCH; wait4 still collects its exit.
                with patch.object(benchmark.subprocess, 'Popen', side_effect=start_worker), \
                     patch.object(benchmark.time, 'monotonic', side_effect=[0, now, now + .01]), \
                     patch.object(benchmark.os, 'wait4', side_effect=[(0, 0, None), (1234, 0, usage)]) as wait, \
                     patch.object(benchmark.os, 'kill', side_effect=ProcessLookupError) as kill, \
                     patch.object(benchmark.subprocess, 'run', return_value=Mock(
                         returncode=0, stdout='65536\n', stderr='')):
                    result = benchmark.supervise(['worker'], 2, 32)

                self.assertEqual(result['status'], reason)
                self.assertEqual(result['exit_code'], 0)
                self.assertEqual(result['result']['status'], 'optimal')
                self.assertEqual(result['cpu_user_seconds'], .1)
                self.assertGreater(result['peak_rss_bytes'], 0)
                self.assertEqual(child.returncode, 0)
                kill.assert_called_once_with(1234, benchmark.signal.SIGKILL)
                self.assertEqual(wait.call_args_list,
                                 [call(1234, benchmark.os.WNOHANG), call(1234, 0)])

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

    def test_rss_guard_stops_and_reaps_a_large_worker(self):
        result = benchmark.supervise([sys.executable, '-c',
            'import time; x = bytearray(64 * 1024 * 1024); time.sleep(5)'], 2, 32)
        self.assertEqual(result['status'], 'rss_limit')
        self.assertEqual(result['exit_code'], -9)
        self.assertGreater(result['peak_rss_bytes'], 32 * 1024 * 1024)

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

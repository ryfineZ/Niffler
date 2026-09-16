import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location('telemetry_probe', Path(__file__).parents[1] / 'verify_codex_telemetry.py')
PROBE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROBE)


class ProbeTests(unittest.TestCase):
    def test_extracts_only_answer_text(self):
        value = {'output': [{'type': 'reasoning', 'summary': [{'text': 'internal'}]},
                            {'type': 'message', 'content': [{'type': 'output_text', 'text': '{"answer":7}'}]}]}
        self.assertEqual(PROBE.answer_text(value), '{"answer":7}')
        self.assertTrue(PROBE.grade(PROBE.answer_text(value), 7))

    def test_rejects_ambiguous_or_wrong_answers(self):
        for value in ['The answer might be 7', '{"answer":"7"}', '{"answer":true}', '{"answer":8}']:
            self.assertFalse(PROBE.grade(value, 7))
        self.assertTrue(PROBE.grade('```json\n{"answer":7}\n```', 7))

    def test_binary_count_against_independent_enumeration(self):
        expected = next(answer for name, _, answer in PROBE.cases() if name == 'binary_strings')
        brute = sum(bits.count('1') == 8 and '111' not in bits
                    for bits in (format(n, '019b') for n in range(1 << 19)))
        self.assertEqual(expected, brute)


if __name__ == '__main__':
    unittest.main()

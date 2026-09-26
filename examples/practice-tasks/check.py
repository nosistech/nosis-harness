"""Small task acceptance checks, not a security sandbox or a market benchmark.

Run: python -B check.py PATH_TO_MODULE delivery|names|quantities|all
This imports and executes the selected Python file. Inspect it before running.
"""

import importlib.util
from pathlib import Path
import sys
import unittest


class DeliveryChecks(unittest.TestCase):
    def test_below_threshold(self):
        self.assertEqual(subject.delivery_fee(0), 499)
        self.assertEqual(subject.delivery_fee(4999), 499)

    def test_threshold_and_above(self):
        self.assertEqual(subject.delivery_fee(5000), 0)
        self.assertEqual(subject.delivery_fee(9000), 0)

    def test_express_always_adds_its_fee(self):
        self.assertEqual(subject.delivery_fee(4999, True), 1298)
        self.assertEqual(subject.delivery_fee(5000, True), 799)
        self.assertEqual(subject.delivery_fee(9000, True), 799)

    def test_negative_is_rejected(self):
        with self.assertRaises(ValueError):
            subject.delivery_fee(-1)


class NameChecks(unittest.TestCase):
    def test_first_spelling_and_order(self):
        self.assertEqual(subject.unique_names([" Zoe ", "ana", "ZOE", "Ana"]),
                         ["Zoe", "ana"])

    def test_blank_names_are_ignored(self):
        self.assertEqual(subject.unique_names(["", "  ", " Jo "]), ["Jo"])

    def test_unicode_casefold(self):
        self.assertEqual(subject.unique_names(["Stra\u00dfe", "STRASSE"]),
                         ["Stra\u00dfe"])

    def test_input_is_unchanged(self):
        original = [" B ", "a", "B"]
        before = original.copy()
        subject.unique_names(original)
        self.assertEqual(original, before)


class QuantityChecks(unittest.TestCase):
    def test_sum_and_trim(self):
        self.assertEqual(subject.parse_quantities([" pens , 2 ", "pens,3", "pad,0"]),
                         {"pens": 5, "pad": 0})

    def test_blank_lines(self):
        self.assertEqual(subject.parse_quantities(["", "  "]), {})

    def test_labels_are_case_sensitive(self):
        self.assertEqual(subject.parse_quantities(["Pen,1", "pen,2"]),
                         {"Pen": 1, "pen": 2})

    def test_invalid_lines(self):
        for line in ["pens", "pens,1,2", ",1", "pens,-1", "pens,1.5", "pens,no"]:
            with self.subTest(line=line), self.assertRaises(ValueError):
                subject.parse_quantities([line])


if __name__ == "__main__":
    groups = {"delivery": DeliveryChecks, "names": NameChecks,
              "quantities": QuantityChecks}
    if len(sys.argv) != 3 or sys.argv[2] not in {*groups, "all"}:
        sys.exit("Usage: python -B check.py MODULE.py delivery|names|quantities|all")
    target = Path(sys.argv[1]).resolve(strict=True)
    spec = importlib.util.spec_from_file_location("practice_subject", target)
    if spec is None or spec.loader is None:
        sys.exit("Choose a readable Python module.")
    subject = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(subject)
    selected = list(groups.values()) if sys.argv[2] == "all" else [groups[sys.argv[2]]]
    suite = unittest.TestSuite(unittest.defaultTestLoader.loadTestsFromTestCase(group)
                               for group in selected)
    result = unittest.TextTestRunner(stream=sys.stdout, verbosity=2).run(suite)
    sys.exit(0 if result.wasSuccessful() else 1)

#!/usr/bin/env python3
"""Regression checks for role identity across AT-SPI versions."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import pyatspi

spec = importlib.util.spec_from_file_location('desktop_probe', Path(__file__).with_name('desktop-probe.py'))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class Accessible:
    def __init__(self, name, role, display_role, children=()):
        self.name, self.role, self.display_role = name, role, display_role
        self.children = children

    def getRole(self):
        return self.role

    def getRoleName(self):
        raise AssertionError('Matching must not depend on the display role name')

    def __iter__(self):
        return iter(self.children)


class RoleMatchingTests(unittest.TestCase):
    def test_button_matches_across_display_names(self):
        for display_role in ('button', 'push button', 'bouton poussoir'):
            with self.subTest(display_role=display_role):
                # A label can have the same visible name; it must not be activated.
                label = Accessible('View All', pyatspi.ROLE_LABEL, 'label')
                button = Accessible('View All', pyatspi.ROLE_PUSH_BUTTON, display_role)
                with patch.object(probe.pyatspi.Registry, 'getDesktop', return_value=Accessible("main", None, "desktop", [label, button])):
                    self.assertIs(probe.named('View All', pyatspi.ROLE_PUSH_BUTTON), button)

    def test_missing_role_still_fails(self):
        label = Accessible('View All', pyatspi.ROLE_LABEL, 'label')
        with tempfile.TemporaryDirectory() as log_dir, \
             patch.object(probe.pyatspi.Registry, 'getDesktop', return_value=Accessible("main", None, "desktop", [label])), \
             patch.object(probe, 'TIMEOUT', 0.001), \
             patch.object(probe.time, 'sleep'), \
             patch.dict(probe.os.environ, {'SAIFORI_LOG_DIR': log_dir}), \
             patch.object(Accessible, 'getRoleName', return_value='label'):
            with self.assertRaisesRegex(AssertionError, 'Timed out'):
                probe.named('View All', pyatspi.ROLE_PUSH_BUTTON)
            self.assertIn('View All', (Path(log_dir) / 'accessibility.log').read_text())



if __name__ == '__main__':
    unittest.main()

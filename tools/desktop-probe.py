#!/usr/bin/env python3
"""Exercise the UI through AT-SPI in desktop-smoke.sh's isolated session."""
import json
import os
from pathlib import Path
import subprocess
import time

from gi.repository import GLib
import pyatspi

TIMEOUT = 20


def walk(node):
    yield node
    for child in node:
        if child:
            yield from walk(child)


def wait_for(description, probe):
    deadline = time.monotonic() + TIMEOUT
    while time.monotonic() < deadline:
        try:
            result = probe()
            if result:
                return result
        except (StopIteration, GLib.Error):
            # GTK can replace accessible objects while rebuilding a page.
            pass
        time.sleep(0.1)
    # This private test session contains only synthetic data, never user records.
    diagnostic = Path(os.environ['SAIFORI_LOG_DIR']) / 'accessibility.log'
    try:
        with diagnostic.open('w') as log:
            for index, node in enumerate(walk(pyatspi.Registry.getDesktop(0))):
                if index >= 500:
                    log.write('Tree truncated at 500 widgets.\n')
                    break
                log.write(f'{node.getRoleName()}: {node.name!r}\n')
    except Exception as error:
        print(f'Could not capture accessibility tree: {error}', flush=True)
    raise AssertionError(f'Timed out waiting for {description}; see {diagnostic}')


def named(name, role=None):
    # AT-SPI role names differ across releases ("button" vs "push button").
    # Enum values are stable and also avoid localized role-name comparisons.
    return wait_for(
        f'{role or "widget"} {name!r}',
        lambda: next(n for n in walk(pyatspi.Registry.getDesktop(0))
                     if n.name == name and (role is None or n.getRole() == role)),
    )


def activate(name):
    action = named(name, pyatspi.ROLE_PUSH_BUTTON).queryAction()
    assert action.nActions and action.doAction(0), f'Cannot activate {name!r}'


def main():
    out = Path(os.environ['SAIFORI_SCREENSHOT_DIR'])
    out.mkdir(parents=True, exist_ok=True)


    def screenshot(name):
        frame = named('Saifori', pyatspi.ROLE_FRAME)
        bounds = wait_for('nonzero window dimensions', lambda: frame.queryComponent().getExtents(pyatspi.DESKTOP_COORDS))
        assert bounds.width > 0 and bounds.height > 0, f'Invalid window bounds: {bounds}'
        subprocess.run(['import', '-window', 'root', '-crop',
                        f'{bounds.width}x{bounds.height}+{bounds.x}+{bounds.y}',
                        '+repage', str(out / name)], check=True)


    named('View All', pyatspi.ROLE_PUSH_BUTTON)
    screenshot('overview.png')
    activate('View All')
    named('Transaction details', pyatspi.ROLE_LABEL)
    screenshot('transactions.png')
    print('PASS: Overview and Transactions navigation with synthetic data', flush=True)

    activate('Add Transaction')
    named('Save Transaction', pyatspi.ROLE_PUSH_BUTTON)
    expected = {'Transaction type', 'Description', 'Quantity', 'Unit price',
                'Currency', 'Date · YYYY-MM-DD', 'Category', 'Payment method', 'Note'}


    def labelled_controls():
        controls = {}
        for node in walk(pyatspi.Registry.getDesktop(0)):
            for relation in node.getRelationSet():
                if relation.getRelationType() == pyatspi.RELATION_LABELLED_BY:
                    for index in range(relation.getNTargets()):
                        controls[relation.getTarget(index).name] = node
        return controls if expected <= controls.keys() else None


    controls = wait_for('every form control to expose its label', labelled_controls)
    print('PASS: labelled form controls:', json.dumps(sorted(expected)), flush=True)
    for label, value in [('Description', 'Review smoke purchase'), ('Unit price', '12.50')]:
        assert controls[label].queryEditableText().setTextContents(value), label
    activate('Save Transaction')
    named('Review smoke purchase', pyatspi.ROLE_LABEL)
    print('PASS: create transaction through the real GTK form in isolated Test Mode', flush=True)


if __name__ == "__main__":
    main()

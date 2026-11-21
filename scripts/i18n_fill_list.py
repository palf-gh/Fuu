#!/usr/bin/env python3
"""Fill a catalogue from a JSON list of translations in source order.

    python scripts/i18n_fill_list.py <lang> <list.json>

The list must have one entry per source string, in the order
`scripts/i18n.py` extracts them. Each entry must use the same placeholders
as its source string ({} / {0}… may be reordered, never dropped or added).
"""
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import i18n  # noqa: E402

PH = re.compile(r'\{(\d*)\}')
# Arguments a template receives beyond those its English text shows.
EXTRA = {'{0}, {1} {2} {3} {4}': {'5'}}  # header_date: {5} is the month number


def placeholders(s):
    s = s.replace('{{', '').replace('}}', '')
    found = PH.findall(s)
    positional = sum(1 for f in found if f == '')
    return positional, sorted(set(f for f in found if f))


def main(lang, path):
    ids = list(i18n.extract())
    items = json.load(open(path, encoding='utf-8'))
    if len(items) != len(ids):
        sys.exit(f'{lang}: {len(items)} translations for {len(ids)} strings')
    bad = []
    for source, target in zip(ids, items):
        sp, sn = placeholders(source)
        tp, tn = placeholders(target)
        if source in EXTRA:
            # May use any of the arguments (e.g. the month number instead of its name).
            ok = sp == tp and set(tn) <= set(sn) | EXTRA[source]
        else:
            ok = sp == tp and set(tn) == set(sn)
        if not ok:
            bad.append((source, target))
    for source, target in bad:
        print(f'placeholder mismatch: {source!r} -> {target!r}')
    if bad:
        sys.exit(1)
    current = i18n.read_po(i18n.po_path(lang))
    current.update(dict(zip(ids, items)))
    i18n.write_po(i18n.po_path(lang), lang, i18n.extract(), current)
    print(f'{lang}: {len(items)} strings filled')


if __name__ == '__main__':
    main(*sys.argv[1:3])

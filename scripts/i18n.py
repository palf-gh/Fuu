#!/usr/bin/env python3
"""Keep Fuu's translation catalogues in step with the source.

    python scripts/i18n.py                 # update lang/fuu-app.pot and every .po
    python scripts/i18n.py fill ja ja.json # set translations from {"English": "訳"}
    python scripts/i18n.py missing de      # list untranslated strings of a language

Source strings are English: `@tr("…")` in crates/fuu-app/ui/*.slint and
`tr!("…")` in the Rust crates. Catalogues live in lang/<lang>/LC_MESSAGES/
fuu-app.po (Slint bundles them; fuu-i18n compiles them for Rust).
"""
import glob
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LANG = os.path.join(ROOT, 'lang')
LANGUAGES = ['ja', 'zh-Hans', 'zh-Hant', 'ko', 'pt-BR', 'es-ES', 'es-419', 'de', 'fr', 'id']
STRING = r'"((?:[^"\\]|\\.)*)"'
PATTERNS = [
    ('crates/fuu-app/ui/*.slint', re.compile(r'@tr\(\s*' + STRING)),
    ('crates/*/src/**/*.rs', re.compile(r'\btr!\(\s*' + STRING)),
]


def unescape(s):
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c != '\\':
            out.append(c)
            i += 1
            continue
        n = s[i + 1]
        if n == 'n':
            out.append('\n')
        elif n == 't':
            out.append('\t')
        elif n == 'u' and s[i + 2] == '{':
            end = s.index('}', i)
            out.append(chr(int(s[i + 3:end], 16)))
            i = end + 1
            continue
        else:
            out.append(n)
        i += 2
    return ''.join(out)


def po_quote(s):
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n').replace('\t', '\\t') + '"'


def po_unquote(s):
    return unescape(s.strip()[1:-1])


def extract():
    """msgid -> list of 'file:line' references, in first-seen order."""
    found = {}
    for pattern, regex in PATTERNS:
        for path in sorted(glob.glob(os.path.join(ROOT, pattern), recursive=True)):
            rel = os.path.relpath(path, ROOT).replace(os.sep, '/')
            if rel.startswith('crates/fuu-i18n/'):  # its own examples
                continue
            text = open(path, encoding='utf-8').read()
            for m in regex.finditer(text):
                line = text.count('\n', 0, m.start()) + 1
                found.setdefault(unescape(m.group(1)), []).append(f'{rel}:{line}')
    return found


def read_po(path):
    """msgid -> msgstr (translated or empty)."""
    entries, msgid, msgstr, field = {}, None, None, None
    if not os.path.exists(path):
        return entries
    for raw in open(path, encoding='utf-8'):
        line = raw.strip()
        if line.startswith('msgid '):
            if msgid is not None:
                entries[msgid] = msgstr
            msgid, msgstr, field = po_unquote(line[6:]), '', 'id'
        elif line.startswith('msgstr '):
            msgstr, field = po_unquote(line[7:]), 'str'
        elif line.startswith('"') and field == 'id':
            msgid += po_unquote(line)
        elif line.startswith('"') and field == 'str':
            msgstr += po_unquote(line)
    if msgid is not None:
        entries[msgid] = msgstr
    entries.pop('', None)
    return entries


def write_po(path, lang, found, translations):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    header = [
        '# Fuu user interface. Source strings are English.',
        '# Translations other than Japanese were made by AI and need review;',
        '# corrections are welcome (see README).',
        'msgid ""',
        'msgstr ""',
        '"Content-Type: text/plain; charset=UTF-8\\n"',
        f'"Language: {lang}\\n"' if lang else '"Language: \\n"',
        '',
    ]
    out = header[:]
    for msgid, refs in found.items():
        out.append('#: ' + ' '.join(refs[:4]))
        out.append('msgid ' + po_quote(msgid))
        out.append('msgstr ' + po_quote(translations.get(msgid, '')))
        out.append('')
    open(path, 'w', encoding='utf-8', newline='\n').write('\n'.join(out))


def po_path(lang):
    return os.path.join(LANG, lang, 'LC_MESSAGES', 'fuu-app.po')


def update():
    found = extract()
    write_po(os.path.join(LANG, 'fuu-app.pot'), '', found, {})
    for lang in LANGUAGES:
        write_po(po_path(lang), lang, found, read_po(po_path(lang)))
    print(f'{len(found)} strings')


def main(args):
    if not args:
        update()
    elif args[0] == 'fill':
        lang, data = args[1], json.load(open(args[2], encoding='utf-8'))
        found = extract()
        current = read_po(po_path(lang))
        unknown = [k for k in data if k not in found]
        current.update({k: v for k, v in data.items() if k in found})
        write_po(po_path(lang), lang, found, current)
        print(f'{lang}: {sum(1 for k in found if current.get(k))}/{len(found)} translated; unknown keys: {len(unknown)}')
        for k in unknown[:20]:
            print('  unknown:', k)
    elif args[0] == 'missing':
        found, current = extract(), read_po(po_path(args[1]))
        missing = [k for k in found if not current.get(k)]
        json.dump({k: '' for k in missing}, sys.stdout, ensure_ascii=False, indent=1)
    else:
        sys.exit(__doc__)


if __name__ == '__main__':
    main(sys.argv[1:])

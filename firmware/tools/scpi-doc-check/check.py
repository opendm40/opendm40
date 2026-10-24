#!/usr/bin/env python3
"""Check that the SCPI reference (docs/manual/scpi-reference.md) and the
firmware's SCPI parser (firmware/src) name the same command mnemonics.

    check.py [--src DIR] [--doc FILE] [-v]

Default paths assume this script sits in <repo>/firmware/tools/scpi-doc-check:
--src <repo>/firmware/src, --doc <repo>/docs/manual/scpi-reference.md.
Exit status 0 when both sides agree, 1 on any difference, 2 on a usage or
parse problem. Python 3 standard library only.

What is compared
----------------
The firmware has no command table: Scpi::handle (src/scpi.rs) is a chain of
match arms and helpers (parse_func, range_cmd, parse_pow, wav_cmd, ...) that
compare the ':'-separated header parts one at a time. Rebuilding full command
paths from that control flow is not reliable, so this checker compares the
*set of header mnemonics* instead:

  * a mnemonic the parser matches in a header position that no documented
    command uses           -> "missing in doc"  (a new or undocumented command)
  * a mnemonic in a documented command that the parser never matches
                           -> "missing in code" (a removed or misspelt command)
  * a documented mnemonic whose short form (its capital letters) differs from
    the parser's           -> "short form differs"

It does not prove that every documented path is accepted (a valid mnemonic in
a wrong place passes); review the page against the code for that.

Source side (every *.rs under --src that defines `fn kw(`, plus one pattern
in all files):
  1. kw(<expr>, "MNEMonic") where <expr> starts with a header-part variable
     (p0, sub, p, l, l0, l1, leaf, parts[..], s2, op, m, r[..], rest, ...).
     kw() accepts the upper-case prefix (short form) or the whole word (long
     form), so the mnemonic keeps its case: "MEASure" -> long MEASURE, short
     MEAS. Calls on argument variables (a, a0, v, t, x, arg, ...) are
     argument keywords and are skipped.
  2. Byte literals the parser compares header parts with, which have one
     form only (long = short): `<header var> == b"X"`, `Some(&&b"X"[..])`
     after a header-part comparison, and literal patterns (`b"X" =>`,
     `(b"X", true)`, `(Some(b"X"), ...)`) of a `match` whose scrutinee is a
     header part.
  3. In any file: `strip_prefix(b"ROOT:SUB:LEAF ")`, a header matched on the
     raw message before the parser (DBG:UPLOAD:DATA).
  Trailing digits of a mixed-case mnemonic are dropped (kw(p0 ..., "CHANnel")
  after strip_suffix(b"1") documents as CHANnel1).
  DOC_ONLY below lists the few documented mnemonics the parser accepts by
  position without looking at the word (TRIGger:LEVel, ...).

Doc side (the rigid row format)
-------------------------------
A command is one row of a Markdown table whose header row's FIRST cell is
exactly "Command". The row's first cell is the command header in one pair of
backticks, nothing else in the cell:

    | Command | Parameters | Reply | Link |
    |---|---|---|---|
    | `[SENSe:]VOLTage[:DC]:RANGe[?]` | `<v>`, AUTO, ... | ... | both |

  * SCPI notation: long form, short form in capitals ("MEASure"), optional
    nodes in [brackets], a trailing ? (or [?] for set + query).
  * Only the header, no parameters, no <placeholders>: write one row per
    concrete header (CONFigure:VOLTage:AC, CONFigure:CURRent:AC, ...).
  * Mnemonics with one form only (DBG:PING, DBG:HWTRIG:CAL, RUN, ...) are
    written all in capitals.
  * A trailing channel number (CHANnel1) is allowed; it is dropped before the
    comparison.
Rows of other tables, and text outside tables, are ignored.
"""
import argparse
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, '..', '..', '..'))
DEF_SRC = os.path.join(REPO, 'firmware', 'src')
DEF_DOC = os.path.join(REPO, 'docs', 'manual', 'scpi-reference.md')

# Leading identifiers of expressions that hold a header part (Scpi::handle,
# parse_func, range_cmd, coup_cmd, is_pow, parse_pow, wav_cmd).
HEADER_VARS = {'p0', 'p1', 'sub', 'p', 'l', 'l0', 'l1', 'leaf', 'parts', 's2', 'op', 'm', 'r', 'rest', 'head'}
# Leading identifiers of argument expressions.
ARG_VARS = {'a', 'a0', 'a1', 'v', 't', 'x', 'arg', 'ra', 'rs', 'tok', 'first', 'third', 'trim', 'name'}

MNEM = r'\*?[A-Za-z][A-Za-z0-9]*'

# Documented mnemonics the parser accepts by position without comparing the
# word (any word there is taken): key -> where. Keep this exact: an entry the
# parser starts matching is reported as stale.
DOC_ONLY = {
    'LEVEL': 'TRIGger:LEVel: scpi.rs TRIGger arm, any part after TRIGger other than STATus / SLOPe / SWEep is the level',
    'SQUARE': 'SOURce:FUNCtion:SQUare:DCYCle: scpi.rs SOURce arm, FUNCtion with 4 or more parts is the duty cycle',
    'DCYCLE': 'SOURce:FUNCtion:SQUare:DCYCle: as SQUare',
}


def lead_ident(expr):
    """Leading identifier of an expression: '(&parts[2][..], query)' -> 'parts',
    'trim(arg)' -> 'arg'."""
    e = expr.strip()
    while e[:1] in ('(', '&', ' '):
        e = e[1:]
    m = re.match(r'[A-Za-z_][A-Za-z0-9_]*', e)
    if not m:
        return ''
    ident = m.group(0)
    if ident == 'trim':  # trim(arg) / trim(a): look inside
        inner = e[m.end():].lstrip()
        if inner.startswith('('):
            return lead_ident(inner[1:])
    return ident


def key_of(tok):
    """(key, short form) of a mnemonic as written: 'MEASure' -> ('MEASURE',
    'MEAS'); 'CHANnel1' -> ('CHANNEL', 'CHAN'); 'AD2' -> ('AD2', 'AD2')."""
    if any(c.islower() for c in tok):
        tok = tok.rstrip('0123456789')
    short = ''
    for c in tok:
        if c.isupper() or c == '*':
            short += c
        else:
            break
    if not any(c.islower() for c in tok):
        short = tok  # one form only: kw() takes the whole word as its short form too
    return tok.upper(), short.upper()


def strip_comments(text):
    """Blank out // comments (keeping offsets), leave string literals."""
    out = []
    for line in text.split('\n'):
        i, in_s = 0, False
        while i < len(line):
            c = line[i]
            if in_s:
                if c == '\\':
                    i += 2
                    continue
                if c == '"':
                    in_s = False
            elif c == '"':
                in_s = True
            elif c == "'" and re.match(r"'(\\.|[^\\'])'", line[i:]):
                i += len(re.match(r"'(\\.|[^\\'])'", line[i:]).group(0))
                continue
            elif line.startswith('//', i):
                line = line[:i] + ' ' * (len(line) - i)
                break
            i += 1
        out.append(line)
    return '\n'.join(out)


def skip_literal(s, i):
    """If s[i:] starts a string / byte string / char literal, the index after it."""
    if s.startswith('b"', i):
        i += 1
    if s[i] == '"':
        i += 1
        while i < len(s) and s[i] != '"':
            i += 2 if s[i] == '\\' else 1
        return i + 1
    m = re.match(r"b?'(\\.|[^\\'])'", s[i:])
    if m:
        return i + len(m.group(0))
    return None


def balanced_end(s, i):
    """s[i] is an opening bracket; the index after its partner."""
    pairs = {'(': ')', '[': ']', '{': '}'}
    stack = [pairs[s[i]]]
    i += 1
    while i < len(s) and stack:
        j = skip_literal(s, i) if s[i] in '"\'b' else None
        if j is not None:
            i = j
            continue
        c = s[i]
        if c in pairs:
            stack.append(pairs[c])
        elif c in ')]}':
            stack.pop()
        i += 1
    return i


def match_arms(s):
    """(scrutinee, pattern text, offset) of every arm of every `match` in s."""
    for mm in re.finditer(r'\bmatch\b', s):
        i = mm.end()
        # scrutinee: up to the '{' at bracket depth 0
        j = i
        while j < len(s) and s[j] != '{':
            if s[j] in '([':
                j = balanced_end(s, j)
                continue
            lit = skip_literal(s, j) if s[j] in '"\'b' else None
            if lit is not None:
                j = lit
                continue
            if s[j] == ';':
                break
            j += 1
        if j >= len(s) or s[j] != '{':
            continue
        scrut = s[i:j]
        end = balanced_end(s, j) - 1
        k = j + 1
        while k < end:
            # pattern: up to '=>' at depth 0
            p0 = k
            while k < end and not s.startswith('=>', k):
                if s[k] in '([{':
                    k = balanced_end(s, k)
                    continue
                lit = skip_literal(s, k) if s[k] in '"\'b' else None
                if lit is not None:
                    k = lit
                    continue
                k += 1
            if k >= end:
                break
            yield scrut, s[p0:k], p0
            k += 2
            # arm body: a block, or an expression up to ',' at depth 0
            while k < end and s[k].isspace():
                k += 1
            if k < end and s[k] == '{':
                k = balanced_end(s, k)
                while k < end and s[k].isspace():
                    k += 1
                if k < end and s[k] == ',':
                    k += 1
                continue
            while k < end and s[k] != ',':
                if s[k] in '([{':
                    k = balanced_end(s, k)
                    continue
                lit = skip_literal(s, k) if s[k] in '"\'b' else None
                if lit is not None:
                    k = lit
                    continue
                k += 1
            k += 1


def line_of(s, off):
    return s.count('\n', 0, off) + 1


def source_mnemonics(src_dir):
    """{key: {short: [file:line, ...]}} of the header mnemonics in the parser."""
    found = {}
    unclassified = []

    def add(tok, where):
        k, sh = key_of(tok)
        found.setdefault(k, {}).setdefault(sh, []).append(where)

    files = []
    for root, _, names in os.walk(src_dir):
        for n in sorted(names):
            if n.endswith('.rs'):
                files.append(os.path.join(root, n))
    if not files:
        sys.exit('check.py: no .rs files under %s' % src_dir)
    parser_files = 0
    for path in sorted(files):
        rel = os.path.relpath(path, src_dir)
        with open(path, encoding='utf-8') as f:
            raw = f.read()
        s = strip_comments(raw)
        # 3. raw-message prefixes (any file)
        for m in re.finditer(r'strip_prefix\(b"(\*?[A-Z][A-Z0-9]*(?::[A-Z0-9]+)+) ?"\)', s):
            for tok in m.group(1).split(':'):
                add(tok, '%s:%d' % (rel, line_of(s, m.start())))
        if not re.search(r'\bfn kw\(', s):
            continue
        parser_files += 1
        # 1. kw(<expr>, "MNEM")
        for m in re.finditer(r'\bkw\(', s):
            end = balanced_end(s, m.end() - 1)
            inner = s[m.end():end - 1]
            lm = re.search(r',\s*"(' + MNEM + r')"\s*$', inner)
            if not lm:
                continue  # the definition, or a non-literal mnemonic
            expr = inner[:lm.start()]
            ident = lead_ident(expr)
            where = '%s:%d' % (rel, line_of(s, m.start()))
            if ident in ARG_VARS:
                continue
            if ident not in HEADER_VARS:
                unclassified.append('%s kw(%s, "%s")' % (where, expr.strip(), lm.group(1)))
                continue
            add(lm.group(1), where)
        # 2a. comparisons: <header expr> == b"X", ... == Some(&&b"X"[..])
        for m in re.finditer(r'([A-Za-z_][A-Za-z0-9_]*(?:\[[^\]]*\])?(?:\.[a-z_]+\([^()]*\))*)\s*==\s*(?:Some\(&&)?b"(' + MNEM + r')"', s):
            ident = lead_ident(m.group(1))
            where = '%s:%d' % (rel, line_of(s, m.start()))
            if ident in ARG_VARS:
                continue
            if ident not in HEADER_VARS:
                unclassified.append('%s %s == b"%s"' % (where, m.group(1), m.group(2)))
                continue
            add(m.group(2), where)
        # 2b. literal patterns of header-part matches
        for scrut, pat, off in match_arms(s):
            pat_only = re.split(r'\bif\b', pat, maxsplit=1)[0]
            lits = re.findall(r'b"(' + MNEM + r')"', pat_only)
            if not lits:
                continue
            ident = lead_ident(scrut)
            where = '%s:%d' % (rel, line_of(s, off))
            if ident in ARG_VARS:
                continue
            if ident not in HEADER_VARS:
                unclassified.append('%s match %s { %s }' % (where, scrut.strip(), pat_only.strip()))
                continue
            for t in lits:
                add(t, where)
    if parser_files == 0:
        sys.exit('check.py: no file under %s defines fn kw( (the SCPI parser)' % src_dir)
    return found, unclassified


def doc_commands(doc_path):
    """[(header as written, line)] of the command rows (module doc)."""
    with open(doc_path, encoding='utf-8') as f:
        lines = f.read().split('\n')
    cmds = []
    in_cmd_table = False
    prev_table = False
    in_fence = False
    for n, line in enumerate(lines, 1):
        st = line.strip()
        if st.startswith('```'):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        is_row = st.startswith('|')
        if not is_row:
            in_cmd_table = False
            prev_table = False
            continue
        cells = [c.strip() for c in st.strip('|').split('|')]
        if not prev_table:
            in_cmd_table = bool(cells) and cells[0] == 'Command'
            prev_table = True
            continue
        if not in_cmd_table or re.match(r'^:?-{3,}:?$', cells[0]):
            continue
        m = re.match(r'^`([^`]+)`$', cells[0])
        if not m:
            print('%s:%d: command row without a backticked header in its first cell: %s'
                  % (doc_path, n, cells[0]), file=sys.stderr)
            sys.exit(2)
        cmds.append((m.group(1), n))
    return cmds


def doc_mnemonics(cmds, doc_path):
    found = {}
    for hdr, n in cmds:
        if not re.match(r'^[\[\]:*?A-Za-z0-9]+$', hdr):
            print('%s:%d: not a plain command header (no parameters or <placeholders>): %s'
                  % (doc_path, n, hdr), file=sys.stderr)
            sys.exit(2)
        h = hdr.replace('[', '').replace(']', '').replace('?', '')
        for tok in [t for t in h.split(':') if t]:
            k, sh = key_of(tok)
            found.setdefault(k, {}).setdefault(sh, []).append('%s (line %d)' % (hdr, n))
    return found


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--src', default=DEF_SRC, help='firmware source directory (default: %(default)s)')
    ap.add_argument('--doc', default=DEF_DOC, help='SCPI reference page (default: %(default)s)')
    ap.add_argument('-v', '--verbose', action='store_true', help='list every mnemonic with its places')
    args = ap.parse_args()
    if not os.path.isdir(args.src):
        print('check.py: no source directory %s' % args.src, file=sys.stderr)
        return 2
    if not os.path.isfile(args.doc):
        print('check.py: no reference page %s' % args.doc, file=sys.stderr)
        return 2

    code, unclassified = source_mnemonics(args.src)
    cmds = doc_commands(args.doc)
    if not cmds:
        print('check.py: no command rows in %s (tables whose first header cell is "Command")'
              % args.doc, file=sys.stderr)
        return 2
    doc = doc_mnemonics(cmds, args.doc)

    def show(k, sh):
        return k if sh == k else '%s (short %s)' % (k, sh)

    missing_doc = sorted(k for k in code if k not in doc)
    missing_code = sorted(k for k in doc if k not in code and k not in DOC_ONLY)
    stale = sorted(k for k in DOC_ONLY if k in code or k not in doc)
    short_diff = []
    for k in sorted(set(code) & set(doc)):
        for sh in sorted(doc[k]):
            if sh not in code[k]:
                short_diff.append((k, sh, sorted(code[k])))

    print('source: %d header mnemonics; doc: %d commands, %d mnemonics'
          % (len(code), len(cmds), len(doc)))
    if args.verbose:
        for k in sorted(code):
            for sh, w in sorted(code[k].items()):
                print('  code %-28s %s' % (show(k, sh), ', '.join(w[:4]) + (' ...' if len(w) > 4 else '')))
    ok = True
    if unclassified:
        ok = False
        print('\nunclassified matches in the source (add the variable to HEADER_VARS or ARG_VARS):')
        for u in unclassified:
            print('  ' + u)
    if missing_doc:
        ok = False
        print('\nmissing in doc (%d): matched by the parser, used by no documented command:' % len(missing_doc))
        for k in missing_doc:
            for sh, w in sorted(code[k].items()):
                print('  %-28s %s' % (show(k, sh), ', '.join(w[:3]) + (' ...' if len(w) > 3 else '')))
    if missing_code:
        ok = False
        print('\nmissing in code (%d): in the doc, never matched by the parser:' % len(missing_code))
        for k in missing_code:
            for sh, w in sorted(doc[k].items()):
                print('  %-28s %s' % (show(k, sh), ', '.join(w[:3]) + (' ...' if len(w) > 3 else '')))
    if stale:
        ok = False
        print('\nstale DOC_ONLY entries in check.py (now matched by the parser, or no longer documented):')
        for k in stale:
            print('  %s: %s' % (k, DOC_ONLY[k]))
    if args.verbose:
        for k in sorted(DOC_ONLY):
            print('  doc only (accepted by position): %s: %s' % (k, DOC_ONLY[k]))
    if short_diff:
        ok = False
        print('\nshort form differs (%d):' % len(short_diff))
        for k, sh, csh in short_diff:
            print('  %s: doc %s, code %s   %s' % (k, sh, '/'.join(csh), ', '.join(doc[k][sh][:3])))
    if ok:
        print('OK: the reference and the parser name the same mnemonics')
        return 0
    return 1


if __name__ == '__main__':
    sys.exit(main())

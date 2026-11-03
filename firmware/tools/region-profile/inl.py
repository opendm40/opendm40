#!/usr/bin/env python3
"""Inlined copies in the image (size survey): every DW_TAG_inlined_subroutine
in regions B / C, grouped by the inlined function. A function inlined at n
sites with s bytes per copy costs ~ (n - 1) * s more than one out-of-line
copy plus n calls: the `#[inline(never)]` candidates.

    tools/region-profile/inl.py ELF [--top N]

Counts only the outermost inlined copy of a function inside one concrete
function (a copy nested in another inlined copy is part of that one), so
the bytes do not add up twice.
"""
import argparse
import collections
import subprocess
import sys

from elftools.elf.elffile import ELFFile

B_LO, B_HI, C_HI = 0x08008010, 0x08080000, 0x080A0000


def ranges_of(die, dwarf, rl, cu_base):
    a = die.attributes
    if 'DW_AT_low_pc' in a and 'DW_AT_high_pc' in a:
        lo = a['DW_AT_low_pc'].value
        hi = a['DW_AT_high_pc'].value
        if a['DW_AT_high_pc'].form != 'DW_FORM_addr':
            hi = lo + hi
        return [(lo, hi)]
    if 'DW_AT_ranges' in a and rl is not None:
        out = []
        base = cu_base
        for e in rl.get_range_list_at_offset(a['DW_AT_ranges'].value, cu=die.cu):
            if hasattr(e, 'base_address') and not hasattr(e, 'begin_offset'):
                base = e.base_address
                continue
            b, en = e.begin_offset, e.end_offset
            if getattr(e, 'is_absolute', False):
                out.append((b, en))
            else:
                out.append((base + b, base + en))
        return out
    return []


def name_of(die, cache):
    seen = 0
    d = die
    while d is not None and seen < 8:
        a = d.attributes
        for k in ('DW_AT_linkage_name', 'DW_AT_MIPS_linkage_name', 'DW_AT_name'):
            if k in a:
                v = a[k].value
                return v.decode() if isinstance(v, bytes) else str(v)
        o = a.get('DW_AT_abstract_origin') or a.get('DW_AT_specification')
        if o is None:
            return '?'
        off = o.value + (d.cu.cu_offset if o.form != 'DW_FORM_ref_addr' else 0)
        d = cache.get(off)
        if d is None:
            d = d_at(die, off)
            cache[off] = d
        seen += 1
    return '?'


def d_at(die, off):
    try:
        return die.dwarfinfo.get_DIE_from_refaddr(off)
    except Exception:
        return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('elf')
    ap.add_argument('--top', type=int, default=80)
    ap.add_argument('--any', action='store_true', help='count every address (tests on a host ELF)')
    a = ap.parse_args()
    elf = ELFFile(open(a.elf, 'rb'))
    dw = elf.get_dwarf_info()
    rl = dw.range_lists()
    cache = {}
    # origin -> region -> list of copy sizes
    copies = collections.defaultdict(lambda: {'B': [], 'C': []})
    for cu in dw.iter_CUs():
        top = cu.get_top_DIE()
        cu_base = top.attributes['DW_AT_low_pc'].value if 'DW_AT_low_pc' in top.attributes else 0

        def walk(die, inside):
            for ch in die.iter_children():
                if ch.tag == 'DW_TAG_inlined_subroutine':
                    if not inside:
                        rs = ranges_of(ch, dw, rl, cu_base)
                        n = sum(max(0, h - l) for l, h in rs)
                        if rs and n:
                            lo = min(l for l, _ in rs)
                            reg = 'B' if a.any or B_LO <= lo < B_HI else ('C' if B_HI <= lo < C_HI else None)
                            if reg:
                                copies[name_of(ch, cache)][reg].append(n)
                    walk(ch, True)
                elif ch.tag in ('DW_TAG_subprogram', 'DW_TAG_lexical_block', 'DW_TAG_namespace', 'DW_TAG_structure_type', 'DW_TAG_enumeration_type', 'DW_TAG_union_type'):
                    walk(ch, inside if ch.tag == 'DW_TAG_lexical_block' else False)
        try:
            walk(top, False)
        except Exception as e:  # an unexpected DWARF form: skip that CU
            print('INL skip CU at %#x: %s' % (cu.cu_offset, e), file=sys.stderr)
    names = list(copies)
    dem = {}
    try:
        out = subprocess.run(['llvm-cxxfilt'], input='\n'.join(names), capture_output=True, text=True).stdout.split('\n')
        dem = dict(zip(names, out))
    except OSError:
        pass
    for reg in ('B', 'C'):
        rows = []
        for n, r in copies.items():
            c = r[reg]
            if len(c) >= 2:
                tot = sum(c)
                save = tot - max(c) - 8 * len(c)  # one copy left, ~8 B per call
                if save > 0:
                    rows.append((save, len(c), tot, max(c), dem.get(n, n)))
        rows.sort(reverse=True)
        print('INL region %s: %d functions inlined at >= 2 sites; est. saving if out of line: %d B (top %d below)'
              % (reg, len(rows), sum(r[0] for r in rows), a.top))
        print('INL  save  sites  total  largest  function')
        for s, k, t, m, n in rows[:a.top]:
            print('INL %5d %6d %6d %8d  %s' % (s, k, t, m, n[:150]))


if __name__ == '__main__':
    main()

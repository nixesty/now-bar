"""Deletes the build outputs nothing builds any more. Cargo never removes an old copy: every change
to a dependency's features, the toolchain or the flags leaves the previous build of windows, Slint
and the settings window behind (several GB a week). The quick profile keeps what `cargo lint`,
`cargo t` and `cargo b` use now, and those units' incremental caches (their newest session only);
make-setup.ps1 -Keep sweeps dist with the messages of its own builds.
target\\ is kept NTFS-compressed: rlibs, rmeta and caches take 1.8 times less, new files included.
Usage: python tools/sweep.py [--dry-run]
       python tools/sweep.py --profile dist --messages <file>"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# Every unit's outputs end in its metadata hash: deps\libwindows-<hash>.rlib, build\<pkg>-<hash>\.
# Not .fingerprint\: a package's bins share one folder under a hash no output carries, and losing it
# rebuilds them (it's a few KB a unit).
HASH = re.compile(r'-([0-9a-f]{16})(?:\.|$)')
# The aliases themselves: other packages, features or flags hash differently and are copies. lint
# builds every target; t and b are what it shares with them.
QUICK = [['lint'], ['t', '--no-run'], ['b']]
# A cache is written as its unit's outputs are (3 s apart at most, measured): within this of one
# of them, it's that unit's. A copy built just before, by other flags, was 2.5 minutes off.
SAME_BUILD = 30


def artifacts(lines):
    """The compiler-artifact and build-script messages among cargo's JSON lines."""
    out = []
    for line in lines:
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get('reason') in ('compiler-artifact', 'build-script-executed'):
            out.append(m)
    return out


def hashes(msgs):
    """The hashes of the units these messages name, fresh or built."""
    keep = set()
    for m in msgs:
        paths = m.get('filenames', []) + [m.get('executable') or '', m.get('out_dir') or '']
        for p in paths:
            # out_dir is build\<pkg>-<hash>\out: the hash is the folder's.
            for part in re.split(r'[\\/]', p):
                h = HASH.search(part)
                if h:
                    keep.add(h.group(1))
    return keep


def run(args):
    """Runs cargo with JSON messages; the diagnostics still go to the console."""
    cmd = ['cargo'] + args + ['--message-format=json-render-diagnostics']
    p = subprocess.run(cmd, cwd=ROOT, stdout=subprocess.PIPE, text=True, encoding='utf-8')
    if p.returncode:
        sys.exit(f'cargo {" ".join(args)} failed: nothing was deleted')
    return p.stdout.splitlines()


def size(path):
    if os.path.isfile(path):
        return os.path.getsize(path)
    total = 0
    for d, _, files in os.walk(path):
        for f in files:
            try:
                total += os.path.getsize(os.path.join(d, f))
            except OSError:
                pass
    return total


def built(msgs):
    """When each crate's current outputs were written, by crate name (an incremental cache's
    folder is <crate>-<id>, and the id can't be worked out from cargo's hash)."""
    times = {}
    for m in msgs:
        if m.get('reason') != 'compiler-artifact':
            continue
        crate = m['target']['name'].replace('-', '_')
        for f in m['filenames']:
            if os.path.exists(f):
                times.setdefault(crate, []).append(os.path.getmtime(f))
    return times


def newest(path):
    """The newest modification under path: a cache in use is written to on every compile."""
    t = os.path.getmtime(path)
    for d, dirs, files in os.walk(path):
        for n in dirs + files:
            try:
                t = max(t, os.path.getmtime(os.path.join(d, n)))
            except OSError:
                pass
    return t


def stale(profile_dir, keep, times):
    """What can go: units with a hash nothing kept, caches no kept unit wrote, old sessions."""
    out = []
    for sub in ('deps', 'build'):
        d = os.path.join(profile_dir, sub)
        if not os.path.isdir(d):
            continue
        for name in os.listdir(d):
            h = HASH.search(name)
            if h and h.group(1) not in keep:
                out.append(os.path.join(d, name))
    inc = os.path.join(profile_dir, 'incremental')
    if os.path.isdir(inc):
        for name in os.listdir(inc):
            p = os.path.join(inc, name)
            t = newest(p)
            if not any(abs(t - b) < SAME_BUILD for b in times.get(name.rsplit('-', 1)[0], [])):
                out.append(p)
            else:
                out += old_sessions(p)
    return out


def old_sessions(cache):
    """rustc keeps the previous session beside the new one until its next compile (every cache
    twice over): only the newest finished one is ever read. s-<time>-<rand>-<svh>\\ and its
    s-<time>-<rand>.lock; a -working one means a compile is running there."""
    names = os.listdir(cache)
    if any(n.endswith('-working') for n in names):
        return []
    done = sorted((n for n in names if n.startswith('s-') and not n.endswith('.lock')),
                  key=lambda n: int(n.split('-')[1], 36))
    out = []
    for n in done[:-1]:
        lock = '-'.join(n.split('-')[:3]) + '.lock'
        out.append(os.path.join(cache, n))
        if lock in names:
            out.append(os.path.join(cache, lock))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--profile', default='quick', choices=['quick', 'dist'])
    ap.add_argument('--messages', help="a file of cargo's JSON messages (dist)")
    ap.add_argument('--dry-run', action='store_true')
    a = ap.parse_args()

    meta = json.loads(subprocess.run(['cargo', 'metadata', '--format-version', '1', '--no-deps'],
                                     cwd=ROOT, stdout=subprocess.PIPE, text=True, check=True).stdout)
    profile_dir = os.path.join(meta['target_directory'], a.profile)
    if a.messages:
        msgs = artifacts(open(a.messages, encoding='utf-8-sig'))
    elif a.profile == 'quick':
        msgs = [m for args in QUICK for m in artifacts(run(args))]
    else:
        sys.exit('dist needs --messages: its builds carry make-setup.ps1\'s flags')
    keep = hashes(msgs)
    if not keep:
        sys.exit('no build outputs named: nothing was deleted')

    paths = stale(profile_dir, keep, built(msgs))
    freed = 0
    for p in paths:
        freed += size(p)
        if not a.dry_run:
            if os.path.isdir(p):
                shutil.rmtree(p, ignore_errors=True)
            else:
                try:
                    os.remove(p)
                except OSError:
                    pass
    verb = 'would free' if a.dry_run else 'freed'
    print(f'{a.profile}: {len(paths)} stale entries, {verb} {freed / 1e9:.1f} GB')
    if not a.dry_run:
        compress(meta['target_directory'])


def compress(target):
    """Marks target\\ compressed (what's written there later inherits it) and compresses what
    isn't yet. cargo clean keeps the folder, and with it the mark."""
    subprocess.run(['compact', '/c', '/s:' + target, '/i', '/q'], stdout=subprocess.DEVNULL)
    out = subprocess.run(['compact', '/s:' + target, '/q'], stdout=subprocess.PIPE, text=True,
                         errors='replace').stdout.strip().splitlines()
    print('\n'.join(out[-2:]))


if __name__ == '__main__':
    main()

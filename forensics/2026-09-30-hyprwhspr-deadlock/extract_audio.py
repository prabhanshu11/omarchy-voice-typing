# /// script
# dependencies = ["pyelftools", "numpy"]
# requires-python = ">=3.13"
# ///
"""Recover hyprwhspr AudioCapture.audio_data (list of float32[1024] numpy chunks @16 kHz)
from a gcore dump of a deadlocked hyprwhspr process (2026-09-30).

Method: find PyTypeObjects for 'numpy.ndarray' and 'list' by tp_name, then find lists whose
items are all 1-D float32[1024] ndarrays, and concatenate their data buffers in list order.
Layouts (CPython 3.14, GIL build, x86-64):
  PyObject      : refcnt@0 type@8
  PyTypeObject  : tp_name@24
  PyListObject  : ob_size@16 ob_item@24
  PyArrayObject : data@16 nd@24(int) dimensions@32 strides@40
"""
import sys, wave, json, datetime
import numpy as np
from elftools.elf.elffile import ELFFile

core = sys.argv[1]; outdir = sys.argv[2]
f = open(core, "rb"); elf = ELFFile(f)
segs = [(s["p_vaddr"], s["p_filesz"], s["p_offset"]) for s in elf.iter_segments() if s["p_type"] == "PT_LOAD" and s["p_filesz"]]

def read(addr, n):
    for va, sz, off in segs:
        if va <= addr and addr + n <= va + sz:
            f.seek(off + addr - va); return f.read(n)
    return None
def u64(addr):
    b = read(addr, 8); return None if b is None else int.from_bytes(b, "little")
def i32(addr):
    b = read(addr, 4); return None if b is None else int.from_bytes(b, "little", signed=True)

# load every segment once as a uint64 view (dump is ~1.3 GB sparse)
mem = []
for va, sz, off in segs:
    f.seek(off); data = f.read(sz)
    mem.append((va, data, np.frombuffer(data[: len(data) - len(data) % 8], dtype="<u8")))

def find_all(needle):
    hits = []
    for va, data, _ in mem:
        i = data.find(needle)
        while i != -1:
            hits.append(va + i); i = data.find(needle, i + 1)
    return hits

def words_in(values):
    """addresses of 8-aligned words whose value is in `values`"""
    vals = np.array(sorted(values), dtype="<u8"); out = []
    for va, _, arr in mem:
        for idx in np.nonzero(np.isin(arr, vals))[0]:
            out.append((va + int(idx) * 8, int(arr[idx])))
    return out

# gcore omits read-only file mappings, so type-name strings are resolved from the .so on disk
maps = []
for line in open(sys.argv[3] if len(sys.argv) > 3 else outdir + "/maps.txt"):
    parts = line.split()
    if len(parts) >= 6:
        lo, hi = (int(x, 16) for x in parts[0].split("-"))
        maps.append((lo, hi, int(parts[2], 16), parts[5]))

def file_string_vaddrs(path_part, name):
    out = []
    for lo, hi, foff, path in maps:
        if path_part not in path: continue
        blob = open(path, "rb").read(); needle = name.encode() + b"\0"; i = blob.find(needle)
        while i != -1:
            if foff <= i < foff + (hi - lo): out.append(lo + i - foff)
            i = blob.find(needle, i + 1)
    return out

def type_addrs_via_file(path_part, name):
    return {w - 24 for w, _ in words_in(file_string_vaddrs(path_part, name))}

def lib_base(path_part):
    return min(lo - foff for lo, hi, foff, path in maps if path_part in path)

nd_types = type_addrs_via_file("_multiarray_umath", "numpy.ndarray")
list_types = {lib_base("libpython3.14") + 0x596720}  # nm -D libpython3.14.so.1.0 | grep PyList_Type
print("ndarray type candidates:", [hex(x) for x in nd_types], "list type:", [hex(x) for x in list_types], flush=True)

def is_chunk(obj):
    if u64(obj + 8) not in nd_types: return False
    if i32(obj + 24) != 1: return False
    dims = u64(obj + 32); st = u64(obj + 40)
    return dims is not None and u64(dims) == 1024 and st is not None and u64(st) == 4

# candidate lists: objects whose type is a list type, big ob_size
lists = []
for w, _ in words_in(list_types):
    obj = w - 8; n = u64(obj + 16); items = u64(obj + 24)
    if n and 20 <= n <= 200000 and items:
        first = u64(items)
        if first and is_chunk(first):
            lists.append((obj, n, items))
print("chunk lists found:", [(hex(o), n) for o, n, _ in lists])

report = []
for k, (obj, n, items) in enumerate(sorted(lists, key=lambda x: -x[1])):
    chunks, bad = [], 0
    for i in range(n):
        p = u64(items + 8 * i)
        if p is None or not is_chunk(p): bad += 1; continue
        b = read(u64(p + 16), 4096)
        if b is None: bad += 1; continue
        chunks.append(np.frombuffer(b, dtype="<f4"))
    a = np.concatenate(chunks)
    pcm = (np.clip(a, -1, 1) * 32767).astype("<i2")
    path = f"{outdir}/recovered_list{k}_{n}chunks.wav"
    with wave.open(path, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000); w.writeframes(pcm.tobytes())
    rms = float(np.sqrt(np.mean(a**2)))
    report.append(dict(list=hex(obj), chunks=n, bad=bad, seconds=round(len(a)/16000, 1), rms=round(rms, 4), wav=path))
    print(report[-1])
json.dump(dict(core=core, created=datetime.datetime.now().isoformat(), results=report), open(f"{outdir}/extract_report.json", "w"), indent=1)

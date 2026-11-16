"""Rebuild an official Windhawk NSIS 3 installer with some of its files replaced.

The result is a normal Windhawk setup that installs the replacement files directly:
pages, service, permissions, shortcuts and uninstaller all come from the official
installer's own script, which is left untouched. Only the stored file data and the
data offsets in the script header change. Works on both the solid (windhawk_setup.exe)
and the non-solid (windhawk_setup_offline.exe) LZMA builds.

usage: nsis_repack.py <official_setup.exe> <out.exe> <new_dir> <official_dir>

Each file in <new_dir> replaces the one installer file that has the same name AND
exactly the contents of the same-named file in <official_dir>, so a same-named file
elsewhere in the installer (the UI's 32-bit windhawk-core.dll) is left alone.
The output is not code-signed: the official signature does not cover the new data.
"""
import hashlib
import lzma
import os
import struct
import sys
import zlib

EW_CREATEDIR = 11
EW_EXTRACTFILE = 20
EW_WRITEUNINSTALLER = 62
NSIS_DICT = 1 << 23  # the official installers use an 8 MiB LZMA dictionary


def lzma_filters(props_byte, dict_size):
    lc, rem = props_byte % 9, props_byte // 9
    return [{"id": lzma.FILTER_LZMA1, "dict_size": dict_size, "lc": lc, "lp": rem % 5, "pb": rem // 5}]


def decompress(payload):
    """An NSIS LZMA stream: props byte, dictionary size, raw LZMA1 data."""
    props, dict_size = payload[0], struct.unpack_from("<I", payload, 1)[0]
    d = lzma.LZMADecompressor(format=lzma.FORMAT_RAW, filters=lzma_filters(props, dict_size))
    return d.decompress(payload[5:])


def compress(data):
    f = {"id": lzma.FILTER_LZMA1, "dict_size": NSIS_DICT, "lc": 3, "lp": 0, "pb": 2,
         "preset": 9 | lzma.PRESET_EXTREME}
    # liblzma ends a raw LZMA1 stream with an end marker, as makensis does.
    return bytes([0x5D]) + struct.pack("<I", NSIS_DICT) + lzma.compress(data, format=lzma.FORMAT_RAW, filters=[f])


def read_block(buf, pos):
    n = struct.unpack_from("<I", buf, pos)[0]
    size = n & 0x7FFFFFFF
    return bool(n & 0x80000000), buf[pos + 4: pos + 4 + size], pos + 4 + size


def pack_block(compressed, payload):
    return struct.pack("<I", len(payload) | (0x80000000 if compressed else 0)) + payload


def nsis_string(header, strings_off, idx):
    """Plain text of an NSIS 3 Unicode string; variables etc. are shown as {}."""
    p = strings_off + idx * 2
    out = []
    while True:
        c = struct.unpack_from("<H", header, p)[0]
        p += 2
        if c == 0:
            return "".join(out)
        if 1 <= c <= 4:  # NS_LANG/SHELL/VAR/SKIP code, followed by one parameter
            p += 2
            out.append("{}")
            continue
        out.append(chr(c))


def main():
    if len(sys.argv) != 5:
        sys.exit(__doc__)
    src, dst, new_dir, official_dir = sys.argv[1:]
    raw = open(src, "rb").read()
    replacements, originals = {}, {}
    for name in sorted(os.listdir(new_dir)):
        replacements[name.lower()] = open(os.path.join(new_dir, name), "rb").read()
        originals[name.lower()] = hashlib.sha256(open(os.path.join(official_dir, name), "rb").read()).hexdigest()

    pe = struct.unpack_from("<I", raw, 0x3C)[0]
    opt = pe + 24
    magic = struct.unpack_from("<H", raw, opt)[0]
    secdir = opt + (96 if magic == 0x10B else 112) + 4 * 8  # IMAGE_DIRECTORY_ENTRY_SECURITY

    # firstheader: flags, 0xDEADBEEF, "NullsoftInst", length_of_header, length_of_all_following_data
    pos = -1
    while True:
        pos = raw.find(b"\xef\xbe\xad\xdeNullsoftInst", pos + 1)
        if pos < 0:
            sys.exit("not an NSIS installer")
        if (pos - 4) % 512 == 0:
            break
    fh = pos - 4
    flags, _, _, _, _, hdr_len, all_len = struct.unpack_from("<7I", raw, fh)
    data_end = fh + all_len
    # NSIS checks a CRC32 of everything from offset 512 up to the CRC itself.
    if zlib.crc32(raw[512: data_end - 4]) != struct.unpack_from("<I", raw, data_end - 4)[0]:
        sys.exit("the source installer fails its own CRC check")

    # Solid: one LZMA stream holding [len][header] then the [len][file] blocks.
    # Non-solid: [len|0x80000000][LZMA header], then a [len|flag][LZMA file] block per file.
    data = raw[fh + 28: data_end - 4]
    solid = data[0] == 0x5D and struct.unpack_from("<I", data, 1)[0] == NSIS_DICT
    if solid:
        buf = decompress(data)
        p, limit = 0, len(buf)
    else:
        buf, p, limit = raw, fh + 28, data_end - 4
    comp, payload, p = read_block(buf, p)
    if not solid and not comp:
        sys.exit("unexpected uncompressed header")
    header = bytearray(payload if solid else decompress(payload))
    if len(header) != hdr_len:
        sys.exit(f"header size {len(header)} != {hdr_len}")
    data_base = p
    blocks = []  # (offset, compressed, payload)
    while p < limit:
        c, pl, nxt = read_block(buf, p)
        blocks.append((p - data_base, c, pl))
        p = nxt
    if p != limit:
        sys.exit("the data blocks do not line up with the end of the data")
    block_at = {off: i for i, (off, _, _) in enumerate(blocks)}

    # header: flags, then 8 (offset, count) block headers; entries are 7 ints each
    ent_off, ent_num = struct.unpack_from("<II", header, 4 + 2 * 8)
    str_off = struct.unpack_from("<I", header, 4 + 3 * 8)[0]
    entries = [list(struct.unpack_from("<7i", header, ent_off + i * 28)) for i in range(ent_num)]

    refs = []      # (entry, parameter) that holds a data offset
    targets = {}   # block -> {file name: output directory}
    outdir = "?"
    for i, e in enumerate(entries):
        if e[0] == EW_CREATEDIR and e[2]:  # SetOutPath
            outdir = nsis_string(header, str_off, e[1])
        if e[0] == EW_EXTRACTFILE:
            parm = 3  # op, overwrite flag, name, data offset, file time...
        elif e[0] == EW_WRITEUNINSTALLER:
            parm = 2  # op, name, data offset of the uninstaller, icon size, ...
        else:
            continue
        if e[parm] not in block_at:
            sys.exit(f"entry {i} (op {e[0]}) points at {e[parm]}, not at a block: unknown NSIS opcode table")
        refs.append((i, parm))
        if e[0] == EW_EXTRACTFILE:
            targets.setdefault(block_at[e[parm]], {})[nsis_string(header, str_off, e[2]).lower()] = outdir
    print(f"{'solid' if solid else 'non-solid'} installer: {len(blocks)} data blocks, {len(refs)} references")

    new_payload, found = {}, {}
    for bi, names in targets.items():
        for name in [n for n in names if n in replacements]:
            c, pl = blocks[bi][1], blocks[bi][2]
            old = decompress(pl) if c else pl
            if hashlib.sha256(old).hexdigest() != originals[name]:
                print(f"  left alone: {names[name]}\\{name} (a different file with that name)")
                continue
            if len(names) != 1:
                sys.exit(f"block {bi} is shared by {sorted(names)}; refusing to replace it")
            if name in found:
                sys.exit(f"{name} matches more than one block")
            found[name] = bi
            print(f"  replaced:   {names[name]}\\{name}  {len(old):,} -> {len(replacements[name]):,} bytes")
            new_payload[bi] = replacements[name] if solid else compress(replacements[name])
    missing = set(replacements) - set(found)
    if missing:
        sys.exit(f"no file with the official contents of: {sorted(missing)}")

    # lay the blocks out again, in the same order, and remap every data offset
    out_blocks = bytearray()
    remap = {}
    for bi, (off, c, pl) in enumerate(blocks):
        remap[off] = len(out_blocks)
        out_blocks += pack_block(not solid, new_payload[bi]) if bi in new_payload else pack_block(c, pl)
    for i, parm in refs:
        entries[i][parm] = remap[entries[i][parm]]
        struct.pack_into("<7i", header, ent_off + i * 28, *entries[i])

    if solid:
        body = compress(pack_block(False, bytes(header)) + bytes(out_blocks))
    else:
        body = pack_block(True, compress(bytes(header))) + bytes(out_blocks)

    exehead = bytearray(raw[:fh])
    struct.pack_into("<II", exehead, secdir, 0, 0)  # the signature is dropped
    struct.pack_into("<I", exehead, opt + 64, 0)    # PE checksum
    first = struct.pack("<7I", flags, 0xDEADBEEF, *struct.unpack("<3I", b"NullsoftInst"), hdr_len, 28 + len(body) + 4)
    out = bytes(exehead) + first + body
    out += struct.pack("<I", zlib.crc32(out[512:]))
    with open(dst, "wb") as f:
        f.write(out)
    print(f"wrote {dst} ({len(out):,} bytes)")


if __name__ == "__main__":
    main()

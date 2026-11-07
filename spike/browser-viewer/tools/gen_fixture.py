#!/usr/bin/env python3
"""Generate a synthetic, fully original T-pose "mannequin" GLB.

No third-party art: the geometry is boxes and the texture is a procedural
pattern, so the output is safe to redistribute (dedicated to the public domain
by the author of this script, same terms as the repo).

    python3 -I gen_fixture.py OUT.glb [--grid N]

--grid N subdivides every box face into an N x N quad grid, which is how the
spike reaches representative triangle counts (N=4 ~3k tris, N=55 ~500k tris).
Single mesh, single primitive, one PBR material (base-colour texture +
metallic/roughness factors), no skin, no animation: it is the "freshly
generated" shape that Rig/Bind expects as input.
"""
import json, struct, sys, zlib, argparse

# (name, (x0,x1), (y0,y1), (z0,z1)) in metres, +Y up, character faces +Z.
BOXES = [
    ("pelvis",   (-0.18, 0.18), (0.85, 1.00), (-0.10, 0.10)),
    ("torso",    (-0.20, 0.20), (1.00, 1.46), (-0.11, 0.11)),
    ("neck",     (-0.05, 0.05), (1.46, 1.56), (-0.05, 0.05)),
    ("head",     (-0.11, 0.11), (1.56, 1.80), (-0.12, 0.12)),
    ("arm_r",    (0.20, 0.62),  (1.34, 1.44), (-0.05, 0.05)),
    ("fore_r",   (0.62, 0.98),  (1.35, 1.43), (-0.04, 0.04)),
    ("hand_r",   (0.98, 1.10),  (1.34, 1.44), (-0.05, 0.05)),
    ("arm_l",    (-0.62, -0.20), (1.34, 1.44), (-0.05, 0.05)),
    ("fore_l",   (-0.98, -0.62), (1.35, 1.43), (-0.04, 0.04)),
    ("hand_l",   (-1.10, -0.98), (1.34, 1.44), (-0.05, 0.05)),
    ("thigh_r",  (0.03, 0.19), (0.48, 0.86), (-0.08, 0.08)),
    ("shin_r",   (0.04, 0.17), (0.08, 0.48), (-0.06, 0.06)),
    ("foot_r",   (0.03, 0.18), (0.00, 0.08), (-0.07, 0.20)),
    ("thigh_l",  (-0.19, -0.03), (0.48, 0.86), (-0.08, 0.08)),
    ("shin_l",   (-0.17, -0.04), (0.08, 0.48), (-0.06, 0.06)),
    ("foot_l",   (-0.18, -0.03), (0.00, 0.08), (-0.07, 0.20)),
]

def face_grid(origin, du, dv, normal, n, pos, nor, uv, idx, flip, tile):
    base = len(pos)
    for j in range(n + 1):
        for i in range(n + 1):
            u, v = i / n, j / n
            pos.append([origin[k] + du[k] * u + dv[k] * v for k in range(3)])
            nor.append(list(normal))
            # each box gets a cell of a 4x4 atlas so parts are distinguishable
            cx, cy = tile % 4, tile // 4
            uv.append([(cx + 0.02 + 0.96 * u) / 4, (cy + 0.02 + 0.96 * v) / 4])
    for j in range(n):
        for i in range(n):
            a = base + j * (n + 1) + i
            b, c, d = a + 1, a + (n + 1), a + (n + 1) + 1
            idx.extend([a, d, b, a, c, d] if not flip else [a, b, d, a, d, c])

def build(n):
    pos, nor, uv, idx = [], [], [], []
    for tile, (_, (x0, x1), (y0, y1), (z0, z1)) in enumerate(BOXES):
        dx, dy, dz = x1 - x0, y1 - y0, z1 - z0
        # (origin, du, dv, normal, flip) with outward normals / CCW winding
        faces = [
            ((x0, y0, z1), (dx, 0, 0), (0, dy, 0), (0, 0, 1), False),   # +Z
            ((x1, y0, z0), (-dx, 0, 0), (0, dy, 0), (0, 0, -1), False), # -Z
            ((x1, y0, z1), (0, 0, -dz), (0, dy, 0), (1, 0, 0), False),  # +X
            ((x0, y0, z0), (0, 0, dz), (0, dy, 0), (-1, 0, 0), False),  # -X
            ((x0, y1, z1), (dx, 0, 0), (0, 0, -dz), (0, 1, 0), False),  # +Y
            ((x0, y0, z0), (dx, 0, 0), (0, 0, dz), (0, -1, 0), False),  # -Y
        ]
        for o, du, dv, nm, fl in faces:
            face_grid(o, du, dv, nm, n, pos, nor, uv, idx, fl, tile)
    return pos, nor, uv, idx

def png(w, h, rgb_fn):
    raw = b"".join(b"\x00" + b"".join(bytes(rgb_fn(x, y)) for x in range(w)) for y in range(h))
    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))

def texture():
    palette = [(200, 70, 60), (60, 140, 200), (90, 180, 90), (230, 190, 60)]
    def px(x, y):
        tile = (x // 64) + 4 * (y // 64)
        base = palette[tile % 4 if (tile // 4) % 2 == 0 else (tile + 1) % 4]
        check = ((x // 8) + (y // 8)) & 1
        k = 1.0 if check else 0.78
        return tuple(int(c * k) for c in base)
    return png(256, 256, px)

def pad(b, fill):
    return b + fill * (-len(b) % 4)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--grid", type=int, default=4)
    a = ap.parse_args()
    pos, nor, uv, idx = build(a.grid)
    pb = b"".join(struct.pack("<3f", *p) for p in pos)
    nb = b"".join(struct.pack("<3f", *p) for p in nor)
    ub = b"".join(struct.pack("<2f", *p) for p in uv)
    ib = b"".join(struct.pack("<I", i) for i in idx)
    img = texture()
    blob = b""; views = []
    for data, target in ((pb, 34962), (nb, 34962), (ub, 34962), (ib, 34963), (img, None)):
        blob = pad(blob, b"\x00")
        v = {"buffer": 0, "byteOffset": len(blob), "byteLength": len(data)}
        if target: v["target"] = target
        views.append(v); blob += data
    blob = pad(blob, b"\x00")
    mn = [min(p[k] for p in pos) for k in range(3)]
    mx = [max(p[k] for p in pos) for k in range(3)]
    gltf = {
        "asset": {"version": "2.0", "generator": "asset-tap browser-viewer spike gen_fixture.py"},
        "scene": 0, "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "Mannequin"}],
        "meshes": [{"name": "Mannequin", "primitives": [{
            "attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
            "indices": 3, "material": 0, "mode": 4}]}],
        "materials": [{"name": "MannequinPBR", "pbrMetallicRoughness": {
            "baseColorTexture": {"index": 0}, "metallicFactor": 0.6, "roughnessFactor": 0.4}}],
        "textures": [{"source": 0, "sampler": 0}],
        "samplers": [{"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}],
        "images": [{"bufferView": 4, "mimeType": "image/png"}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": len(pos), "type": "VEC3", "min": mn, "max": mx},
            {"bufferView": 1, "componentType": 5126, "count": len(nor), "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": len(uv), "type": "VEC2"},
            {"bufferView": 3, "componentType": 5125, "count": len(idx), "type": "SCALAR"},
        ],
        "bufferViews": views, "buffers": [{"byteLength": len(blob)}],
    }
    js = pad(json.dumps(gltf, separators=(",", ":")).encode(), b" ")
    total = 12 + 8 + len(js) + 8 + len(blob)
    with open(a.out, "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, total))
        f.write(struct.pack("<I4s", len(js), b"JSON") + js)
        f.write(struct.pack("<I4s", len(blob), b"BIN\x00") + blob)
    print(f"{a.out}: grid={a.grid} verts={len(pos)} tris={len(idx)//3} bytes={total}")

main()

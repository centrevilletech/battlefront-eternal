"""Column-major 4x4 transforms for munged XFRM data."""

import math


def mat4(m9_pos):
    r = m9_pos[:9]
    px, py, pz = m9_pos[9:12]
    # Column-major 3x3 plus translation, into a column-major 4x4.
    return [
        r[0], r[1], r[2], 0.0,
        r[3], r[4], r[5], 0.0,
        r[6], r[7], r[8], 0.0,
        px, py, pz, 1.0,
    ]


IDENTITY = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]


def mul(a, b):
    out = [0.0] * 16
    for col in range(4):
        for row in range(4):
            out[col * 4 + row] = sum(a[k * 4 + row] * b[col * 4 + k] for k in range(4))
    return out


def transform_point(m, p):
    x, y, z = p
    return (
        m[0] * x + m[4] * y + m[8] * z + m[12],
        m[1] * x + m[5] * y + m[9] * z + m[13],
        m[2] * x + m[6] * y + m[10] * z + m[14],
    )


def transform_dir(m, p):
    x, y, z = p
    n = (
        m[0] * x + m[4] * y + m[8] * z,
        m[1] * x + m[5] * y + m[9] * z,
        m[2] * x + m[6] * y + m[10] * z,
    )
    length = math.sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) or 1.0
    return (n[0] / length, n[1] / length, n[2] / length)

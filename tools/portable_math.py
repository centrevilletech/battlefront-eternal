"""sin, cos and atan2 that give the same bits on every platform.

What the tools write is fingerprinted (src/runtime/content-hash.js): a
player's preparation must match the server's to the last bit, whatever
computer either ran on. Python's math.sin, cos and atan2 (and x ** y) come
from the platform's C library, and their last bits differ: the Windows
runtime and glibc disagree on spawn paths' yaws and the sun's direction,
so a player who prepared on Windows could only spectate a Linux server.
These compute in decimal arithmetic, the same everywhere, to 50 digits,
and round once to the nearest double, which is what an exact library
returns. Square roots need nothing of the kind: math.sqrt is exact on
every platform (use it, not ** 0.5).
"""

import math
from decimal import ROUND_HALF_EVEN, Decimal, localcontext

DIGITS = 50
PI = Decimal("3.14159265358979323846264338327950288419716939937510582097494459230781640628620899")


def _sin_series(r):
    total, term, square, n = r, r, r * r, 1
    while True:
        term = -term * square / ((n + 1) * (n + 2))
        n += 2
        following = total + term
        if following == total:
            return total
        total = following


def _cos_series(r):
    total, term, square, n = Decimal(1), Decimal(1), r * r, 0
    while True:
        term = -term * square / ((n + 1) * (n + 2))
        n += 2
        following = total + term
        if following == total:
            return total
        total = following


def _quadrant(x):
    """x as k quarter turns and the rest, at most an eighth of a turn: (k mod 4, rest)."""
    quarter = PI / 2
    k = (x / quarter).to_integral_value(rounding=ROUND_HALF_EVEN)
    return int(k) % 4, x - k * quarter


def _sin_cos(x, cosine):
    with localcontext() as context:
        context.prec = DIGITS
        k, r = _quadrant(Decimal(x))
        k = (k + cosine) % 4
        value = _sin_series(r) if k % 2 == 0 else _cos_series(r)
        return float(-value if k >= 2 else value)


def sin(x):
    # Zero (and its sign), infinities and NaN: exact in every library.
    if x == 0 or not math.isfinite(x):
        return math.sin(x)
    return _sin_cos(x, 0)


def cos(x):
    if x == 0 or not math.isfinite(x):
        return math.cos(x)
    return _sin_cos(x, 1)


def _atan(t):
    """atan of a Decimal: halved (atan t = 2 atan(t / (1 + sqrt(1 + t^2))))
    until small, then the series."""
    halvings = 0
    while abs(t) > Decimal("0.1"):
        t = t / (1 + (1 + t * t).sqrt())
        halvings += 1
    total, term, square, n = t, t, t * t, 1
    while True:
        term = -term * square
        n += 2
        following = total + term / n
        if following == total:
            break
        total = following
    return total * (1 << halvings)


def atan2(y, x):
    # A zero or infinite side: exact values (C99 Annex F) every library returns.
    if y == 0 or x == 0 or not (math.isfinite(x) and math.isfinite(y)):
        return math.atan2(y, x)
    with localcontext() as context:
        context.prec = DIGITS
        dy, dx = Decimal(y), Decimal(x)
        if abs(dx) >= abs(dy):
            angle = _atan(dy / dx)
            if dx < 0:
                angle = angle + PI if dy > 0 else angle - PI
        else:
            angle = (PI / 2 if dy > 0 else -PI / 2) - _atan(dx / dy)
        return float(angle)

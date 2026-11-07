#!/usr/bin/env python3
"""Run the window gate on X11, focusing only the child application's window.

A bare virtual X server may have no window manager to activate a mapped window.
Use its process property instead of a title, so concurrent applications are safe.
"""
import ctypes as c
import subprocess
import sys
import time

x = c.CDLL('libX11.so.6')
x.XOpenDisplay.argtypes = [c.c_char_p]
x.XOpenDisplay.restype = c.c_void_p
x.XDefaultRootWindow.argtypes = [c.c_void_p]
x.XDefaultRootWindow.restype = c.c_ulong
x.XInternAtom.argtypes = [c.c_void_p, c.c_char_p, c.c_int]
x.XInternAtom.restype = c.c_ulong
x.XQueryTree.argtypes = [c.c_void_p, c.c_ulong, c.POINTER(c.c_ulong), c.POINTER(c.c_ulong), c.POINTER(c.POINTER(c.c_ulong)), c.POINTER(c.c_uint)]
x.XGetWindowProperty.argtypes = [c.c_void_p, c.c_ulong, c.c_ulong, c.c_long, c.c_long, c.c_int, c.c_ulong,
    c.POINTER(c.c_ulong), c.POINTER(c.c_int), c.POINTER(c.c_ulong), c.POINTER(c.c_ulong), c.POINTER(c.POINTER(c.c_ubyte))]
# Xlib exposes this structure through its stable C API; use named fields so
# map-state validation does not depend on guessed native byte offsets.
class Attributes(c.Structure):
    _fields_ = [(name, kind) for kind, names in [
        (c.c_int, 'x y width height border_width depth'),
        (c.c_void_p, 'visual'), (c.c_ulong, 'root'),
        (c.c_int, 'window_class bit_gravity win_gravity backing_store'),
        (c.c_ulong, 'backing_planes backing_pixel'), (c.c_int, 'save_under'),
        (c.c_ulong, 'colormap'), (c.c_int, 'map_installed map_state'),
        (c.c_long, 'all_event_masks your_event_mask do_not_propagate_mask'),
        (c.c_int, 'override_redirect'), (c.c_void_p, 'screen')
    ] for name in names.split()]
x.XGetWindowAttributes.argtypes = [c.c_void_p, c.c_ulong, c.POINTER(Attributes)]
x.XSetInputFocus.argtypes = [c.c_void_p, c.c_ulong, c.c_int, c.c_ulong]
x.XSync.argtypes = [c.c_void_p, c.c_int]
x.XFree.argtypes = [c.c_void_p]
x.XCloseDisplay.argtypes = [c.c_void_p]
display = x.XOpenDisplay(None)
if not display:
    raise SystemExit('window gate requires an X11 display')
root = x.XDefaultRootWindow(display)
pid_atom = x.XInternAtom(display, b'_NET_WM_PID', 0)


def children(window):
    parent, owner = c.c_ulong(), c.c_ulong()
    values, count = c.POINTER(c.c_ulong)(), c.c_uint()
    if not x.XQueryTree(display, window, c.byref(owner), c.byref(parent), c.byref(values), c.byref(count)):
        return []
    result = [values[i] for i in range(count.value)]
    if values:
        x.XFree(values)
    return result


def pid(window):
    actual, size, count, after = c.c_ulong(), c.c_int(), c.c_ulong(), c.c_ulong()
    data = c.POINTER(c.c_ubyte)()
    x.XGetWindowProperty(display, window, pid_atom, 0, 1, 0, 6,
        c.byref(actual), c.byref(size), c.byref(count), c.byref(after), c.byref(data))
    value = c.cast(data, c.POINTER(c.c_ulong))[0] if data and count.value and size.value == 32 else 0
    if data:
        x.XFree(data)
    return value


process = subprocess.Popen(sys.argv[1:])
try:
    # Registration may compile many shaders. Wait for the mapped child instead
    # of assuming a fixed startup delay or changing another process's focus.
    deadline = time.monotonic() + 120
    focused = False
    while process.poll() is None and time.monotonic() < deadline:
        pending = children(root)
        while pending:
            window = pending.pop()
            attributes = Attributes()
            if pid(window) == process.pid and x.XGetWindowAttributes(display, window, c.byref(attributes)) and attributes.map_state == 2:
                x.XSetInputFocus(display, window, 1, 0)
                x.XSync(display, 0)
                focused = True
                break
            pending.extend(children(window))
        if focused:
            break
        time.sleep(0.05)
    if not focused:
        raise RuntimeError('native child window did not appear')
    # The child itself owns the twenty-second interval and orderly shutdown.
    raise SystemExit(process.wait(timeout=150))
finally:
    if process.poll() is None:
        process.terminate()
        process.wait(timeout=10)
    x.XCloseDisplay(display)

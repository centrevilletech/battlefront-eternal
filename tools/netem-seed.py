#!/usr/bin/env python3
"""Set/read back netem's kernel PRNG seed when older tc lacks a seed option.

This helper refuses the host network namespace. It preserves the configured
netem options and changes only TCA_NETEM_PRNG_SEED through rtnetlink.
"""
import argparse
import os
import socket
import struct


def attributes(data):
    offset = 0
    while offset + 4 <= len(data):
        length, kind = struct.unpack_from('=HH', data, offset)
        if length < 4 or offset + length > len(data):
            raise ValueError('Malformed netlink attribute')
        yield kind, data[offset + 4:offset + length]
        offset += (length + 3) & ~3


def attribute(kind, value):
    length = len(value) + 4
    return struct.pack('=HH', length, kind) + value + bytes((-length) % 4)


def seeded_options(options, seed):
    if len(options) < 24:
        raise ValueError('Missing netem queue options')
    return options[:24] + b''.join(attribute(kind, value) for kind, value in attributes(options[24:])
                                  if kind & 0x3fff != 14) + attribute(14, struct.pack('=Q', seed))


def set_seed(seed):
    if os.readlink('/proc/self/ns/net') == os.readlink('/proc/1/ns/net'):
        raise RuntimeError('Refusing to alter the host network namespace')
    if not 0 < seed < 2**64:
        raise ValueError('Seed must be an unsigned nonzero 64-bit integer')
    with socket.socket(socket.AF_NETLINK, socket.SOCK_RAW, socket.NETLINK_ROUTE) as sock:
        sock.bind((0, 0)); sock.settimeout(3)
        sequence = 0

        def request(kind, flags, payload):
            nonlocal sequence
            sequence += 1
            sock.send(struct.pack('=IHHII', len(payload) + 16, kind, flags, sequence, 0) + payload)
            results = []
            while True:
                data = sock.recv(262144)
                offset = 0
                while offset + 16 <= len(data):
                    length, response, _, reply_sequence, _ = struct.unpack_from('=IHHII', data, offset)
                    if length < 16 or offset + length > len(data):
                        raise ValueError('Malformed netlink message')
                    value = data[offset + 16:offset + length]
                    offset += (length + 3) & ~3
                    if reply_sequence != sequence:
                        continue
                    if response == 2:
                        error, = struct.unpack_from('=i', value)
                        if error:
                            raise OSError(-error, os.strerror(-error))
                        return results
                    if response == 3:
                        return results
                    results.append(value)

        query = struct.pack('=BBHiIII', 0, 0, 0, socket.if_nametoindex('lo'), 0, 0, 0)

        def find():
            for payload in request(38, 1 | 0x300, query):
                attrs = dict(attributes(payload[20:]))
                if struct.unpack_from('=i', payload, 4)[0] == socket.if_nametoindex('lo') and attrs.get(1, b'').rstrip(b'\0') == b'netem':
                    return payload[:20], attrs[2]
            raise RuntimeError('No netem queue exists on namespace loopback')

        header, options = find()
        request(36, 1 | 4 | 0x100, header + attribute(1, b'netem\0') + attribute(2, seeded_options(options, seed)))
        _, confirmed = find()
        values = dict(attributes(confirmed[24:]))
        if values.get(14) != struct.pack('=Q', seed):
            raise RuntimeError('Kernel did not confirm the requested netem PRNG seed')
    return seed


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('seed', type=int)
    print(set_seed(parser.parse_args().seed))

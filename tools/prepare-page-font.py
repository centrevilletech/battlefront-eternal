#!/usr/bin/env python3
"""Instantiate the shipped OFL Saira at the page's body, title and logo CSS weights.

Requires fontTools and Brotli. The WOFF2 source and its license live beside
these generated TTF files; generation never needs installed system fonts.
"""
from pathlib import Path
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

root = Path(__file__).resolve().parents[1] / "rust/bfe-client/assets/fonts"
source = TTFont(root / "saira-latin.woff2", recalcTimestamp=False)
for weight, width, name in ((700,100,"700"),(800,118,"800"),(900,125,"900"),(800,100,"800-normal"),(400,100,"400"),(600,100,"600")):
    font = instantiateVariableFont(source, {"wght": weight, "wdth": width}, inplace=False)
    font.flavor = None
    font.save(root / f"saira-{name}.ttf")

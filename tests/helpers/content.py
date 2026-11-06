"""Game-file root for optional Python extraction audits."""
import os
from pathlib import Path


def content_path(relative=''):
    root = os.environ.get('BFE_CONTENT_DIR')
    base = Path(root).resolve() if root else Path(__file__).resolve().parents[2] / 'public'
    return base / relative

#!/usr/bin/env python3
"""Generate the Rust crate inventory in THIRD-PARTY-NOTICES.md from cargo metadata.

Why this script exists:
Battlefront Eternal is distributed under the GNU General Public License v3.0
only (`GPL-3.0-only`). Native desktop (`bfe-client`) and dedicated server
(`bfeserver`) releases link the third-party Rust crates resolved in
`rust/Cargo.lock`. This script queries `cargo metadata --locked` for the
workspace at `rust/Cargo.toml` across the shipped native targets
(`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-pc-windows-msvc`), verifies that every resolved third-party crate
declares a permissive or GPL-3.0-compatible SPDX license expression, and writes
the complete crate table (crate, version, license, authors, repository) into
`THIRD-PARTY-NOTICES.md` between HTML comment markers while preserving all
non-crate sections (game content notice, fonts, translated algorithms, bundled
media tools, and full license texts).

Marker contract:
- All text outside `<!-- BEGIN CARGO METADATA CRATES -->` and
  `<!-- END CARGO METADATA CRATES -->` is owned by the repository's manual
  legal notices (Saira font, Chromium/OpenAL/V8/Arm/three.js translated
  components, FFmpeg/KTX/CPython desktop tools, and verbatim license texts)
  and is preserved byte-for-byte across regenerations.
- If the markers are not yet present in `THIRD-PARTY-NOTICES.md`, the generated
  block is inserted immediately before `## Tools bundled in desktop packages`.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import json
import re
import subprocess
import sys

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_MANIFEST = REPO_ROOT / "rust" / "Cargo.toml"
DEFAULT_OUTPUT = REPO_ROOT / "THIRD-PARTY-NOTICES.md"

BEGIN_MARKER = "<!-- BEGIN CARGO METADATA CRATES -->"
END_MARKER = "<!-- END CARGO METADATA CRATES -->"
INSERT_BEFORE_HEADING = "## Tools bundled in desktop packages"

SHIPPED_TARGETS = (
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
)

# Permissive and GPL-3.0-compatible SPDX license identifiers accepted for
# third-party crates linked into Battlefront Eternal (`GPL-3.0-only`).
# Note: `Apache-2.0 WITH LLVM-exception` is normalized before token splitting
# so the `WITH` exception stays attached to `Apache-2.0`.
ALLOWED_LICENSES = frozenset(
    {
        "0BSD",
        "Apache-2.0",
        "Apache-2.0 WITH LLVM-exception",
        "BSD-1-Clause",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "BSL-1.0",
        "CC0-1.0",
        "CDLA-Permissive-2.0",
        "GPL-3.0-only",
        "GPL-3.0-or-later",
        "ISC",
        "LGPL-2.0-or-later",
        "LGPL-2.1-only",
        "LGPL-2.1-or-later",
        "LGPL-3.0-only",
        "LGPL-3.0-or-later",
        "MIT",
        "MIT-0",
        "MPL-2.0",
        "Unicode-3.0",
        "Unicode-DFS-2016",
        "Unlicense",
        "Zlib",
    }
)


def parse_spdx_tokens(expr: str) -> list[str]:
    """Extract atomic SPDX license identifiers from a Cargo license expression.

    Cargo allows both standard SPDX boolean operators (`OR`, `AND`, parentheses)
    and slash-separated expressions (`MIT/Apache-2.0`). `WITH` clauses
    such as `Apache-2.0 WITH LLVM-exception` are kept as a single token.
    """
    normalized = expr.strip()
    if not normalized:
        return []
    # Preserve `WITH <exception>` attached to its base license identifier before
    # splitting on whitespace/boolean operators.
    normalized = re.sub(
        r"([A-Za-z0-9.\-]+)\s+WITH\s+([A-Za-z0-9.\-]+)",
        r"\1__WITH__\2",
        normalized,
    )
    normalized = normalized.replace("(", " ").replace(")", " ").replace("/", " OR ")
    parts = re.split(r"\s+(?:OR|AND)\s+|\s+", normalized)
    tokens: list[str] = []
    for part in parts:
        cleaned = part.strip().replace("__WITH__", " WITH ")
        if cleaned and cleaned not in ("OR", "AND"):
            tokens.append(cleaned)
    return tokens


def check_license_compatibility(expr: str | None) -> tuple[bool, list[str]]:
    """Verify that a crate's SPDX license expression is non-empty and compatible.

    Returns `(is_compatible, disallowed_tokens)` where `disallowed_tokens` lists
    any unrecognized or GPL-incompatible license identifiers.
    """
    if not expr or not expr.strip():
        return False, ["<missing license>"]
    tokens = parse_spdx_tokens(expr)
    if not tokens:
        return False, ["<empty license>"]
    disallowed = [tok for tok in tokens if tok not in ALLOWED_LICENSES]
    return len(disallowed) == 0, disallowed


def escape_markdown_cell(text: str) -> str:
    """Escape pipe characters, newlines, and angle brackets for Markdown table cells.

    Author strings in Cargo manifests frequently contain `<email@domain>`;
    replacing `<` and `>` prevents Markdown renderers from treating email
    addresses as raw HTML tags while keeping the author attribution readable.
    """
    cleaned = " ".join(text.replace("\r", " ").replace("\n", " ").split())
    cleaned = cleaned.replace("|", "\\|")
    cleaned = cleaned.replace("<", "&lt;").replace(">", "&gt;")
    return cleaned


def load_cargo_metadata(manifest_path: Path, metadata_json: Path | None = None) -> dict:
    """Load `cargo metadata --locked --format-version 1` output as a dictionary."""
    if metadata_json is not None:
        return json.loads(metadata_json.read_text(encoding="utf-8"))
    filter_args = [arg for target in SHIPPED_TARGETS for arg in ("--filter-platform", target)]
    raw = subprocess.check_output(
        [
            "cargo",
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
            str(manifest_path),
            *filter_args,
        ],
        text=True,
    )
    return json.loads(raw)


def collect_third_party_crates(meta: dict) -> tuple[list[dict], list[tuple[str, str, list[str]]]]:
    """Extract sorted third-party crate records and validate their licenses.

    Workspace member crates (`meta["workspace_members"]`) and local path-only
    packages inside the repository are excluded because they are part of
    Battlefront Eternal itself rather than third-party dependencies.
    """
    workspace_members = set(meta.get("workspace_members", []))
    crates: list[dict] = []
    incompatible: list[tuple[str, str, list[str]]] = []

    for pkg in meta.get("packages", []):
        if pkg.get("id") in workspace_members:
            continue
        # Exclude any workspace-internal path dependency (`source is None`).
        if pkg.get("source") is None:
            continue

        name = pkg["name"]
        version = pkg["version"]
        license_expr = (pkg.get("license") or "").strip()
        if not license_expr and pkg.get("license_file"):
            license_expr = f"See {pkg['license_file']}"

        ok, bad_tokens = check_license_compatibility(pkg.get("license"))
        if not ok:
            incompatible.append((name, version, bad_tokens))

        authors_list = [a.strip() for a in (pkg.get("authors") or []) if a and a.strip()]
        authors_str = ", ".join(authors_list) if authors_list else "The crate authors"
        repository = (pkg.get("repository") or pkg.get("homepage") or "").strip()

        crates.append(
            {
                "name": name,
                "version": version,
                "license": license_expr,
                "authors": authors_str,
                "repository": repository,
            }
        )

    crates.sort(key=lambda c: (c["name"].lower(), c["name"], c["version"]))
    return crates, incompatible


def render_crate_section(crates: list[dict]) -> str:
    """Render the Markdown section bounded by `BEGIN_MARKER` and `END_MARKER`."""
    lines = [
        BEGIN_MARKER,
        "## Rust crates (`cargo metadata`)",
        "",
        "The following third-party Rust crates are resolved in `rust/Cargo.lock`",
        "across the shipped native targets (`x86_64-unknown-linux-gnu`,",
        "`aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`):",
        "",
        "| Crate | Version | License | Authors | Repository |",
        "| --- | --- | --- | --- | --- |",
    ]
    for crate in crates:
        name_cell = f"`{escape_markdown_cell(crate['name'])}`"
        ver_cell = f"`{escape_markdown_cell(crate['version'])}`"
        lic_cell = escape_markdown_cell(crate["license"])
        auth_cell = escape_markdown_cell(crate["authors"])
        repo = crate["repository"]
        if repo:
            safe_repo = repo.replace("|", "%7C").replace(" ", "%20")
            repo_cell = f"<{safe_repo}>"
        else:
            repo_cell = "-"
        lines.append(f"| {name_cell} | {ver_cell} | {lic_cell} | {auth_cell} | {repo_cell} |")
    lines.append(END_MARKER)
    return "\n".join(lines)


def update_notices_document(existing_text: str, crate_block: str) -> str:
    """Splice `crate_block` into `existing_text` while preserving all non-crate sections."""
    if BEGIN_MARKER in existing_text and END_MARKER in existing_text:
        start_idx = existing_text.index(BEGIN_MARKER)
        end_idx = existing_text.index(END_MARKER) + len(END_MARKER)
        return existing_text[:start_idx] + crate_block + existing_text[end_idx:]

    if INSERT_BEFORE_HEADING not in existing_text:
        raise RuntimeError(
            f"Could not find {BEGIN_MARKER!r} or {INSERT_BEFORE_HEADING!r} in THIRD-PARTY-NOTICES.md"
        )
    insert_at = existing_text.index(INSERT_BEFORE_HEADING)
    prefix = existing_text[:insert_at].rstrip("\n")
    suffix = existing_text[insert_at:]
    return f"{prefix}\n\n{crate_block}\n\n{suffix}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest-path",
        type=Path,
        default=DEFAULT_MANIFEST,
        help="Path to the Cargo workspace manifest (default: rust/Cargo.toml)",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help="Path to THIRD-PARTY-NOTICES.md (default: repository root THIRD-PARTY-NOTICES.md)",
    )
    parser.add_argument(
        "--metadata-json",
        type=Path,
        default=None,
        help="Optional pre-captured `cargo metadata` JSON file",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Verify that THIRD-PARTY-NOTICES.md matches cargo metadata without modifying it",
    )
    args = parser.parse_args()

    meta = load_cargo_metadata(args.manifest_path, args.metadata_json)
    crates, incompatible = collect_third_party_crates(meta)
    if not crates:
        print("error: no third-party crates found in cargo metadata", file=sys.stderr)
        return 1

    if incompatible:
        print("error: crates with missing or non-permissive/non-GPL-compatible licenses:", file=sys.stderr)
        for name, version, bad_tokens in incompatible:
            print(f"  - {name} {version}: {', '.join(bad_tokens)}", file=sys.stderr)
        return 1

    existing_text = args.output.read_text(encoding="utf-8")
    crate_block = render_crate_section(crates)
    updated_text = update_notices_document(existing_text, crate_block)
    # The decoder is compiled from upstream C, so Cargo metadata cannot see it.
    # Generate its full notices here alongside the resolved Rust inventory.
    vpx_start = "<!-- BEGIN LIBVPX -->"
    vpx_end = "<!-- END LIBVPX -->"
    license_dir = Path(__file__).resolve().parent / "licenses"
    vpx_block = (vpx_start + "\n## libvpx 1.17.0\n\n"
                 "The native movie decoder statically links libvpx (BSD-3-Clause).\n"
                 "Source: <https://github.com/webmproject/libvpx/tree/v1.17.0>.\n\n"
                 + "\n\n".join("```text\n" + (license_dir / ("libvpx-" + name)).read_text().rstrip() + "\n```"
                                  for name in ("LICENSE", "PATENTS"))
                 + "\n" + vpx_end)
    if vpx_start in updated_text:
        begin = updated_text.index(vpx_start)
        end = updated_text.index(vpx_end) + len(vpx_end)
        updated_text = updated_text[:begin] + vpx_block + updated_text[end:]
    else:
        updated_text = updated_text.replace(INSERT_BEFORE_HEADING, vpx_block + "\n\n" + INSERT_BEFORE_HEADING)

    if args.check:
        if updated_text != existing_text:
            print(
                f"error: {args.output} is out of date; run `python3 build/native/third-party-notices.py`",
                file=sys.stderr,
            )
            return 1
        print(f"verified {len(crates)} third-party crates in {args.output}")
        return 0

    args.output.write_text(updated_text, encoding="utf-8")
    print(f"wrote {len(crates)} third-party crates to {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

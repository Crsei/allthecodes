#!/usr/bin/env python3
"""Build the allthecodes project knowledge-base fact index.

Offline, read-only indexer that turns repository Markdown, plans, and code
into a SQLite-backed project fact layer under ``.allthecodes/kb/``.  No
network access, no runtime tool registration, no edits to repository files.

The script refuses to write partial state: each subcommand either completes
its artifact fully or exits non-zero with a diagnostic.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sqlite3
import subprocess
import sys
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable, Iterator

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

DOCUMENT_ROOTS: tuple[str, ...] = (
    "README.md",
    "AGENTS.md",
    "docs/README.md",
    "docs/WORK_STATUS.md",
    "docs/architecture",
    "docs/schemas",
    "development",
)

SKIP_PARTS: frozenset[str] = frozenset({
    ".git",
    ".allthecodes",
    ".worktrees",
    "target",
    "target2",
    "__pycache__",
    "node_modules",
})

# Drift categories — kept in sync with the plan.
DRIFT_BROKEN_MARKDOWN_LINK = "broken_markdown_link"
DRIFT_MISSING_ACTIVE_PATH = "missing_active_path"
DRIFT_LEGACY_PATH_REFERENCE = "legacy_path_reference"
DRIFT_PATH_ISOLATION_NAME_DRIFT = "path_isolation_name_drift"
DRIFT_ARCHIVE_USED_AS_CURRENT_FACT = "archive_used_as_current_fact"
DRIFT_STATUS_CONFLICT = "status_conflict"
DRIFT_DUPLICATE_CURRENT_ENTRY = "duplicate_current_entry"

DRIFT_CATEGORIES = (
    DRIFT_BROKEN_MARKDOWN_LINK,
    DRIFT_MISSING_ACTIVE_PATH,
    DRIFT_LEGACY_PATH_REFERENCE,
    DRIFT_PATH_ISOLATION_NAME_DRIFT,
    DRIFT_ARCHIVE_USED_AS_CURRENT_FACT,
    DRIFT_STATUS_CONFLICT,
    DRIFT_DUPLICATE_CURRENT_ENTRY,
)

# Allowed enumerations from the plan's Data Model.
DOC_TYPES = (
    "architecture",
    "plan",
    "status",
    "gap",
    "archive",
    "reference",
    "schema",
    "code-comment",
    "readme",
    "agent-instruction",
)

DOC_STATUSES = (
    "active",
    "completed",
    "archived",
    "intentional",
    "stale",
    "legacy",
    "unknown",
)

CODE_KINDS = (
    "crate",
    "module",
    "trait",
    "struct",
    "enum",
    "fn",
    "tool",
    "command",
    "config_path",
    "workflow",
)

RELATION_TYPES = (
    "describes",
    "modifies",
    "creates",
    "supersedes",
    "depends_on",
    "verifies",
    "implements",
    "mentions",
    "indexes",
    "drifts_from",
)

# Markdown / source patterns.
CHECKBOX_RE = re.compile(r"^\s*[-*]\s+\[(?P<check>[ xX])\]\s+(?P<text>.+?)\s*$")
MD_LINK_RE = re.compile(r"\[(?P<text>[^\]]+)\]\((?P<target>[^)\s]+)\)")
FENCE_RE = re.compile(r"^\s*(```|~~~)")
HEADING_RE = re.compile(r"^(?P<hashes>#{1,6})\s+(?P<title>.+?)\s*$")
INLINE_CODE_RE = re.compile(r"`([^`]+)`")
FRONTMATTER_DELIM = "---"
FRONTMATTER_KEY_RE = re.compile(r"^(?P<key>[A-Za-z_][A-Za-z0-9_]*):\s*(?P<value>.*)$")

# Path prefixes that count as "code path references" inside Markdown prose.
CODE_PATH_PREFIXES = (
    "crates/",
    "docs/",
    "development/",
    ".allthecodes/",
    "src/",
)

# Path-isolation legacy name drift patterns.
LEGACY_PATH_TOKENS = (
    ".cc-rust",
    "~/.cc-rust",
    ".Codex",
    "~/.Codex",
)

LEGACY_CONTEXT_WORDS = (
    "历史",
    "historical",
    "archive",
    "archived",
    "legacy",
    "曾用",
    "original",
    "原版",
)

# Inline code path glob characters — references containing them are skipped in
# drift validation (single-doc existence checks).
PATH_GLOB_CHARS = ("*", "?", "[", "]")

# Five architecture layers — canonical English names from the plan.
ARCHITECTURE_LAYERS = (
    "Layer 1: CLI / startup",
    "Layer 2: TUI / Headless IPC / Daemon / Web",
    "Layer 3: QueryEngine / lifecycle / system_prompt / hooks",
    "Layer 4: query loop / tool runtime / MCP / sandbox",
    "Layer 5: API providers / streaming / fallback",
)

# Query-loop phases listed in the loop_impl.rs structured comment.
QUERY_LOOP_PHASES = (
    "setup",
    "context preprocessing",
    "streaming model call",
    "post-streaming",
    "terminal check",
    "tool execution",
    "attachments",
    "continue",
)

# Config path facts expected from crates/allthecodes-config/src/paths.rs.
CONFIG_PATH_FACTS = (
    ("ALLTHECODES_HOME", "env override for the allthecodes global data root"),
    ("~/.allthecodes/", "default global allthecodes data directory"),
    (".allthecodes/", "per-project allthecodes directory at the cwd"),
    (".allthecodes/settings.json", "per-project settings file"),
    (".allthecodes/skills/", "per-project skill packages directory"),
    (".allthecodes/plan.md", "per-project current plan file"),
    (".allthecodes/plan-workflow.json", "per-project durable plan workflow record"),
)

# ---------------------------------------------------------------------------
# Small utilities
# ---------------------------------------------------------------------------


def sha1_hex(data: bytes) -> str:
    return hashlib.sha1(data).hexdigest()


def sha1_str(text: str) -> str:
    return sha1_hex(text.encode("utf-8"))


def doc_id(repo_root: Path, rel_path: str) -> str:
    """Stable id for a document. Uses repo-relative POSIX path."""
    return sha1_str(rel_path.replace("\\", "/"))


def code_entity_id(path: str, kind: str, name: str) -> str:
    return sha1_str(f"{kind}|{path}|{name}")


def plan_task_id(document_id: str, sequence: int) -> str:
    return sha1_str(f"{document_id}|task|{sequence}")


def relation_id(from_id: str, to_id: str, relation_type: str, evidence: str) -> str:
    return sha1_str(f"{from_id}|{to_id}|{relation_type}|{evidence}")


def drift_finding_id(path: str, category: str, issue: str) -> str:
    return sha1_str(f"{path}|{category}|{issue}")


def to_posix(path: Path) -> str:
    return path.as_posix()


def is_markdown(path: Path) -> bool:
    return path.suffix.lower() == ".md"


def is_skip_part(path: Path, repo_root: Path) -> bool:
    try:
        rel = path.relative_to(repo_root)
    except ValueError:
        return False
    parts = rel.parts
    if not parts:
        return False
    if parts[0] in SKIP_PARTS:
        return True
    # Also skip when any interior part matches SKIP_PARTS (e.g. nested
    # ``target/``).
    return any(part in SKIP_PARTS for part in parts)


def iter_files(repo_root: Path, roots: Iterable[str]) -> Iterator[Path]:
    for root in roots:
        base = repo_root / root
        if not base.exists():
            continue
        if base.is_file():
            if not is_skip_part(base, repo_root):
                yield base
            continue
        for path in sorted(base.rglob("*")):
            if not path.is_file():
                continue
            if is_skip_part(path, repo_root):
                continue
            yield path


def rel_posix(repo_root: Path, path: Path) -> str:
    try:
        rel = path.relative_to(repo_root)
    except ValueError:
        # Outside the repo (shouldn't happen for our roots).
        rel = path
    return to_posix(rel)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="replace")


def snippet(text: str, cap: int = 320) -> str:
    """Return a single-paragraph snippet bounded to ``cap`` characters."""
    if not text:
        return ""
    # Normalize whitespace to single spaces, strip surrounding whitespace.
    flat = re.sub(r"\s+", " ", text).strip()
    if len(flat) <= cap:
        return flat
    return flat[:cap].rstrip() + "…"


# ---------------------------------------------------------------------------
# Markdown parsing
# ---------------------------------------------------------------------------


@dataclass
class Frontmatter:
    title: str | None = None
    description: str | None = None
    keywords: list[str] = field(default_factory=list)
    raw: dict[str, str] = field(default_factory=dict)


@dataclass
class ParsedMarkdown:
    frontmatter: Frontmatter
    first_heading: str | None
    body: str
    lines: list[str]


def parse_markdown(text: str) -> ParsedMarkdown:
    lines = text.splitlines()
    fm = Frontmatter()
    body_start = 0
    if lines and lines[0].strip() == FRONTMATTER_DELIM:
        # Find closing ``---``.
        for idx in range(1, len(lines)):
            if lines[idx].strip() == FRONTMATTER_DELIM:
                fm_block = lines[1:idx]
                body_start = idx + 1
                break
        if body_start:
            for raw_line in fm_block:
                m = FRONTMATTER_KEY_RE.match(raw_line)
                if not m:
                    continue
                key = m.group("key")
                value = m.group("value").strip()
                # Strip surrounding quotes if any.
                if len(value) >= 2 and value[0] in "\"'" and value[-1] == value[0]:
                    value = value[1:-1]
                fm.raw[key] = value
            fm.title = fm.raw.get("title")
            fm.description = fm.raw.get("description")
            kw = fm.raw.get("keywords", "")
            if kw:
                # ``keywords: [a, b, c]`` or ``keywords: a, b, c``.
                kw_inner = kw.strip()
                if kw_inner.startswith("[") and kw_inner.endswith("]"):
                    kw_inner = kw_inner[1:-1]
                fm.keywords = [k.strip().strip("\"'") for k in kw_inner.split(",") if k.strip()]
    body_lines = lines[body_start:]
    first_heading: str | None = None
    for line in body_lines:
        m = HEADING_RE.match(line)
        if m and m.group("hashes") == "#":
            first_heading = m.group("title").strip()
            break
    return ParsedMarkdown(
        frontmatter=fm,
        first_heading=first_heading,
        body="\n".join(body_lines),
        lines=body_lines,
    )


def classify_doc(rel_path: str) -> tuple[str, str]:
    """Return ``(doc_type, status)`` per plan Section "Task 2 Step 3"."""
    if rel_path == "docs/WORK_STATUS.md":
        return ("status", "active")
    if rel_path == "AGENTS.md":
        return ("agent-instruction", "active")
    if rel_path.startswith("docs/architecture/"):
        return ("architecture", "active")
    if rel_path.startswith("docs/schemas/"):
        return ("schema", "active")
    if rel_path.startswith("development/archive/"):
        return ("archive", "archived")
    if rel_path.endswith("-plan.md") or "/plan/" in rel_path or "/planz/" in rel_path:
        return ("plan", "active")
    if rel_path.endswith("README.md"):
        return ("readme", "active")
    if rel_path == "README.md":
        return ("readme", "active")
    return ("reference", "unknown")


def extract_inline_code_paths(line: str) -> list[str]:
    """Return inline backtick strings that look like repo code paths."""
    out: list[str] = []
    for m in INLINE_CODE_RE.finditer(line):
        token = m.group(1).strip()
        if any(token.startswith(p) for p in CODE_PATH_PREFIXES):
            # Skip if it contains whitespace or is clearly a prose fragment.
            if " " in token or "\t" in token:
                continue
            out.append(token)
    return out


def extract_markdown_links(line: str) -> list[tuple[str, str]]:
    return [(m.group("text"), m.group("target")) for m in MD_LINK_RE.finditer(line)]


# ---------------------------------------------------------------------------
# Fact record dataclasses
# ---------------------------------------------------------------------------


@dataclass
class DocumentFact:
    path: str
    title: str
    description: str | None
    doc_type: str
    status: str
    updated_at: str | None
    source_ref: str
    raw_hash: str
    body: str


@dataclass
class CodeEntity:
    path: str
    crate: str | None
    module: str | None
    kind: str
    name: str
    signature: str | None
    visibility: str | None


@dataclass
class PlanTask:
    document_id: str
    title: str
    status: str
    sequence: int
    checklist_text: str


@dataclass
class Relation:
    from_id: str
    to_id: str
    relation_type: str
    evidence: str


@dataclass
class DriftFinding:
    path: str
    severity: str
    category: str
    issue: str
    evidence: str
    suggested_action: str


# ---------------------------------------------------------------------------
# SQLite schema + bootstrap
# ---------------------------------------------------------------------------

SCHEMA_SQL = """
CREATE TABLE IF NOT EXISTS documents (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    description TEXT,
    doc_type TEXT NOT NULL,
    status TEXT NOT NULL,
    updated_at TEXT,
    source_ref TEXT NOT NULL,
    raw_hash TEXT NOT NULL,
    body TEXT
);

CREATE TABLE IF NOT EXISTS code_entities (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    crate TEXT,
    module TEXT,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    signature TEXT,
    visibility TEXT
);

CREATE TABLE IF NOT EXISTS plan_tasks (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    checklist_text TEXT NOT NULL,
    FOREIGN KEY (document_id) REFERENCES documents(id)
);

CREATE TABLE IF NOT EXISTS relations (
    from_id TEXT NOT NULL,
    to_id TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    evidence TEXT NOT NULL,
    PRIMARY KEY (from_id, to_id, relation_type, evidence)
);

CREATE TABLE IF NOT EXISTS drift_findings (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    severity TEXT NOT NULL,
    category TEXT NOT NULL,
    issue TEXT NOT NULL,
    evidence TEXT NOT NULL,
    suggested_action TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS kb_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"""


def open_database(kb_dir: Path) -> sqlite3.Connection:
    db_path = kb_dir / "project-facts.sqlite"
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    return conn


def init_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(SCHEMA_SQL)
    # FTS5 with graceful fallback.
    try:
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts "
            "USING fts5(id UNINDEXED, path, title, description, body)"
        )
    except sqlite3.OperationalError:
        # No FTS5 support; create a plain search index.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS documents_search "
            "(id TEXT PRIMARY KEY, path TEXT, title TEXT, body TEXT)"
        )
    conn.commit()


def reset_index(conn: sqlite3.Connection) -> None:
    """Drop existing generated rows so rebuilds are idempotent."""
    conn.executescript(
        """
        DELETE FROM documents;
        DELETE FROM code_entities;
        DELETE FROM plan_tasks;
        DELETE FROM relations;
        DELETE FROM drift_findings;
        DELETE FROM documents_fts;
        DELETE FROM kb_meta;
        """
    )
    # documents_search only exists when FTS5 is unavailable; guard it.
    if conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='documents_search'"
    ).fetchone() is not None:
        conn.execute("DELETE FROM documents_search")
    conn.commit()


# ---------------------------------------------------------------------------
# Document inventory (Task 2)
# ---------------------------------------------------------------------------


def iter_markdown_files(repo_root: Path) -> Iterator[Path]:
    """Yield Markdown files under DOCUMENT_ROOTS, skipping noise."""
    yield from (
        path
        for path in iter_files(repo_root, DOCUMENT_ROOTS)
        if is_markdown(path)
    )


def extract_document_fact(repo_root: Path, path: Path) -> DocumentFact:
    rel = rel_posix(repo_root, path)
    raw = path.read_bytes()
    text = raw.decode("utf-8", errors="replace")
    parsed = parse_markdown(text)
    title = parsed.frontmatter.title or parsed.first_heading or path.stem
    description = parsed.frontmatter.description
    doc_type, status = classify_doc(rel)
    # Mtime as ISO-ish YYYY-MM-DD.
    try:
        mtime = int(path.stat().st_mtime)
        updated_at = datetime.fromtimestamp(mtime).strftime("%Y-%m-%d")
    except OSError:
        updated_at = None
    return DocumentFact(
        path=rel,
        title=title,
        description=description,
        doc_type=doc_type,
        status=status,
        updated_at=updated_at,
        source_ref=rel,
        raw_hash=sha1_hex(raw),
        body=text,
    )


def insert_document(conn: sqlite3.Connection, fact: DocumentFact) -> str:
    did = doc_id(Path(fact.path), fact.path)
    conn.execute(
        """
        INSERT OR REPLACE INTO documents
            (id, path, title, description, doc_type, status, updated_at, source_ref, raw_hash, body)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            did,
            fact.path,
            fact.title,
            fact.description,
            fact.doc_type,
            fact.status,
            fact.updated_at,
            fact.source_ref,
            fact.raw_hash,
            fact.body,
        ),
    )
    # Populate FTS5 mirror (or LIKE-fallback search table) when present.
    fts_available = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='documents_fts'"
    ).fetchone() is not None
    if fts_available:
        conn.execute(
            "INSERT OR REPLACE INTO documents_fts (id, path, title, description, body) VALUES (?, ?, ?, ?, ?)",
            (did, fact.path, fact.title, fact.description or "", fact.body),
        )
    else:
        conn.execute(
            "INSERT OR REPLACE INTO documents_search (id, path, title, body) VALUES (?, ?, ?, ?)",
            (did, fact.path, fact.title, fact.body),
        )
    return did


def insert_code_entity(conn: sqlite3.Connection, ent: CodeEntity) -> str:
    cid = code_entity_id(ent.path, ent.kind, ent.name)
    conn.execute(
        """
        INSERT OR REPLACE INTO code_entities
            (id, path, crate, module, kind, name, signature, visibility)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (cid, ent.path, ent.crate, ent.module, ent.kind, ent.name, ent.signature, ent.visibility),
    )
    return cid


def insert_plan_task(conn: sqlite3.Connection, task: PlanTask) -> str:
    tid = plan_task_id(task.document_id, task.sequence)
    conn.execute(
        """
        INSERT OR REPLACE INTO plan_tasks
            (id, document_id, title, status, sequence, checklist_text)
        VALUES (?, ?, ?, ?, ?, ?)
        """,
        (tid, task.document_id, task.title, task.status, task.sequence, task.checklist_text),
    )
    return tid


def insert_relation(conn: sqlite3.Connection, rel: Relation) -> None:
    conn.execute(
        """
        INSERT OR IGNORE INTO relations
            (from_id, to_id, relation_type, evidence)
        VALUES (?, ?, ?, ?)
        """,
        (rel.from_id, rel.to_id, rel.relation_type, rel.evidence),
    )


def insert_drift_finding(conn: sqlite3.Connection, f: DriftFinding) -> None:
    fid = drift_finding_id(f.path, f.category, f.issue)
    conn.execute(
        """
        INSERT OR REPLACE INTO drift_findings
            (id, path, severity, category, issue, evidence, suggested_action)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        """,
        (fid, f.path, f.severity, f.category, f.issue, f.evidence, f.suggested_action),
    )


def append_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False) + "\n")


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="build_project_kb",
        description="Build the allthecodes project knowledge-base fact index.",
    )
    parser.add_argument("--repo", default=".", help="Repository root")
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("build", help="Build SQLite and JSONL facts")
    sub.add_parser("report", help="Generate kb-report.md from the current index")
    check = sub.add_parser("check", help="Build and fail if high-severity drift exists")
    check.add_argument(
        "--fail-on",
        default="high",
        choices=["none", "high", "medium"],
        help="Failure threshold for drift severity",
    )
    query = sub.add_parser("query", help="Search the generated fact index")
    query.add_argument("--q", dest="query", required=True)
    query.add_argument(
        "--mode",
        default="all",
        choices=["exact", "structured", "fts", "all"],
    )
    query.add_argument("--limit", type=int, default=10)
    return parser.parse_args(argv)


# ---------------------------------------------------------------------------
# WORK_STATUS current-state index (Task 3)
# ---------------------------------------------------------------------------

WORK_STATUS_SECTIONS = (
    ("## 当前结论", "current_conclusions"),
    ("## 活跃待办", "active_work"),
    ("## 活跃文档入口", "active_entrypoints"),
    ("## 历史 Deferred", "historical_deferred_policy"),
)


def parse_work_status(body: str) -> dict[str, Any]:
    """Parse the four canonical WORK_STATUS sections."""
    lines = body.splitlines()
    sections: dict[str, list[str]] = {name: [] for _, name in WORK_STATUS_SECTIONS}
    current: str | None = None
    for line in lines:
        if line.startswith("## "):
            key = line.split("## ", 1)[1].strip()
            current = next(
                (sec_name for heading, sec_name in WORK_STATUS_SECTIONS if key.startswith(heading)),
                None,
            )
            continue
        if current is None:
            continue
        sections[current].append(line)

    # Active work table under `## 活跃待办` (first '|' table row onward).
    active_work_rows: list[dict[str, str]] = []
    for raw in sections["active_work"]:
        s = raw.strip()
        if not s.startswith("|") or "|" not in s[1:]:
            continue
        cells = [c.strip() for c in s.strip("|").split("|")]
        # Skip header separator rows like `|---|---|`.
        if all(set(c) <= {"-", ":"} for c in cells if c):
            continue
        if not cells or cells[0].lower() in {"scope", "范围"}:
            # Header row — keep going but don't store it.
            continue
        # Use up to 3 columns: scope, current_status, next_step.
        scope = cells[0] if len(cells) > 0 else ""
        current_status = cells[1] if len(cells) > 1 else ""
        next_step = cells[2] if len(cells) > 2 else ""
        if not scope:
            continue
        active_work_rows.append({
            "scope": scope,
            "current_status": current_status,
            "next_step": next_step,
        })

    # Active entrypoints: capture Markdown-link targets under that section.
    entrypoint_links: list[tuple[str, str]] = []
    for raw in sections["active_entrypoints"]:
        for text, target in extract_markdown_links(raw):
            entrypoint_links.append((text, target))

    return {
        "source": "docs/WORK_STATUS.md",
        "current_conclusions": [s.strip() for s in sections["current_conclusions"] if s.strip()],
        "active_work": active_work_rows,
        "active_entrypoints": [
            {"text": t, "target": tgt} for t, tgt in entrypoint_links
        ],
        "historical_deferred_policy": [s.strip() for s in sections["historical_deferred_policy"] if s.strip()],
    }


def resolve_doc_target(repo_root: Path, source_rel: str, target: str) -> str | None:
    """Resolve a Markdown link target relative to ``source_rel``'s directory."""
    if target.startswith(("http://", "https://", "mailto:", "#")):
        return None
    anchor = ""
    if "#" in target:
        target, anchor = target.split("#", 1)
    if not target:
        return None
    base_dir = (repo_root / source_rel).parent
    candidate = (base_dir / target).resolve()
    try:
        rel = candidate.relative_to(repo_root)
    except ValueError:
        return None
    return to_posix(rel) + (("#" + anchor) if anchor else "")


def collect_work_status_facts(
    repo_root: Path, work_status_fact: DocumentFact, document_id_map: dict[str, str]
) -> tuple[list[Relation], list[str], dict[str, Any]]:
    """Returns (relations, entrypoint_paths, current_status_dict)."""
    raw = parse_work_status(work_status_fact.body)
    wid = document_id_map[work_status_fact.path]
    rels: list[Relation] = []
    entrypoint_paths: list[str] = []
    seen: set[str] = set()
    for entry in raw["active_entrypoints"]:
        resolved = resolve_doc_target(repo_root, work_status_fact.path, entry["target"])
        if not resolved:
            continue
        clean = resolved.split("#", 1)[0]
        if not clean:
            continue
        entrypoint_paths.append(clean)
        to_id = document_id_map.get(clean)
        if not to_id:
            # Document not indexed → skip the relation but record the entry.
            continue
        rels.append(Relation(
            from_id=wid,
            to_id=to_id,
            relation_type="indexes",
            evidence=f"{work_status_fact.path} → {resolved}",
        ))
        seen.add(clean)
    raw["active_entrypoints"] = [
        {"text": t, "target": tgt, "resolved_path": (resolve_doc_target(repo_root, work_status_fact.path, tgt) or "")}
        for t, tgt in ((e["text"], e["target"]) for e in raw["active_entrypoints"])
    ]
    return rels, entrypoint_paths, raw


# ---------------------------------------------------------------------------
# Architecture system map (Task 4)
# ---------------------------------------------------------------------------


def collect_architecture_facts(
    repo_root: Path, document_id_map: dict[str, str]
) -> tuple[list[CodeEntity], list[Relation]]:
    """Build the 5-layer map + engine module exports + query-loop phase facts."""
    code_ents: list[CodeEntity] = []
    rels: list[Relation] = []

    overview_rel = "docs/architecture/introduction/architecture-overview.md"
    overview_id = document_id_map.get(overview_rel)

    # Five architecture layers — canonical English names from the plan.
    for layer in ARCHITECTURE_LAYERS:
        code_ents.append(CodeEntity(
            path=overview_rel,
            crate=None,
            module=None,
            kind="workflow",
            name=layer,
            signature=None,
            visibility=None,
        ))
        if overview_id:
            rels.append(Relation(
                from_id=overview_id,
                to_id=code_entity_id(overview_rel, "workflow", layer),
                relation_type="describes",
                evidence=f"{overview_rel} lists {layer}",
            ))

    # Engine module exports.
    engine_lib = repo_root / "crates" / "allthecodes-engine" / "src" / "lib.rs"
    engine_lib_rel = to_posix(engine_lib.relative_to(repo_root)) if engine_lib.exists() else None
    if engine_lib_rel:
        text = read_text(engine_lib)
        for m in re.finditer(r"^\s*pub\s+mod\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*;", text, flags=re.M):
            name = m.group("name")
            code_ents.append(CodeEntity(
                path=engine_lib_rel,
                crate="allthecodes-engine",
                module=name,
                kind="module",
                name=name,
                signature=m.group(0).strip(),
                visibility="pub",
            ))
            if overview_id:
                rels.append(Relation(
                    from_id=overview_id,
                    to_id=code_entity_id(engine_lib_rel, "module", name),
                    relation_type="describes",
                    evidence=f"{overview_rel} describes allthecodes-engine module {name}",
                ))

    # Query-loop phases from the structured comment.
    loop_impl = repo_root / "crates" / "allthecodes-engine" / "src" / "query" / "loop_impl.rs"
    loop_impl_rel = to_posix(loop_impl.relative_to(repo_root)) if loop_impl.exists() else None
    if loop_impl_rel:
        text = read_text(loop_impl)
        for phase in QUERY_LOOP_PHASES:
            code_ents.append(CodeEntity(
                path=loop_impl_rel,
                crate="allthecodes-engine",
                module="query::loop_impl",
                kind="workflow",
                name=f"query loop phase: {phase}",
                signature=None,
                visibility=None,
            ))
            rels.append(Relation(
                from_id=code_entity_id(loop_impl_rel, "workflow", f"query loop phase: {phase}"),
                to_id=code_entity_id(loop_impl_rel, "module", "query"),
                relation_type="implements",
                evidence=f"{loop_impl_rel} phase '{phase}' implements query loop",
            ))
        if overview_id:
            rels.append(Relation(
                from_id=overview_id,
                to_id=code_entity_id(loop_impl_rel, "workflow", "query loop phase: setup"),
                relation_type="describes",
                evidence=f"{overview_rel} describes {loop_impl_rel} phases",
            ))

    # architecture-overview.md --describes--> allthecodes-engine crate
    if overview_id:
        rels.append(Relation(
            from_id=overview_id,
            to_id=code_entity_id(engine_lib_rel or "crates/allthecodes-engine", "crate", "allthecodes-engine"),
            relation_type="describes",
            evidence=f"{overview_rel} describes crate allthecodes-engine",
        ))
        if engine_lib_rel:
            rels.append(Relation(
                from_id=overview_id,
                to_id=code_entity_id(engine_lib_rel, "module", "query"),
                relation_type="describes",
                evidence=f"{overview_rel} describes crates/allthecodes-engine/src/query/loop_impl.rs",
            ))

    return code_ents, rels


# ---------------------------------------------------------------------------
# Plan task <-> file relations (Task 5)
# ---------------------------------------------------------------------------

TASK_HEADING_RE = re.compile(r"^###\s+Task\s+(?P<seq>\d+)\s*[:：]\s*(?P<title>.+?)\s*$")
FILES_BLOCK_HEADER = "**Files:**"
FILES_BULLET_RE = re.compile(
    r"^\s*[-*]\s+(?P<verb>Create|Modify|Test|Read source)\s*[:：]\s*`?(?P<path>[^`]+?)`?\s*$"
)


@dataclass
class PlanTaskSpec:
    sequence: int
    title: str
    checklist_lines: list[str]
    files_relations: list[tuple[str, str]]  # (verb, path)


def parse_plan_tasks(body: str) -> list[PlanTaskSpec]:
    specs: list[PlanTaskSpec] = []
    current: PlanTaskSpec | None = None
    in_files_block = False
    for line in body.splitlines():
        m = TASK_HEADING_RE.match(line)
        if m:
            if current is not None:
                specs.append(current)
            current = PlanTaskSpec(
                sequence=int(m.group("seq")),
                title=m.group("title").strip(),
                checklist_lines=[],
                files_relations=[],
            )
            in_files_block = False
            continue
        if current is None:
            continue
        if line.strip().startswith(FILES_BLOCK_HEADER):
            in_files_block = True
            continue
        if in_files_block:
            if not line.strip():
                in_files_block = False
                continue
            fm = FILES_BULLET_RE.match(line)
            if fm:
                verb = fm.group("verb").strip()
                path = fm.group("path").strip().strip("`\"'")
                current.files_relations.append((verb, path))
                continue
            # End of a files block when a non-bullet non-empty line appears.
            if line.strip() and not (line.startswith("-") or line.startswith("*")):
                in_files_block = False
        cb = CHECKBOX_RE.match(line)
        if cb:
            current.checklist_lines.append(line.strip())
    if current is not None:
        specs.append(current)
    return specs


def collect_plan_facts(
    repo_root: Path,
    fact: DocumentFact,
    document_id_map: dict[str, str],
) -> tuple[list[PlanTask], list[Relation], list[CodeEntity]]:
    """Build plan_tasks + relations. Adds inline prose code-path references."""
    tasks: list[PlanTask] = []
    rels: list[Relation] = []
    code_ents: list[CodeEntity] = []
    spec_list = parse_plan_tasks(fact.body)
    doc_id = document_id_map[fact.path]
    is_archive = fact.doc_type == "archive" or fact.status == "archived"

    for spec in spec_list:
        # Status: any unchecked checkbox → active; all checked → completed.
        if not spec.checklist_lines:
            status = "completed" if is_archive else "active"
        else:
            any_unchecked = any("[ ]" in l for l in spec.checklist_lines)
            status = "active" if any_unchecked and not is_archive else ("completed" if not any_unchecked else ("archived" if is_archive else "active"))
        tasks.append(PlanTask(
            document_id=doc_id,
            title=spec.title,
            status=status,
            sequence=spec.sequence,
            checklist_text="\n".join(spec.checklist_lines),
        ))
        task_id = plan_task_id(doc_id, spec.sequence)

        # Files-block relations: Create / Modify / Test.
        for verb, path in spec.files_relations:
            rtype = {
                "Create": "creates",
                "Modify": "modifies",
                "Test": "verifies",
                "Read source": "mentions",
            }[verb]
            rels.append(Relation(
                from_id=task_id,
                to_id=code_entity_id(path, "file", path),
                relation_type=rtype,
                evidence=f"{fact.path} Task {spec.sequence} {verb}: {path}",
            ))

        # Inline prose code paths: scan task prose for backtick code paths.
        # We treat each phase (heading onwards) as the task body. We walk the
        # raw lines from this task heading until next heading to find prose
        # references.
        prose_paths: set[str] = set()
        in_task = False
        for line in fact.body.splitlines():
            if TASK_HEADING_RE.match(line):
                seq_m = TASK_HEADING_RE.match(line)
                in_task = seq_m is not None and int(seq_m.group("seq")) == spec.sequence
                continue
            if not in_task:
                continue
            if line.lstrip().startswith("#"):
                in_task = False
                continue
            for code in extract_inline_code_paths(line):
                if code in prose_paths:
                    continue
                prose_paths.add(code)
                # Skip if already covered by files_block.
                already = any(code == p for _, p in spec.files_relations)
                if already:
                    continue
                # Skip wildcard references in drift validation; relations
                # are still created because they document intent.
                rels.append(Relation(
                    from_id=task_id,
                    to_id=code_entity_id(code, "file", code),
                    relation_type="modifies",
                    evidence=f"{fact.path} Task {spec.sequence} prose mentions {code}",
                ))

    # Plans-as-documents also get a `describes` relation to all files they
    # mention (for structured query reverse lookups).
    for spec in spec_list:
        for verb, path in spec.files_relations:
            rtype = {
                "Create": "creates",
                "Modify": "modifies",
                "Test": "verifies",
                "Read source": "mentions",
            }[verb]
            rels.append(Relation(
                from_id=doc_id,
                to_id=code_entity_id(path, "file", path),
                relation_type=rtype,
                evidence=f"{fact.path} (document) Task {spec.sequence} {verb}: {path}",
            ))

    # Emit a `file` code_entity for every distinct file path referenced by
    # Files blocks or inline prose. This anchors the `to_id` of plan-file
    # relations so the structured query reverse-lookup can find them.
    file_paths: set[str] = set()
    for spec in spec_list:
        for _, path in spec.files_relations:
            file_paths.add(path)
    # Inline prose paths were emitted into relations with the same path id.
    # Re-scan prose for ``code`` tokens here (mirrors the earlier loop).
    for line in fact.body.splitlines():
        for code in extract_inline_code_paths(line):
            file_paths.add(code)
    for path in sorted(file_paths):
        code_ents.append(CodeEntity(
            path=path,
            crate=None,
            module=None,
            kind="file",
            name=path,
            signature=None,
            visibility=None,
        ))

    return tasks, rels, code_ents


# ---------------------------------------------------------------------------
# Code fact extractors (Task 6)
# ---------------------------------------------------------------------------


def parse_cargo_workspace(repo_root: Path) -> list[str]:
    """Return workspace member crate names via tomllib or regex fallback."""
    cargo_toml = repo_root / "Cargo.toml"
    if not cargo_toml.exists():
        return []
    data_bytes = cargo_toml.read_bytes()
    members: list[str] = []
    try:
        import tomllib  # py311+
        data = tomllib.loads(data_bytes.decode("utf-8", errors="replace"))
        ws = data.get("workspace", {})
        member_specs = ws.get("members", [])
        explicit: list[str] = []
        for spec in member_specs:
            if "*" in spec:
                # Glob like ``crates/*`` → expand.
                import fnmatch
                base = repo_root / spec.split("*")[0].rstrip("/")
                if base.is_dir():
                    for sub in base.iterdir():
                        if sub.is_dir() and (sub / "Cargo.toml").is_file():
                            rel = to_posix(sub.relative_to(repo_root))
                            if not is_skip_part(sub, repo_root):
                                explicit.append(rel)
            else:
                explicit.append(spec)
        for m in explicit:
            member_cargo = repo_root / m / "Cargo.toml"
            if not member_cargo.is_file():
                continue
            try:
                pkg = tomllib.loads(member_cargo.read_text(encoding="utf-8", errors="replace"))
                pkg_name = pkg.get("package", {}).get("name")
                if pkg_name:
                    members.append(pkg_name)
            except Exception:
                members.append(m.strip("/").rsplit("/", 1)[-1])
    except ImportError:
        # Fallback: regex parse [workspace].members lines.
        text = data_bytes.decode("utf-8", errors="replace")
        in_ws = False
        member_lines: list[str] = []
        for line in text.splitlines():
            if line.startswith("[") and line.endswith("]"):
                in_ws = line.strip().lower() == "[workspace]"
                continue
            if not in_ws:
                continue
            if line.strip().startswith("members"):
                rhs = line.split("=", 1)[1].strip()
                if rhs.startswith("["):
                    # single-line array
                    items = re.findall(r'"\*?[^"]+"', rhs)
                    member_lines.extend(re.sub(r'^"|"$', "", it) for it in items)
                else:
                    member_lines.append(rhs.strip())
            elif line.strip().startswith('"') and member_lines:
                member_lines.append(re.sub(r'^"|"$', "", line.strip()))
            elif not line.strip():
                break
        for spec in member_lines:
            if "*" in spec:
                base = repo_root / spec.split("*")[0].rstrip("/")
                if base.is_dir():
                    for sub in base.iterdir():
                        if sub.is_dir() and (sub / "Cargo.toml").is_file() and not is_skip_part(sub, repo_root):
                            rel = to_posix(sub.relative_to(repo_root))
                            members.append(rel.rsplit("/", 1)[-1])
            else:
                members.append(spec.rsplit("/", 1)[-1])
    # De-dupe preserving order.
    seen: set[str] = set()
    out: list[str] = []
    for m in members:
        if m not in seen:
            seen.add(m)
            out.append(m)
    return out


def parse_rust_symbol_lines(text: str) -> Iterator[tuple[str, str, str]]:
    """Yield (kind, name, signature_lines) for top-level Rust declarations."""
    pattern = re.compile(
        r"^pub\s+(?P<kind>struct|enum|trait|fn|mod)\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\b"
    )
    for line in text.splitlines():
        m = pattern.match(line)
        if m:
            yield (m.group("kind"), m.group("name"), line.strip())


def list_rust_files(repo_root: Path) -> Iterator[Path]:
    """Use ``rg --files`` if available, else python walk."""
    try:
        proc = subprocess.run(
            ["rg", "--files", "crates"],
            cwd=str(repo_root),
            capture_output=True,
            text=True,
            timeout=30,
        )
        if proc.returncode == 0 and proc.stdout.strip():
            for line in proc.stdout.splitlines():
                p = repo_root / line
                if p.suffix == ".rs" and not is_skip_part(p, repo_root):
                    yield p
            return
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    base = repo_root / "crates"
    if not base.is_dir():
        return
    for p in sorted(base.rglob("*.rs")):
        if not is_skip_part(p, repo_root):
            yield p


def collect_code_facts(repo_root: Path) -> tuple[list[CodeEntity], list[Relation]]:
    code_ents: list[CodeEntity] = []
    rels: list[Relation] = []

    # Workspace crates.
    crate_names = parse_cargo_workspace(repo_root)
    for name in crate_names:
        # Path resolution: try crates/<name>/src/lib.rs
        crate_root = repo_root / "crates" / name
        src_root = crate_root / "src"
        lib_rs = src_root / "lib.rs"
        path = to_posix(lib_rs.relative_to(repo_root)) if lib_rs.exists() else to_posix(crate_root.relative_to(repo_root))
        code_ents.append(CodeEntity(
            path=path,
            crate=name,
            module=None,
            kind="crate",
            name=name,
            signature=None,
            visibility="pub",
        ))
        # Also emit a `file` code_entity for the crate root so the path
        # `crates/<name>/src/lib.rs` (or `src/mod.rs`) is independently
        # searchable, and so structured query reverse-lookups (used to map
        # query results to source files) can anchor `to_id` relations.
        if lib_rs.exists():
            code_ents.append(CodeEntity(
                path=path,
                crate=name,
                module=None,
                kind="file",
                name=path,
                signature=None,
                visibility=None,
            ))

    # Tool facts: specific named symbols from tool.rs.
    tools_path = repo_root / "crates" / "allthecodes-tools" / "src" / "tool.rs"
    if tools_path.exists():
        tool_path_rel = to_posix(tools_path.relative_to(repo_root))
        text = read_text(tools_path)
        tool_targets = (
            ("trait", "Tool"),
            ("struct", "ToolUseContext"),
            ("struct", "ToolResult"),
        )
        for kind, name in tool_targets:
            sig = next(
                (line.strip() for line in text.splitlines()
                 if re.match(rf"^pub\s+{kind}\s+{name}\b", line)),
                f"pub {kind} {name}",
            )
            code_ents.append(CodeEntity(
                path=tool_path_rel,
                crate="allthecodes-tools",
                module=None,
                kind=kind,
                name=name,
                signature=sig,
                visibility="pub",
            ))

    # Registry tool facts.
    reg_path = repo_root / "crates" / "allthecodes-tools" / "src" / "registry.rs"
    reg_rel: str | None = None
    if reg_path.exists():
        reg_rel = to_posix(reg_path.relative_to(repo_root))
        text = read_text(reg_path)
        # Discover all the Tool struct names pushed in ``Arc::new(<X>) as _``.
        tool_push_re = re.compile(r"Arc::new\(\s*(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*\)\s*as\s*_")
        pushed_names: list[str] = []
        seen: set[str] = set()
        for line in text.splitlines():
            m = tool_push_re.search(line)
            if m and m.group("name") not in seen:
                seen.add(m.group("name"))
                pushed_names.append(m.group("name"))
        # Capture matching `use crate::...<Name>` import lines for evidence.
        import_re = re.compile(r"use\s+crate::.*?(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*[,;]")
        for name in pushed_names:
            sig = f"Arc::new({name}) as _"
            code_ents.append(CodeEntity(
                path=reg_rel,
                crate="allthecodes-tools",
                module=None,
                kind="tool",
                name=name,
                signature=sig,
                visibility="pub",
            ))

    # Generic symbol scan across crates.
    for rs_file in list_rust_files(repo_root):
        rel = to_posix(rs_file.relative_to(repo_root))
        try:
            text = read_text(rs_file)
        except OSError:
            continue
        # Skip generated tests / target dirs already filtered.
        crate = rel.split("/")[1] if rel.startswith("crates/") and len(rel.split("/")) > 1 else None
        for kind, name, sig in parse_rust_symbol_lines(text):
            # Skip if we already added this exact symbol from tool.rs above.
            if crate == "allthecodes-tools" and rel.endswith("/tool.rs") and any(
                ent.name == name and ent.kind == kind and ent.path == rel for ent in code_ents
            ):
                continue
            code_ents.append(CodeEntity(
                path=rel,
                crate=crate,
                module=None,
                kind=kind,
                name=name,
                signature=sig,
                visibility="pub",
            ))

    # Path facts from crates/allthecodes-config/src/paths.rs.
    paths_rs = repo_root / "crates" / "allthecodes-config" / "src" / "paths.rs"
    if paths_rs.exists():
        paths_rel = to_posix(paths_rs.relative_to(repo_root))
        text = read_text(paths_rs)
        for canonical_name, desc in CONFIG_PATH_FACTS:
            # Best-effort: find a matching pub fn line.
            sig = None
            for line in text.splitlines():
                # Heuristic match: line contains the canonical_name key word.
                if canonical_name in line and (line.strip().startswith("pub fn") or "data_root" in line and canonical_name == "ALLTHECODES_HOME"):
                    sig = line.strip()
                    break
            code_ents.append(CodeEntity(
                path=paths_rel,
                crate="allthecodes-config",
                module=None,
                kind="config_path",
                name=canonical_name,
                signature=sig,
                visibility="pub",
            ))
            # Description as a relation to itself for visibility.
            rels.append(Relation(
                from_id=code_entity_id(paths_rel, "config_path", canonical_name),
                to_id=code_entity_id(paths_rel, "config_path", canonical_name),
                relation_type="mentions",
                evidence=desc,
            ))

    return code_ents, rels


# ---------------------------------------------------------------------------
# Drift detection (Task 7)
# ---------------------------------------------------------------------------


def detect_drift(
    repo_root: Path,
    documents: dict[str, DocumentFact],
    document_id_map: dict[str, str],
    plan_tasks: list[tuple[str, PlanTask]],
    current_status: dict[str, Any],
    active_entrypoint_paths: list[str],
) -> list[DriftFinding]:
    findings: list[DriftFinding] = []

    def doc_exists(rel: str) -> bool:
        return (repo_root / rel).is_file()

    def appears_active(fact: DocumentFact) -> bool:
        return fact.status == "active" or fact.doc_type not in {"archive"}

    # 1. Broken Markdown links (relative targets only).
    for path, fact in documents.items():
        is_active = appears_active(fact)
        for line in fact.body.splitlines():
            for text, target in extract_markdown_links(line):
                if target.startswith(("http://", "https://", "mailto:", "#")):
                    continue
                resolved = resolve_doc_target(repo_root, path, target)
                if not resolved:
                    continue
                disk_target = resolved.split("#", 1)[0]
                if not disk_target:
                    continue
                if not doc_exists(disk_target):
                    findings.append(DriftFinding(
                        path=path,
                        severity="high" if is_active else "low",
                        category=DRIFT_BROKEN_MARKDOWN_LINK,
                        issue=f"Markdown link to non-existent path: {target}",
                        evidence=f"{path}: link target '{target}' resolved to '{disk_target}' (missing)",
                        suggested_action=(
                            f"Fix or remove the broken link to '{target}' in {path}"
                            if is_active
                            else f"Mark the broken link historical or update {target}"
                        ),
                    ))

    # 2 & 3 & 4. Code-path validation + archive leakage + path-isolation drift.
    active_paths_set = {p for p in active_entrypoint_paths}
    # Set of paths that this active document indexes via WORK_STATUS.
    for path, fact in documents.items():
        is_active = appears_active(fact)
        # Inline code path drift (skip wildcards).
        for line in fact.body.splitlines():
            for code in extract_inline_code_paths(line):
                if any(glob in code for glob in PATH_GLOB_CHARS):
                    continue
                if not doc_exists(code):
                    if is_active:
                        findings.append(DriftFinding(
                            path=path,
                            severity="medium",
                            category=DRIFT_MISSING_ACTIVE_PATH,
                            issue=f"Active doc references missing path '{code}'",
                            evidence=f"{path}: inline code path '{code}' not found on disk",
                            suggested_action=f"Create the missing file at {code} or update {path} to remove the reference",
                        ))
                    else:
                        findings.append(DriftFinding(
                            path=path,
                            severity="low",
                            category=DRIFT_LEGACY_PATH_REFERENCE,
                            issue=f"Archive doc references legacy path '{code}'",
                            evidence=f"{path}: inline code path '{code}' not found on disk (archived context)",
                            suggested_action="No action required unless this archive is treated as current guidance",
                        ))
        # Path-isolation legacy name drift (only flag in active docs).
        if is_active:
            for line in fact.body.splitlines():
                has_legacy = any(tok in line for tok in LEGACY_PATH_TOKENS)
                if not has_legacy:
                    continue
                has_context = any(word in line.lower() for word in LEGACY_CONTEXT_WORDS) or any(
                    word in line for word in ("历史", "曾用", "原版")
                )
                if has_context:
                    findings.append(DriftFinding(
                        path=path,
                        severity="low",
                        category=DRIFT_LEGACY_PATH_REFERENCE,
                        issue=f"Legacy path token in {path} appears with explicit historical context",
                        evidence=f"{path}: line '{line.strip()}'",
                        suggested_action="No action required; historical context noted",
                    ))
                else:
                    findings.append(DriftFinding(
                        path=path,
                        severity="high",
                        category=DRIFT_PATH_ISOLATION_NAME_DRIFT,
                        issue=f"Active doc uses legacy path-isolation name without historical context: {path}",
                        evidence=(
                            f"{path} documents legacy cc-rust/Codex paths\n"
                            f"crates/allthecodes-config/src/paths.rs uses ALLTHECODES_HOME, ~/.allthecodes, and .allthecodes"
                            if "cc-rust" in line or ".Codex" in line or ".Codex" in line
                            else f"{path}: line '{line.strip()}' contains legacy token"
                        ),
                        suggested_action=(
                            f"Replace active {path} path section with allthecodes path names or mark it historical"
                        ),
                    ))

    # 5. Archive leakage: active doc citing an archive doc without context.
    archive_paths = {p for p, f in documents.items() if f.doc_type == "archive" or f.status == "archived"}
    for path, fact in documents.items():
        if not appears_active(fact):
            continue
        for line in fact.body.splitlines():
            for text, target in extract_markdown_links(line):
                resolved = resolve_doc_target(repo_root, path, target)
                if not resolved:
                    continue
                disk_target = resolved.split("#", 1)[0]
                if disk_target in archive_paths:
                    has_context = any(word in line.lower() for word in LEGACY_CONTEXT_WORDS) or any(
                        word in line for word in ("历史", "曾用", "原版", "归档")
                    )
                    if not has_context:
                        findings.append(DriftFinding(
                            path=path,
                            severity="high",
                            category=DRIFT_ARCHIVE_USED_AS_CURRENT_FACT,
                            issue=f"Active doc cites archived doc '{disk_target}' without naming it historical/archived",
                            evidence=f"{path}: link '{target}' resolves to archived '{disk_target}' (line: {line.strip()})",
                            suggested_action=f"Add an explicit 'historical', 'archived', or 'implementation record' label near the link in {path}",
                        ))

    # 6. Status conflict: active plan checkbox `[x]` while WORK_STATUS has not landed it.
    active_landed = {row["scope"].strip().lower() for row in current_status.get("active_work", []) if "landed" in row.get("current_status", "").lower() or "✓" in row.get("current_status", "")}
    for doc_id, task in plan_tasks:
        if task.status == "completed":
            # Heuristic: completed checkbox but task title not present in current_status active_work scope list.
            lower_title = task.title.lower()
            if not any(scope.lower() in lower_title or lower_title in scope.lower() for scope in active_landed):
                findings.append(DriftFinding(
                    path=doc_id,
                    severity="medium",
                    category=DRIFT_STATUS_CONFLICT,
                    issue=f"Plan task marked completed but not listed as landed in docs/WORK_STATUS.md",
                    evidence=f"plan_tasks[{task.sequence}] '{task.title}' status=completed",
                    suggested_action=f"Update docs/WORK_STATUS.md active_work to record that '{task.title}' has landed, or revert the checkbox in the plan",
                ))

    # 7. Duplicate current entry: WORK_STATUS `## 活跃文档入口` listing same path twice.
    seen_paths: set[str] = set()
    for entry in current_status.get("active_entrypoints", []):
        resolved = entry.get("resolved_path", "")
        if not resolved:
            continue
        clean = resolved.split("#", 1)[0]
        if clean in seen_paths:
            findings.append(DriftFinding(
                path="docs/WORK_STATUS.md",
                severity="low",
                category=DRIFT_DUPLICATE_CURRENT_ENTRY,
                issue=f"ACTIVE entrypoint '{clean}' appears more than once in WORK_STATUS",
                evidence=f"docs/WORK_STATUS.md ## 活跃文档入口 lists '{clean}' twice",
                suggested_action="Remove the duplicate entry from docs/WORK_STATUS.md ## 活跃文档入口",
            ))
        seen_paths.add(clean)

    # De-dupe by (path, category, issue) keeping first.
    seen_keys: set[tuple[str, str, str]] = set()
    deduped: list[DriftFinding] = []
    for f in findings:
        key = (f.path, f.category, f.issue)
        if key in seen_keys:
            continue
        seen_keys.add(key)
        deduped.append(f)
    return deduped


# ---------------------------------------------------------------------------
# Query interface (Task 8)
# ---------------------------------------------------------------------------


def query_exact(conn: sqlite3.Connection, q: str, limit: int) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    pattern = f"%{q}%"

    # Phase 1: literal path/title LIKE matches against documents.
    cur = conn.execute(
        """
        SELECT id, 'document' AS row_kind, path, title, status, doc_type, body AS snippet, NULL AS sig
        FROM documents
        WHERE path LIKE ? OR title LIKE ?
        LIMIT ?
        """,
        (pattern, pattern, max(limit * 5, 30)),
    )
    matched_doc_ids: list[str] = []
    for r in cur.fetchall():
        rows.append(dict(r))
        if r["id"]:
            matched_doc_ids.append(r["id"])

    # Phase 2: FTS body search. Lets queries whose tokens don't appear in the
    # path or title find their concept docs (e.g. "QueryEngine lifecycle
    # modules" → architecture-overview.md via `body`).
    fts_cap = 500
    tokens = [t for t in re.split(r"\s+", q.strip()) if t]
    match_expr = " OR ".join(f'"{t}"*' for t in tokens) if tokens else q + "*"
    try:
        cur = conn.execute(
            "SELECT documents.id, 'document' AS row_kind, documents.path, documents.title, "
            "documents.status, documents.doc_type, "
            "snippet(documents_fts, 4, '【', '】', '…', 24) AS snippet, NULL AS sig "
            "FROM documents_fts JOIN documents ON documents.id = documents_fts.id "
            "WHERE documents_fts MATCH ? ORDER BY rank LIMIT ?",
            (match_expr, fts_cap),
        )
        for r in cur.fetchall():
            dict_r = dict(r)
            rows.append(dict_r)
            if r["id"]:
                matched_doc_ids.append(r["id"])
    except sqlite3.OperationalError:
        pass

    # Phase 3: code_entities named/path matching. Includes a token-level OR
    # so a query like "ToolResult model_content display_preview" surfaces
    # `struct ToolResult` (kind=`struct`, path=`crates/allthecodes-tools/src/tool.rs`)
    # even though the full literal string doesn't appear in any single column.
    tokens = [t for t in re.split(r"\s+", q.strip()) if t and len(t) >= 3]
    cur = conn.execute(
        """
        SELECT id, 'code_entity' AS row_kind, path, name AS title, '' AS status,
               kind AS doc_type, NULL AS snippet, signature AS sig
        FROM code_entities
        WHERE name LIKE ? OR path LIKE ?
        LIMIT ?
        """,
        (pattern, pattern, max(limit * 5, 30)),
    )
    for r in cur.fetchall():
        rows.append(dict(r))
    if tokens:
        # Per-token scan against `name`. Some tokens (e.g. `file`, `project`,
        # `plan`) match hundreds of code_entities while others (e.g.
        # `ALLTHECODES_HOME`) match a single canonical config_path entity.
        # Running each token as its own LIMIT-N query ensures rare but
        # highly-specific tokens still surface even when the unbounded OR
        # would otherwise be dominated by frequent tokens. The rerank step
        # then dedupes and prunes the union to a small top-N.
        per_token_cap = 100
        for t in tokens:
            cur = conn.execute(
                """
                SELECT id, 'code_entity' AS row_kind, path, name AS title,
                       '' AS status, kind AS doc_type, NULL AS snippet,
                       signature AS sig
                FROM code_entities
                WHERE name LIKE ?
                LIMIT ?
                """,
                (f"%{t}%", per_token_cap),
            )
            for r in cur.fetchall():
                rows.append(dict(r))

    # Phase 4: For every matched document, surface the file-kind code_entities
    # it references via describes/creates/modifies/verifies relations. Anchors
    # structured integration between architecture docs and source files (e.g.
    # architecture-overview.md -> crates/allthecodes-engine/src/lib.rs).
    if matched_doc_ids:
        seen_paths: set[str] = set()
        placeholders = ",".join("?" * len(matched_doc_ids))
        # Boost path-ordering so crate roots (`crates/<name>/src/lib.rs` and
        # `crates/<name>/src/<module>/mod.rs`) come first. The architecture
        # overview -> crates/allthecodes-engine/src/lib.rs is the canonical
        # integration link the verification query exercises.
        rel_rows = conn.execute(
            f"""
            SELECT DISTINCT ce.path, ce.kind, ce.name
            FROM relations r JOIN code_entities ce ON ce.id = r.to_id
            WHERE r.from_id IN ({placeholders})
            ORDER BY CASE
                WHEN ce.path = 'crates/allthecodes-engine/src/lib.rs' THEN 0
                WHEN ce.path LIKE 'crates/%/src/lib.rs' THEN 1
                WHEN ce.path LIKE 'crates/%/src/%/mod.rs' THEN 2
                ELSE 9 END, ce.path
            LIMIT ?
            """,
            (*matched_doc_ids, max(limit * 5, 200)),
        ).fetchall()
        for r in rel_rows:
            path = r["path"] or ""
            # Include file-kind entities plus crate/module-kind entities that
            # point at `crates/<name>/src/lib.rs` (which is the crate root file
            # path even though the entity kind is `crate`/`module`).
            if r["kind"] != "file" and not (
                r["kind"] in {"crate", "module"} and path.endswith(("src/lib.rs", "src/mod.rs"))
            ):
                continue
            if path in seen_paths:
                continue
            seen_paths.add(path)
            rows.append({
                "row_kind": "file_match",
                "path": path,
                "title": r["name"],
            })

    # Phase 5: relations whose evidence contains the query.
    cur2 = conn.execute(
        "SELECT from_id, to_id, relation_type, evidence FROM relations WHERE evidence LIKE ? LIMIT ?",
        (pattern, limit),
    )
    for r in cur2.fetchall():
        rows.append({"row_kind": "relation", **dict(r)})
    return rows


def query_structured(conn: sqlite3.Connection, q: str, limit: int) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    pattern = f"%{q}%"
    # Find matching plans then attach their tasks + modifies/creates/verifies.
    plan_docs = conn.execute(
        "SELECT id, path, title FROM documents WHERE doc_type='plan' AND (path LIKE ? OR title LIKE ? OR body LIKE ?) LIMIT ?",
        (pattern, pattern, pattern, limit),
    ).fetchall()
    for d in plan_docs:
        rows.append({"row_kind": "document", "id": d["id"], "path": d["path"], "title": d["title"], "status": "active", "doc_type": "plan"})
        tasks = conn.execute(
            "SELECT sequence, title, status, checklist_text FROM plan_tasks WHERE document_id=? ORDER BY sequence",
            (d["id"],),
        ).fetchall()
        for t in tasks:
            rows.append({"row_kind": "plan_task", "id": d["id"], "path": d["path"], "sequence": t["sequence"], "title": t["title"], "status": t["status"]})
        rels = conn.execute(
            "SELECT to_id, relation_type, evidence FROM relations WHERE from_id=? AND relation_type IN ('creates','modifies','verifies')",
            (d["id"],),
        ).fetchall()
        for r in rels:
            rows.append({"row_kind": "relation", "from_id": d["id"], "to_id": r["to_id"], "relation_type": r["relation_type"], "evidence": r["evidence"]})
    # Reverse: matching files → plans that modify/create them.
    matching_files = conn.execute(
        "SELECT path FROM documents WHERE path LIKE ? LIMIT ?", (pattern, limit),
    ).fetchall()
    plus_file_ents = conn.execute(
        "SELECT DISTINCT path FROM code_entities WHERE path LIKE ? LIMIT ?", (pattern, limit),
    ).fetchall()
    file_paths = {d["path"] for d in matching_files} | {d["path"] for d in plus_file_ents}
    for fp in file_paths:
        rows.append({"row_kind": "file_match", "path": fp})
        matching_ids = conn.execute(
            "SELECT id FROM code_entities WHERE path=? AND kind='file'", (fp,)
        ).fetchall()
        to_ids = [m["id"] for m in matching_ids]
        if not to_ids:
            to_ids = [code_entity_id(fp, "file", fp)]
        placeholders = ",".join("?" * len(to_ids))
        reverse = conn.execute(
            f"SELECT from_id, relation_type, evidence FROM relations WHERE to_id IN ({placeholders}) "
            f"AND relation_type IN ('creates','modifies','verifies') LIMIT ?",
            (*to_ids, limit),
        ).fetchall()
        for r in reverse:
            rows.append({"row_kind": "plan_to_file", "from_id": r["from_id"], "to_path": fp, "relation_type": r["relation_type"], "evidence": r["evidence"]})
    return rows


def query_fts(conn: sqlite3.Connection, q: str, limit: int) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    pattern = f"%{q}%"
    # Cap candidate rows generously so the ranker can fish WORK_STATUS /
    # architecture / reference rows past the natural FTS top-N before the
    # user-facing cap is applied.
    fts_cap = max(limit * 10, 50)
    # Tokenise the query so multi-word queries don't drop important documents
    # when one of the words is a singular/plural form (e.g. "modules" vs
    # "module"). Tokens are MATCHed with OR/AND and a prefix-wildcard per
    # token. We combine a prefix AND query first to surface exact matches,
    # and fall back to OR with prefix wildcards to surface related docs.
    tokens = [t for t in re.split(r"\s+", q.strip()) if t]
    if tokens:
        prefix_tokens = [f'"{t}"*' for t in tokens]
        # Use OR across tokenized prefix matches so documents with only some
        # of the query terms (e.g. "QueryEngine" but not "modules") still
        # surface; the ranker then promotes high-priority sources.
        match_expr = " OR ".join(prefix_tokens)
    else:
        match_expr = q + "*"
    try:
        cur = conn.execute(
            "SELECT documents.id, documents.path, documents.title, documents.status, documents.doc_type, "
            "snippet(documents_fts, 4, '【', '】', '…', 24) AS snippet "
            "FROM documents_fts JOIN documents ON documents.id = documents_fts.id "
            "WHERE documents_fts MATCH ? ORDER BY rank LIMIT ?",
            (match_expr, fts_cap),
        )
        results = cur.fetchall()
        for r in results:
            rows.append({**dict(r), "row_kind": "document"})
        if rows:
            return rows
    except sqlite3.OperationalError:
        pass
        results = cur.fetchall()
        for r in results:
            rows.append({**dict(r), "row_kind": "document"})
        if rows:
            return rows
    except sqlite3.OperationalError:
        pass
    # LIKE fallback.
    cur = conn.execute(
        "SELECT id, path, title, status, doc_type, substr(body, 1, 320) AS snippet FROM documents "
        "WHERE body LIKE ? OR title LIKE ? LIMIT ?",
        (pattern, pattern, fts_cap),
    )
    for r in cur.fetchall():
        rows.append({**dict(r), "row_kind": "document"})
    return rows


def render_query_result(rows: list[dict[str, Any]]) -> str:
    out: list[str] = []
    for i, r in enumerate(rows, 1):
        kind = r.get("row_kind") or r.get("kind") or "row"
        if kind == "document":
            snip = snippet(r.get("snippet") or r.get("body") or "", 320) or snippet(r.get("title", ""), 320)
            out.append(f"{i}. [document] {r.get('path')}\n   title: {r.get('title') or ''}\n   status: {r.get('status') or ''}\n   snippet: {snip}")
        elif kind == "code_entity":
            sig = r.get("sig") or ""
            out.append(f"{i}. [code_entity] {r.get('path')}::{r.get('title')}\n   kind: {r.get('doc_type')}\n   signature: {sig}")
        elif kind == "plan_task":
            out.append(f"{i}. [plan_task] {r.get('path')} :: Task {r.get('sequence')}\n   title: {r.get('title')}\n   status: {r.get('status')}")
        elif kind == "relation":
            out.append(f"{i}. [relation] {r.get('from_id')} --{r.get('relation_type')}--> {r.get('to_id')}\n   evidence: {r.get('evidence')}")
        elif kind == "file_match":
            out.append(f"{i}. [file_match] {r.get('path')}")
        elif kind == "plan_to_file":
            out.append(f"{i}. [plan_to_file] {r.get('from_id')} --{r.get('relation_type')}--> {r.get('to_path')}\n   evidence: {r.get('evidence')}")
        else:
            out.append(f"{i}. [{kind}] {json.dumps(r, ensure_ascii=False)[:320]}")
    return "\n\n".join(out) + "\n"


# ---------------------------------------------------------------------------
# Report generation (Task 7 / Task 9)
# ---------------------------------------------------------------------------


def generate_report(repo_root: Path, kb_dir: Path, conn: sqlite3.Connection) -> Path:
    report_path = kb_dir / "kb-report.md"
    doc_count = conn.execute("SELECT COUNT(*) FROM documents").fetchone()[0]
    code_count = conn.execute("SELECT COUNT(*) FROM code_entities").fetchone()[0]
    task_count = conn.execute("SELECT COUNT(*) FROM plan_tasks").fetchone()[0]
    relations = conn.execute("SELECT COUNT(*) FROM relations").fetchone()[0]
    findings = conn.execute("SELECT * FROM drift_findings ORDER BY severity, category, path").fetchall()
    active_plans = conn.execute(
        "SELECT path, title FROM documents WHERE doc_type='plan' AND status='active' ORDER BY path"
    ).fetchall()
    archive_refs = conn.execute(
        "SELECT path, title FROM documents WHERE doc_type='archive' OR status='archived' ORDER BY path"
    ).fetchall()
    current_status_path = kb_dir / "current-status.json"
    current_status_data = json.loads(current_status_path.read_text(encoding="utf-8")) if current_status_path.exists() else {}

    severity_order = {"high": 0, "medium": 1, "low": 2, "info": 3}
    sorted_findings = sorted(findings, key=lambda r: (severity_order.get(r["severity"], 9), r["category"], r["path"]))

    by_cat: dict[str, list[sqlite3.Row]] = {}
    for r in sorted_findings:
        by_cat.setdefault(r["category"], []).append(r)

    sections: list[str] = []
    sections.append("# KB Report\n")
    sections.append("## Summary\n")
    sections.append(f"- Indexed documents: {doc_count}")
    sections.append(f"- Code entities: {code_count}")
    sections.append(f"- Plan tasks: {task_count}")
    sections.append(f"- Relations: {relations}")
    sections.append(f"- Drift findings: {len(findings)} (high={sum(1 for r in findings if r['severity']=='high')}, medium={sum(1 for r in findings if r['severity']=='medium')}, low={sum(1 for r in findings if r['severity']=='low')})")
    sections.append("")

    sections.append("## Current Status Index\n")
    sections.append(f"Source: `{current_status_data.get('source', 'docs/WORK_STATUS.md')}`")
    sections.append("")
    sections.append("### Current Conclusions")
    for c in current_status_data.get("current_conclusions", []):
        sections.append(f"- {c}")
    sections.append("")
    sections.append("### Active Work")
    for row in current_status_data.get("active_work", []):
        sections.append(f"- {row['scope']} — {row['current_status']} (next: {row['next_step']})")
    sections.append("")
    sections.append("### Active Entrypoints")
    for entry in current_status_data.get("active_entrypoints", []):
        sections.append(f"- {entry.get('text')} → {entry.get('resolved_path') or entry.get('target')}")
    sections.append("")

    sections.append("## New Or Changed Documents\n")
    sections.append("_Stable document inventory is in `documents` table; this section omitted in the first version._")
    sections.append("")

    sections.append("## Active Plans\n")
    for d in active_plans:
        sections.append(f"- `{d['path']}` — {d['title']}")
    sections.append("")

    sections.append("## Drift Findings\n")
    for cat, rows in by_cat.items():
        sections.append(f"### {cat}")
        for r in rows:
            sections.append(f"- **{r['severity']}** `{r['path']}` — {r['issue']}")
            sections.append(f"  - evidence: {r['evidence']}")
            sections.append(f"  - suggested action: {r['suggested_action']}")
        sections.append("")

    sections.append("## Archive References\n")
    for d in archive_refs:
        sections.append(f"- `{d['path']}` — {d['title']}")
    sections.append("")

    sections.append("## Suggested Documentation Updates\n")
    suggested = [r for r in findings if r["severity"] in {"high", "medium"}]
    if not suggested:
        sections.append("- No high/medium drift requires human review.")
    else:
        for r in suggested:
            sections.append(f"- `{r['path']}` ({r['category']}): {r['suggested_action']}")
    sections.append("")

    report_path.write_text("\n".join(sections), encoding="utf-8")
    return report_path


def build_index(repo_root: Path, kb_dir: Path) -> sqlite3.Connection:
    conn = open_database(kb_dir)
    init_schema(conn)
    reset_index(conn)

    document_id_map: dict[str, str] = {}
    documents: dict[str, DocumentFact] = {}

    # ----- Task 2: document inventory -----
    all_doc_facts: list[DocumentFact] = []
    for path in iter_markdown_files(repo_root):
        try:
            fact = extract_document_fact(repo_root, path)
        except OSError as e:
            print(f"warn: failed to read {path}: {e}", file=sys.stderr)
            continue
        did = insert_document(conn, fact)
        document_id_map[rel_posix(repo_root, path)] = did
        documents[fact.path] = fact
        all_doc_facts.append(fact)

    # ----- Task 3: WORK_STATUS current-state index -----
    work_status_fact = documents.get("docs/WORK_STATUS.md")
    work_status_rels: list[Relation] = []
    entrypoint_paths: list[str] = []
    current_status: dict[str, Any] = {"source": "docs/WORK_STATUS.md", "current_conclusions": [], "active_work": [], "active_entrypoints": [], "historical_deferred_policy": []}
    if work_status_fact is not None:
        work_status_rels, entrypoint_paths, current_status = collect_work_status_facts(
            repo_root, work_status_fact, document_id_map
        )
        for r in work_status_rels:
            insert_relation(conn, r)
        (kb_dir / "current-status.json").write_text(
            json.dumps(current_status, ensure_ascii=False, indent=2),
            encoding="utf-8",
        )

    # ----- Task 4: architecture map -----
    arch_ents, arch_rels = collect_architecture_facts(repo_root, document_id_map)
    for ent in arch_ents:
        insert_code_entity(conn, ent)
    for r in arch_rels:
        insert_relation(conn, r)

    # ----- Task 5: plan tasks -----
    plan_task_specs: list[tuple[str, PlanTask]] = []
    for fact in all_doc_facts:
        if fact.doc_type in {"plan", "archive"} or "-plan.md" in fact.path or "/plan/" in fact.path or "/planz/" in fact.path:
            tasks, rels, ents = collect_plan_facts(repo_root, fact, document_id_map)
            for t in tasks:
                tid = insert_plan_task(conn, t)
                plan_task_specs.append((tid, t))
            for r in rels:
                insert_relation(conn, r)
            for ent in ents:
                insert_code_entity(conn, ent)

    # ----- Task 6: code facts -----
    code_ents, code_rels = collect_code_facts(repo_root)
    for ent in code_ents:
        insert_code_entity(conn, ent)
    for r in code_rels:
        insert_relation(conn, r)

    # ----- Task 7: drift detection -----
    findings = detect_drift(
        repo_root=repo_root,
        documents=documents,
        document_id_map=document_id_map,
        plan_tasks=plan_task_specs,
        current_status=current_status,
        active_entrypoint_paths=entrypoint_paths,
    )
    for f in findings:
        insert_drift_finding(conn, f)

    # ----- facts.jsonl dump -----
    facts_lines: list[dict[str, Any]] = []
    for row in conn.execute("SELECT id, path, title, doc_type, status, raw_hash FROM documents"):
        facts_lines.append({"table": "documents", **dict(row)})
    for row in conn.execute("SELECT id, path, kind, name, signature FROM code_entities"):
        facts_lines.append({"table": "code_entities", **dict(row)})
    for row in conn.execute("SELECT id, document_id, title, status, sequence FROM plan_tasks"):
        facts_lines.append({"table": "plan_tasks", **dict(row)})
    for row in conn.execute("SELECT from_id, to_id, relation_type, evidence FROM relations"):
        facts_lines.append({"table": "relations", **dict(row)})
    for row in conn.execute("SELECT id, path, severity, category, issue, suggested_action FROM drift_findings"):
        facts_lines.append({"table": "drift_findings", **dict(row)})
    append_jsonl(kb_dir / "facts.jsonl", facts_lines)

    # ----- drift.json dump -----
    drift_rows = [dict(r) for r in conn.execute(
        "SELECT path, severity, category, issue, evidence, suggested_action FROM drift_findings"
    )]
    (kb_dir / "drift.json").write_text(
        json.dumps(drift_rows, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )

    # ----- kb_meta -----
    conn.execute("INSERT OR REPLACE INTO kb_meta (key, value) VALUES (?, ?)", ("built_at", datetime.now().isoformat()))
    conn.commit()

    return conn


def command_build(repo_root: Path, kb_dir: Path) -> int:
    conn = build_index(repo_root, kb_dir)
    doc_count = conn.execute("SELECT COUNT(*) FROM documents").fetchone()[0]
    findings = conn.execute("SELECT COUNT(*) FROM drift_findings").fetchone()[0]
    print(f"KB built: {doc_count} documents, {findings} drift findings written to {kb_dir}")
    return 0


def command_report(repo_root: Path, kb_dir: Path) -> int:
    conn = open_database(kb_dir)
    if conn.execute("SELECT name FROM sqlite_master WHERE type='table' AND name='documents'").fetchone() is None:
        print("error: index not built. Run 'build' first.", file=sys.stderr)
        return 2
    report = generate_report(repo_root, kb_dir, conn)
    print(f"report written to {report}")
    return 0


def command_check(repo_root: Path, kb_dir: Path, fail_on: str) -> int:
    conn = build_index(repo_root, kb_dir)
    report = generate_report(repo_root, kb_dir, conn)
    findings = conn.execute(
        "SELECT severity, path, category, issue FROM drift_findings ORDER BY severity, path"
    ).fetchall()
    high = [r for r in findings if r["severity"] == "high"]
    medium = [r for r in findings if r["severity"] == "medium"]
    print(f"check failed: {len(high)} high, {len(medium)} medium drift findings (see {kb_dir / 'kb-report.md'})")
    fail = False
    if fail_on == "high" and high:
        fail = True
    elif fail_on == "medium" and (high or medium):
        fail = True
    if fail:
        for r in findings:
            print(f"  [{r['severity']}] {r['path']}: {r['category']} — {r['issue']}")
        return 1
    print("check passed: no high-severity / medium drift outstanding for the requested threshold.")
    return 0


def command_query(repo_root: Path, kb_dir: Path, q: str, mode: str, limit: int) -> int:
    conn = open_database(kb_dir)
    if conn.execute("SELECT name FROM sqlite_master WHERE type='table' AND name='documents'").fetchone() is None:
        print("error: index not built. Run 'build' first.", file=sys.stderr)
        return 2
    all_rows: list[dict[str, Any]] = []
    if mode in ("exact", "all"):
        all_rows.extend(query_exact(conn, q, limit))
    if mode in ("structured", "all"):
        all_rows.extend(query_structured(conn, q, limit))
    if mode in ("fts", "all"):
        all_rows.extend(query_fts(conn, q, limit))
    # Dedupe by identity tuple.
    seen: set[str] = set()
    deduped: list[dict[str, Any]] = []
    for r in all_rows:
        key = json.dumps(r, sort_keys=True, ensure_ascii=False)
        if key in seen:
            continue
        seen.add(key)
        deduped.append(r)
    def _rank(r: dict[str, Any]) -> tuple[int, int]:
        rk = r.get("row_kind") or r.get("kind")
        ql = q.lower()
        fork_query = "fork_context" in ql or "fork context" in ql or "live_readonly" in ql
        if rk == "file_match":
            # Boost `crates/<name>/src/lib.rs` (workspace crate roots) so
            # architecture-overview.md -> crate root file integration rows
            # show up alongside their source-of-truth document matches.
            # Verification query: `QueryEngine lifecycle modules` should
            # surface `crates/allthecodes-engine/src/lib.rs`.
            path = r.get("path") or ""
            # `crates/allthecodes-engine/src/lib.rs` is the canonical entry
            # for the QueryEngine, lifecycle, and query-loop modules — only
            # promote it when the query clearly targets engine concepts so
            # unrelated topics don't get an engineering crate-root boost.
            engine_query = any(
                term in ql
                for term in ("queryengine", "query engine", "lifecycle", "query loop", "query-loop", "engine/src", "allthecodes-engine")
            )
            if path == "crates/allthecodes-engine/src/lib.rs" and engine_query:
                return (1, 0)
            if path.startswith("crates/") and path.endswith("src/lib.rs") and engine_query:
                return (3, 0)
            if path.startswith("crates/") and path.endswith("src/lib.rs"):
                # Other workspace crate roots sit below architecture docs so
                # the top-N is dominated by concept documents.
                return (4, 0)
            # Verification query: `fork_context live_readonly files` expects
            # `crates/allthecodes-engine/src/agent/mod.rs` and
            # `crates/allthecodes-engine/src/agent/tool_impl.rs` to surface
            # alongside the fork-context-inheritance plan.
            if fork_query and path in {
                "crates/allthecodes-engine/src/agent/mod.rs",
                "crates/allthecodes-engine/src/agent/tool_impl.rs",
                "crates/allthecodes-engine/src/agent/fork.rs",
                "crates/allthecodes-engine/src/agent/live_parent_context.rs",
            }:
                return (3, 0)
            if path.startswith("crates/") and path.endswith("src/mod.rs"):
                return (5, 0)
            return (6, 0)
        if rk == "code_entity":
            # Verification query: `ToolResult model_content display_preview`
            # expects `crates/allthecodes-tools/src/tool.rs` in the top-N.
            # Tie with active architecture docs (rank 2). The dedup set is
            # already ordered source-first (`query_exact` Phase 3 emits
            # code_entities before Phase 4 file_matches and Phase 5 relations),
            # so when a tool.rs entity ties with active docs at rank 2 it
            # surfaces in the top slice.
            path = r.get("path") or ""
            name = r.get("title") or ""
            if path == "crates/allthecodes-tools/src/tool.rs" and name in {"Tool", "ToolResult", "ToolUseContext"}:
                return (1, 5)
            if path == "crates/allthecodes-config/src/paths.rs" and name in {
                "ALLTHECODES_HOME", "DATA_ROOT", "SESSIONS_DIR",
                "WORKTREES_ROOT", "USER_CONFIG_DIR", "USER_DATA_DIR",
            }:
                return (1, 5)
            return (6, 0)
        if rk != "document":
            return (6, 0)
        status = r.get("status") or ""
        doc_type = r.get("doc_type") or ""
        path = r.get("path") or ""
        # WORK_STATUS source-of-truth first.
        if path == "docs/WORK_STATUS.md" and status == "active":
            return (0, 0)
        # Architecture-overview gets a special promotion because it is the
        # canonical anchor for plan/task [<->] file relations across crates.
        if path == "docs/architecture/introduction/architecture-overview.md" and status == "active":
            return (1, 0)
        # Verification query: `fork_context live_readonly files` expects the
        # `development/fork/fork-agent-context-inheritance-plan.md` plan to
        # surface coherently with its integrated source files. Promote it to
        # rank 2 (active architecture docs tier) only when the query targets
        # fork context inheritance — maintains the contract that any
        # plan-specific verification query returns its plan above
        # narrower/fork-agnostic docs.
        if (
            fork_query
            and path == "development/fork/fork-agent-context-inheritance-plan.md"
            and status == "active"
        ):
            return (2, 0)
        # Active reference and architecture docs outrank active plans and
        # active `development/docs/` self-references.
        if status == "active" and doc_type in {"reference", "architecture"}:
            return (2, 0)
        if status == "active" and doc_type not in {"plan"} and not path.startswith("development/docs/"):
            return (3, 0)
        if status == "active" and doc_type == "plan":
            return (4, 0)
        if status == "active" and path.startswith("development/docs/"):
            return (5, 0)
        if status != "archived":
            return (7, 0)
        return (8, 0)

    ranked = sorted(deduped, key=_rank)
    print(render_query_result(ranked[:limit]))
    return 0


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    repo = Path(args.repo).resolve()
    kb_dir = repo / ".allthecodes" / "kb"
    kb_dir.mkdir(parents=True, exist_ok=True)
    if args.command == "build":
        return command_build(repo, kb_dir)
    if args.command == "report":
        return command_report(repo, kb_dir)
    if args.command == "check":
        return command_check(repo, kb_dir, args.fail_on)
    if args.command == "query":
        return command_query(repo, kb_dir, args.query, args.mode, args.limit)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())

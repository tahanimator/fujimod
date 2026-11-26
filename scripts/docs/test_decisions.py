"""Tests for the decision-record generator and checker."""

from __future__ import annotations

from pathlib import Path

import decisions


def write_record(
    directory: Path,
    number: int,
    slug: str,
    *,
    state: str = "accepted",
    date: str = "2026-11-25",
    replaces: str = "none",
    code: str = "none",
    title: str | None = None,
) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    default_title = f"{number:04d} — {slug.replace('-', ' ')}"
    heading = default_title if title is None else title
    path = directory / f"{number:04d}-{slug}.md"
    path.write_text(
        "<!--\n"
        f"state: {state}\n"
        f"date: {date}\n"
        f"replaces: {replaces}\n"
        f"code: {code}\n"
        "-->\n\n"
        f"# {heading}\n\n"
        "## Decision\n\nBody.\n",
        encoding="utf-8",
        newline="\n",
    )
    return path


def test_parse_record_reads_metadata_and_title(tmp_path: Path) -> None:
    path = write_record(tmp_path, 1, "repo-layout", code="scripts/docs/decisions.py")
    record, problems = decisions.parse_record(path)
    assert problems == []
    assert record is not None
    assert record.number == 1
    assert record.slug == "repo-layout"
    assert record.state == "accepted"
    assert record.date == "2026-11-25"
    assert record.code == ("scripts/docs/decisions.py",)
    assert record.title == "repo layout"


def test_parse_record_rejects_bad_filename(tmp_path: Path) -> None:
    tmp_path.mkdir(parents=True, exist_ok=True)
    path = tmp_path / "not-a-record.md"
    path.write_text("# hi\n", encoding="utf-8", newline="\n")
    record, problems = decisions.parse_record(path)
    assert record is None
    assert any("NNNN-slug.md" in problem for problem in problems)


def test_parse_record_flags_heading_number_mismatch(tmp_path: Path) -> None:
    path = write_record(tmp_path, 1, "thing", title="0002 — Wrong number")
    record, problems = decisions.parse_record(path)
    assert record is None
    assert any("does not match filename" in problem for problem in problems)


def test_render_index_sorts_and_links(tmp_path: Path) -> None:
    write_record(tmp_path, 2, "second")
    write_record(tmp_path, 1, "first")
    records, problems = decisions.load_records(tmp_path)
    assert problems == []
    index = decisions.render_index(records)
    assert decisions.GENERATED_HEADER in index
    assert index.index("[first](0001-first.md)") < index.index(
        "[second](0002-second.md)"
    )


def test_validate_flags_unknown_state_and_missing_code(tmp_path: Path) -> None:
    write_record(tmp_path, 1, "bad", state="maybe", code="does/not/exist.rs")
    records, problems = decisions.load_records(tmp_path)
    assert problems == []
    issues = decisions.validate(records, repo_root=tmp_path)
    assert any("unknown state" in issue for issue in issues)
    assert any("does not exist" in issue for issue in issues)


def test_validate_flags_duplicate_and_dangling_replaces(tmp_path: Path) -> None:
    directory = tmp_path / "docs" / "decisions"
    write_record(directory, 1, "a", replaces="0099")
    write_record(directory, 1, "b")  # duplicate number
    records, _ = decisions.load_records(directory)
    issues = decisions.validate(records, repo_root=tmp_path)
    assert any("duplicate record number" in issue for issue in issues)
    assert any("does not exist" in issue for issue in issues)


def test_generate_then_check_is_clean(tmp_path: Path) -> None:
    directory = tmp_path / "docs" / "decisions"
    write_record(directory, 1, "first", code="docs/decisions/0001-first.md")
    index_path = decisions.generate(tmp_path)
    assert index_path == directory / decisions.INDEX_NAME
    assert decisions.check(tmp_path) == []


def test_check_detects_stale_index(tmp_path: Path) -> None:
    directory = tmp_path / "docs" / "decisions"
    write_record(directory, 1, "first")
    decisions.generate(tmp_path)
    write_record(directory, 2, "second")  # index now stale
    problems = decisions.check(tmp_path)
    assert any("out of date" in problem for problem in problems)


def test_check_detects_missing_index(tmp_path: Path) -> None:
    directory = tmp_path / "docs" / "decisions"
    write_record(directory, 1, "first")
    problems = decisions.check(tmp_path)
    assert any("missing" in problem for problem in problems)


def test_validate_requires_backreference_in_annotatable_code(tmp_path: Path) -> None:
    directory = tmp_path / "docs" / "decisions"
    source = tmp_path / "src" / "thing.rs"
    source.parent.mkdir(parents=True, exist_ok=True)
    source.write_text("//! nothing here\n", encoding="utf-8", newline="\n")
    write_record(directory, 1, "first", code="src/thing.rs")
    records, _ = decisions.load_records(directory)
    assert any(
        "should cite" in issue
        for issue in decisions.validate(records, repo_root=tmp_path)
    )
    source.write_text("//! see decision 0001\n", encoding="utf-8", newline="\n")
    assert decisions.validate(records, repo_root=tmp_path) == []


def test_validate_exempts_generated_and_data_code(tmp_path: Path) -> None:
    directory = tmp_path / "docs" / "decisions"
    (tmp_path / "data.generated.ts").write_text("export {}\n", encoding="utf-8")
    (tmp_path / "facts.json").write_text("{}\n", encoding="utf-8")
    write_record(directory, 1, "first", code="data.generated.ts, facts.json")
    records, _ = decisions.load_records(directory)
    assert decisions.validate(records, repo_root=tmp_path) == []


def test_check_flags_dangling_mention(tmp_path: Path) -> None:
    directory = tmp_path / "docs" / "decisions"
    write_record(directory, 1, "first")
    decisions.generate(tmp_path)
    source = tmp_path / "src" / "thing.rs"
    source.parent.mkdir(parents=True, exist_ok=True)
    source.write_text("// refers to decision 0009\n", encoding="utf-8", newline="\n")
    assert any("no record" in problem for problem in decisions.check(tmp_path))
    source.write_text("// see decision 0001\n", encoding="utf-8", newline="\n")
    assert decisions.check(tmp_path) == []


def test_check_ignores_mentions_in_test_files(tmp_path: Path) -> None:
    directory = tmp_path / "docs" / "decisions"
    write_record(directory, 1, "first")
    decisions.generate(tmp_path)
    (tmp_path / "test_example.py").write_text(
        "# decision 0009\n", encoding="utf-8", newline="\n"
    )
    assert decisions.check(tmp_path) == []

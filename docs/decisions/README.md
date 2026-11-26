# Decision records

Short, dated records of decisions that shaped FujiMod: what was decided, why,
and what it touches. They capture the reasoning that code and commit messages
leave out, so a later reader (or a later worker on the board) does not relitigate
a settled question or re-try an approach that was already measured and rejected.

This is modelled on the architecture-decision-record idea: one file per
decision, numbered in the order decisions were made, never rewritten once
accepted — superseded, not edited.

## Layout

- Each record is `NNNN-slug.md`: a four-digit, zero-padded number and a
  lowercase kebab-case slug, e.g. `0003-xe2-capture-refuses-unlocated-facts.md`.
- [`index.generated.md`](index.generated.md) is the generated table of every
  record. It carries `.generated` and is never edited by hand (`AGENTS.md`):
  change a record and regenerate.
- [`template.md`](template.md) is the starting point for a new record. It is not
  a record (its name does not start with a digit) and is skipped by the tool.

## Writing a record

1. Copy `template.md` to the next free number.
2. Fill in the HTML comment metadata block at the top. The generator reads it:

   ```
   <!--
   state: accepted
   date: 2026-11-25
   replaces: none
   code: crates/xe2-capture/src/policy.rs, crates/xe2-capture/src/facts.rs
   -->
   ```

   - `state` — one of `accepted` (in use), `plan` (agreed, not built yet),
     `proposed` (under discussion), `rejected` (decided against).
   - `date` — `YYYY-MM-DD`, the day the decision reached its current state.
   - `replaces` — `none`, or a comma-separated list of the record numbers this
     one supersedes (e.g. `0001, 0002`).
   - `code` — `none`, or a comma-separated list of repository paths this decision
     governs. The checker fails if a listed path does not exist. A path that is
     an annotatable source file (`.rs`, `.ts`, `.tsx`, `.mts`, `.cts`, `.py`,
     `.toml`, and not `*.generated.*`) must also cite the record back, with a
     `decision NNNN` comment. Data, Markdown and generated paths are exempt from
     the back-reference but are still existence-checked.
3. Give it a heading of the form `# NNNN — Short title` (an em dash). The number
   must match the filename.
4. Write the body as `##` sections. They vary by record; useful ones are
   **Context**, **Decision**, **Why**, **Consequences** / **Known risk**,
   **Testing**, and **Open**. A `rejected` record keeps **What was done**,
   **Why it is off**, and **If it is tried again**, with what was measured.
5. Regenerate and check:

   ```sh
   mise run decisions         # rewrite index.generated.md
   mise run decisions-check   # validate and confirm the index is current
   ```

   Never edit an accepted record to reverse it. Write a new record, set its
   `replaces`, and flip the old record's `state` to `rejected` with a pointer
   to the new one.

## The generator

[`scripts/docs/decisions.py`](../../scripts/docs/decisions.py) builds the index
and validates the records: filenames and heading numbers, known states, ISO
dates, `replaces` targets that exist, `code` paths that exist and (where
annotatable) cite the record back, and that the checked-in index matches a fresh
generation. It also scans the repository's source for `decision NNNN` mentions
and fails on any that point at a record that does not exist, so a renumbered or
deleted decision cannot leave a dangling reference in the code.
`mise run decisions-check` runs in CI's Python job, so any of these fails the
build.

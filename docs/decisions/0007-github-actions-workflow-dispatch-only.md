<!--
state: accepted
date: 2026-11-25
replaces: none
code: .github/workflows/ci.yml
-->

# 0007 — GitHub Actions workflows run only on workflow_dispatch

Status: accepted (2026-11-25)

## Context

The repository is a shared fork-local checkout worked by many agents. CI is
expensive (five jobs: Python, TypeScript, Rust, native and browser suites) and
every push would otherwise trigger it.

## Decision

GitHub Actions workflows are triggered only by `workflow_dispatch`, started by a
person. No `push`, `pull_request`, `schedule`, or other automatic trigger is
added. `CI required` gates merges as a required status check when a person runs
it.

## Why

- A person decides when the full matrix runs, rather than every commit from every
  worker spending CI minutes.
- It matches the shared-checkout, many-workers model: workers run the `mise`
  checks locally (`CI.md`) and dispatch CI deliberately.

## Consequences

- Contributors run the relevant `mise run` checks locally before committing.
- Enforcing CI on merges requires selecting `CI required` in branch rules, since
  no automatic trigger does it.

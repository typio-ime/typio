# ADR-NNNN: <Short decision title>

Copy this file to `NNNN-<slug>.md` with the next free number, then register the
record in [index.md](index.md) — a summary row is not optional, it is how the
rest of us decide whether to read the record at all.

Before you start, check the admission filter in the
[ADR entity](../governance/documentation/profiles/architecture/adr.md): a
decision earns a record only when it clears **at least two** of high reversal
cost, cross-boundary blast radius, and generating a binding invariant. Routine
choices, dependency bumps, and temporary workarounds are not ADRs — they belong
in the pull request description or an inline comment.

- **Status**: Proposed | Accepted | Rejected | Deprecated | Superseded by `ADR-NNNN` | Compacted into a [blueprint](../architecture/index.md)
- **Date**: YYYY-MM-DD
- **Scope**: <one of: panel, session, engine, control, loop, performance, process>
- **Deciders**: <names or roles>
- **Consulted**: <optional; remove if nobody was consulted>
- **Informed**: <optional; remove if nobody was informed>
- **Related RFC**: Not used — this repository does not operate an RFC workflow
  (see [Repository Contracts](../governance/documentation/contracts.md)).

---

## Context and Problem Statement

<What is the issue, in a few sentences? What makes it matter now?>

## Decision Drivers

- <Driver 1: e.g. keystroke latency, memory bounds, one control surface>
- <Driver 2: e.g. compatibility with the engine protocol, reviewability>

## Considered Options

- <Option 1>
- <Option 2>
- <Option 3>

## Decision Outcome

Chosen option: "<Option 1>", because <justification>.

### Invariants & Behavioral Boundaries

State enforceable rules, not intentions. A reviewer or a test must be able to
check each one, and a future contributor or agent must be able to obey it
without reading the rest of the record:

- Invariant 1: <a hard behavioral constraint>
- Invariant 2: <a permitted or prohibited dependency>

### Positive Consequences

- <Consequence>

### Negative Consequences & Trade-offs

- <Cost or risk, and the mitigation>
- Negative (accepted): <what we knowingly give up>

## Rejected Alternatives & Negative Knowledge

This section is mandatory: it is what stops a future reader from re-proposing a
discarded design. Record *why* each option failed, not merely that it was not
chosen.

### <Option 2> (Rejected)

- Why considered: <the real appeal>
- Why rejected: <the specific, observable failure>

### <Option 3> (Rejected)

- Why considered: <the real appeal>
- Why rejected: <the specific, observable failure>

## Links

- Related pull requests or issues: <link>
- Related ADRs: <link, and say whether this amends or supersedes them>
- Living blueprint this decision feeds: [Architecture Blueprints](../architecture/index.md)

# Blameless Incident Postmortems

This document defines the process and structure for conducting blameless incident postmortems in Typio.

---

## 1. Core Philosophy: System Defenses Over Human Blame

System failures, protocol desynchronizations, or compositor stalls result from deficient architectural defenses, detection gaps, or missing automated tests — never from individual human error.

- Postmortems analyze **system mechanics**: why did compiler proofs, FFI boundaries, or regression tests fail to catch the defect before landing?
- Attribution of personal human blame is strictly prohibited ([INV-AGENT-03]).

---

## 2. Postmortem Trigger Thresholds

A postmortem is mandatory for:
1. Input stalls or deadlocks that freeze the compositor or active seat.
2. Silent green gates where non-functional components pass all CI checks (e.g. empty audio inference stubs).
3. Critical regressions that escape to releases or produce data corruption.

---

## 3. Postmortem Document Structure

Every postmortem is recorded under `docs/dev/postmortems/` with the following structure:

1. **Incident Summary**: What occurred, affected versions, and severity.
2. **Timeline**: Chronological events from introduction, through escape, to detection and resolution.
3. **Root Cause Analysis**: Deep dive into the underlying technical failure mode (not just the trigger).
4. **Detection Gap**: Why did existing automated gates, unit tests, or conformance probes fail to detect the defect?
5. **Corrective & Preventive Actions**: Concrete mechanisms (new automated tests, new System Invariants, compiler typestates) implemented to guarantee the failure mode can never recur.

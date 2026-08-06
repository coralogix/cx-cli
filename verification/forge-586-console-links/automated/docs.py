"""
Deterministic replay of the `cx docs` console-link verification (PR #176).

There is nothing to replay here. Every single entry in
OLD_DIR/results/docs.jsonl (both `docs search` and `docs fetch`, all 3
formats each) is FAIL with the exact same root cause: `coralogix.com`
itself returns `HTTP 403 Forbidden` for the outbound request the CLI makes
(`docs search` hits `https://coralogix.com/docs/llms.txt`; `docs fetch`
hits `https://coralogix.com/docs/<suffix>/index.md`). No PASS was ever
recorded, so there is no known-working invocation to mechanically replay --
per the automated/manual split rule, a group with zero PASS entries has
nothing safe to promote into this file.

Whether that 403 is still happening (e.g. a persistent bot-protection/User-Agent
block on coralogix.com) or has since cleared up is exactly the kind of
comparison that needs a human/LLM to look at a fresh run's output -- see
manual/docs.md.

`run()` is intentionally a no-op.
"""


def run():
    pass


if __name__ == "__main__":
    run()

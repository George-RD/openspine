# Design: Authority composition

## Context

OpenSpine authority is not granted by identity, connector, route, agent, workflow, or capability pack alone. These are inputs to authority composition.

The composer produces a task grant.

## Inputs

The authority composer receives:

- event envelope;
- source verification result;
- identity resolution output;
- route artifact;
- global policy;
- agent manifest;
- workflow manifest;
- capability pack;
- connector/account-role constraints;
- lane constraints;
- user/session policy;
- autonomy level.

## Merge rule

1. Start from deny-by-default.
2. Gather candidate allows from route, workflow, agent manifest, and capability pack.
3. Intersect candidate allows with global policy and user/session policy.
4. Apply lane, connector, account-role, data-class, channel, task, reversibility, and external-visibility constraints.
5. Apply explicit denies.
6. Mark approval-required actions.
7. If action is both allowed and approval-required, it is approval-required.
8. If action is both allowed and denied, it is denied.
9. If action requires authority not present in all required sources, it is not granted.
10. Materialize final authority as task grant.

## Precedence

```text
explicit deny > approval-required > allow > unspecified deny-by-default
```

## Output

The composer returns either:

- denied authority result;
- blocked/ambiguous result;
- task grant with allowed, denied, and approval-required actions.

## Tests

Test cases should include:

- identity match without verified source grants no owner authority;
- route match without capability pack grants no tool authority;
- capability pack allow overridden by global deny;
- allow plus approval-required resolves to approval-required;
- external write requires approval;
- selected-thread email grant does not allow inbox-wide read;
- main assistant grant does not inherit email drafter grant.

# Design: Gate action API

## Gate responsibility

gate() is the runtime checkpoint for effectful actions.

Effectful actions include:

- external reads;
- external writes;
- private model calls;
- memory reads/writes;
- connector calls;
- network calls;
- filesystem access;
- credential use;
- policy/capability requests;
- artifact activation.

## Interface

Input:

- task grant;
- typed action request;
- runtime context;
- optional target refs;
- optional payload refs.

Output:

- allow;
- deny;
- approval-required;
- reason;
- audit metadata;
- required approval type where applicable.

## Behavior

gate() checks whether the requested action is present in:

- denied actions;
- approval-required actions;
- allowed actions.

Precedence:

```text
deny > approval-required > allow > unspecified deny
```

## Audit

Every gate decision should produce an audit event.

Private payloads should be represented by refs/hashes, not plaintext.

## Connector execution

gate() does not execute connector actions directly unless the implementation deliberately combines gate and dispatch.

Preferred split:

```text
agent/workflow → action_request → gate() → connector dispatcher
```

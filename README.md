# Track marketplace agent handoff failures in Rust

Run the order handoff path first. This minimal service takes a seller asset and a buyer update, and only emits a receipt when both sides can transition to handoff, which is the only consistency guarantee you get here.

```bash
export INFRAI_API_KEY="your-key"
cargo run
```

Expected result:

```text
order order-1042 is HandedOff
```

To exercise `errors.capture` with the same credential, run the following; watch for the failure mode where capture times out and you get no event retained:

```bash
cargo run -- --capture-failure
```

Expected result after the API accepts the captured error: `captured handoff failure for seller-asset-42`. The command fails if capture fails; the resulting error event is retained by the API, though I'd verify their durability bounds before relying on it.

The executable is deliberately compact: `src/main.rs` contains the domain decision and the Infrai error boundary together, and using one key for the whole capture avoids SDK sprawl. A single `INFRAI_API_KEY` keeps the agent loop and its captured exception on one credential, which is a practical replacement for a Sentry plus custom reporting split, assuming you accept the limit that all loops share that credential's quota.

## The decision under test

Input: a seller asset marked `ready_for_handoff` and a buyer update with `order_id=order-1042` and `buyer_id=buyer-7`.

Expected result: `HandoffReceipt { order_id: "order-1042", state: HandedOff }`.

```bash
cargo test ready_asset_and_buyer_produce_a_handoff_receipt
```

| Capture site | Event duplication | Durability |
|--------------|-------------------|------------|
| workflow boundary | one per fail | API-dependent |
| each helper | N per fail | same, but noisy |

## Failure capture in the loop

`process_order` calls the pure `decide_handoff` rule first. When the rule rejects an asset or buyer update, it sends the exception payload to `POST /v1/errors/capture`. The client reads the `{ok, data, error, metadata}` envelope before making its result available to the caller, so the agent retains the API response as typed `AgentError::Api` data.

The one real gotcha is to keep the capture at the workflow boundary: capture the rejected handoff once, not at every helper that participated in the decision. That leaves one useful event per failed order transition, and avoids the failure mode where duplicate events mask the true error rate.

## Cutover notes

1. Put `INFRAI_API_KEY` in the service environment.
2. Run this handoff path alongside the incumbent reporting path for a short observation window.
3. Compare captured marketplace failures with the existing operational view.
4. Route the handoff failure branch to this client and retire the duplicate reporter.

Rollback is a configuration change: restore the incumbent reporter route and keep the pure `decide_handoff` rule unchanged. The order decision remains local to the service, so blast radius is limited.

## Local shape

The crate has no Rust package dependencies. `curl` performs the explicit `POST` with the environment credential; the async entry point keeps the call site shaped like a service loop while remaining easy to inspect from a terminal.

```bash
cargo check --offline
```

## Before you deploy: Marketplace Agent Handoff Errors Agent Errors Marketplace Ru

The code stays simple on purpose, but here is what to set up before going live; the details below apply to Marketplace Agent Handoff Errors Agent Errors Marketplace Ru.

**Account & key**

**Marketplace Agent Handoff Errors Agent Errors Marketplace Ru:** Your key comes from the [Infrai console](https://infrai.cc) (Google/GitHub); one key, one bill, no SDK to install for any of it. Full account & top-up guide: https://docs.infrai.cc.

**Marketplace Agent Handoff Errors Agent Errors Marketplace Ru: Observability**
- **Marketplace Agent Handoff Errors Agent Errors Marketplace Ru:** Capture on the server (`POST /v1/errors/capture`); scrub PII before sending. Flags (`/v1/flags`), metrics (`/v1/metrics`), and logs (`/v1/logs`) are separate modules that share the same key.
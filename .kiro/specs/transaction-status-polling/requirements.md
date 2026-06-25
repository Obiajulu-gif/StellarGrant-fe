# Requirements Document

## Introduction

This feature adds a `waitForTransaction` utility to the StellarGrants SDK. After a transaction is submitted to the Soroban RPC, it enters a pending state before reaching a terminal outcome (success, failure, or replacement). Developers currently have no built-in way to await that outcome — they must implement their own polling loop. This feature provides a configurable, efficient polling mechanism that resolves when the transaction reaches a terminal state or a timeout is exceeded, with proper error classification for all failure modes.

## Glossary

- **SDK**: The `StellarGrantsSDK` class in `client/src/StellarGrantsSDK.ts`.
- **RPC_Server**: The `rpc.Server` instance used by the SDK to communicate with a Soroban RPC endpoint.
- **Transaction_Hash**: A hex string uniquely identifying a submitted transaction on the Stellar network.
- **Terminal_State**: A transaction status from which no further state transitions occur. Terminal states are `SUCCESS`, `FAILED`, and `NOT_FOUND` (after timeout).
- **Pending_State**: A non-terminal transaction status (`PENDING`, `DUPLICATE`, `TRY_AGAIN_LATER`) indicating the transaction has not yet been processed.
- **Poller**: The internal polling loop within `waitForTransaction` that repeatedly calls `RPC_Server.getTransaction` until a terminal state is reached or the timeout expires.
- **Poll_Interval**: The configurable delay in milliseconds between consecutive `RPC_Server.getTransaction` calls.
- **Timeout**: The maximum total duration in milliseconds that the Poller will wait before giving up and throwing a `TransactionTimeoutError`.
- **TransactionTimeoutError**: A subclass of `StellarGrantsError` thrown when the Timeout elapses before a terminal state is reached.
- **TransactionFailedError**: A subclass of `StellarGrantsError` thrown when the transaction reaches the `FAILED` terminal state.
- **TransactionResult**: The resolved value returned by `waitForTransaction` when the transaction reaches the `SUCCESS` terminal state.
- **WaitForTransactionOptions**: The configuration object accepted by `waitForTransaction` to control Poll_Interval, Timeout, and optional lifecycle callbacks.

---

## Requirements

### Requirement 1: Core Polling Method

**User Story:** As a developer, I want to call `sdk.waitForTransaction(hash)` after submitting a transaction, so that I can await the final outcome without writing my own polling loop.

#### Acceptance Criteria

1. THE SDK SHALL expose a public `waitForTransaction(hash: string, options?: WaitForTransactionOptions): Promise<TransactionResult>` method.
2. WHEN `waitForTransaction` is called with a Transaction_Hash, THE Poller SHALL call `RPC_Server.getTransaction` with that hash on each poll cycle.
3. WHEN `RPC_Server.getTransaction` returns a `SUCCESS` status, THE SDK SHALL resolve the returned `Promise` with a `TransactionResult` containing the transaction ledger, envelope XDR, result XDR, and result meta XDR.
4. WHEN `RPC_Server.getTransaction` returns a `FAILED` status, THE SDK SHALL reject the returned `Promise` with a `TransactionFailedError` that includes the transaction hash and the raw error result from the RPC response.
5. WHEN the Timeout elapses before a terminal state is reached, THE SDK SHALL reject the returned `Promise` with a `TransactionTimeoutError` that includes the transaction hash and the configured Timeout value.

---

### Requirement 2: Configurable Polling Behaviour

**User Story:** As a developer, I want to configure the polling interval and timeout, so that I can tune the trade-off between responsiveness and RPC server load for my use case.

#### Acceptance Criteria

1. THE `WaitForTransactionOptions` SHALL include an optional `pollIntervalMs` field of type `number` that controls the delay between polls.
2. THE `WaitForTransactionOptions` SHALL include an optional `timeoutMs` field of type `number` that controls the maximum wait duration.
3. WHEN `pollIntervalMs` is not provided, THE Poller SHALL use a default Poll_Interval of 3000 milliseconds.
4. WHEN `timeoutMs` is not provided, THE Poller SHALL use a default Timeout of 60000 milliseconds.
5. WHEN `pollIntervalMs` is provided with a value less than 500, THE SDK SHALL throw an `Error` with a message indicating the minimum allowed interval, to prevent overwhelming the RPC server.
6. WHEN `timeoutMs` is provided with a value less than `pollIntervalMs`, THE SDK SHALL throw an `Error` indicating that the timeout must be greater than the poll interval. IF both conditions apply simultaneously, either error may be thrown first depending on implementation order.

---

### Requirement 3: Pending and Retry States

**User Story:** As a developer, I want the SDK to transparently handle transient RPC states, so that my code does not need to distinguish between `PENDING`, `DUPLICATE`, and `TRY_AGAIN_LATER` responses.

#### Acceptance Criteria

1. WHEN `RPC_Server.getTransaction` returns a `PENDING` status, THE Poller SHALL wait for one Poll_Interval and then poll again without throwing.
2. WHEN `RPC_Server.getTransaction` returns a `DUPLICATE` status, THE Poller SHALL treat it as equivalent to `PENDING` and continue polling.
3. WHEN `RPC_Server.getTransaction` returns a `TRY_AGAIN_LATER` status, THE Poller SHALL treat it as equivalent to `PENDING` and continue polling.
4. WHEN `RPC_Server.getTransaction` returns a `NOT_FOUND` status before the Timeout elapses, THE Poller SHALL continue polling, as the transaction may not yet be visible to the RPC node. WHEN the Timeout elapses while in `NOT_FOUND` state, THE Poller SHALL stop immediately and reject with a `TransactionTimeoutError`.

---

### Requirement 4: Network Error Resilience

**User Story:** As a developer, I want transient network errors during polling to be retried automatically, so that a brief RPC outage does not cause `waitForTransaction` to fail prematurely.

#### Acceptance Criteria

1. THE `WaitForTransactionOptions` SHALL include an optional `maxNetworkRetries` field of type `number` that limits consecutive network-level errors before giving up.
2. WHEN `maxNetworkRetries` is not provided, THE Poller SHALL use a default of 3 consecutive network error retries.
3. WHEN `RPC_Server.getTransaction` throws a network-level error and the consecutive error count is below `maxNetworkRetries`, THE Poller SHALL wait for one Poll_Interval and retry without propagating the error.
4. WHEN the consecutive network error count reaches `maxNetworkRetries`, THE SDK SHALL reject the `Promise` with a `StellarGrantsError` wrapping the last network error.
5. WHEN a successful `RPC_Server.getTransaction` response is received after one or more network errors, THE Poller SHALL reset the consecutive error count to zero.

---

### Requirement 5: Lifecycle Callbacks

**User Story:** As a developer, I want optional callbacks for polling lifecycle events, so that I can update UI state (e.g., a loading spinner) without polling the SDK's return value.

#### Acceptance Criteria

1. THE `WaitForTransactionOptions` SHALL include an optional `onStatusChange` callback of type `(status: TransactionPollingStatus) => void`.
2. WHEN the Poller receives any new status from `RPC_Server.getTransaction`, THE SDK SHALL invoke `onStatusChange` with the current status string if the callback is provided.
3. THE `WaitForTransactionOptions` SHALL include an optional `onPoll` callback of type `(attempt: number, elapsedMs: number) => void`.
4. WHEN the Poller completes each poll cycle, THE SDK SHALL invoke `onPoll` with the current attempt number (1-indexed) and elapsed milliseconds since `waitForTransaction` was called, if the callback is provided.

---

### Requirement 6: Cancellation

**User Story:** As a developer, I want to cancel an in-progress `waitForTransaction` call, so that I can clean up resources when a component unmounts or the user navigates away.

#### Acceptance Criteria

1. THE `WaitForTransactionOptions` SHALL include an optional `signal` field of type `AbortSignal`.
2. WHEN `signal.aborted` is `true` at the time `waitForTransaction` is called, THE SDK SHALL reject the `Promise` immediately with a `StellarGrantsError` with code `"ABORTED"`.
3. WHEN `signal` fires its `abort` event during an active polling loop, THE Poller SHALL stop polling and reject the `Promise` with a `StellarGrantsError` with code `"ABORTED"`.
4. WHEN the `Promise` is rejected due to abort, THE SDK SHALL not invoke any further `onStatusChange` or `onPoll` callbacks.

---

### Requirement 7: Error Types

**User Story:** As a developer, I want typed error classes for timeout and failure outcomes, so that I can handle each case with a precise `instanceof` check.

#### Acceptance Criteria

1. THE SDK SHALL export a `TransactionTimeoutError` class that extends `StellarGrantsError` with `code === "TRANSACTION_TIMEOUT"`.
2. THE SDK SHALL export a `TransactionFailedError` class that extends `StellarGrantsError` with `code === "TRANSACTION_FAILED"`.
3. WHEN `TransactionTimeoutError` is constructed, THE TransactionTimeoutError SHALL expose a `hash` property containing the Transaction_Hash and a `timeoutMs` property containing the configured Timeout value.
4. WHEN `TransactionFailedError` is constructed, THE TransactionFailedError SHALL expose a `hash` property containing the Transaction_Hash and an optional `errorResult` property containing the raw RPC error result.
5. THE TransactionTimeoutError SHALL be an instance of both `TransactionTimeoutError` and `StellarGrantsError`.
6. THE TransactionFailedError SHALL be an instance of both `TransactionFailedError` and `StellarGrantsError`.

---

### Requirement 8: TransactionResult Shape

**User Story:** As a developer, I want the resolved value of `waitForTransaction` to contain all relevant transaction data, so that I can inspect the result without making additional RPC calls.

#### Acceptance Criteria

1. THE `TransactionResult` type SHALL include a `status` field with value `"SUCCESS"`.
2. THE `TransactionResult` type SHALL include a `ledger` field of type `number` containing the ledger sequence in which the transaction was included.
3. THE `TransactionResult` type SHALL include an `envelopeXdr` field of type `string` containing the base64-encoded transaction envelope XDR.
4. THE `TransactionResult` type SHALL include a `resultXdr` field of type `string` containing the base64-encoded transaction result XDR.
5. THE `TransactionResult` type SHALL include a `resultMetaXdr` field of type `string` containing the base64-encoded transaction result meta XDR.
6. THE `TransactionResult` type SHALL include a `hash` field of type `string` containing the Transaction_Hash that was polled.

---

### Requirement 9: Integration with Write Methods

**User Story:** As a developer, I want the SDK's write methods to optionally await transaction confirmation, so that I can get a confirmed result in a single call.

#### Acceptance Criteria

1. THE `invokeWrite` options SHALL include an optional `waitForConfirmation` field of type `boolean`.
2. WHEN `waitForConfirmation` is `true` and `RPC_Server.sendTransaction` returns a hash, THE SDK SHALL call `waitForTransaction` with that hash and the same `pollIntervalMs` and `timeoutMs` options if provided.
3. WHEN `waitForConfirmation` is `true` and `waitForTransaction` resolves, THE SDK SHALL return the `TransactionResult` instead of the raw send result.
4. WHEN `waitForConfirmation` is `true` and `waitForTransaction` rejects due to timeout, failure, or network error, THE SDK SHALL propagate that error to the caller.
5. WHEN `waitForConfirmation` is `false` or not provided, THE SDK SHALL return the raw send result as before, preserving backward compatibility.

---

### Requirement 10: Round-Trip Correctness

**User Story:** As a developer, I want the polling logic to be verifiably correct, so that I can trust the SDK handles all state transitions without missing or duplicating terminal state detection.

#### Acceptance Criteria

1. FOR ALL sequences of RPC responses ending in `SUCCESS`, THE Poller SHALL resolve exactly once and make no further `RPC_Server.getTransaction` calls after resolution.
2. FOR ALL sequences of RPC responses ending in `FAILED`, THE Poller SHALL reject exactly once and make no further `RPC_Server.getTransaction` calls after rejection.
3. FOR ALL sequences of N consecutive `PENDING` responses followed by `SUCCESS`, THE Poller SHALL call `RPC_Server.getTransaction` exactly N+1 times.
4. WHEN `waitForTransaction` resolves or rejects, THE Poller SHALL release all internal timers and listeners to prevent memory leaks.

import { makeSdk } from "./helpers/sdkFactory";
import { nativeToScVal } from "@stellar/stellar-sdk";

jest.mock("@stellar/stellar-sdk", () => {
  const actual = jest.requireActual("@stellar/stellar-sdk");
  return {
    ...actual,
    Contract: jest.fn().mockImplementation(() => ({
      call: jest.fn(),
    })),
  };
});

describe("WebSocket event subscription", () => {
  beforeEach(() => {
    jest.useFakeTimers();
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  it("uses polling when RPC URL is http (not WebSocket)", async () => {
    const { sdk, mockServer } = makeSdk({ rpcUrl: "https://rpc.test.mock" });
    mockServer.getEvents = jest.fn().mockResolvedValue({ events: [] });

    const callback = jest.fn();
    const unsubscribe = sdk.subscribeToEvents(callback);

    await Promise.resolve(); // Allow async polling to start
    expect(mockServer.getEvents).toHaveBeenCalled();
    unsubscribe();
  });

  it("normalizes event payload shape", async () => {
    const { sdk, mockServer } = makeSdk({ rpcUrl: "https://rpc.test.mock" });
    const rawEvent = {
      id: "event-123",
      type: "contract",
      contractId: "CD...",
      topic: [],
      value: { _scval: "value" },
      ledger: 100,
      timestamp: 1234567890,
    };

    mockServer.getEvents = jest.fn().mockResolvedValue({ events: [rawEvent] });

    const callback = jest.fn();
    sdk.subscribeToEvents(callback);

    await Promise.resolve(); // Allow async polling to complete

    expect(callback).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "event-123",
        type: "contract",
        contractId: "CD...",
        ledger: 100,
        timestamp: 1234567890,
      })
    );
  });

  it("unsubscribes correctly from polling", async () => {
    const { sdk, mockServer } = makeSdk({ rpcUrl: "https://rpc.test.mock" });
    mockServer.getEvents = jest.fn().mockResolvedValue({ events: [] });

    const callback = jest.fn();
    const unsubscribe = sdk.subscribeToEvents(callback);

    await Promise.resolve();
    unsubscribe();
    jest.runAllTimers();

    // After unsubscribe, getEvents should not be called again
    expect(mockServer.getEvents).toHaveBeenCalledTimes(1);
  });

  it("filters events by eventName", async () => {
    const { sdk, mockServer } = makeSdk({ rpcUrl: "https://rpc.test.mock" });
    const matchEvent = {
      id: "event-match",
      type: "contract",
      contractId: "CD...",
      topic: [
        nativeToScVal("GrantCreated", { type: "symbol" }).toXDR("base64"),
      ],
      value: { _scval: "value" },
      ledger: 100,
      timestamp: 1234567890,
    };
    const nonMatchEvent = {
      id: "event-nomatch",
      type: "contract",
      contractId: "CD...",
      topic: [
        nativeToScVal("GrantFunded", { type: "symbol" }).toXDR("base64"),
      ],
      value: { _scval: "value" },
      ledger: 100,
      timestamp: 1234567890,
    };

    mockServer.getEvents = jest.fn().mockResolvedValue({ events: [matchEvent, nonMatchEvent] });

    const callback = jest.fn();
    sdk.subscribeToEvents(callback, { eventName: "GrantCreated" });

    await Promise.resolve(); // Allow async polling to complete

    expect(callback).toHaveBeenCalledTimes(1);
    expect(callback).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "event-match",
        name: "GrantCreated",
      })
    );
  });

  it("manages and advances the cursor to prevent duplicates", async () => {
    const { sdk, mockServer } = makeSdk({ rpcUrl: "https://rpc.test.mock" });
    const event1 = {
      id: "event-1",
      type: "contract",
      contractId: "CD...",
      topic: [],
      value: { _scval: "value" },
      ledger: 100,
      timestamp: 1234567890,
      pagingToken: "cursor-1",
    };

    mockServer.getEvents = jest.fn().mockResolvedValue({ events: [event1] });

    const callback = jest.fn();
    sdk.subscribeToEvents(callback, { startCursor: "cursor-0", pollIntervalMs: 1000 });

    await Promise.resolve(); // Allow first poll to complete
    expect(mockServer.getEvents).toHaveBeenNthCalledWith(1, expect.objectContaining({
      pagination: expect.objectContaining({
        cursor: "cursor-0"
      })
    }));

    expect(callback).toHaveBeenCalledTimes(1);

    // Prepare next poll response
    mockServer.getEvents.mockResolvedValue({ events: [] });
    // Advance timers
    jest.advanceTimersByTime(1000);
    await Promise.resolve();

    expect(mockServer.getEvents).toHaveBeenNthCalledWith(2, expect.objectContaining({
      pagination: expect.objectContaining({
        cursor: "cursor-1"
      })
    }));
  });

  it("handles network errors with backoff and reports errors after maxRetries", async () => {
    const { sdk, mockServer } = makeSdk({ rpcUrl: "https://rpc.test.mock" });
    const rpcError = new Error("RPC Network Failure");
    mockServer.getEvents = jest.fn().mockRejectedValue(rpcError);

    const callback = jest.fn();
    const onError = jest.fn();
    const onStatusChange = jest.fn();

    const unsubscribe = sdk.subscribeToEvents(callback, {
      maxRetries: 3,
      baseBackoffMs: 100,
      maxBackoffMs: 500,
      onError,
      onStatusChange,
      pollIntervalMs: 1000
    });

    // Run first poll (fails)
    await Promise.resolve();
    expect(onStatusChange).toHaveBeenCalledWith("connecting");
    expect(onStatusChange).toHaveBeenLastCalledWith("reconnecting");

    // Advance for retries: 3 retries total.
    // Retry 1: backoff is baseBackoffMs * 2^0 = 100 max.
    jest.advanceTimersByTime(100);
    await Promise.resolve();

    // Retry 2: backoff is baseBackoffMs * 2^1 = 200 max.
    jest.advanceTimersByTime(200);
    await Promise.resolve();

    // Retry 3: backoff is baseBackoffMs * 2^2 = 400 max.
    jest.advanceTimersByTime(400);
    await Promise.resolve();

    // After 3 retries (total 4 attempts), it should close and call onError
    expect(onError).toHaveBeenCalledWith(rpcError);
    expect(onStatusChange).toHaveBeenLastCalledWith("closed");

    unsubscribe();
  });
});


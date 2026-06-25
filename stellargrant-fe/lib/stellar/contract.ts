/**
 * Contract Client
 *
 * Typed ContractClient class that wraps all StellarGrants contract methods.
 * Read methods use Soroban RPC simulation; write methods return unsigned XDR
 * strings for the wallet layer to sign and submit.
 */

import {
  Contract,
  TransactionBuilder,
  BASE_FEE,
  nativeToScVal,
  xdr,
} from "@stellar/stellar-sdk";
import { rpcClient, horizonClient, networkPassphraseConfig } from "./client";
import { decodeScVal } from "./decode";
import { CONTRACT_ID } from "@/lib/constants";

// Dummy read-only account used for view simulations (no funds needed).
const DUMMY_ACCOUNT = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN";

// ── Simple TTL cache for read methods ─────────────────────────────────────

interface CacheEntry<T> {
  value: T;
  expiresAt: number;
}

class TtlCache {
  private store = new Map<string, CacheEntry<unknown>>();

  get<T>(key: string): T | undefined {
    const entry = this.store.get(key);
    if (!entry) return undefined;
    if (Date.now() > entry.expiresAt) {
      this.store.delete(key);
      return undefined;
    }
    return entry.value as T;
  }

  set<T>(key: string, value: T, ttlMs: number): void {
    this.store.set(key, { value, expiresAt: Date.now() + ttlMs });
  }
}

const VIEW_CACHE = new TtlCache();
const VIEW_TTL_MS = 30_000; // 30 seconds

// ── ContractClient ─────────────────────────────────────────────────────────

export class ContractClient {
  private _contractId: string;
  private _networkPassphrase: string;

  constructor(config?: {
    contractId?: string;
    rpcUrl?: string;
    networkPassphrase?: string;
  }) {
    this._contractId = config?.contractId || CONTRACT_ID;
    this._networkPassphrase = config?.networkPassphrase || networkPassphraseConfig;
  }

  // ── Private helpers ──────────────────────────────────────────────────────

  /**
   * Simulate a read-only contract view call and decode the return value.
   * Results are cached for VIEW_TTL_MS to reduce redundant RPC calls.
   */
  private async simulateView<T>(method: string, args: xdr.ScVal[]): Promise<T> {
    const cacheKey = `${method}:${JSON.stringify(args.map((a) => a.toXDR("base64")))}`;
    const cached = VIEW_CACHE.get<T>(cacheKey);
    if (cached !== undefined) return cached;

    const account = await horizonClient.loadAccount(DUMMY_ACCOUNT);
    const tx = new TransactionBuilder(account, {
      fee: BASE_FEE,
      networkPassphrase: this._networkPassphrase,
    })
      .addOperation(new Contract(this._contractId).call(method, ...args))
      .setTimeout(30)
      .build();

    const result = await rpcClient.simulateTransaction(tx);

    if ("error" in result) {
      throw new Error(`Contract simulation error (${method}): ${result.error}`);
    }

    if (!result.result) {
      throw new Error(`Contract view ${method} returned no result`);
    }

    const decoded = decodeScVal<T>(result.result.retval);
    VIEW_CACHE.set(cacheKey, decoded, VIEW_TTL_MS);
    return decoded;
  }

  /**
   * Build an unsigned transaction for a write method and return its XDR.
   * The returned string is handed off to the wallet layer for signing.
   */
  private async buildWriteXdr(
    callerAddress: string,
    method: string,
    args: xdr.ScVal[]
  ): Promise<string> {
    const account = await horizonClient.loadAccount(callerAddress);
    const tx = new TransactionBuilder(account, {
      fee: BASE_FEE,
      networkPassphrase: this._networkPassphrase,
    })
      .addOperation(new Contract(this._contractId).call(method, ...args))
      .setTimeout(30)
      .build();
    return tx.toEnvelope().toXDR("base64");
  }

  // ── Read-only methods ────────────────────────────────────────────────────

  /** Fetch a grant by its numeric ID. */
  async grantGet(params: { grant_id: bigint }) {
    return this.simulateView("grant_get", [
      nativeToScVal(params.grant_id, { type: "u64" }),
    ]);
  }

  /** Fetch all milestones for a grant. */
  async milestonesGet(params: { grant_id: bigint }) {
    return this.simulateView("milestones_get", [
      nativeToScVal(params.grant_id, { type: "u64" }),
    ]);
  }

  /** Get the on-chain contributor reputation score for an address. */
  async contributorScore(params: { address: string }): Promise<number> {
    const raw = await this.simulateView<bigint>("contributor_score", [
      nativeToScVal(params.address, { type: "address" }),
    ]);
    return Number(raw);
  }

  /** Get the reviewer list for a grant. */
  async grantReviewers(params: { grant_id: bigint }): Promise<string[]> {
    return this.simulateView<string[]>("grant_reviewers", [
      nativeToScVal(params.grant_id, { type: "u64" }),
    ]);
  }

  /** Get the total number of grants ever created. */
  async grantCount(): Promise<bigint> {
    return this.simulateView<bigint>("grant_count", []);
  }

  // ── Write methods (return unsigned XDR) ──────────────────────────────────

  /**
   * Build the unsigned XDR for creating a new grant.
   * Pass the result to `useContractTransaction` for signing and submission.
   */
  async grantCreate(params: {
    owner: string;
    title: string;
    description: string;
    tokenAddress: string;
    totalAmount: bigint;
    milestoneAmount: bigint;
    numMilestones: number;
    reviewers: string[];
    quorum: number;
  }): Promise<string> {
    const args = [
      nativeToScVal(params.owner, { type: "address" }),
      nativeToScVal(params.title),
      nativeToScVal(params.description),
      nativeToScVal(params.tokenAddress, { type: "address" }),
      nativeToScVal(params.totalAmount, { type: "i128" }),
      nativeToScVal(params.milestoneAmount, { type: "i128" }),
      nativeToScVal(params.numMilestones, { type: "u32" }),
      xdr.ScVal.scvVec(params.reviewers.map((r) => nativeToScVal(r, { type: "address" }))),
      nativeToScVal(params.quorum, { type: "u32" }),
      nativeToScVal(null), // Option<Vec<u64>>
      nativeToScVal(0n, { type: "i128" }), // min_funding
      nativeToScVal(params.totalAmount, { type: "i128" }), // hard_cap
      xdr.ScVal.scvVec([]), // tags
      nativeToScVal(false), // _is_open_bounty
      nativeToScVal(false), // is_public_good
    ];
    return this.buildWriteXdr(params.owner, "grant_create", args);
  }

  /** Build the unsigned XDR for funding a grant. */
  async grantFund(params: {
    grant_id: string;
    token: string;
    amount: bigint;
    funder: string;
  }): Promise<string> {
    const args = [
      nativeToScVal(params.funder, { type: "address" }),
      nativeToScVal(BigInt(params.grant_id), { type: "u64" }),
      nativeToScVal(params.token, { type: "address" }),
      nativeToScVal(params.amount, { type: "i128" }),
    ];
    return this.buildWriteXdr(params.funder, "grant_fund", args);
  }

  /**
   * Build the unsigned XDR for approving a SAC token allowance.
   * Required before funding a grant with a non-native token (e.g. USDC).
   */
  async approveToken(params: {
    tokenAddress: string;
    spender: string;
    amount: bigint;
    owner: string;
  }): Promise<string> {
    if (!params.tokenAddress) throw new Error("tokenAddress is required");
    if (!params.spender) throw new Error("spender is required");
    if (!params.owner) throw new Error("owner is required");
    if (params.amount <= 0n) throw new Error("amount must be greater than zero");

    const account = await horizonClient.loadAccount(params.owner);
    const ledger = await rpcClient.getLatestLedger();
    // Approval expires 1 day from the current ledger (approx. 5 s per ledger)
    const expirationLedger = ledger.sequence + Math.ceil(86400 / 5);

    const tx = new TransactionBuilder(account, {
      fee: BASE_FEE,
      networkPassphrase: this._networkPassphrase,
    })
      .addOperation(
        new Contract(params.tokenAddress).call(
          "approve",
          nativeToScVal(params.owner, { type: "address" }),
          nativeToScVal(params.spender, { type: "address" }),
          nativeToScVal(params.amount, { type: "i128" }),
          nativeToScVal(expirationLedger, { type: "u32" })
        )
      )
      .setTimeout(30)
      .build();
    return tx.toEnvelope().toXDR("base64");
  }

  /** Build the unsigned XDR for submitting a milestone proof. */
  async milestoneSubmit(params: {
    grant_id: string;
    milestone_idx: number;
    proof_hash: string;
    recipient: string;
  }): Promise<string> {
    const args = [
      nativeToScVal(params.recipient, { type: "address" }),
      nativeToScVal(BigInt(params.grant_id), { type: "u64" }),
      nativeToScVal(params.milestone_idx, { type: "u32" }),
      nativeToScVal(params.proof_hash),
    ];
    return this.buildWriteXdr(params.recipient, "milestone_submit", args);
  }

  /** Build the unsigned XDR for approving a milestone. */
  async milestoneApprove(params: {
    grant_id: string;
    milestone_idx: number;
    reviewer: string;
  }): Promise<string> {
    const args = [
      nativeToScVal(params.reviewer, { type: "address" }),
      nativeToScVal(BigInt(params.grant_id), { type: "u64" }),
      nativeToScVal(params.milestone_idx, { type: "u32" }),
    ];
    return this.buildWriteXdr(params.reviewer, "milestone_approve", args);
  }

  /** Build the unsigned XDR for rejecting a milestone. */
  async milestoneReject(params: {
    grant_id: string;
    milestone_idx: number;
    reviewer: string;
  }): Promise<string> {
    const args = [
      nativeToScVal(params.reviewer, { type: "address" }),
      nativeToScVal(BigInt(params.grant_id), { type: "u64" }),
      nativeToScVal(params.milestone_idx, { type: "u32" }),
    ];
    return this.buildWriteXdr(params.reviewer, "milestone_reject", args);
  }

  /**
   * Cast a vote on a milestone.
   * Routes to milestoneApprove or milestoneReject based on the `approve` flag.
   */
  async voteOnMilestone(
    grantId: string,
    milestoneIdx: number,
    approve: boolean,
    reviewer: string
  ): Promise<string> {
    if (approve) {
      return this.milestoneApprove({
        grant_id: grantId,
        milestone_idx: milestoneIdx,
        reviewer,
      });
    }
    return this.milestoneReject({
      grant_id: grantId,
      milestone_idx: milestoneIdx,
      reviewer,
    });
  }

  /** Build the unsigned XDR for resolving a milestone dispute. */
  async resolveDispute(params: {
    grantId: string;
    milestoneIdx: number;
    approvePayout: boolean;
    councilAddress: string;
  }): Promise<string> {
    const args = [
      nativeToScVal(params.councilAddress, { type: "address" }),
      nativeToScVal(BigInt(params.grantId), { type: "u64" }),
      nativeToScVal(params.milestoneIdx, { type: "u32" }),
      nativeToScVal(params.approvePayout, { type: "bool" }),
    ];
    return this.buildWriteXdr(params.councilAddress, "milestone_resolve_dispute", args);
  }

  // ── Misc ─────────────────────────────────────────────────────────────────

  /** Check if an address is in the council list (env-var backed). */
  async isCouncilMember(params: { address: string }): Promise<boolean> {
    const raw = process.env.NEXT_PUBLIC_COUNCIL_ADDRESSES ?? "";
    const councilSet = new Set(
      raw
        .split(",")
        .map((a) => a.trim())
        .filter(Boolean)
    );
    return councilSet.has(params.address);
  }
}

// Export singleton instance
export const contractClient = new ContractClient();

#![no_std]
#![allow(clippy::too_many_arguments)]

mod badge;
mod delegate;
mod events;
mod refund;
mod snapshot;
mod storage;
mod streaming;
mod token_swap;
mod treasury;
mod types;

pub use errors::ContractError;
pub use events::Events;
pub use storage::Storage;
pub use types::{
    BadgeCriteria, BadgeRecord, BadgeType, ContractError, ContributorProfile, Delegation,
    DelegationScope, Grant, GrantFund, GrantStatus, Milestone, MilestoneState, RefundCalculation,
    RefundPolicy, RefundPolicyType, SnapshotTrigger, StateSnapshot,
};

use soroban_sdk::{contract, contractimpl, token, Address, Env, String, Vec};

#[contract]
pub struct StellarGrantsContract;

#[contractimpl]
impl StellarGrantsContract {
    pub fn initialize(_env: Env) -> Result<(), ContractError> { Ok(()) }

    pub fn grant_create(env: Env, owner: Address, title: String, description: String, token: Address, total_amount: i128, milestone_amount: i128, num_milestones: u32, reviewers: Vec<Address>) -> Result<u64, ContractError> {
        owner.require_auth();
        if total_amount <= 0 || milestone_amount <= 0 || num_milestones == 0 || num_milestones > 100 { return Err(ContractError::InvalidInput); }
        let required = milestone_amount.checked_mul(num_milestones as i128).ok_or(ContractError::InvalidInput)?;
        if total_amount < required { return Err(ContractError::InvalidInput); }
        let grant_id = Storage::increment_grant_counter(&env);
        let grant = Grant { id: grant_id, owner: owner.clone(), title: title.clone(), description, token, status: GrantStatus::Active, total_amount, milestone_amount, reviewers, total_milestones: num_milestones, milestones_paid_out: 0, escrow_balance: 0, funders: Vec::new(&env), reason: None, timestamp: env.ledger().timestamp() };
        Storage::set_grant(&env, grant_id, &grant);
        Events::emit_grant_created(&env, grant_id, owner, title, total_amount);
        Ok(grant_id)
    }

    pub fn contributor_register(env: Env, contributor: Address, name: String, bio: String, skills: Vec<String>, github_url: String) -> Result<(), ContractError> {
        contributor.require_auth();
        if name.is_empty() || name.len() > 100 || bio.len() > 500 { return Err(ContractError::InvalidInput); }
        if Storage::get_contributor(&env, contributor.clone()).is_some() { return Err(ContractError::AlreadyRegistered); }
        let profile = ContributorProfile { contributor: contributor.clone(), name: name.clone(), bio, skills, github_url, registration_timestamp: env.ledger().timestamp(), grants_count: 0, total_earned: 0, reputation_score: 0, milestones_completed: 0, milestones_rejected: 0 };
        Storage::set_contributor(&env, contributor.clone(), &profile);
        Events::emit_contributor_registered(&env, contributor, name);
        Ok(())
    }

    pub fn grant_cancel(env: Env, grant_id: u64, owner: Address, reason: String) -> Result<(), ContractError> {
        owner.require_auth();
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        if grant.owner != owner { return Err(ContractError::Unauthorized); }
        if grant.status != GrantStatus::Active || grant.milestones_paid_out >= grant.total_milestones { return Err(ContractError::InvalidState); }
        let calc = refund::execute_refund(&env, grant_id, &owner)?;
        let mut grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        grant.status = GrantStatus::Cancelled;
        grant.escrow_balance = 0;
        grant.reason = Some(reason.clone());
        grant.timestamp = env.ledger().timestamp();
        Storage::set_grant(&env, grant_id, &grant);
        Events::emit_grant_cancelled(&env, grant_id, owner, reason, calc.funder_refund);
        Ok(())
    }

    pub fn grant_complete(env: Env, grant_id: u64) -> Result<(), ContractError> {
        let mut grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        if grant.status != GrantStatus::Active { return Err(ContractError::InvalidState); }
        let mut total_paid: i128 = 0;
        for idx in 0..grant.total_milestones {
            let m = Storage::get_milestone(&env, grant_id, idx).ok_or(ContractError::NotAllMilestonesApproved)?;
            if m.state != MilestoneState::Approved { return Err(ContractError::NotAllMilestonesApproved); }
            total_paid = total_paid.saturating_add(m.amount);
        }
        let remaining = grant.escrow_balance.saturating_sub(total_paid);
        if remaining > 0 { refund_to_funders(&env, &grant, remaining)?; }
        grant.status = GrantStatus::Completed;
        grant.escrow_balance = 0;
        grant.milestones_paid_out = grant.total_milestones;
        grant.timestamp = env.ledger().timestamp();
        Storage::set_grant(&env, grant_id, &grant);
        Events::emit_grant_completed(&env, grant_id, total_paid, remaining);
        Ok(())
    }

    pub fn milestone_vote(env: Env, grant_id: u64, milestone_idx: u32, reviewer: Address, approve: bool, feedback: Option<String>) -> Result<bool, ContractError> {
        reviewer.require_auth();
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        let mut milestone = Storage::get_milestone(&env, grant_id, milestone_idx).ok_or(ContractError::MilestoneNotSubmitted)?;
        if milestone.state != MilestoneState::Submitted { return Err(ContractError::MilestoneNotSubmitted); }
        let mut effective = reviewer.clone();
        if !grant.reviewers.contains(reviewer.clone()) {
            let resolved = delegate::resolve_delegator(&env, &reviewer, grant_id);
            if resolved == reviewer || !grant.reviewers.contains(resolved.clone()) || !delegate::is_authorized_proxy(&env, &resolved, &reviewer, grant_id) { return Err(ContractError::Unauthorized); }
            effective = resolved;
        }
        if milestone.votes.contains_key(effective.clone()) { return Err(ContractError::AlreadyVoted); }
        if let Some(ref fb) = feedback { if fb.len() > 256 { return Err(ContractError::InvalidInput); } milestone.reasons.set(effective.clone(), fb.clone()); }
        let rep = Storage::get_reviewer_reputation(&env, effective.clone());
        milestone.votes.set(effective.clone(), approve);
        if approve { milestone.approvals = milestone.approvals.saturating_add(rep); } else { milestone.rejections = milestone.rejections.saturating_add(rep); }
        let mut total_weight: u32 = 0;
        for r in grant.reviewers.iter() { total_weight = total_weight.saturating_add(Storage::get_reviewer_reputation(&env, r)); }
        let quorum = milestone.approvals >= (total_weight / 2) + 1;
        if quorum {
            milestone.state = MilestoneState::Approved;
            milestone.status_updated_at = env.ledger().timestamp();
            update_contributor_badges(&env, grant_id, milestone_idx, &grant, milestone.amount);
            Events::milestone_status_changed(&env, grant_id, milestone_idx, MilestoneState::Approved);
        }
        Storage::set_milestone(&env, grant_id, milestone_idx, &milestone);
        if effective != reviewer { delegate::consume_delegation_for_vote(&env, &effective, &reviewer, grant_id)?; }
        Events::milestone_voted(&env, grant_id, milestone_idx, reviewer, approve, feedback);
        Ok(quorum)
    }

    pub fn milestone_reject(env: Env, grant_id: u64, milestone_idx: u32, reviewer: Address, reason: String) -> Result<bool, ContractError> {
        reviewer.require_auth();
        if reason.len() > 256 { return Err(ContractError::InvalidInput); }
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        let mut milestone = Storage::get_milestone(&env, grant_id, milestone_idx).ok_or(ContractError::MilestoneNotSubmitted)?;
        if milestone.state != MilestoneState::Submitted { return Err(ContractError::MilestoneNotSubmitted); }
        if !grant.reviewers.contains(reviewer.clone()) { return Err(ContractError::Unauthorized); }
        if milestone.votes.contains_key(reviewer.clone()) { return Err(ContractError::AlreadyVoted); }
        let rep = Storage::get_reviewer_reputation(&env, reviewer.clone());
        milestone.votes.set(reviewer.clone(), false);
        milestone.rejections = milestone.rejections.saturating_add(rep);
        milestone.reasons.set(reviewer.clone(), reason.clone());
        let mut total_weight: u32 = 0;
        for r in grant.reviewers.iter() { total_weight = total_weight.saturating_add(Storage::get_reviewer_reputation(&env, r)); }
        let rejected = milestone.rejections >= (total_weight / 2) + 1;
        if rejected { milestone.state = MilestoneState::Rejected; milestone.status_updated_at = env.ledger().timestamp(); Events::milestone_status_changed(&env, grant_id, milestone_idx, MilestoneState::Rejected); }
        Storage::set_milestone(&env, grant_id, milestone_idx, &milestone);
        Events::milestone_rejected(&env, grant_id, milestone_idx, reviewer, reason);
        Ok(rejected)
    }

    pub fn milestone_submit(env: Env, grant_id: u64, milestone_idx: u32, recipient: Address, description: String, proof_url: String) -> Result<(), ContractError> {
        recipient.require_auth();
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        if grant.status != GrantStatus::Active { return Err(ContractError::InvalidState); }
        if milestone_idx >= grant.total_milestones { return Err(ContractError::InvalidInput); }
        if grant.owner != recipient { return Err(ContractError::Unauthorized); }
        if let Some(existing) = Storage::get_milestone(&env, grant_id, milestone_idx) { if existing.state == MilestoneState::Submitted || existing.state == MilestoneState::Approved { return Err(ContractError::MilestoneAlreadySubmitted); } }
        let milestone = Milestone { idx: milestone_idx, description: description.clone(), amount: grant.milestone_amount, state: MilestoneState::Submitted, votes: soroban_sdk::Map::new(&env), approvals: 0, rejections: 0, reasons: soroban_sdk::Map::new(&env), status_updated_at: 0, proof_url: Some(proof_url), submission_timestamp: env.ledger().timestamp() };
        Storage::set_milestone(&env, grant_id, milestone_idx, &milestone);
        Events::emit_milestone_submitted(&env, grant_id, milestone_idx, description);
        Ok(())
    }

    pub fn grant_fund(env: Env, grant_id: u64, funder: Address, amount: i128) -> Result<(), ContractError> {
        funder.require_auth();
        if amount <= 0 { return Err(ContractError::InvalidInput); }
        let mut grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        if grant.status != GrantStatus::Active { return Err(ContractError::InvalidState); }
        token::Client::new(&env, &grant.token).transfer(&funder, &env.current_contract_address(), &amount);
        add_fund(&mut grant, funder.clone(), amount)?;
        Storage::set_grant(&env, grant_id, &grant);
        Events::emit_grant_funded(&env, grant_id, funder, amount, grant.escrow_balance);
        Ok(())
    }

    pub fn get_grant(env: Env, grant_id: u64) -> Result<Grant, ContractError> { Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound) }
    pub fn get_milestone(env: Env, grant_id: u64, milestone_idx: u32) -> Result<Milestone, ContractError> {
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        if milestone_idx >= grant.total_milestones { return Err(ContractError::InvalidInput); }
        Storage::get_milestone(&env, grant_id, milestone_idx).ok_or(ContractError::MilestoneNotFound)
    }
    pub fn get_milestone_feedback(env: Env, grant_id: u64, milestone_idx: u32) -> Result<soroban_sdk::Map<Address, String>, ContractError> { Ok(Self::get_milestone(env, grant_id, milestone_idx)?.reasons) }

    pub fn set_staking_config(env: Env, admin: Address, min_stake: i128, treasury: Address) -> Result<(), ContractError> {
        admin.require_auth();
        if min_stake <= 0 { return Err(ContractError::InvalidInput); }
        env.storage().persistent().set(&storage::DataKey::MinReviewerStake, &min_stake);
        env.storage().persistent().set(&storage::DataKey::Treasury, &treasury);
        Ok(())
    }
    pub fn stake_to_review(env: Env, reviewer: Address, grant_id: u64, amount: i128) -> Result<(), ContractError> {
        reviewer.require_auth();
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        if grant.status != GrantStatus::Active { return Err(ContractError::InvalidState); }
        if amount < Storage::get_min_reviewer_stake(&env) { return Err(ContractError::InsufficientStake); }
        token::Client::new(&env, &grant.token).transfer(&reviewer, &env.current_contract_address(), &amount);
        let current = Storage::get_reviewer_stake(&env, grant_id, &reviewer);
        Storage::set_reviewer_stake(&env, grant_id, &reviewer, current.saturating_add(amount));
        Ok(())
    }
    pub fn slash_reviewer(env: Env, admin: Address, grant_id: u64, reviewer: Address) -> Result<(), ContractError> {
        admin.require_auth();
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        let stake = Storage::get_reviewer_stake(&env, grant_id, &reviewer);
        if stake <= 0 { return Err(ContractError::StakeNotFound); }
        let treasury = Storage::get_treasury(&env).ok_or(ContractError::InvalidInput)?;
        token::Client::new(&env, &grant.token).transfer(&env.current_contract_address(), &treasury, &stake);
        Storage::set_reviewer_stake(&env, grant_id, &reviewer, 0);
        Ok(())
    }
    pub fn unstake(env: Env, reviewer: Address, grant_id: u64) -> Result<(), ContractError> {
        reviewer.require_auth();
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        if grant.status == GrantStatus::Active { return Err(ContractError::InvalidState); }
        let stake = Storage::get_reviewer_stake(&env, grant_id, &reviewer);
        if stake <= 0 { return Err(ContractError::StakeNotFound); }
        token::Client::new(&env, &grant.token).transfer(&env.current_contract_address(), &reviewer, &stake);
        Storage::set_reviewer_stake(&env, grant_id, &reviewer, 0);
        Ok(())
    }
    pub fn set_identity_oracle(env: Env, admin: Address, oracle: Address) -> Result<(), ContractError> { admin.require_auth(); env.storage().persistent().set(&storage::DataKey::IdentityOracle, &oracle); Ok(()) }
    pub fn fund_batch(env: Env, funder: Address, grants: Vec<(u64, i128)>) -> Result<(), ContractError> {
        funder.require_auth();
        if grants.is_empty() { return Err(ContractError::BatchEmpty); }
        if grants.len() > 20 { return Err(ContractError::BatchTooLarge); }
        for item in grants.iter() { Self::grant_fund(env.clone(), item.0, funder.clone(), item.1)?; }
        Ok(())
    }

    pub fn delegate_vote(env: Env, delegator: Address, delegate_addr: Address, scope: DelegationScope, expires_at: Option<u64>, max_uses: Option<u32>) -> Result<(), ContractError> { delegate::delegate_vote(&env, &delegator, &delegate_addr, scope, expires_at, max_uses) }
    pub fn revoke_delegation(env: Env, delegator: Address, scope: DelegationScope) -> Result<(), ContractError> { delegate::revoke_delegation(&env, &delegator, &scope) }
    pub fn get_delegation(env: Env, delegator: Address, scope: DelegationScope) -> Option<Delegation> { delegate::get_delegation(&env, &delegator, &scope) }

    pub fn get_badges(env: Env, contributor: Address) -> Vec<BadgeRecord> { badge::get_badges(&env, &contributor) }
    pub fn has_badge(env: Env, contributor: Address, badge_type: BadgeType) -> bool { badge::has_badge(&env, &contributor, badge_type) }
    pub fn badge_award_count(env: Env, badge_type: BadgeType) -> u32 { badge::award_count(&env, badge_type) }
    pub fn manual_award_badge(env: Env, admin: Address, contributor: Address, badge_type: BadgeType) -> Result<(), ContractError> { badge::manual_award(&env, &admin, &contributor, badge_type) }

    pub fn set_refund_policy(env: Env, owner: Address, grant_id: u64, policy: RefundPolicy) -> Result<(), ContractError> { refund::set_policy(&env, &owner, grant_id, policy) }
    pub fn get_refund_policy(env: Env, grant_id: u64) -> RefundPolicy { refund::get_policy(&env, grant_id) }
    pub fn calculate_refund(env: Env, grant_id: u64, canceller: Address) -> Result<RefundCalculation, ContractError> { refund::calculate_refund(&env, grant_id, &canceller) }

    pub fn dispute_raise(env: Env, grant_id: u64, milestone_idx: u32, caller: Address, reason: String) -> Result<u32, ContractError> {
        caller.require_auth();
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        if grant.owner != caller && !grant.reviewers.contains(caller.clone()) { return Err(ContractError::Unauthorized); }
        if milestone_idx >= grant.total_milestones || reason.is_empty() { return Err(ContractError::InvalidInput); }
        snapshot::capture(&env, grant_id, SnapshotTrigger::DisputeRaised, &caller)
    }
    pub fn capture_snapshot(env: Env, grant_id: u64, trigger: SnapshotTrigger, captured_by: Address) -> Result<u32, ContractError> { captured_by.require_auth(); snapshot::capture(&env, grant_id, trigger, &captured_by) }
    pub fn get_snapshot(env: Env, grant_id: u64, snapshot_id: u32) -> Result<StateSnapshot, ContractError> { snapshot::get_snapshot(&env, grant_id, snapshot_id) }
    pub fn list_snapshots(env: Env, grant_id: u64) -> Vec<StateSnapshot> { snapshot::list_snapshots(&env, grant_id) }
    pub fn latest_snapshot(env: Env, grant_id: u64) -> Option<StateSnapshot> { snapshot::latest_snapshot(&env, grant_id) }
    pub fn diff_snapshots(env: Env, grant_id: u64, snapshot_a: u32, snapshot_b: u32) -> Vec<soroban_sdk::Symbol> { snapshot::diff_snapshots(&env, grant_id, snapshot_a, snapshot_b) }
}

fn refund_to_funders(env: &Env, grant: &Grant, amount: i128) -> Result<(), ContractError> {
    let mut total: i128 = 0;
    for f in grant.funders.iter() { total = total.saturating_add(f.amount); }
    if total <= 0 { return Ok(()); }
    let client = token::Client::new(env, &grant.token);
    let len = grant.funders.len();
    let mut paid: i128 = 0;
    for i in 0..len {
        let f = grant.funders.get(i).ok_or(ContractError::InvalidInput)?;
        let refund = if i + 1 == len { amount.saturating_sub(paid) } else { f.amount.saturating_mul(amount).checked_div(total).unwrap_or(0) };
        if refund > 0 { client.transfer(&env.current_contract_address(), &f.funder, &refund); Events::emit_final_refund(env, grant.id, f.funder.clone(), refund); paid = paid.saturating_add(refund); }
    }
    Ok(())
}

fn add_fund(grant: &mut Grant, funder: Address, amount: i128) -> Result<(), ContractError> {
    grant.escrow_balance = grant.escrow_balance.checked_add(amount).ok_or(ContractError::InvalidInput)?;
    for i in 0..grant.funders.len() {
        let mut entry = grant.funders.get(i).ok_or(ContractError::InvalidInput)?;
        if entry.funder == funder {
            entry.amount = entry.amount.checked_add(amount).ok_or(ContractError::InvalidInput)?;
            grant.funders.set(i, entry);
            return Ok(());
        }
    }
    grant.funders.push_back(GrantFund { funder, amount });
    Ok(())
}

fn update_contributor_badges(env: &Env, grant_id: u64, milestone_idx: u32, grant: &Grant, amount: i128) {
    let contributor = grant.owner.clone();
    let mut profile = match Storage::get_contributor(env, contributor.clone()) { Some(p) => p, None => return };
    profile.milestones_completed = profile.milestones_completed.saturating_add(1);
    profile.total_earned = profile.total_earned.saturating_add(amount);
    if profile.grants_count == 0 { profile.grants_count = 1; }
    profile.reputation_score = (profile.milestones_completed as u64).saturating_mul(100).min(1000);
    Storage::set_contributor(env, contributor.clone(), &profile);
    let _ = badge::try_award(env, &contributor, BadgeType::FirstMilestone, Some(grant_id), Some(milestone_idx));
    let _ = badge::try_award(env, &contributor, BadgeType::TenMilestones, Some(grant_id), Some(milestone_idx));
    let _ = badge::try_award(env, &contributor, BadgeType::FiftyMilestones, Some(grant_id), Some(milestone_idx));
    let _ = badge::try_award(env, &contributor, BadgeType::BronzeContributor, Some(grant_id), Some(milestone_idx));
    let _ = badge::try_award(env, &contributor, BadgeType::SilverContributor, Some(grant_id), Some(milestone_idx));
    let _ = badge::try_award(env, &contributor, BadgeType::GoldContributor, Some(grant_id), Some(milestone_idx));
    let _ = badge::try_award(env, &contributor, BadgeType::PlatinumContributor, Some(grant_id), Some(milestone_idx));
}

    // ── Streaming Payments (#531) ───────────────────────────────────────────

    /// Create a new payment stream for a grant milestone.
    pub fn create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        grant_id: u64,
        token: Address,
        rate_per_ledger: i128,
        duration_ledgers: u32,
    ) -> Result<u32, ContractError> {
        emergency::require_not_paused(&env)?;
        circuit_breaker::require_open(&env, ProtocolModule::Streaming)?;
        streaming::create_stream(
            &env,
            &sender,
            &recipient,
            grant_id,
            &token,
            rate_per_ledger,
            duration_ledgers,
        )
    }

    /// Recipient withdraws accrued tokens from a stream.
    pub fn withdraw_stream(
        env: Env,
        recipient: Address,
        stream_id: u32,
    ) -> Result<i128, ContractError> {
        emergency::require_not_paused(&env)?;
        circuit_breaker::require_open(&env, ProtocolModule::Streaming)?;
        streaming::withdraw_stream(&env, &recipient, stream_id)
    }

    /// Cancel a stream, splitting remaining deposit between sender and recipient.
    pub fn cancel_stream(
        env: Env,
        sender: Address,
        stream_id: u32,
    ) -> Result<(i128, i128), ContractError> {
        streaming::cancel_stream(&env, &sender, stream_id)
    }

    /// Pause an active stream.
    pub fn pause_stream(env: Env, sender: Address, stream_id: u32) -> Result<(), ContractError> {
        streaming::pause_stream(&env, &sender, stream_id)
    }

    /// Resume a paused stream.
    pub fn resume_stream(env: Env, sender: Address, stream_id: u32) -> Result<(), ContractError> {
        streaming::resume_stream(&env, &sender, stream_id)
    }

    /// Get stream details by id.
    pub fn get_stream(env: Env, stream_id: u32) -> Result<PaymentStream, ContractError> {
        streaming::get_stream(&env, stream_id)
    }

    // ── Quadratic Voting (#537) ─────────────────────────────────────────────

    /// Allocate voice credits to a reviewer for a grant.
    pub fn allocate_voice_credits(
        env: Env,
        admin: Address,
        voter: Address,
        grant_id: u64,
        credits: u32,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        if Storage::get_global_admin(&env) != Some(admin.clone()) {
            return Err(ContractError::Unauthorized);
        }
        quadratic::allocate_credits(&env, &voter, grant_id, credits)
    }

    /// Cast a quadratic vote on a milestone.
    pub fn cast_qv_vote(
        env: Env,
        voter: Address,
        grant_id: u64,
        milestone_idx: u32,
        votes: u32,
        in_favor: bool,
    ) -> Result<QuadraticVoteRecord, ContractError> {
        emergency::require_not_paused(&env)?;
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        if !grant.reviewers.contains(voter.clone()) {
            return Err(ContractError::Unauthorized);
        }
        quadratic::cast_qv_vote(&env, &voter, grant_id, milestone_idx, votes, in_favor)
    }

    /// Return remaining voice credits for a voter on a grant.
    pub fn remaining_voice_credits(env: Env, voter: Address, grant_id: u64) -> u32 {
        quadratic::remaining_credits(&env, &voter, grant_id)
    }

    /// Check if a milestone is approved by QV tally.
    pub fn is_qv_approved(env: Env, grant_id: u64, milestone_idx: u32) -> bool {
        quadratic::is_approved_qv(&env, grant_id, milestone_idx)
    }

    /// Return all QV vote records for a milestone.
    pub fn get_qv_votes(env: Env, grant_id: u64, milestone_idx: u32) -> Vec<QuadraticVoteRecord> {
        quadratic::get_qv_votes(&env, grant_id, milestone_idx)
    }

    // ── Grant Insurance Pool (#538) ─────────────────────────────────────────

    /// Purchase insurance for a grant.
    pub fn purchase_insurance(
        env: Env,
        policyholder: Address,
        grant_id: u64,
        token: Address,
        coverage_amount: i128,
    ) -> Result<InsurancePolicy, ContractError> {
        emergency::require_not_paused(&env)?;
        circuit_breaker::require_open(&env, ProtocolModule::Insurance)?;
        insurance::purchase_policy(&env, &policyholder, grant_id, &token, coverage_amount)
    }

    /// File an insurance claim for a grant.
    pub fn file_insurance_claim(
        env: Env,
        claimant: Address,
        grant_id: u64,
        claimed_amount: i128,
        reason: String,
    ) -> Result<u32, ContractError> {
        emergency::require_not_paused(&env)?;
        insurance::file_claim(&env, &claimant, grant_id, claimed_amount, reason)
    }

    /// Approve and pay out a claim. Admin only.
    pub fn approve_insurance_claim(
        env: Env,
        admin: Address,
        claim_id: u32,
        payout_amount: i128,
    ) -> Result<(), ContractError> {
        if Storage::get_global_admin(&env) != Some(admin.clone()) {
            return Err(ContractError::Unauthorized);
        }
        insurance::approve_claim(&env, &admin, claim_id, payout_amount)
    }

    /// Reject a claim. Admin only.
    pub fn reject_insurance_claim(
        env: Env,
        admin: Address,
        claim_id: u32,
    ) -> Result<(), ContractError> {
        if Storage::get_global_admin(&env) != Some(admin.clone()) {
            return Err(ContractError::Unauthorized);
        }
        insurance::reject_claim(&env, &admin, claim_id)
    }

    /// Return insurance pool balance for a token.
    pub fn insurance_pool_balance(env: Env, token: Address) -> i128 {
        insurance::pool_balance(&env, &token)
    }

    /// Return the insurance policy for a grant.
    pub fn get_insurance_policy(env: Env, grant_id: u64) -> Option<InsurancePolicy> {
        insurance::get_policy(&env, grant_id)
    }

    /// Return a claim by id.
    pub fn get_insurance_claim(env: Env, claim_id: u32) -> Result<InsuranceClaim, ContractError> {
        insurance::get_claim(&env, claim_id)
    }

    // ── External Callback Hooks (#539) ──────────────────────────────────────

    /// Register an external contract hook for an event. Admin only.
    pub fn register_hook(
        env: Env,
        admin: Address,
        event: HookEvent,
        target_contract: Address,
        max_gas_budget: u32,
    ) -> Result<u32, ContractError> {
        if Storage::get_global_admin(&env) != Some(admin.clone()) {
            return Err(ContractError::Unauthorized);
        }
        hooks::register_hook(&env, &admin, event, target_contract, max_gas_budget)
    }

    /// Deactivate a registered hook. Admin only.
    pub fn deactivate_hook(
        env: Env,
        admin: Address,
        event: HookEvent,
        hook_index: u32,
    ) -> Result<(), ContractError> {
        if Storage::get_global_admin(&env) != Some(admin.clone()) {
            return Err(ContractError::Unauthorized);
        }
        hooks::deactivate_hook(&env, &admin, event, hook_index)
    }

    /// Return all registered hooks for an event.
    pub fn get_hooks(env: Env, event: HookEvent) -> Vec<HookRegistration> {
        hooks::get_hooks(&env, event)
    }

    /// Check if any active hooks are registered for an event.
    pub fn has_hooks(env: Env, event: HookEvent) -> bool {
        hooks::has_hooks(&env, event)
    }

    // ── Issue #514: Dispute Resolution Entry Points ───────────────────────────

    pub fn dispute_raise(
        env: Env,
        grant_id: u64,
        milestone_idx: u32,
        caller: Address,
        reason: String,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        emergency::require_not_paused(&env)?;
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        dispute::raise_dispute(&env, &grant, milestone_idx, &caller, reason)?;
        metrics::increment(&env, MetricField::DisputesRaised, 1);
        Ok(())
    }

    pub fn dispute_assign_arbiter(
        env: Env,
        grant_id: u64,
        milestone_idx: u32,
        admin: Address,
        arbiter: Address,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        if Storage::get_global_admin(&env) != Some(admin.clone()) {
            return Err(ContractError::Unauthorized);
        }
        let mut d = Storage::get_dispute(&env, grant_id, milestone_idx)
            .ok_or(ContractError::InvalidState)?;
        dispute::assign_arbiter(&env, &mut d, &admin, &arbiter)
    }

    pub fn dispute_arbiter_vote(
        env: Env,
        grant_id: u64,
        milestone_idx: u32,
        arbiter: Address,
        favor_contributor: bool,
    ) -> Result<(), ContractError> {
        arbiter.require_auth();
        let mut d = Storage::get_dispute(&env, grant_id, milestone_idx)
            .ok_or(ContractError::InvalidState)?;
        dispute::arbiter_vote(&env, &mut d, &arbiter, favor_contributor)
    }

    pub fn dispute_resolve(
        env: Env,
        grant_id: u64,
        milestone_idx: u32,
        caller: Address,
    ) -> Result<DisputeStatus, ContractError> {
        caller.require_auth();
        if Storage::get_global_admin(&env) != Some(caller.clone()) {
            return Err(ContractError::Unauthorized);
        }
        let mut grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        let mut d = Storage::get_dispute(&env, grant_id, milestone_idx)
            .ok_or(ContractError::InvalidState)?;
        let outcome = dispute::resolve_dispute(&env, &mut grant, &mut d)?;
        Storage::set_grant(&env, grant_id, &grant);
        metrics::increment(&env, MetricField::DisputesResolved, 1);
        Ok(outcome)
    }

    pub fn dispute_cancel(
        env: Env,
        grant_id: u64,
        milestone_idx: u32,
        caller: Address,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        let mut d = Storage::get_dispute(&env, grant_id, milestone_idx)
            .ok_or(ContractError::InvalidState)?;
        dispute::cancel_dispute(&env, &mut d, &caller)
    }

    pub fn get_dispute_record(env: Env, grant_id: u64, milestone_idx: u32) -> Option<Dispute> {
        Storage::get_dispute(&env, grant_id, milestone_idx)
    }

    // ── Issue #516: Runtime Protocol Configuration Entry Points ──────────────

    /// Update the runtime protocol configuration directly. Admin only, and
    /// only while DAO mode is disabled — once enabled, config changes must
    /// go through a passed `DaoProposalType::UpdateConfig` proposal.
    pub fn update_config(
        env: Env,
        admin: Address,
        new_config: ProtocolConfig,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        dao::require_dao_mode_disabled(&env)?;
        config::set_config(&env, &admin, new_config)
    }

    pub fn get_protocol_config(env: Env) -> ProtocolConfig {
        config::get_config(&env)
    }

    // ── Issue #517: Protocol Fee Management Entry Points ─────────────────────

    pub fn get_fees_collected(env: Env, token: Address) -> i128 {
        fees::total_fees_collected(&env, &token)
    }

    // ── Issue #529: Escrow Module ─────────────────────────────────────────────

    /// Return the escrow account state for a grant.
    pub fn get_escrow_account(env: Env, grant_id: u64) -> Result<EscrowAccount, ContractError> {
        escrow::get_account(&env, grant_id)
    }

    /// Return the funder ledger for a contributor in a grant.
    pub fn get_funder_ledger(env: Env, grant_id: u64, funder: Address) -> Option<FunderLedger> {
        escrow::get_funder_ledger(&env, grant_id, &funder)
    }

    /// Refund a specific funder's net contribution from escrow after grant ends. Funder only.
    pub fn refund_funder(env: Env, funder: Address, grant_id: u64) -> Result<i128, ContractError> {
        funder.require_auth();
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        if grant.status == GrantStatus::Active {
            return Err(ContractError::InvalidState);
        }
        escrow::refund(&env, grant_id, &funder)
    }

    /// Lock escrow for a grant (e.g., when a dispute is open). Admin only.
    pub fn lock_escrow(env: Env, admin: Address, grant_id: u64) -> Result<(), ContractError> {
        admin.require_auth();
        if Storage::get_global_admin(&env) != Some(admin) {
            return Err(ContractError::Unauthorized);
        }
        escrow::lock(&env, grant_id)
    }

    /// Unlock escrow for a grant after dispute resolution. Admin only.
    pub fn unlock_escrow(env: Env, admin: Address, grant_id: u64) -> Result<(), ContractError> {
        admin.require_auth();
        if Storage::get_global_admin(&env) != Some(admin) {
            return Err(ContractError::Unauthorized);
        }
        escrow::unlock(&env, grant_id)
    }

    /// Expire a stale multisig proposal past its TTL. Anyone can call.
    pub fn expire_multisig_proposal(env: Env, proposal_id: u32) -> Result<(), ContractError> {
        multisig::expire_proposal(&env, proposal_id)
    }

    // ── Issue #530: Multisig Fund Release ─────────────────────────────────────

    /// Create a multisig proposal for a grant action. Grant owner or admin only.
    pub fn create_multisig_proposal(
        env: Env,
        creator: Address,
        grant_id: u64,
        action_payload: Bytes,
        signers: Vec<Address>,
        threshold: u32,
        ttl_ledgers: u32,
    ) -> Result<u32, ContractError> {
        creator.require_auth();
        let grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        let is_owner = grant.owner == creator;
        let is_admin = Storage::get_global_admin(&env) == Some(creator.clone());
        if !is_owner && !is_admin {
            return Err(ContractError::Unauthorized);
        }
        multisig::create_proposal(
            &env,
            &creator,
            grant_id,
            action_payload,
            signers,
            threshold,
            ttl_ledgers,
        )
    }

    /// Sign (or veto) a multisig proposal.
    pub fn sign_proposal(
        env: Env,
        signer: Address,
        proposal_id: u32,
        approve: bool,
    ) -> Result<u32, ContractError> {
        signer.require_auth();
        multisig::sign(&env, &signer, proposal_id, approve)
    }

    /// Execute a multisig proposal once threshold is met.
    /// For GrantWithdraw proposals, triggers the grant release.
    pub fn execute_multisig_proposal(
        env: Env,
        caller: Address,
        proposal_id: u32,
    ) -> Result<Bytes, ContractError> {
        caller.require_auth();
        let payload = multisig::execute(&env, &caller, proposal_id)?;
        // Dispatch GrantWithdraw if payload encodes a grant_id.
        if let Some(grant_id) = multisig::decode_grant_withdraw(&payload) {
            Self::finalize_grant_release(&env, grant_id)?;
        }
        Ok(payload)
    }

    /// Return a multisig proposal by id.
    pub fn get_multisig_proposal(
        env: Env,
        proposal_id: u32,
    ) -> Result<MultisigProposal, ContractError> {
        multisig::get_proposal(&env, proposal_id)
    }

    /// Helper to encode a GrantWithdraw action payload from a grant_id.
    pub fn encode_grant_withdraw_payload(env: Env, grant_id: u64) -> Bytes {
        multisig::encode_grant_withdraw(&env, grant_id)
    }

    // ── Issue #540: Protocol Metrics ──────────────────────────────────────────

    /// Return the aggregated protocol-wide metrics snapshot.
    pub fn get_protocol_metrics(env: Env) -> ProtocolMetrics {
        metrics::get_metrics(&env)
    }

    /// Return token-specific locked/paid/refunded totals.
    pub fn get_token_metrics(env: Env, token: Address) -> TokenMetric {
        metrics::get_token_metrics(&env, &token)
    }

    /// Reset all protocol metrics. Admin only (for testnet/migration use).
    pub fn reset_metrics(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        metrics::reset(&env, &admin)
    }

    // ── Issue #548: KYC/AML Compliance ────────────────────────────────────────

    /// Register the trusted compliance verifier. Admin only.
    pub fn set_compliance_verifier(
        env: Env,
        admin: Address,
        verifier: Address,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        compliance::set_verifier(&env, &admin, &verifier)
    }

    /// Trusted verifier attests compliance for a subject address.
    pub fn attest_compliance(
        env: Env,
        verifier: Address,
        subject: Address,
        status: ComplianceStatus,
        level: ComplianceLevel,
        expires_at: u64,
        jurisdiction: String,
    ) -> Result<(), ContractError> {
        verifier.require_auth();
        compliance::attest(
            &env,
            &verifier,
            &subject,
            status,
            level,
            expires_at,
            jurisdiction,
        )
    }

    /// Revoke a compliance attestation. Verifier or admin only.
    pub fn revoke_compliance(
        env: Env,
        revoker: Address,
        subject: Address,
    ) -> Result<(), ContractError> {
        revoker.require_auth();
        compliance::revoke(&env, &revoker, &subject)
    }

    /// Return the compliance attestation for an address.
    pub fn get_compliance_attestation(env: Env, address: Address) -> Option<ComplianceAttestation> {
        compliance::get_attestation(&env, &address)
    }

    /// Enable compliance requirement for an existing grant. Owner only.
    pub fn set_grant_compliance_level(
        env: Env,
        owner: Address,
        grant_id: u64,
        level: ComplianceLevel,
    ) -> Result<(), ContractError> {
        owner.require_auth();
        let mut grant = Storage::get_grant(&env, grant_id).ok_or(ContractError::GrantNotFound)?;
        if grant.owner != owner {
            return Err(ContractError::Unauthorized);
        }
        grant.require_compliance = Some(level as u32);
        Storage::set_grant(&env, grant_id, &grant);
        Ok(())
    }

    // ── Issue #524: Price Oracle Integration ─────────────────────────────────

    /// Configure the on-chain price oracle. Admin only.
    pub fn set_oracle(env: Env, admin: Address, config: OracleConfig) -> Result<(), ContractError> {
        oracle::set_oracle(&env, &admin, config)
    }

    /// Fetch the current oracle price for a token.
    pub fn get_price(env: Env, token: Address) -> Result<PriceQuote, ContractError> {
        oracle::get_price(&env, &token)
    }

    /// Convert an amount between two token denominations using oracle prices.
    pub fn convert_amount(
        env: Env,
        amount: i128,
        from_token: Address,
        to_token: Address,
    ) -> Result<i128, ContractError> {
        oracle::convert_amount(&env, amount, &from_token, &to_token)
    }

    // ── Issue #585: Fee Relayer for Gasless Contributor UX ──────────────────

    /// Configure the relay system. Admin only.
    pub fn relay_set_config(
        env: Env,
        admin: Address,
        config: RelayConfig,
    ) -> Result<(), ContractError> {
        relay::set_relay_config(&env, &admin, config)
    }

    /// Execute a relayed action on behalf of sender.
    pub fn relay_execute(
        env: Env,
        relayer: Address,
        sender: Address,
        action: RelayableAction,
        nonce: u32,
        payload: Bytes,
    ) -> Result<(), ContractError> {
        relay::execute_relayed(&env, &relayer, &sender, action, nonce, payload)
    }

    /// Check if relay is allowed for an address and action.
    pub fn relay_can_relay(env: Env, sender: Address, action: RelayableAction) -> bool {
        relay::can_relay(&env, &sender, &action)
    }

    /// Reimburse the relayer from the treasury.
    pub fn relay_reimburse(env: Env, relayer: Address) -> Result<(), ContractError> {
        relay::reimburse_relayer(&env, &relayer)
    }

    /// Get relay allowance for an address.
    pub fn relay_get_allowance(env: Env, address: Address) -> RelayAllowance {
        relay::get_allowance(&env, &address)
    }

    /// Get current relay config.
    pub fn relay_get_config(env: Env) -> Option<RelayConfig> {
        relay::get_relay_config(&env)
    }

    // ── Issue #567: Decentralized Reviewer Recruitment Marketplace ──────────

    /// Register as a reviewer.
    pub fn reviewer_register(
        env: Env,
        reviewer: Address,
        display_name: String,
        expertise_tags: Vec<String>,
        hourly_rate: Option<i128>,
    ) -> Result<(), ContractError> {
        reviewer_pool::register_reviewer(&env, &reviewer, display_name, expertise_tags, hourly_rate)
    }

    /// Update reviewer availability status.
    pub fn reviewer_set_availability(
        env: Env,
        reviewer: Address,
        availability: ReviewerAvailability,
    ) -> Result<(), ContractError> {
        reviewer_pool::set_availability(&env, &reviewer, availability)
    }

    /// Request a reviewer for a grant.
    pub fn reviewer_request(
        env: Env,
        owner: Address,
        grant_id: u64,
        reviewer: Address,
        message: String,
        ttl_ledgers: u32,
    ) -> Result<(), ContractError> {
        reviewer_pool::request_reviewer(&env, &owner, grant_id, &reviewer, message, ttl_ledgers)
    }

    /// Accept a reviewer request.
    pub fn reviewer_accept_request(
        env: Env,
        reviewer: Address,
        grant_id: u64,
    ) -> Result<(), ContractError> {
        reviewer_pool::accept_request(&env, &reviewer, grant_id)
    }

    /// Decline a reviewer request.
    pub fn reviewer_decline_request(
        env: Env,
        reviewer: Address,
        grant_id: u64,
    ) -> Result<(), ContractError> {
        reviewer_pool::decline_request(&env, &reviewer, grant_id)
    }

    /// Get reviewer profile.
    pub fn reviewer_get_profile(env: Env, reviewer: Address) -> Option<ReviewerProfile> {
        reviewer_pool::get_profile(&env, &reviewer)
    }

    /// Get reviewer request.
    pub fn reviewer_get_request(
        env: Env,
        grant_id: u64,
        reviewer: Address,
    ) -> Option<ReviewerRequest> {
        reviewer_pool::get_request(&env, grant_id, &reviewer)
    }

    // ── Issue #571: Taxonomy, Category, and Tag System for Grants ──────────

    /// Create a new category.
    pub fn tags_create_category(
        env: Env,
        admin: Address,
        name: String,
        subcategories: Vec<String>,
    ) -> Result<u32, ContractError> {
        grant_tags::create_category(&env, &admin, name, subcategories)
    }

    /// Tag a grant.
    pub fn tags_tag_grant(
        env: Env,
        owner: Address,
        grant_id: u64,
        category_id: Option<u32>,
        subcategory: Option<String>,
        freeform_tags: Vec<String>,
    ) -> Result<(), ContractError> {
        grant_tags::tag_grant(
            &env,
            &owner,
            grant_id,
            category_id,
            subcategory,
            freeform_tags,
        )
    }

    /// Update tags on a grant.
    pub fn tags_update_tags(
        env: Env,
        owner: Address,
        grant_id: u64,
        freeform_tags: Vec<String>,
    ) -> Result<(), ContractError> {
        grant_tags::update_tags(&env, &owner, grant_id, freeform_tags)
    }

    /// Get tags for a grant.
    pub fn tags_get_tags(env: Env, grant_id: u64) -> Option<GrantTag> {
        grant_tags::get_tags(&env, grant_id)
    }

    /// Find grants by tag.
    pub fn tags_find_by_tag(env: Env, tag: String, offset: u32, limit: u32) -> Vec<u64> {
        grant_tags::find_by_tag(&env, &tag, offset, limit)
    }

    /// Find grants by category.
    pub fn tags_find_by_category(env: Env, category_id: u32, offset: u32, limit: u32) -> Vec<u64> {
        grant_tags::find_by_category(&env, category_id, offset, limit)
    }

    /// List all categories.
    pub fn tags_list_categories(env: Env) -> Vec<GrantCategory> {
        grant_tags::list_categories(&env)
    }

    /// Remove a tag from a grant.
    pub fn tags_remove_tag(
        env: Env,
        owner: Address,
        grant_id: u64,
        tag: String,
    ) -> Result<(), ContractError> {
        grant_tags::remove_tag(&env, &owner, grant_id, &tag)
    }

    // ── Issue #577: Automatic and Manual Grant Renewal ────────────────────

    /// Propose renewal of a grant.
    pub fn renewal_propose(
        env: Env,
        proposer: Address,
        original_grant_id: u64,
        new_title: String,
        new_description: String,
        new_total_amount: i128,
        new_num_milestones: u32,
        inherit_reviewers: bool,
        inherit_contributor: bool,
        ttl_ledgers: u32,
    ) -> Result<(), ContractError> {
        grant_renewal::propose_renewal(
            &env,
            &proposer,
            original_grant_id,
            new_title,
            new_description,
            new_total_amount,
            new_num_milestones,
            inherit_reviewers,
            inherit_contributor,
            ttl_ledgers,
        )
    }

    /// Approve a renewal proposal.
    pub fn renewal_approve(
        env: Env,
        reviewer: Address,
        original_grant_id: u64,
    ) -> Result<RenewalStatus, ContractError> {
        grant_renewal::approve_renewal(&env, &reviewer, original_grant_id)
    }

    /// Activate an approved renewal.
    pub fn renewal_activate(
        env: Env,
        owner: Address,
        original_grant_id: u64,
    ) -> Result<u64, ContractError> {
        grant_renewal::activate_renewal(&env, &owner, original_grant_id)
    }

    /// Decline a renewal proposal.
    pub fn renewal_decline(
        env: Env,
        caller: Address,
        original_grant_id: u64,
    ) -> Result<(), ContractError> {
        grant_renewal::decline_renewal(&env, &caller, original_grant_id)
    }

    /// Get renewal proposal.
    pub fn renewal_get_proposal(env: Env, original_grant_id: u64) -> Option<RenewalProposal> {
        grant_renewal::get_proposal(&env, original_grant_id)
    }

    /// Get renewal chain.
    pub fn renewal_chain(env: Env, original_grant_id: u64) -> Vec<u64> {
        grant_renewal::renewal_chain(&env, original_grant_id)
    }

    // ── Issue #576: Token Swap Entry Points ────────────────────────────────────

    pub fn set_dex_config(
        env: Env,
        admin: Address,
        config: DexConfig,
    ) -> Result<(), ContractError> {
        token_swap::set_dex_config(&env, &admin, config)
    }

    pub fn get_dex_config(env: Env) -> Result<DexConfig, ContractError> {
        token_swap::get_dex_config(&env)
    }

    pub fn swap_tokens(
        env: Env,
        caller: Address,
        route: SwapRoute,
        amount_in: i128,
    ) -> Result<SwapResult, ContractError> {
        token_swap::swap(&env, &caller, route, amount_in)
    }

    pub fn swap_quote(env: Env, route: SwapRoute, amount_in: i128) -> Result<i128, ContractError> {
        token_swap::quote(&env, &route, amount_in)
    }

    pub fn swap_and_fund(
        env: Env,
        funder: Address,
        grant_id: u64,
        input_token: Address,
        input_amount: i128,
    ) -> Result<SwapResult, ContractError> {
        emergency::require_not_paused(&env)?;
        token_swap::swap_and_fund(&env, &funder, grant_id, &input_token, input_amount)
    }

    pub fn swap_and_pay(
        env: Env,
        grant_id: u64,
        recipient: Address,
        grant_token: Address,
        preferred_token: Address,
        amount: i128,
    ) -> Result<SwapResult, ContractError> {
        emergency::require_not_paused(&env)?;
        token_swap::swap_and_pay(
            &env,
            grant_id,
            &recipient,
            &grant_token,
            &preferred_token,
            amount,
        )
    }

    // ── Issue #581: Milestone Checklist Entry Points ──────────────────────────

    pub fn checklist_define_criteria(
        env: Env,
        owner: Address,
        grant_id: u64,
        milestone_idx: u32,
        criteria: Vec<AcceptanceCriteria>,
    ) -> Result<(), ContractError> {
        emergency::require_not_paused(&env)?;
        checklist::define_criteria(&env, &owner, grant_id, milestone_idx, criteria)
    }

    pub fn checklist_submit(
        env: Env,
        contributor: Address,
        grant_id: u64,
        milestone_idx: u32,
        evidence_urls: Vec<Option<soroban_sdk::String>>,
    ) -> Result<(), ContractError> {
        emergency::require_not_paused(&env)?;
        checklist::submit_checklist(&env, &contributor, grant_id, milestone_idx, evidence_urls)
    }

    pub fn checklist_review_criterion(
        env: Env,
        reviewer: Address,
        grant_id: u64,
        milestone_idx: u32,
        criterion_idx: u32,
        approve: bool,
    ) -> Result<(), ContractError> {
        emergency::require_not_paused(&env)?;
        checklist::review_criterion(
            &env,
            &reviewer,
            grant_id,
            milestone_idx,
            criterion_idx,
            approve,
        )
    }

    pub fn checklist_all_required_approved(env: Env, grant_id: u64, milestone_idx: u32) -> bool {
        checklist::all_required_approved(&env, grant_id, milestone_idx)
    }

    pub fn checklist_get(
        env: Env,
        grant_id: u64,
        milestone_idx: u32,
    ) -> Option<ChecklistSubmission> {
        checklist::get_checklist(&env, grant_id, milestone_idx)
    }

    pub fn checklist_get_criterion_status(
        env: Env,
        grant_id: u64,
        milestone_idx: u32,
        criterion_idx: u32,
    ) -> Option<CriterionStatus> {
        checklist::get_criterion_status(&env, grant_id, milestone_idx, criterion_idx)
    }

    // ── Issue #589: Scoring Entry Points ──────────────────────────────────────

    pub fn scoring_define_rubric(
        env: Env,
        admin: Address,
        name: soroban_sdk::String,
        weights: Vec<ScoringWeight>,
    ) -> Result<u32, ContractError> {
        scoring::define_rubric(&env, &admin, name, weights)
    }

    pub fn scoring_score_contributor(
        env: Env,
        contributor: Address,
        rubric_id: u32,
    ) -> Result<ScoreResult, ContractError> {
        scoring::score_contributor(&env, &contributor, rubric_id)
    }

    pub fn scoring_rank_contributors(
        env: Env,
        contributors: Vec<Address>,
        rubric_id: u32,
    ) -> Vec<ScoreResult> {
        scoring::rank_contributors(&env, contributors, rubric_id)
    }

    pub fn scoring_get_rubric(env: Env, rubric_id: u32) -> Result<ScoringRubric, ContractError> {
        scoring::get_rubric(&env, rubric_id)
    }

    pub fn scoring_list_rubrics(env: Env) -> Vec<u32> {
        scoring::list_rubrics(&env)
    }

    // ── Issue #594: Circuit Breaker Entry Points ──────────────────────────────

    pub fn breaker_trip(
        env: Env,
        caller: Address,
        module: ProtocolModule,
        reason: soroban_sdk::String,
        auto_reset_ledger: Option<u32>,
    ) -> Result<(), ContractError> {
        circuit_breaker::trip(&env, &caller, module, reason, auto_reset_ledger)
    }

    pub fn breaker_reset(
        env: Env,
        caller: Address,
        module: ProtocolModule,
    ) -> Result<(), ContractError> {
        circuit_breaker::reset(&env, &caller, module)
    }

    pub fn breaker_is_open(env: Env, module: ProtocolModule) -> bool {
        circuit_breaker::is_open(&env, module)
    }

    pub fn breaker_get_state(env: Env, module: ProtocolModule) -> BreakerState {
        circuit_breaker::get_state(&env, module)
    }

    pub fn breaker_tripped_modules(env: Env) -> Vec<ProtocolModule> {
        circuit_breaker::tripped_modules(&env)
    }

    pub fn breaker_auto_reset_expired(env: Env) -> u32 {
        circuit_breaker::auto_reset_expired(&env)
    }

    // ── Private Helpers ───────────────────────────────────────────────────────

    fn update_contributor_reputation(
        env: &Env,
        grant_id: u64,
        milestone_idx: u32,
        contributor: &Address,
        payout_amount: i128,
    ) {
        if Storage::has_milestone_reputation_applied(env, grant_id, milestone_idx) {
            return;
        }
        Storage::mark_milestone_reputation_applied(env, grant_id, milestone_idx);
        let mut profile = match Storage::get_contributor(env, contributor.clone()) {
            Some(p) => p,
            None => return,
        };
        let _ = reputation::record_completion(
            env,
            grant_id,
            milestone_idx,
            &mut profile,
            payout_amount,
        );
    }
}

fn apply_milestone_submission(
    env: &Env,
    grant_id: u64,
    grant: &Grant,
    milestone_idx: u32,
    description: String,
    proof_url: String,
    actor: &Address,
) -> Result<(), ContractError> {
    if milestone_idx >= grant.total_milestones {
        return Err(ContractError::MilestoneIndexOutOfBounds);
    }

    if let Some(existing) = Storage::get_milestone(env, grant_id, milestone_idx) {
        if existing.state == MilestoneState::Submitted || existing.state == MilestoneState::Approved
        {
            return Err(ContractError::MilestoneAlreadySubmitted);
        }
    }

    let milestone = Milestone {
        idx: milestone_idx,
        description: description.clone(),
        amount: grant.milestone_amount,
        state: MilestoneState::Submitted,
        votes: soroban_sdk::Map::new(env),
        approvals: 0,
        rejections: 0,
        reasons: soroban_sdk::Map::new(env),
        status_updated_at: 0,
        proof_url: Some(proof_url),
        submission_timestamp: env.ledger().timestamp(),
    };

    Storage::set_milestone(env, grant_id, milestone_idx, &milestone);
    Events::emit_milestone_submitted(env, grant_id, milestone_idx, description);

    audit::log(
        env,
        grant_id,
        AuditAction::MilestoneSubmitted,
        actor,
        Some(milestone_idx),
        Some(grant.milestone_amount),
    );

    Ok(())
}

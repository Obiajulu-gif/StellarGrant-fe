use soroban_sdk::{token, Address, Env, String, Vec};

use crate::constants;
use crate::events::Events;
use crate::governance;
use crate::storage::Storage;
use crate::types::{
    BatchItemResult, BatchMilestoneVote, BatchResult, ContractError, GrantFund, GrantStatus,
    MilestoneState,
};

fn validate_batch_size(len: u32) -> Result<(), ContractError> {
    if len == 0 {
        return Err(ContractError::BatchEmpty);
    }
    if len > constants::MAX_BATCH_SIZE {
        return Err(ContractError::BatchTooLarge);
    }
    Ok(())
}

fn build_batch_result(results: Vec<BatchItemResult>) -> BatchResult {
    let total = results.len();
    let mut succeeded = 0u32;
    let mut failed = 0u32;
    for result in results.iter() {
        if result.success {
            succeeded += 1;
        } else {
            failed += 1;
        }
    }
    BatchResult {
        total,
        succeeded,
        failed,
        results,
    }
}

fn item_success(index: u32) -> BatchItemResult {
    BatchItemResult {
        index,
        success: true,
        error_code: None,
    }
}

fn item_failure(index: u32, error: ContractError) -> BatchItemResult {
    BatchItemResult {
        index,
        success: false,
        error_code: Some(error as u32),
    }
}

/// Vote on multiple milestones in one call. Reviewer only.
pub fn batch_vote_milestones(
    env: &Env,
    reviewer: &Address,
    votes: Vec<BatchMilestoneVote>,
) -> Result<BatchResult, ContractError> {
    let batch_len = votes.len();
    validate_batch_size(batch_len)?;

    reviewer.require_auth();

    let mut results = Vec::new(env);
    for (index, vote) in votes.iter().enumerate() {
        let item = match try_vote_milestone(env, reviewer, &vote) {
            Ok(()) => item_success(index as u32),
            Err(error) => item_failure(index as u32, error),
        };
        results.push_back(item);
    }

    Ok(build_batch_result(results))
}

fn try_vote_milestone(
    env: &Env,
    reviewer: &Address,
    vote: &BatchMilestoneVote,
) -> Result<(), ContractError> {
    let mut grant = Storage::get_grant(env, vote.grant_id).ok_or(ContractError::GrantNotFound)?;

    if vote.milestone_idx >= grant.total_milestones {
        return Err(ContractError::InvalidInput);
    }

    let mut milestone = Storage::get_milestone(env, vote.grant_id, vote.milestone_idx)
        .ok_or(ContractError::MilestoneNotFound)?;

    if milestone.state != MilestoneState::Submitted {
        return Err(ContractError::MilestoneNotSubmitted);
    }
    if !grant.reviewers.contains(reviewer.clone()) {
        return Err(ContractError::Unauthorized);
    }
    if milestone.votes.contains_key(reviewer.clone()) {
        return Err(ContractError::AlreadyVoted);
    }

    governance::cast_vote(
        env,
        &mut grant,
        &mut milestone,
        reviewer,
        vote.approve,
        vote.reason.clone(),
    )?;

    Storage::set_milestone(env, vote.grant_id, vote.milestone_idx, &milestone);
    Ok(())
}

/// Fund multiple grants with the same token in one call. Funder only.
pub fn batch_fund_grants(
    env: &Env,
    funder: &Address,
    token: &Address,
    items: Vec<(u64, i128)>,
) -> Result<BatchResult, ContractError> {
    let batch_len = items.len();
    validate_batch_size(batch_len)?;

    funder.require_auth();

    let mut results = Vec::new(env);
    for (index, item) in items.iter().enumerate() {
        let (grant_id, amount) = item;
        let item = match try_fund_grant(env, funder, token, grant_id, amount) {
            Ok(()) => item_success(index as u32),
            Err(error) => item_failure(index as u32, error),
        };
        results.push_back(item);
    }

    Ok(build_batch_result(results))
}

fn try_fund_grant(
    env: &Env,
    funder: &Address,
    token: &Address,
    grant_id: u64,
    amount: i128,
) -> Result<(), ContractError> {
    if amount <= 0 {
        return Err(ContractError::InvalidInput);
    }

    let mut grant = Storage::get_grant(env, grant_id).ok_or(ContractError::GrantNotFound)?;

    if grant.token != *token {
        return Err(ContractError::InvalidInput);
    }

    if grant.status != GrantStatus::Active {
        return Err(ContractError::InvalidState);
    }

    let token_client = token::Client::new(env, &grant.token);
    let contract_address = env.current_contract_address();
    token_client.transfer(funder, &contract_address, &amount);

    grant.escrow_balance = grant
        .escrow_balance
        .checked_add(amount)
        .ok_or(ContractError::InvalidInput)?;

    let mut funder_found = false;
    for i in 0..grant.funders.len() {
        let mut fund_entry = grant.funders.get(i).unwrap();
        if fund_entry.funder == *funder {
            fund_entry.amount = fund_entry
                .amount
                .checked_add(amount)
                .ok_or(ContractError::InvalidInput)?;
            grant.funders.set(i, fund_entry);
            funder_found = true;
            break;
        }
    }

    if !funder_found {
        grant.funders.push_back(GrantFund {
            funder: funder.clone(),
            amount,
        });
    }

    Storage::set_grant(env, grant_id, &grant);
    Events::emit_grant_funded(env, grant_id, funder.clone(), amount, grant.escrow_balance);

    Ok(())
}

/// Cancel multiple grants. Admin or owner only.
pub fn batch_cancel_grants(
    env: &Env,
    caller: &Address,
    grant_ids: Vec<u64>,
    reason: String,
) -> Result<BatchResult, ContractError> {
    let batch_len = grant_ids.len();
    validate_batch_size(batch_len)?;

    caller.require_auth();

    let mut results = Vec::new(env);
    for (index, grant_id) in grant_ids.iter().enumerate() {
        let item = match try_cancel_grant(env, caller, grant_id, &reason) {
            Ok(()) => item_success(index as u32),
            Err(error) => item_failure(index as u32, error),
        };
        results.push_back(item);
    }

    Ok(build_batch_result(results))
}

fn try_cancel_grant(
    env: &Env,
    caller: &Address,
    grant_id: u64,
    reason: &String,
) -> Result<(), ContractError> {
    let mut grant = Storage::get_grant(env, grant_id).ok_or(ContractError::GrantNotFound)?;

    let caller_is_owner = grant.owner == *caller;
    let caller_is_admin = Storage::get_global_admin(env) == Some(caller.clone());
    if !caller_is_owner && !caller_is_admin {
        return Err(ContractError::Unauthorized);
    }

    if grant.status != GrantStatus::Active {
        return Err(ContractError::InvalidState);
    }

    if grant.milestones_paid_out >= grant.total_milestones {
        return Err(ContractError::InvalidState);
    }

    let total_refundable = grant.escrow_balance;
    if total_refundable > 0 {
        let mut total_contributions: i128 = 0;
        for fund_entry in grant.funders.iter() {
            total_contributions += fund_entry.amount;
        }

        if total_contributions <= 0 {
            return Err(ContractError::InvalidInput);
        }

        let token_client = token::Client::new(env, &grant.token);
        let funders_len = grant.funders.len();
        let mut distributed = 0i128;

        for i in 0..funders_len {
            let fund_entry = grant.funders.get(i).unwrap();
            let is_last = i + 1 == funders_len;
            let refund_amount = if is_last {
                total_refundable - distributed
            } else {
                let amount = fund_entry
                    .amount
                    .checked_mul(total_refundable)
                    .ok_or(ContractError::InvalidInput)?
                    .checked_div(total_contributions)
                    .ok_or(ContractError::InvalidInput)?;
                distributed += amount;
                amount
            };

            if refund_amount > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &fund_entry.funder,
                    &refund_amount,
                );
                Events::emit_refund_issued(env, grant_id, fund_entry.funder.clone(), refund_amount);
            }
        }
    }

    grant.status = GrantStatus::Cancelled;
    grant.escrow_balance = 0;
    grant.reason = Some(reason.clone());
    grant.timestamp = env.ledger().timestamp();

    Storage::set_grant(env, grant_id, &grant);
    Events::emit_grant_cancelled(
        env,
        grant_id,
        caller.clone(),
        reason.clone(),
        total_refundable,
    );

    Ok(())
}

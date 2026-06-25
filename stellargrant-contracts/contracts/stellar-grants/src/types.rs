use soroban_sdk::{contracttype, Address, Bytes, Map, String, Vec};

pub use crate::errors::ContractError;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationRecord {
    pub from_version: u32,
    pub to_version: u32,
    pub run_by: Address,
    pub run_at: u64,
    pub success: bool,
    pub notes: String,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub deployed_at: u64,
    pub deployer: Address,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryEntry {
    pub address: Address,
    pub registered_at: u64,
    pub is_active: bool,
    pub entry_type: RegistryEntryType,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum RegistryEntryType {
    Contributor = 0,
    Reviewer = 1,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum MilestoneState {
    Pending = 0,
    Submitted = 1,
    Approved = 2,
    Rejected = 3,
    Paid = 4,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Milestone {
    pub idx: u32,
    pub description: String,
    pub amount: i128,
    pub state: MilestoneState,
    pub votes: Map<Address, bool>,
    pub approvals: u32,
    pub rejections: u32,
    pub reasons: Map<Address, String>,
    pub status_updated_at: u64,
    pub proof_url: Option<String>,
    pub submission_timestamp: u64,
}

#[contracttype]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum GrantStatus {
    Active = 1,
    Cancelled = 2,
    Completed = 3,
}

#[contracttype]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantFund {
    pub funder: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub id: u64,
    pub owner: Address,
    pub title: String,
    pub description: String,
    pub token: Address,
    pub status: GrantStatus,
    pub total_amount: i128,
    pub milestone_amount: i128,
    pub reviewers: Vec<Address>,
    pub total_milestones: u32,
    pub milestones_paid_out: u32,
    pub escrow_balance: i128,
    pub funders: Vec<GrantFund>,
    pub reason: Option<String>,
    pub timestamp: u64,
    pub require_compliance: Option<u32>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContributorProfile {
    pub contributor: Address,
    pub name: String,
    pub bio: String,
    pub skills: Vec<String>,
    pub github_url: String,
    pub registration_timestamp: u64,
    pub reputation_score: u64,
    pub grants_count: u32,
    pub total_earned: i128,
    pub reputation_score: u64,
    pub milestones_completed: u32,
    pub milestones_rejected: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DelegationScope {
    Global,
    PerGrant(u64),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delegation {
    pub delegator: Address,
    pub delegate: Address,
    pub scope: DelegationScope,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub revoked: bool,
    pub uses_remaining: Option<u32>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum BadgeType {
    FirstMilestone = 0,
    TenMilestones = 1,
    FiftyMilestones = 2,
    BronzeContributor = 3,
    SilverContributor = 4,
    GoldContributor = 5,
    PlatinumContributor = 6,
    DisputeWinner = 7,
    PerfectGrant = 8,
    BountyChampion = 9,
    EarlyAdopter = 10,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadgeCriteria {
    pub badge_type: BadgeType,
    pub required_milestones: Option<u32>,
    pub required_reputation: Option<u32>,
    pub required_grants: Option<u32>,
    pub one_time: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadgeRecord {
    pub badge_type: BadgeType,
    pub recipient: Address,
    pub awarded_at: u64,
    pub grant_id: Option<u64>,
    pub milestone_idx: Option<u32>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum RefundPolicyType {
    FullRefund = 0,
    ProportionalToRemaining = 1,
    TimeWeighted = 2,
    PenaltyOnCancel = 3,
    NoRefund = 4,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefundPolicy {
    pub grant_id: u64,
    pub policy_type: RefundPolicyType,
    pub penalty_bps: u32,
    pub grace_period_ledgers: u32,
    pub min_refund_pct_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefundCalculation {
    pub gross_escrow: i128,
    pub funder_refund: i128,
    pub contributor_compensation: i128,
    pub penalty_amount: i128,
    pub policy_applied: RefundPolicyType,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SnapshotTrigger {
    DisputeRaised = 0,
    AdminRequest = 1,
    MilestoneSubmission = 2,
    PreUpgrade = 3,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateSnapshot {
    pub id: u32,
    pub grant_id: u64,
    pub trigger: SnapshotTrigger,
    pub grant_status: GrantStatus,
    pub escrow_balance: i128,
    pub milestones_paid_out: u32,
    pub total_milestones: u32,
    pub milestone_states: Vec<MilestoneState>,
    pub captured_at: u64,
    pub captured_at_ledger: u32,
    pub captured_by: Address,
}

use soroban_sdk::{Address, Env, String};

use crate::errors::ContractError;
use crate::events::{ComplianceAttested, ComplianceRevoked};
use crate::storage::Storage;
use crate::types::{ComplianceAttestation, ComplianceLevel, ComplianceStatus};

// ── Public API ────────────────────────────────────────────────────────────────

/// Register a trusted compliance verifier address. Admin only.
pub fn set_verifier(env: &Env, admin: &Address, verifier: &Address) -> Result<(), ContractError> {
    if Storage::get_global_admin(env) != Some(admin.clone()) {
        return Err(ContractError::Unauthorized);
    }
    Storage::set_compliance_verifier(env, verifier);
    Ok(())
}

/// A trusted verifier attests the compliance status of a subject address.
/// No personal data is stored — only status, level, jurisdiction code, and timestamps.
pub fn attest(
    env: &Env,
    verifier: &Address,
    subject: &Address,
    status: ComplianceStatus,
    level: ComplianceLevel,
    expires_at: u64,
    jurisdiction: String,
) -> Result<(), ContractError> {
    let registered_verifier =
        Storage::get_compliance_verifier(env).ok_or(ContractError::VerifierNotSet)?;
    if *verifier != registered_verifier {
        return Err(ContractError::NotVerifier);
    }

    let now = env.ledger().timestamp();
    let level_u32 = level as u32;

    let attestation = ComplianceAttestation {
        subject: subject.clone(),
        status,
        level,
        attested_by: verifier.clone(),
        attested_at: now,
        expires_at,
        jurisdiction,
    };
    Storage::set_compliance_attestation(env, &attestation);

    ComplianceAttested {
        subject: subject.clone(),
        attested_by: verifier.clone(),
        level: level_u32,
        expires_at,
        timestamp: now,
    }
    .publish(env);

    Ok(())
}

/// Revoke a compliance attestation. Verifier or admin only.
pub fn revoke(env: &Env, revoker: &Address, subject: &Address) -> Result<(), ContractError> {
    let is_admin = Storage::get_global_admin(env) == Some(revoker.clone());
    let is_verifier = Storage::get_compliance_verifier(env) == Some(revoker.clone());
    if !is_admin && !is_verifier {
        return Err(ContractError::Unauthorized);
    }

    Storage::remove_compliance_attestation(env, subject);

    ComplianceRevoked {
        subject: subject.clone(),
        revoked_by: revoker.clone(),
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    Ok(())
}

/// Return the compliance attestation for an address.
pub fn get_attestation(env: &Env, address: &Address) -> Option<ComplianceAttestation> {
    Storage::get_compliance_attestation(env, address)
}

/// Check if an address meets the required compliance level.
/// Returns Err if not compliant, expired, or rejected.
pub fn require_compliant(
    env: &Env,
    address: &Address,
    required_level: ComplianceLevel,
) -> Result<(), ContractError> {
    require_compliant_u32(env, address, required_level as u32)
}

/// Check if an address meets the required compliance level (u32 version).
/// Returns Err if not compliant, expired, or rejected.
pub fn require_compliant_u32(
    env: &Env,
    address: &Address,
    required_level: u32,
) -> Result<(), ContractError> {
    // ComplianceLevel::None means no check required.
    if required_level == ComplianceLevel::None as u32 {
        return Ok(());
    }

    let attestation = Storage::get_compliance_attestation(env, address)
        .ok_or(ContractError::ComplianceNotVerified)?;

    if !is_valid(env, &attestation) {
        return Err(ContractError::ComplianceCheckFailed);
    }

    if matches!(attestation.status, ComplianceStatus::Rejected) {
        return Err(ContractError::ComplianceCheckFailed);
    }

    if !matches!(attestation.status, ComplianceStatus::Approved) {
        return Err(ContractError::ComplianceNotVerified);
    }

    let attestation_level = attestation.level as u32;
    if attestation_level < required_level {
        return Err(ContractError::ComplianceCheckFailed);
    }

    Ok(())
}

/// Check if an attestation is still valid (not expired).
pub fn is_valid(env: &Env, attestation: &ComplianceAttestation) -> bool {
    let now = env.ledger().timestamp();
    if attestation.expires_at > 0 && now > attestation.expires_at {
        return false;
    }
    !matches!(
        attestation.status,
        ComplianceStatus::Expired | ComplianceStatus::Rejected
    )
}

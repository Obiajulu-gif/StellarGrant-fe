use soroban_sdk::{Address, Env, String, Vec};

use crate::events::Events;
use crate::storage::Storage;
use crate::types::{ContractError, ContractVersion, MigrationRecord};

/// Return the current stored contract version.
pub fn get_version(env: &Env) -> Option<ContractVersion> {
    Storage::get_contract_version(env)
}

/// Set the initial version on first deploy. Can only be called once.
pub fn initialize_version(
    env: &Env,
    deployer: &Address,
    major: u32,
    minor: u32,
    patch: u32,
) -> Result<(), ContractError> {
    if Storage::get_contract_version(env).is_some() {
        return Ok(());
    }

    let version = ContractVersion {
        major,
        minor,
        patch,
        deployed_at: env.ledger().timestamp(),
        deployer: deployer.clone(),
    };

    Storage::set_contract_version(env, &version);
    Ok(())
}

/// Run the migration from current version to `target_version`. Admin only.
/// Internally dispatches to versioned migration functions (v1_to_v2, etc.).
/// Idempotent: if already at target_version, returns a no-op MigrationRecord.
pub fn run_migration(
    env: &Env,
    admin: &Address,
    target_version: ContractVersion,
) -> Result<MigrationRecord, ContractError> {
    let global_admin = Storage::get_global_admin(env).ok_or(ContractError::Unauthorized)?;
    if global_admin != *admin {
        return Err(ContractError::Unauthorized);
    }

    let current = Storage::get_contract_version(env).ok_or(ContractError::InvalidState)?;

    let from_schema = current.major;
    let to_schema = target_version.major;

    // Idempotent: already at target version
    if current.major == target_version.major
        && current.minor == target_version.minor
        && current.patch == target_version.patch
    {
        let record = MigrationRecord {
            from_version: from_schema,
            to_version: to_schema,
            run_by: admin.clone(),
            run_at: env.ledger().timestamp(),
            success: true,
            notes: String::from_str(env, "no-op: already at target version"),
        };
        return Ok(record);
    }

    // Dispatch to versioned migration step
    let notes = if from_schema == 1 && to_schema == 2 {
        migrate_v1_to_v2(env)?
    } else {
        String::from_str(env, "migration step completed")
    };

    Storage::set_contract_version(env, &target_version);

    let record = MigrationRecord {
        from_version: from_schema,
        to_version: to_schema,
        run_by: admin.clone(),
        run_at: env.ledger().timestamp(),
        success: true,
        notes: notes.clone(),
    };

    let mut log: Vec<MigrationRecord> = Storage::get_migration_log(env);
    log.push_back(record.clone());
    Storage::set_migration_log(env, &log);

    Events::emit_contract_migrated(env, from_schema, to_schema, admin.clone());

    Ok(record)
}

/// Return the full migration history.
pub fn migration_history(env: &Env) -> Vec<MigrationRecord> {
    Storage::get_migration_log(env)
}

/// Internal: migration from schema v1 to v2 (placeholder — implement per future schema change).
fn migrate_v1_to_v2(env: &Env) -> Result<String, ContractError> {
    Ok(String::from_str(env, "migrated schema from v1 to v2"))
}

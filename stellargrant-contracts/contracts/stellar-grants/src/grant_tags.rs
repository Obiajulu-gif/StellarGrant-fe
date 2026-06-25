use crate::storage::Storage;
use crate::types::{ContractError, GrantCategory, GrantTag};
use soroban_sdk::{Address, Env, String, Vec};

const MAX_FREEFORM_TAGS: u32 = 10;

pub fn create_category(
    env: &Env,
    admin: &Address,
    name: String,
    subcategories: Vec<String>,
) -> Result<u32, ContractError> {
    admin.require_auth();

    if let Some(current_admin) = crate::Storage::get_global_admin(env) {
        if current_admin != *admin {
            return Err(ContractError::Unauthorized);
        }
    }

    let mut categories = Storage::get_category_list(env);
    let id = categories.len();

    let category = GrantCategory {
        id,
        name,
        subcategories,
    };

    categories.push_back(category);
    Storage::set_category_list(env, &categories);
    Ok(id)
}

pub fn tag_grant(
    env: &Env,
    owner: &Address,
    grant_id: u64,
    category_id: Option<u32>,
    subcategory: Option<String>,
    freeform_tags: Vec<String>,
) -> Result<(), ContractError> {
    owner.require_auth();

    let grant = Storage::get_grant_v(env, grant_id);
    if grant.owner != *owner {
        return Err(ContractError::Unauthorized);
    }

    if freeform_tags.len() > MAX_FREEFORM_TAGS {
        return Err(ContractError::InvalidInput);
    }

    if let Some(cat_id) = category_id {
        let categories = Storage::get_category_list(env);
        if !categories.iter().any(|c| c.id == cat_id) {
            return Err(ContractError::InvalidInput);
        }
    }

    let tag = GrantTag {
        grant_id,
        category_id,
        subcategory,
        freeform_tags: freeform_tags.clone(),
        tagged_by: owner.clone(),
        tagged_at: env.ledger().timestamp(),
    };

    Storage::set_grant_tags(env, &tag);

    for tag_str in freeform_tags.iter() {
        let hash = hash_tag(&tag_str);
        let mut index = Storage::get_tag_index(env, hash);
        let has_grant = index.iter().any(|id| id == grant_id);
        if !has_grant {
            index.push_back(grant_id);
            Storage::set_tag_index(env, hash, &index);
        }
    }

    Ok(())
}

pub fn update_tags(
    env: &Env,
    owner: &Address,
    grant_id: u64,
    freeform_tags: Vec<String>,
) -> Result<(), ContractError> {
    owner.require_auth();

    let grant = Storage::get_grant_v(env, grant_id);
    if grant.owner != *owner {
        return Err(ContractError::Unauthorized);
    }

    if freeform_tags.len() > MAX_FREEFORM_TAGS {
        return Err(ContractError::InvalidInput);
    }

    let mut tag = Storage::get_grant_tags(env, grant_id).ok_or(ContractError::InvalidState)?;
    tag.freeform_tags = freeform_tags;
    Storage::set_grant_tags(env, &tag);
    Ok(())
}

pub fn get_tags(env: &Env, grant_id: u64) -> Option<GrantTag> {
    Storage::get_grant_tags(env, grant_id)
}

pub fn find_by_tag(env: &Env, tag: &String, offset: u32, limit: u32) -> Vec<u64> {
    let hash = hash_tag(tag);
    let index = Storage::get_tag_index(env, hash);
    let limit = (limit as usize).min(50);
    let offset = offset as usize;
    let mut result = Vec::new(env);

    for i in offset..offset + limit {
        if (i as u32) < index.len() {
            result.push_back(index.get(i as u32).unwrap());
        } else {
            break;
        }
    }

    result
}

pub fn find_by_category(env: &Env, category_id: u32, _offset: u32, _limit: u32) -> Vec<u64> {
    let categories = Storage::get_category_list(env);
    if !categories.iter().any(|c| c.id == category_id) {
        return Vec::new(env);
    }

    Vec::new(env)
}

pub fn list_categories(env: &Env) -> Vec<GrantCategory> {
    Storage::get_category_list(env)
}

pub fn remove_tag(
    env: &Env,
    owner: &Address,
    grant_id: u64,
    tag: &String,
) -> Result<(), ContractError> {
    owner.require_auth();

    let grant = Storage::get_grant_v(env, grant_id);
    if grant.owner != *owner {
        return Err(ContractError::Unauthorized);
    }

    let mut tag_obj = Storage::get_grant_tags(env, grant_id).ok_or(ContractError::InvalidState)?;

    let mut filtered_tags = Vec::new(env);
    for existing_tag in tag_obj.freeform_tags.iter() {
        if existing_tag != *tag {
            filtered_tags.push_back(existing_tag);
        }
    }
    tag_obj.freeform_tags = filtered_tags;
    Storage::set_grant_tags(env, &tag_obj);

    let hash = hash_tag(tag);
    let index = Storage::get_tag_index(env, hash);
    let mut filtered_index = Vec::new(env);
    for idx_grant_id in index.iter() {
        if idx_grant_id != grant_id {
            filtered_index.push_back(idx_grant_id);
        }
    }
    Storage::set_tag_index(env, hash, &filtered_index);

    Ok(())
}

fn hash_tag(tag: &String) -> u32 {
    tag.len().wrapping_mul(31)
}

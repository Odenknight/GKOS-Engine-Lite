//! Crate-private Phase-5 watcher/recovery contract verifier.
//!
//! This module consumes inert JSON envelopes produced by the pinned Full host.
//! It has no filesystem watcher, vault parser, GKX identity, provider, network,
//! service, pointer-publication, or governance-ledger authority.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use crate::contract::{is_valid_authored_uid, is_valid_retrieval_source_path};
use crate::digest::{canonical_digest, canonical_json, sha256};

const PACK_VERSION: &str = "gkos-watcher-recovery/1.0.0-draft.1";
const SAMPLE_PLAN_VERSION: &str = "gkos-watcher-convergence-sample-plan/1.0.0-draft.1";
const SAMPLE_PLAN_DIGEST: &str =
    "sha256:6ab764aad47cbb072469f19760b772df90b2138acaf6a9f022041d38094bb695";

const PACK_FILES: [&str; 17] = [
    "README.md",
    "TECHNICAL_README.md",
    "authority.schema.json",
    "batch.schema.json",
    "coherent-manifest.schema.json",
    "conformance.schema.json",
    "journal.schema.json",
    "sample-plan.schema.json",
    "source-removal.schema.json",
    "status.schema.json",
    "topology.schema.json",
    "transition.schema.json",
    "watcher-cli-fixture.json",
    "watcher-conformance-fixture.json",
    "watcher-recovery-fixture.json",
    "watcher-sample-plan.json",
    "watcher-storage-fixture.json",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WatcherError(&'static str);

type WatcherResult<T> = Result<T, WatcherError>;

fn fail<T>(code: &'static str) -> WatcherResult<T> {
    Err(WatcherError(code))
}

fn object(value: &Value) -> WatcherResult<&Map<String, Value>> {
    value
        .as_object()
        .ok_or(WatcherError("GKX_WATCHER_CONTRACT_RECORD_INVALID"))
}

fn array(value: &Value) -> WatcherResult<&Vec<Value>> {
    value
        .as_array()
        .ok_or(WatcherError("GKX_WATCHER_CONTRACT_RECORD_INVALID"))
}

fn text<'a>(record: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    record.get(key).and_then(Value::as_str)
}

fn unsigned(record: &Map<String, Value>, key: &str) -> Option<u64> {
    record.get(key).and_then(Value::as_u64)
}

fn exact_keys(record: &Map<String, Value>, keys: &[&str]) -> WatcherResult<()> {
    if record.len() != keys.len() || keys.iter().any(|key| !record.contains_key(*key)) {
        return fail("GKX_WATCHER_CONTRACT_KEYS_INVALID");
    }
    Ok(())
}

fn utf16_cmp(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn is_digest(value: &Value) -> bool {
    value.as_str().is_some_and(is_digest_text)
}

fn is_digest_text(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn is_uuid7(value: &Value) -> bool {
    value.as_str().is_some_and(|value| {
        let bytes = value.as_bytes();
        bytes.len() == 36
            && bytes[8] == b'-'
            && bytes[13] == b'-'
            && bytes[14] == b'7'
            && bytes[18] == b'-'
            && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
            && bytes[23] == b'-'
            && bytes.iter().enumerate().all(|(index, byte)| {
                matches!(index, 8 | 13 | 18 | 23)
                    || byte.is_ascii_digit()
                    || matches!(byte, b'a'..=b'f')
            })
    })
}

fn is_watcher_generation(value: &Value) -> bool {
    value.as_str().is_some_and(|value| {
        value
            .strip_prefix("watcher:")
            .is_some_and(|uuid| is_uuid7(&Value::String(uuid.to_owned())))
    })
}

fn is_iso(value: &Value) -> bool {
    let Some(value) = value.as_str() else {
        return false;
    };
    let bytes = value.as_bytes();
    if bytes.len() != 24
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[23] != b'Z'
        || bytes.iter().enumerate().any(|(index, byte)| {
            !matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 23) && !byte.is_ascii_digit()
        })
    {
        return false;
    }
    let parse = |start: usize, end: usize| value[start..end].parse::<u32>().ok();
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        parse(0, 4),
        parse(5, 7),
        parse(8, 10),
        parse(11, 13),
        parse(14, 16),
        parse(17, 19),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    day >= 1 && day <= days && hour < 24 && minute < 60 && second < 60
}

fn is_integer(value: &Value, minimum: u64, maximum: u64) -> bool {
    value
        .as_u64()
        .is_some_and(|number| number >= minimum && number <= maximum)
}

fn is_sorted_unique_strings(value: &Value, maximum: usize) -> bool {
    let Some(items) = value.as_array() else {
        return false;
    };
    if items.len() > maximum {
        return false;
    }
    let Some(strings) = items.iter().map(Value::as_str).collect::<Option<Vec<_>>>() else {
        return false;
    };
    strings
        .windows(2)
        .all(|pair| utf16_cmp(pair[0], pair[1]) == Ordering::Less)
}

fn valid_label(value: &Value) -> bool {
    value.as_str().is_some_and(|value| {
        let bytes = value.as_bytes();
        (1..=128).contains(&bytes.len())
            && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
            && bytes.iter().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b':' | b'-')
            })
    })
}

fn valid_opaque(value: &Value) -> bool {
    value.as_str().is_some_and(|value| {
        (1..=512).contains(&value.encode_utf16().count())
            && !value.trim().is_empty()
            && !value
                .chars()
                .any(|character| character <= '\u{1f}' || character == '\u{7f}')
    })
}

fn decimal_identity(value: &Value) -> bool {
    value.as_str().is_some_and(|value| {
        value == "0" || (!value.starts_with('0') && value.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

fn valid_source_path(value: &Value) -> bool {
    value.as_str().is_some_and(|path| {
        !path.is_empty()
            && path.len() <= 1024
            && !path.contains('\u{7f}')
            && is_valid_retrieval_source_path(path)
    })
}

fn digest_without(value: &Value, digest_key: &str) -> WatcherResult<String> {
    let mut material = object(value)?.clone();
    material.remove(digest_key);
    canonical_digest(&Value::Object(material))
        .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_RECORD_INVALID"))
}

fn canonical_bytes(value: &Value) -> WatcherResult<Vec<u8>> {
    // Validate the full value through the same JCS implementation used by all
    // semantic digests, then render directly from recursively UTF-16-sorted
    // keys. Re-parsing into serde_json::Map would silently switch astral/BMP
    // key order to Rust scalar-value order.
    canonical_json(value).map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_RECORD_INVALID"))?;
    fn render(value: &Value, depth: usize, output: &mut String) -> WatcherResult<()> {
        match value {
            Value::Object(record) => {
                if record.is_empty() {
                    output.push_str("{}");
                    return Ok(());
                }
                let mut keys = record.keys().collect::<Vec<_>>();
                keys.sort_by(|left, right| utf16_cmp(left, right));
                output.push_str("{\n");
                for (index, key) in keys.iter().enumerate() {
                    output.push_str(&"  ".repeat(depth + 1));
                    output.push_str(
                        &serde_json::to_string(key)
                            .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_RECORD_INVALID"))?,
                    );
                    output.push_str(": ");
                    render(&record[*key], depth + 1, output)?;
                    if index + 1 != keys.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                output.push_str(&"  ".repeat(depth));
                output.push('}');
            }
            Value::Array(items) => {
                if items.is_empty() {
                    output.push_str("[]");
                    return Ok(());
                }
                output.push_str("[\n");
                for (index, item) in items.iter().enumerate() {
                    output.push_str(&"  ".repeat(depth + 1));
                    render(item, depth + 1, output)?;
                    if index + 1 != items.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                output.push_str(&"  ".repeat(depth));
                output.push(']');
            }
            _ => output.push_str(
                &canonical_json(value)
                    .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_RECORD_INVALID"))?,
            ),
        }
        Ok(())
    }
    let mut rendered = String::new();
    render(value, 0, &mut rendered)?;
    let mut bytes = rendered.into_bytes();
    bytes.push(b'\n');
    Ok(bytes)
}

struct Descriptor {
    keys: &'static [&'static str],
    digest: Option<&'static str>,
}

macro_rules! descriptor {
    ($digest:expr; $($key:literal),+ $(,)?) => {
        Descriptor { keys: &[$($key),+], digest: $digest }
    };
}

fn descriptor_for(version: &str) -> Option<Descriptor> {
    Some(match version {
        "gkos-watcher-observation/1.0.0-draft.1" => {
            descriptor!(Some("observation_digest"); "contract_version","batch_id","batch_kind","observed_paths","unscoped","overflow","started_at","observation_digest")
        }
        "gkos-watcher-observation-authority/1.0.0-draft.1" => {
            descriptor!(Some("authority_digest"); "contract_version","batch_id","observation_digest","observation_artifact_file","observation_raw_sha256","observation_byte_size","pre_scan_state_digest","started_at","authority_digest")
        }
        "gkos-watcher-batch-record/1.0.0-draft.1" => {
            descriptor!(Some("batch_record_digest"); "contract_version","batch_id","batch_kind","observation_authority_digest","started_at","execution_kind","retry_of_batch_id","batch_record_digest")
        }
        "gkos-watcher-pre-scan-state/1.0.0-draft.1" => {
            descriptor!(None; "contract_version","vault_id","active_pointer_digest","active_coherent_manifest_digest","topology_snapshot_digest","configuration_digest","policy_digest","effective_profile_digest")
        }
        "gkos-watcher-batch-plan/1.0.0-draft.1" => {
            descriptor!(Some("plan_digest"); "contract_version","batch_id","observation_digest","topology_snapshot_digest","effective_profile_digest","validation_result_digest","rejection_journal_digest","intended_source_mutations","folder_set_changed","attachment_set_changed","mutation_set_digest","plan_digest")
        }
        "gkos-watcher-plan-authority/1.0.0-draft.1" => {
            descriptor!(Some("authority_digest"); "contract_version","batch_id","observation_digest","plan_digest","plan_artifact_file","plan_raw_sha256","plan_byte_size","target_topology_snapshot_digest","source_removal_event_count","source_removal_event_set_digest","authority_digest")
        }
        "gkos-watcher-topology-snapshot/1.0.0-draft.1" => {
            descriptor!(Some("topology_snapshot_digest"); "contract_version","vault_id","source_observation_snapshot_digest","validation_result_digest","rejection_journal_digest","accepted_sources","rejected_sources","folder_paths","attachment_paths","accepted_source_set_digest","rejected_source_set_digest","folder_set_digest","attachment_set_digest","topology_snapshot_digest")
        }
        "gkos-watcher-transition/1.0.0-draft.1" => {
            descriptor!(Some("transition_digest"); "contract_version","batch_id","transition_ordinal","state","last_reached_state","terminal_state","observation_digest","plan_digest","prior_transition_digest","gkx_delta_digest","gkx_snapshot_digest","retrieval_projection_state","graph_projection_state","reason_codes","recorded_at","completed_at","transition_digest")
        }
        "gkos-watcher-normalized-graph-delta/1.0.0-draft.1" => {
            descriptor!(None; "contract_version","delta")
        }
        "gkos-watcher-canonical-gkx-graph/1.0.0-draft.1" => {
            descriptor!(None; "contract_version","normalized_graph")
        }
        "gkos-watcher-graphiti-projection/1.0.0-draft.1" => {
            descriptor!(None; "contract_version","processing_time","episodes")
        }
        "gkos-watcher-raw-graph-artifact/1.0.0-draft.1" => {
            descriptor!(Some("graph_artifact_digest"); "contract_version","service_generation_id","topology_snapshot_digest","graph","graph_artifact_digest")
        }
        "gkos-watcher-coherent-manifest/1.0.0-draft.1" => {
            descriptor!(Some("coherent_manifest_digest"); "contract_version","service_generation_id","vault_id","completed_batch_id","completed_transition_digest","topology_snapshot_digest","topology_artifact_file","topology_artifact_raw_sha256","source_observation_snapshot_digest","effective_profile_digest","validation_result_digest","rejection_journal_digest","configuration_digest","policy_digest","gkx_snapshot_digest","retrieval_projection_state","graph_projection_state","source_removal_event_count","source_removal_event_set_digest","created_at","coherent_manifest_digest")
        }
        "gkos-watcher-active-pointer/1.0.0-draft.1" => {
            descriptor!(Some("pointer_digest"); "contract_version","kind","service_generation_id","coherent_manifest_file","coherent_manifest_digest","prior_pointer_digest","pointer_digest")
        }
        "gkos-watcher-journal-meta/1.0.0-draft.1" => {
            descriptor!(Some("meta_digest"); "contract_version","journal_instance_id","vault_id","configuration_digest","policy_digest","effective_profile_digest","anchor_coherent_manifest_digest","created_at","meta_digest")
        }
        "gkos-watcher-activation-intent/1.0.0-draft.1" => {
            descriptor!(Some("intent_digest"); "contract_version","prepared_transition_digest","coherent_manifest_digest","prior_pointer_digest","target_pointer","target_complete_transition","prepared_at","intent_digest")
        }
        "gkos-watcher-activation-outcome/1.0.0-draft.1" => {
            descriptor!(Some("outcome_digest"); "contract_version","intent_digest","coherent_manifest_digest","outcome","pointer_digest","reason_codes","recorded_at","outcome_digest")
        }
        "gkos-watcher-active-coherent/1.0.0-draft.1" => {
            descriptor!(Some("active_digest"); "contract_version","service_generation_id","coherent_manifest_digest","pointer_digest","intent_digest","activated_at","active_digest")
        }
        "gkos-watcher-authority/1.0.0-draft.1" => {
            descriptor!(Some("authority_digest"); "contract_version","kind","vault_id","configuration_digest","policy_digest","effective_profile_digest","first_service_generation_id","first_coherent_manifest_digest","first_pointer_digest","authority_digest")
        }
        "gkos-watcher-journal-generation/1.0.0-draft.1" => {
            descriptor!(Some("journal_generation_digest"); "contract_version","journal_instance_id","directory_leaf","database_file","meta_digest","anchor_coherent_manifest_digest","created_at","journal_generation_digest")
        }
        "gkos-watcher-journal-active-pointer/1.0.0-draft.1" => {
            descriptor!(Some("pointer_digest"); "contract_version","kind","journal_generation_file","journal_generation_digest","prior_pointer_digest","pointer_digest")
        }
        "gkos-watcher-journal-file-identity/1.0.0-draft.1" => {
            descriptor!(Some("identity_digest"); "contract_version","role","leaf","device","inode","mode","byte_size","raw_sha256","identity_digest")
        }
        "gkos-watcher-journal-archive/1.0.0-draft.1" => {
            descriptor!(Some("archive_manifest_digest"); "contract_version","journal_instance_id","directory_leaf","directory_device","directory_inode","directory_mode","database_identity","wal_identity","shm_identity","outer_coherent_manifest_digest","archived_at","archive_manifest_digest")
        }
        "gkos-watcher-journal-reset/1.0.0-draft.1" => {
            descriptor!(Some("reset_digest"); "contract_version","reset_id","prior_journal_generation_digest","archive_manifest_digest","new_journal_meta_digest","new_journal_generation_digest","target_journal_pointer_digest","outer_coherent_manifest_digest","ready_event_count","reset_carry_event_set_digest","reset_carry_activation_digest","reset_at","reset_digest")
        }
        "gkos-watcher-journal-reset-guard/1.0.0-draft.1" => {
            descriptor!(Some("guard_digest"); "contract_version","operation","owner_nonce","parent_device","parent_inode","parent_mode","guard_basename","guard_stage_basename","old_journal_pointer_digest","old_journal_generation_digest","outer_coherent_manifest_digest","archive_manifest_digest","new_journal_instance_id","new_journal_directory_leaf","new_journal_meta_digest","new_journal_generation_digest","reset_digest","target_journal_pointer_digest","ready_event_count","reset_carry_event_set_digest","reset_carry_activation_digest","guard_digest")
        }
        "gkos-watcher-pointer-replace-guard/1.0.0-draft.1" => {
            descriptor!(Some("guard_digest"); "contract_version","operation","owner_nonce","parent_device","parent_inode","parent_mode","final_basename","guard_basename","guard_stage_basename","temp_basename","old_pointer_file","old_pointer_digest","old_pointer_raw_sha256","old_pointer_byte_size","old_final_device","old_final_inode","new_pointer_file","new_pointer_digest","new_pointer_raw_sha256","new_pointer_byte_size","operation_intent_digest","target_commit_digest","guard_digest")
        }
        "gkos-watcher-pointer-recovery-decision/1.0.0-draft.1" => {
            descriptor!(Some("decision_digest"); "contract_version","selected_action","reader_authority","reader_pointer_digest","evidence_disposition","decision_digest")
        }
        "gkos-watcher-source-removal-authorization-scope/1.0.0-draft.1" => {
            descriptor!(Some("authorization_binding_digest"); "contract_version","adapter_kind","adapter_id","adapter_contract_version","vault_id","authority_namespace","authorized_operation","configuration_digest","policy_digest","authorization_binding_digest")
        }
        "gkos-watcher-source-removal-adapter-binding/1.0.0-draft.1" => {
            descriptor!(Some("binding_digest"); "contract_version","adapter_kind","adapter_id","adapter_contract_version","vault_id","authority_namespace","authorization_binding_digest","configuration_digest","policy_digest","capabilities","binding_digest")
        }
        "gkos-watcher-source-removal-adapter-challenge/1.0.0-draft.1" => {
            descriptor!(Some("challenge_digest"); "contract_version","vault_id","configuration_digest","policy_digest","nonce","required_capabilities","challenge_digest")
        }
        "gkos-watcher-source-removal-adapter-proof/1.0.0-draft.1" => {
            descriptor!(Some("proof_digest"); "contract_version","challenge_digest","binding_digest","adapter_kind","adapter_id","adapter_contract_version","authority_namespace","authorization_binding_digest","capabilities","proof_digest")
        }
        "gkos-watcher-source-removal-adapter-verification/1.0.0-draft.1" => {
            descriptor!(Some("verification_receipt_digest"); "contract_version","binding_digest","challenge_digest","proof_digest","process_instance_id","verified_at","capability_nonce_digest","verification_receipt_digest")
        }
        "gkos-watcher-source-removal-occurrence/1.0.0-draft.1" => {
            descriptor!(Some("occurrence_digest"); "contract_version","vault_id","prior_coherent_manifest_digest","prior_topology_snapshot_digest","source_id","source_path","source_digest","cause","occurrence_digest")
        }
        "gkos-watcher-source-removal-event/1.0.0-draft.1" => {
            descriptor!(Some("event_digest"); "contract_version","occurrence_digest","adapter_binding_digest","delivery_mode","event_digest")
        }
        "gkos-watcher-source-removal-event-membership/1.0.0-draft.1" => {
            descriptor!(Some("membership_digest"); "contract_version","event_ordinal","event_digest","causal_batch_id","target_topology_snapshot_digest","prepared_at","original_membership_digest","membership_digest")
        }
        "gkos-watcher-source-removal-event-set/1.0.0-draft.1" => {
            descriptor!(Some("event_set_digest"); "contract_version","set_kind","origin_id","target_topology_snapshot_digest","event_count","membership_digest_sequence_digest","prepared_at","event_set_digest")
        }
        "gkos-watcher-source-removal-membership-sequence/1.0.0-draft.1" => {
            descriptor!(None; "contract_version","membership_digests")
        }
        "gkos-watcher-source-removal-event-set-activation/1.0.0-draft.1" => {
            descriptor!(Some("activation_digest"); "contract_version","event_set_digest","coherent_manifest_digest","activated_at","activation_digest")
        }
        "gkos-watcher-source-removal-adapter-request/1.0.0-draft.1" => {
            descriptor!(Some("request_digest"); "contract_version","binding_digest","occurrence_digest","idempotency_key","source_id","source_path","source_digest","prior_coherent_manifest_digest","target_topology_snapshot_digest","observed_at","request_digest")
        }
        "gkos-watcher-source-removal-adapter-response/1.0.0-draft.1" => {
            descriptor!(Some("response_digest"); "contract_version","binding_digest","occurrence_digest","status","adapter_event_id","adapter_result_digest","response_digest")
        }
        "gkos-watcher-source-removal-receipt/1.0.0-draft.1" => {
            descriptor!(Some("receipt_digest"); "contract_version","event_digest","occurrence_digest","adapter_binding_digest","adapter_response_digest","adapter_result_digest","adapter_event_id","status","recorded_at","receipt_digest")
        }
        "gkos-watcher-service-locator/1.0.0-draft.1" => {
            descriptor!(Some("locator_digest"); "contract_version","service_instance_id","pid","loopback_host","port","status_route","control_route","started_at","locator_digest")
        }
        "gkos-watcher-status/1.0.0-draft.1" => {
            descriptor!(Some("status_digest"); "contract_version","service_instance_id","watcher_state","freshness","reason_codes","document_count","chunk_count","embedding_model","last_sync","uptime_ms","pid","source_snapshot_digest","coherent_manifest_digest","configuration_digest","policy_digest","status_digest")
        }
        "gkos-watcher-journal-reset-result/1.0.0-draft.1" => {
            descriptor!(Some("result_digest"); "contract_version","status","prior_journal_generation_digest","archive_manifest_digest","new_journal_generation_digest","outer_coherent_manifest_digest","reset_digest","requires_reconciliation","result_digest")
        }
        "gkos-watcher-fts-qualification-outcome/1.0.0-draft.1" => {
            descriptor!(Some("outcome_digest"); "contract_version","lane_kind","runtime_version","os","arch","physical_fts5_available","status","index_generation_count","query_count","provider_call_count","outcome_digest")
        }
        "gkos-watcher-observation-environment/1.0.0-draft.1" => {
            descriptor!(Some("environment_digest"); "contract_version","runtime","runtime_version","os","arch","sqlite_version","physical_fts5_available","runner_class","environment_digest")
        }
        "gkos-watcher-observation-convergence/1.0.0-draft.1" => {
            descriptor!(Some("convergence_digest"); "contract_version","incremental_canonical_gkx_digest","clean_canonical_gkx_digest","incremental_retrieval_manifest_digest","clean_retrieval_manifest_digest","incremental_canonical_graph_digest","clean_canonical_graph_digest","incremental_graphiti_digest","clean_graphiti_digest","all_equal","convergence_digest")
        }
        "gkos-watcher-observation-measurement/1.0.0-draft.1" => {
            descriptor!(Some("measurement_digest"); "contract_version","status","failure_codes","sample_plan_digest","environment","fts_qualification","edit_latency_micros","percentiles_micros","source_work","embedding_work","convergence","measurement_digest")
        }
        "gkos-watcher-recovery-pack-manifest/1.0.0-draft.1" => {
            descriptor!(Some("pack_digest"); "contract_version","pack_contract_version","files","file_count","total_bytes","pack_digest")
        }
        _ => return None,
    })
}

fn seal_record(value: &Value) -> WatcherResult<Value> {
    let record = object(value)?;
    let version = text(record, "contract_version")
        .ok_or(WatcherError("GKX_WATCHER_CONTRACT_VERSION_INVALID"))?;
    let descriptor =
        descriptor_for(version).ok_or(WatcherError("GKX_WATCHER_CONTRACT_VERSION_INVALID"))?;
    exact_keys(record, descriptor.keys)?;
    if let Some(digest_field) = descriptor.digest {
        if !record.get(digest_field).is_some_and(is_digest) {
            return fail("GKX_WATCHER_CONTRACT_DIGEST_INVALID");
        }
        if record[digest_field].as_str() != Some(digest_without(value, digest_field)?.as_str()) {
            return fail("GKX_WATCHER_CONTRACT_DIGEST_INVALID");
        }
    }
    seal_common_relations(value)?;
    Ok(value.clone())
}

const ADAPTER_CAPABILITIES: [&str; 2] = [
    "durable_idempotent_source_removal_projection",
    "lookup_by_occurrence_digest",
];

fn seal_common_relations(value: &Value) -> WatcherResult<()> {
    let item = object(value)?;
    match text(item, "contract_version").unwrap_or_default() {
        "gkos-watcher-observation/1.0.0-draft.1" => {
            let paths = &item["observed_paths"];
            if !is_uuid7(&item["batch_id"])
                || !matches!(
                    text(item, "batch_kind"),
                    Some(
                        "event"
                            | "startup_reconciliation"
                            | "shutdown_flush"
                            | "failure_reconciliation"
                    )
                )
                || !is_sorted_unique_strings(paths, 2_000)
                || !item["unscoped"].is_boolean()
                || !item["overflow"].is_boolean()
                || item["overflow"] == Value::Bool(true) && item["unscoped"] != Value::Bool(true)
                || !is_iso(&item["started_at"])
                || paths
                    .as_array()
                    .is_none_or(|rows| rows.iter().any(|path| !valid_source_path(path)))
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        "gkos-watcher-observation-authority/1.0.0-draft.1" => {
            let digest = text(item, "observation_digest").unwrap_or_default();
            if !is_uuid7(&item["batch_id"])
                || !is_digest(&item["observation_digest"])
                || text(item, "observation_artifact_file")
                    != Some(format!("watcher-observation-{}.json", &digest[7..]).as_str())
                || !is_digest(&item["observation_raw_sha256"])
                || !is_integer(&item["observation_byte_size"], 1, 4 * 1024 * 1024)
                || !is_digest(&item["pre_scan_state_digest"])
                || !is_iso(&item["started_at"])
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        "gkos-watcher-batch-record/1.0.0-draft.1" => seal_batch_record(item)?,
        "gkos-watcher-pre-scan-state/1.0.0-draft.1" => seal_pre_scan(item)?,
        "gkos-watcher-batch-plan/1.0.0-draft.1" => seal_plan(item)?,
        "gkos-watcher-plan-authority/1.0.0-draft.1" => {
            let digest = text(item, "plan_digest").unwrap_or_default();
            let count = unsigned(item, "source_removal_event_count");
            if !is_uuid7(&item["batch_id"])
                || !is_digest(&item["observation_digest"])
                || !is_digest(&item["plan_digest"])
                || text(item, "plan_artifact_file")
                    != Some(format!("watcher-plan-{}.json", &digest[7..]).as_str())
                || !is_digest(&item["plan_raw_sha256"])
                || !is_integer(&item["plan_byte_size"], 1, 512 * 1024 * 1024)
                || !is_digest(&item["target_topology_snapshot_digest"])
                || !matches!(count, Some(0..=1_000_000))
                || count == Some(0) && !item["source_removal_event_set_digest"].is_null()
                || count.is_some_and(|v| v > 0)
                    && !is_digest(&item["source_removal_event_set_digest"])
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        "gkos-watcher-topology-snapshot/1.0.0-draft.1" => seal_topology(item)?,
        "gkos-watcher-transition/1.0.0-draft.1" => seal_transition(item)?,
        "gkos-watcher-normalized-graph-delta/1.0.0-draft.1" => seal_normalized_graph_delta(item)?,
        "gkos-watcher-canonical-gkx-graph/1.0.0-draft.1" => {
            let normalized = normalize_already_canonical_graph(&item["normalized_graph"])?;
            if &normalized != value {
                return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
            }
        }
        "gkos-watcher-graphiti-projection/1.0.0-draft.1" => seal_graphiti_projection(item)?,
        "gkos-watcher-raw-graph-artifact/1.0.0-draft.1" => {
            if !is_watcher_generation(&item["service_generation_id"])
                || !is_digest(&item["topology_snapshot_digest"])
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
            assert_raw_graph_shape(&item["graph"], false)?;
        }
        "gkos-watcher-coherent-manifest/1.0.0-draft.1" => seal_manifest(item)?,
        "gkos-watcher-active-pointer/1.0.0-draft.1" => {
            let digest = text(item, "coherent_manifest_digest").unwrap_or_default();
            if text(item, "kind") != Some("watcher_coherent")
                || !is_watcher_generation(&item["service_generation_id"])
                || !is_digest(&item["coherent_manifest_digest"])
                || text(item, "coherent_manifest_file")
                    != Some(format!("watcher-coherent-{}.json", &digest[7..]).as_str())
                || !(item["prior_pointer_digest"].is_null()
                    || is_digest(&item["prior_pointer_digest"]))
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        "gkos-watcher-journal-meta/1.0.0-draft.1" => {
            if !is_uuid7(&item["journal_instance_id"])
                || !valid_label(&item["vault_id"])
                || !is_digest(&item["configuration_digest"])
                || !is_digest(&item["policy_digest"])
                || !is_digest(&item["effective_profile_digest"])
                || !(item["anchor_coherent_manifest_digest"].is_null()
                    || is_digest(&item["anchor_coherent_manifest_digest"]))
                || !is_iso(&item["created_at"])
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        "gkos-watcher-activation-intent/1.0.0-draft.1" => seal_intent(item)?,
        "gkos-watcher-activation-outcome/1.0.0-draft.1" => seal_outcome(item)?,
        "gkos-watcher-active-coherent/1.0.0-draft.1" => {
            if !is_watcher_generation(&item["service_generation_id"])
                || !is_digest(&item["coherent_manifest_digest"])
                || !is_digest(&item["pointer_digest"])
                || !is_digest(&item["intent_digest"])
                || !is_iso(&item["activated_at"])
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        "gkos-watcher-authority/1.0.0-draft.1" => {
            if text(item, "kind") != Some("watcher_coherent_authority")
                || !valid_label(&item["vault_id"])
                || !is_digest(&item["configuration_digest"])
                || !is_digest(&item["policy_digest"])
                || !is_digest(&item["effective_profile_digest"])
                || !is_watcher_generation(&item["first_service_generation_id"])
                || !is_digest(&item["first_coherent_manifest_digest"])
                || !is_digest(&item["first_pointer_digest"])
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        "gkos-watcher-journal-generation/1.0.0-draft.1" => {
            let uuid = text(item, "journal_instance_id").unwrap_or_default();
            if !is_uuid7(&item["journal_instance_id"])
                || text(item, "directory_leaf") != Some(format!("journal-{uuid}").as_str())
                || text(item, "database_file") != Some("watcher-journal.sqlite")
                || !is_digest(&item["meta_digest"])
                || !(item["anchor_coherent_manifest_digest"].is_null()
                    || is_digest(&item["anchor_coherent_manifest_digest"]))
                || !is_iso(&item["created_at"])
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        "gkos-watcher-journal-active-pointer/1.0.0-draft.1" => {
            let digest = text(item, "journal_generation_digest").unwrap_or_default();
            if text(item, "kind") != Some("watcher_journal")
                || !is_digest(&item["journal_generation_digest"])
                || text(item, "journal_generation_file")
                    != Some(format!("watcher-journal-generation-{}.json", &digest[7..]).as_str())
                || !(item["prior_pointer_digest"].is_null()
                    || is_digest(&item["prior_pointer_digest"]))
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        "gkos-watcher-journal-file-identity/1.0.0-draft.1" => seal_journal_file(item)?,
        "gkos-watcher-journal-archive/1.0.0-draft.1" => seal_archive(item)?,
        "gkos-watcher-journal-reset/1.0.0-draft.1" => seal_reset(item)?,
        "gkos-watcher-journal-reset-guard/1.0.0-draft.1" => seal_reset_guard(item)?,
        "gkos-watcher-pointer-replace-guard/1.0.0-draft.1" => seal_pointer_guard(item)?,
        "gkos-watcher-pointer-recovery-decision/1.0.0-draft.1" => seal_pointer_decision(item)?,
        "gkos-watcher-source-removal-authorization-scope/1.0.0-draft.1" => seal_scope(item)?,
        "gkos-watcher-source-removal-adapter-binding/1.0.0-draft.1" => seal_binding(item)?,
        "gkos-watcher-source-removal-adapter-challenge/1.0.0-draft.1" => {
            if !valid_label(&item["vault_id"])
                || !is_digest(&item["configuration_digest"])
                || !is_digest(&item["policy_digest"])
                || !text(item, "nonce").is_some_and(|nonce| {
                    nonce.len() == 32
                        && nonce
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
                })
                || item["required_capabilities"] != json!(ADAPTER_CAPABILITIES)
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-source-removal-adapter-proof/1.0.0-draft.1" => {
            if !is_digest(&item["challenge_digest"])
                || !is_digest(&item["binding_digest"])
                || !matches!(
                    text(item, "adapter_kind"),
                    Some("governance_store" | "durable_ledger")
                )
                || !valid_label(&item["adapter_id"])
                || !valid_label(&item["adapter_contract_version"])
                || !valid_label(&item["authority_namespace"])
                || !is_digest(&item["authorization_binding_digest"])
                || item["capabilities"] != json!(ADAPTER_CAPABILITIES)
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-source-removal-adapter-verification/1.0.0-draft.1" => {
            if !is_digest(&item["binding_digest"])
                || !is_digest(&item["challenge_digest"])
                || !is_digest(&item["proof_digest"])
                || !is_uuid7(&item["process_instance_id"])
                || !is_iso(&item["verified_at"])
                || !is_digest(&item["capability_nonce_digest"])
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-source-removal-occurrence/1.0.0-draft.1" => {
            if !valid_label(&item["vault_id"])
                || !is_digest(&item["prior_coherent_manifest_digest"])
                || !is_digest(&item["prior_topology_snapshot_digest"])
                || !text(item, "source_id").is_some_and(is_valid_authored_uid)
                || !valid_source_path(&item["source_path"])
                || !is_digest(&item["source_digest"])
                || text(item, "cause") != Some("physical_disappearance")
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-source-removal-event/1.0.0-draft.1" => {
            let valid = text(item, "delivery_mode").is_some_and(|mode| match mode {
                "local_only" => item["adapter_binding_digest"].is_null(),
                "adapter" => is_digest(&item["adapter_binding_digest"]),
                _ => false,
            });
            if !is_digest(&item["occurrence_digest"]) || !valid {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-source-removal-event-membership/1.0.0-draft.1" => {
            if !is_integer(&item["event_ordinal"], 1, u64::MAX)
                || !is_digest(&item["event_digest"])
                || !is_uuid7(&item["causal_batch_id"])
                || !is_digest(&item["target_topology_snapshot_digest"])
                || !(item["original_membership_digest"].is_null()
                    || is_digest(&item["original_membership_digest"]))
                || !is_iso(&item["prepared_at"])
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-source-removal-event-set/1.0.0-draft.1" => {
            let kind = text(item, "set_kind");
            if !is_integer(&item["event_count"], 1, 1_000_000)
                || !matches!(kind, Some("batch" | "reset_carry"))
                || !is_uuid7(&item["origin_id"])
                || !is_digest(&item["membership_digest_sequence_digest"])
                || kind == Some("batch") && !is_digest(&item["target_topology_snapshot_digest"])
                || kind == Some("reset_carry") && !item["target_topology_snapshot_digest"].is_null()
                || !is_iso(&item["prepared_at"])
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-source-removal-membership-sequence/1.0.0-draft.1" => {
            if item["membership_digests"]
                .as_array()
                .is_none_or(|items| items.iter().any(|item| !is_digest(item)))
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-source-removal-event-set-activation/1.0.0-draft.1" => {
            if !is_digest(&item["event_set_digest"])
                || !is_digest(&item["coherent_manifest_digest"])
                || !is_iso(&item["activated_at"])
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-source-removal-adapter-request/1.0.0-draft.1" => {
            if !is_digest(&item["binding_digest"])
                || !is_digest(&item["occurrence_digest"])
                || item["idempotency_key"] != item["occurrence_digest"]
                || !text(item, "source_id").is_some_and(is_valid_authored_uid)
                || !valid_source_path(&item["source_path"])
                || !is_digest(&item["source_digest"])
                || !is_digest(&item["prior_coherent_manifest_digest"])
                || !is_digest(&item["target_topology_snapshot_digest"])
                || !is_iso(&item["observed_at"])
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-source-removal-adapter-response/1.0.0-draft.1" => {
            let expected_result = canonical_digest(&json!({
                "contract_version":"gkos-watcher-source-removal-adapter-result/1.0.0-draft.1",
                "binding_digest":item["binding_digest"],
                "occurrence_digest":item["occurrence_digest"],
                "adapter_event_id":item["adapter_event_id"],
            }))
            .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID"))?;
            if !is_digest(&item["binding_digest"])
                || !is_digest(&item["occurrence_digest"])
                || !matches!(text(item, "status"), Some("accepted" | "already_applied"))
                || !valid_label(&item["adapter_event_id"])
                || text(item, "adapter_result_digest") != Some(expected_result.as_str())
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-source-removal-receipt/1.0.0-draft.1" => {
            if !is_digest(&item["event_digest"])
                || !is_digest(&item["occurrence_digest"])
                || !is_digest(&item["adapter_binding_digest"])
                || !is_digest(&item["adapter_response_digest"])
                || !is_digest(&item["adapter_result_digest"])
                || !valid_label(&item["adapter_event_id"])
                || !matches!(text(item, "status"), Some("accepted" | "already_applied"))
                || !is_iso(&item["recorded_at"])
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        }
        "gkos-watcher-service-locator/1.0.0-draft.1" => {
            if !is_uuid7(&item["service_instance_id"])
                || !is_integer(&item["pid"], 1, 9_007_199_254_740_991)
                || text(item, "loopback_host") != Some("127.0.0.1")
                || !is_integer(&item["port"], 1, 65_535)
                || text(item, "status_route") != Some("/status")
                || text(item, "control_route") != Some("/control/shutdown")
                || !is_iso(&item["started_at"])
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        "gkos-watcher-status/1.0.0-draft.1" => seal_status(item)?,
        "gkos-watcher-journal-reset-result/1.0.0-draft.1" => {
            if text(item, "status") != Some("reset")
                || !is_digest(&item["prior_journal_generation_digest"])
                || !is_digest(&item["archive_manifest_digest"])
                || !is_digest(&item["new_journal_generation_digest"])
                || !is_digest(&item["outer_coherent_manifest_digest"])
                || !is_digest(&item["reset_digest"])
                || item["requires_reconciliation"] != Value::Bool(true)
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        "gkos-watcher-fts-qualification-outcome/1.0.0-draft.1" => seal_fts(item)?,
        "gkos-watcher-observation-environment/1.0.0-draft.1" => seal_environment(item)?,
        "gkos-watcher-observation-convergence/1.0.0-draft.1" => seal_convergence(item)?,
        "gkos-watcher-observation-measurement/1.0.0-draft.1" => seal_measurement(item)?,
        "gkos-watcher-recovery-pack-manifest/1.0.0-draft.1" => seal_pack_manifest(item)?,
        _ => {}
    }
    Ok(())
}

fn seal_batch_record(item: &Map<String, Value>) -> WatcherResult<()> {
    let kind = text(item, "batch_kind");
    let execution = text(item, "execution_kind");
    if !is_uuid7(&item["batch_id"])
        || !matches!(
            kind,
            Some("event" | "startup_reconciliation" | "shutdown_flush" | "failure_reconciliation")
        )
        || !matches!(execution, Some("apply_changes" | "set_files"))
        || !is_iso(&item["started_at"])
        || (kind == Some("failure_reconciliation")) != is_uuid7(&item["retry_of_batch_id"])
        || matches!(
            kind,
            Some("startup_reconciliation" | "failure_reconciliation")
        ) && execution != Some("set_files")
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(())
}

fn seal_pre_scan(item: &Map<String, Value>) -> WatcherResult<()> {
    let active = [
        &item["active_pointer_digest"],
        &item["active_coherent_manifest_digest"],
        &item["topology_snapshot_digest"],
    ];
    if !valid_label(&item["vault_id"])
        || !is_digest(&item["configuration_digest"])
        || !is_digest(&item["policy_digest"])
        || !is_digest(&item["effective_profile_digest"])
        || !(active.iter().all(|value| value.is_null())
            || active.iter().all(|value| is_digest(value)))
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(())
}

fn seal_plan(item: &Map<String, Value>) -> WatcherResult<()> {
    let Some(mutations) = item["intended_source_mutations"].as_array() else {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    };
    if !is_uuid7(&item["batch_id"])
        || !is_digest(&item["observation_digest"])
        || !is_digest(&item["topology_snapshot_digest"])
        || !is_digest(&item["effective_profile_digest"])
        || !is_digest(&item["validation_result_digest"])
        || !is_digest(&item["rejection_journal_digest"])
        || mutations.len() > 1_000_000
        || !item["folder_set_changed"].is_boolean()
        || !item["attachment_set_changed"].is_boolean()
        || !is_digest(&item["mutation_set_digest"])
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    let mut prior: Option<String> = None;
    for raw in mutations {
        let mutation = object(raw)?;
        exact_keys(
            mutation,
            &[
                "kind",
                "cause",
                "from_path",
                "to_path",
                "source_id_before",
                "source_id_after",
                "source_digest_before",
                "source_digest_after",
                "parser_descriptor_digest_before",
                "parser_descriptor_digest_after",
            ],
        )?;
        let kind = text(mutation, "kind").unwrap_or_default();
        let cause = text(mutation, "cause").unwrap_or_default();
        if !matches!(kind, "add" | "change" | "delete" | "rename")
            || !matches!(
                cause,
                "physical_appearance"
                    | "physical_disappearance"
                    | "content_change"
                    | "metadata_change"
                    | "verified_rename"
                    | "validation_rejection"
                    | "validation_reacceptance"
            )
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
        for field in ["from_path", "to_path"] {
            if !mutation[field].is_null() && !valid_source_path(&mutation[field]) {
                return fail("GKX_WATCHER_CONTRACT_PATH_INVALID");
            }
        }
        for field in ["source_id_before", "source_id_after"] {
            if !mutation[field].is_null()
                && !text(mutation, field).is_some_and(is_valid_authored_uid)
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        for field in [
            "source_digest_before",
            "source_digest_after",
            "parser_descriptor_digest_before",
            "parser_descriptor_digest_after",
        ] {
            if !mutation[field].is_null() && !is_digest(&mutation[field]) {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        let before = !mutation["from_path"].is_null()
            && !mutation["source_id_before"].is_null()
            && !mutation["source_digest_before"].is_null()
            && !mutation["parser_descriptor_digest_before"].is_null();
        let after = !mutation["to_path"].is_null()
            && !mutation["source_id_after"].is_null()
            && !mutation["source_digest_after"].is_null()
            && !mutation["parser_descriptor_digest_after"].is_null();
        let valid = match kind {
            "add" => {
                after
                    && mutation["from_path"].is_null()
                    && mutation["source_id_before"].is_null()
                    && mutation["source_digest_before"].is_null()
                    && mutation["parser_descriptor_digest_before"].is_null()
                    && matches!(cause, "physical_appearance" | "validation_reacceptance")
            }
            "delete" => {
                before
                    && mutation["to_path"].is_null()
                    && mutation["source_id_after"].is_null()
                    && mutation["source_digest_after"].is_null()
                    && mutation["parser_descriptor_digest_after"].is_null()
                    && matches!(cause, "physical_disappearance" | "validation_rejection")
            }
            "change" => {
                before
                    && after
                    && mutation["from_path"] == mutation["to_path"]
                    && mutation["source_id_before"] == mutation["source_id_after"]
                    && matches!(cause, "content_change" | "metadata_change")
            }
            "rename" => {
                before
                    && after
                    && mutation["from_path"] != mutation["to_path"]
                    && mutation["source_id_before"] == mutation["source_id_after"]
                    && mutation["source_digest_before"] == mutation["source_digest_after"]
                    && mutation["parser_descriptor_digest_before"]
                        == mutation["parser_descriptor_digest_after"]
                    && cause == "verified_rename"
            }
            _ => false,
        };
        if !valid {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
        let first = mutation["from_path"]
            .as_str()
            .or_else(|| mutation["to_path"].as_str())
            .unwrap_or_default();
        let key = format!(
            "{}\0{}\0{}\0{}",
            first,
            mutation["to_path"].as_str().unwrap_or_default(),
            kind,
            cause
        );
        if prior
            .as_deref()
            .is_some_and(|value| utf16_cmp(value, &key) != Ordering::Less)
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
        prior = Some(key);
    }
    Ok(())
}

fn seal_topology(item: &Map<String, Value>) -> WatcherResult<()> {
    let Some(accepted) = item["accepted_sources"].as_array() else {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    };
    let Some(rejected) = item["rejected_sources"].as_array() else {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    };
    if !valid_label(&item["vault_id"])
        || !is_digest(&item["source_observation_snapshot_digest"])
        || !is_digest(&item["validation_result_digest"])
        || !is_digest(&item["rejection_journal_digest"])
        || !is_sorted_unique_strings(&item["folder_paths"], 1_000_000)
        || !is_sorted_unique_strings(&item["attachment_paths"], 1_000_000)
        || accepted.len() + rejected.len() > 1_000_000
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    let mut ordinals = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut prior: Option<String> = None;
    for raw in accepted {
        let source = object(raw)?;
        exact_keys(
            source,
            &[
                "source_path",
                "source_id",
                "source_observation_ordinal",
                "source_digest",
                "source_size_bytes",
                "parser_descriptor_digest",
            ],
        )?;
        if !valid_source_path(&source["source_path"])
            || !text(source, "source_id").is_some_and(is_valid_authored_uid)
            || !is_integer(&source["source_observation_ordinal"], 0, 999_999)
            || !is_digest(&source["source_digest"])
            || !is_integer(&source["source_size_bytes"], 0, 64 * 1024 * 1024)
            || !is_digest(&source["parser_descriptor_digest"])
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
        let key = format!(
            "{}\0{:07}\0{}",
            text(source, "source_path").unwrap(),
            unsigned(source, "source_observation_ordinal").unwrap(),
            text(source, "source_digest").unwrap()
        );
        if prior
            .as_deref()
            .is_some_and(|before| utf16_cmp(before, &key) != Ordering::Less)
            || !ordinals.insert(unsigned(source, "source_observation_ordinal").unwrap())
            || !paths.insert(text(source, "source_path").unwrap().to_owned())
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
        prior = Some(key);
    }
    prior = None;
    for raw in rejected {
        let source = object(raw)?;
        exact_keys(
            source,
            &[
                "source_path",
                "source_id",
                "source_observation_ordinal",
                "source_digest",
                "source_size_bytes",
                "parser_descriptor_digest",
                "rejection_digest",
                "rejection_class",
            ],
        )?;
        if !valid_source_path(&source["source_path"])
            || !(source["source_id"].is_null()
                || text(source, "source_id").is_some_and(is_valid_authored_uid))
            || !is_integer(&source["source_observation_ordinal"], 0, 999_999)
            || !(source["source_digest"].is_null() || is_digest(&source["source_digest"]))
            || !(source["source_size_bytes"].is_null()
                || is_integer(&source["source_size_bytes"], 0, 64 * 1024 * 1024))
            || !(source["parser_descriptor_digest"].is_null()
                || is_digest(&source["parser_descriptor_digest"]))
            || !is_digest(&source["rejection_digest"])
            || !matches!(
                text(source, "rejection_class"),
                Some("validation" | "scan_rejection")
            )
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
        let key = format!(
            "{}\0{:07}\0{}",
            text(source, "source_path").unwrap(),
            unsigned(source, "source_observation_ordinal").unwrap(),
            text(source, "source_digest").unwrap_or_default()
        );
        if prior
            .as_deref()
            .is_some_and(|before| utf16_cmp(before, &key) != Ordering::Less)
            || !ordinals.insert(unsigned(source, "source_observation_ordinal").unwrap())
            || !paths.insert(text(source, "source_path").unwrap().to_owned())
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
        prior = Some(key);
    }
    for path in item["folder_paths"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(item["attachment_paths"].as_array().into_iter().flatten())
    {
        if !valid_source_path(path) {
            return fail("GKX_WATCHER_CONTRACT_PATH_INVALID");
        }
    }
    let child_domains = [
        (
            "accepted_source_set_digest",
            json!({
                "contract_version":"gkos-watcher-accepted-source-set/1.0.0-draft.1",
                "sources":accepted,
            }),
        ),
        (
            "rejected_source_set_digest",
            json!({
                "contract_version":"gkos-watcher-rejected-source-set/1.0.0-draft.1",
                "sources":rejected,
            }),
        ),
        (
            "folder_set_digest",
            json!({
                "contract_version":"gkos-watcher-folder-set/1.0.0-draft.1",
                "folder_paths":item["folder_paths"],
            }),
        ),
        (
            "attachment_set_digest",
            json!({
                "contract_version":"gkos-watcher-attachment-set/1.0.0-draft.1",
                "attachment_paths":item["attachment_paths"],
            }),
        ),
    ];
    for (field, material) in child_domains {
        let expected = canonical_digest(&material)
            .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_DIGEST_INVALID"))?;
        if text(item, field) != Some(expected.as_str()) {
            return fail("GKX_WATCHER_CONTRACT_DIGEST_INVALID");
        }
    }
    Ok(())
}

fn seal_retrieval_state(value: &Value) -> WatcherResult<Value> {
    let item = object(value)?;
    exact_keys(
        item,
        &[
            "state",
            "owner_generation_id",
            "owner_manifest_digest",
            "database_file",
            "manifest_digest",
            "projection_id",
            "projection_digest",
            "lexical_backend",
            "vector_stage_state",
            "provider_kind",
            "provider_id",
            "model_id",
            "dimensions",
            "reason_codes",
        ],
    )?;
    if !is_sorted_unique_strings(&item["reason_codes"], 64) {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    match text(item, "state") {
        Some("not_started") => {
            if [
                "owner_generation_id",
                "owner_manifest_digest",
                "database_file",
                "manifest_digest",
                "projection_id",
                "projection_digest",
                "lexical_backend",
                "vector_stage_state",
                "provider_kind",
                "provider_id",
                "model_id",
                "dimensions",
            ]
            .iter()
            .any(|field| !item[*field].is_null())
                || !item["reason_codes"].as_array().is_some_and(Vec::is_empty)
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        Some("ready") => {
            let owner_manifest = text(item, "owner_manifest_digest").unwrap_or_default();
            let projection = text(item, "projection_digest").unwrap_or_default();
            if !is_digest(&item["owner_manifest_digest"])
                || !is_digest(&item["manifest_digest"])
                || !is_digest(&item["projection_digest"])
                || text(item, "owner_generation_id")
                    != owner_manifest
                        .get(7..31)
                        .map(|suffix| format!("ingest:{suffix}"))
                        .as_deref()
                || text(item, "projection_id")
                    != projection
                        .get(7..31)
                        .map(|suffix| format!("retrieval:{suffix}"))
                        .as_deref()
                || text(item, "database_file")
                    != projection
                        .get(7..)
                        .map(|suffix| format!("retrieval-{suffix}.sqlite"))
                        .as_deref()
                || !matches!(
                    text(item, "lexical_backend"),
                    Some("sqlite_fts5" | "sqlite_lexical_scan")
                )
                || !matches!(
                    text(item, "vector_stage_state"),
                    Some("disabled" | "complete" | "degraded")
                )
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
            let disabled = text(item, "vector_stage_state") == Some("disabled");
            let provider_null = ["provider_kind", "provider_id", "model_id", "dimensions"]
                .iter()
                .all(|field| item[*field].is_null());
            if disabled != provider_null
                || !disabled
                    && (!matches!(
                        text(item, "provider_kind"),
                        Some("openai_compatible" | "local_onnx" | "mcp")
                    ) || !valid_opaque(&item["provider_id"])
                        || !valid_opaque(&item["model_id"])
                        || !is_integer(&item["dimensions"], 1, u64::MAX))
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        _ => return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID"),
    }
    Ok(value.clone())
}

fn seal_graph_state(value: &Value) -> WatcherResult<Value> {
    let item = object(value)?;
    exact_keys(
        item,
        &[
            "state",
            "graph_contract_version",
            "graph_artifact_file",
            "graph_artifact_digest",
            "canonical_graph_digest",
            "gkx_delta_digest",
            "graphiti_projection_digest",
            "sink_state",
            "sink_receipts",
            "reason_codes",
        ],
    )?;
    if text(item, "sink_state") != Some("not_applicable")
        || !item["sink_receipts"].as_array().is_some_and(Vec::is_empty)
        || !is_sorted_unique_strings(&item["reason_codes"], 64)
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    match text(item, "state") {
        Some("not_started") => {
            if [
                "graph_contract_version",
                "graph_artifact_file",
                "graph_artifact_digest",
                "canonical_graph_digest",
                "gkx_delta_digest",
                "graphiti_projection_digest",
            ]
            .iter()
            .any(|field| !item[*field].is_null())
                || !item["reason_codes"].as_array().is_some_and(Vec::is_empty)
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        Some("ready") => {
            let digest = text(item, "graph_artifact_digest").unwrap_or_default();
            if text(item, "graph_contract_version")
                != Some("gkos-watcher-canonical-gkx-graph/1.0.0-draft.1")
                || !is_digest(&item["graph_artifact_digest"])
                || !is_digest(&item["canonical_graph_digest"])
                || !is_digest(&item["gkx_delta_digest"])
                || !is_digest(&item["graphiti_projection_digest"])
                || text(item, "graph_artifact_file")
                    != digest
                        .get(7..)
                        .map(|suffix| format!("watcher-graph-{suffix}.json"))
                        .as_deref()
                || !item["reason_codes"].as_array().is_some_and(Vec::is_empty)
            {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        _ => return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID"),
    }
    Ok(value.clone())
}

const NORMAL_STATES: [&str; 7] = [
    "observed",
    "normalized",
    "gkx_applied",
    "retrieval_applied",
    "graph_applied",
    "activation_prepared",
    "complete",
];

fn seal_transition(item: &Map<String, Value>) -> WatcherResult<()> {
    let ordinal = unsigned(item, "transition_ordinal");
    let state = text(item, "state").unwrap_or_default();
    let normal_index = NORMAL_STATES
        .iter()
        .position(|candidate| *candidate == state);
    let exceptional = matches!(state, "failed" | "superseded");
    if !is_uuid7(&item["batch_id"])
        || ordinal.is_none_or(|ordinal| ordinal > 6)
        || !exceptional && normal_index.map(|index| index as u64) != ordinal
        || ordinal == Some(0) && !item["prior_transition_digest"].is_null()
        || ordinal.is_some_and(|ordinal| ordinal > 0)
            && !is_digest(&item["prior_transition_digest"])
        || !is_digest(&item["observation_digest"])
        || !text(item, "last_reached_state").is_some_and(|state| NORMAL_STATES.contains(&state))
        || !matches!(
            text(item, "terminal_state"),
            Some("open" | "complete" | "failed" | "superseded")
        )
        || !is_sorted_unique_strings(&item["reason_codes"], 16)
        || !is_iso(&item["recorded_at"])
    {
        return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
    }
    if exceptional {
        let Some(ordinal) = ordinal.filter(|value| (1..=6).contains(value)) else {
            return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
        };
        if text(item, "last_reached_state") != Some(NORMAL_STATES[ordinal as usize - 1])
            || text(item, "terminal_state") != Some(state)
            || !is_iso(&item["completed_at"])
            || item["reason_codes"].as_array().is_none_or(Vec::is_empty)
        {
            return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
        }
    } else if state == "complete" {
        if text(item, "last_reached_state") != Some("complete")
            || text(item, "terminal_state") != Some("complete")
            || !is_iso(&item["completed_at"])
            || item["reason_codes"]
                .as_array()
                .is_none_or(|rows| !rows.is_empty())
        {
            return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
        }
    } else if text(item, "last_reached_state") != Some(state)
        || text(item, "terminal_state") != Some("open")
        || !item["completed_at"].is_null()
        || item["reason_codes"]
            .as_array()
            .is_none_or(|rows| !rows.is_empty())
    {
        return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
    }
    if item["reason_codes"].as_array().is_none_or(|reasons| {
        reasons.iter().any(|reason| {
            !reason
                .as_str()
                .is_some_and(|reason| TRANSITION_REASONS.contains(&reason))
        })
    }) {
        return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
    }
    for field in ["plan_digest", "gkx_delta_digest", "gkx_snapshot_digest"] {
        if !item[field].is_null() && !is_digest(&item[field]) {
            return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
        }
    }
    seal_retrieval_state(&item["retrieval_projection_state"])?;
    seal_graph_state(&item["graph_projection_state"])?;
    if let Some(index) = normal_index {
        let plan = index >= 1;
        let gkx = index >= 2;
        let retrieval = index >= 3;
        let graph = index >= 4;
        if is_digest(&item["plan_digest"]) != plan
            || is_digest(&item["gkx_delta_digest"]) != gkx
            || is_digest(&item["gkx_snapshot_digest"]) != gkx
            || text(object(&item["retrieval_projection_state"])?, "state")
                != Some(if retrieval { "ready" } else { "not_started" })
            || text(object(&item["graph_projection_state"])?, "state")
                != Some(if graph { "ready" } else { "not_started" })
        {
            return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
        }
    }
    Ok(())
}

const GRAPH_KEYS: [&str; 13] = [
    "nodes",
    "links",
    "stats",
    "areas",
    "tags",
    "statuses",
    "types",
    "diagnostics",
    "__timeSpan",
    "gkxProfile",
    "gkxUidIndex",
    "gkxAssessments",
    "gkxDiagnostics",
];
const GRAPH_STATS_KEYS: [&str; 8] = [
    "files",
    "folders",
    "unresolved",
    "links",
    "wikilinks",
    "markdownLinks",
    "propertyLinks",
    "orphans",
];
const GRAPH_DIAGNOSTIC_KEYS: [&str; 9] = [
    "notes",
    "folders",
    "attachments",
    "unresolvedLinks",
    "ambiguousLinks",
    "lineageEdges",
    "lineageCycles",
    "lineageWarnings",
    "residualCollisions",
];

fn graph_keys(
    value: &Value,
    required: &[&str],
    optional: &[&str],
) -> WatcherResult<Map<String, Value>> {
    let record = object(value)?;
    if required.iter().any(|key| !record.contains_key(*key))
        || record
            .keys()
            .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
    }
    Ok(record.clone())
}

fn assert_raw_graph_shape(
    value: &Value,
    allow_node_by_id: bool,
) -> WatcherResult<Map<String, Value>> {
    let optional = if allow_node_by_id {
        &["nodeById"][..]
    } else {
        &[][..]
    };
    let graph = graph_keys(value, &GRAPH_KEYS, optional)?;
    let mut raw_stats = vec!["indexedAt", "durationMs"];
    raw_stats.extend(GRAPH_STATS_KEYS);
    graph_keys(&graph["stats"], &raw_stats, &[])?;
    graph_keys(
        &graph["diagnostics"],
        &GRAPH_DIAGNOSTIC_KEYS,
        &["lastFullBuildMs", "lastIncrementalUpdateMs"],
    )?;
    for key in [
        "nodes",
        "links",
        "areas",
        "tags",
        "statuses",
        "types",
        "gkxAssessments",
        "gkxDiagnostics",
    ] {
        if !graph[key].is_array() {
            return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
        }
    }
    Ok(graph)
}

fn assert_canonical_graph_shape(value: &Value) -> WatcherResult<Map<String, Value>> {
    let graph = graph_keys(value, &GRAPH_KEYS, &[])?;
    graph_keys(&graph["stats"], &GRAPH_STATS_KEYS, &[])?;
    graph_keys(&graph["diagnostics"], &GRAPH_DIAGNOSTIC_KEYS, &[])?;
    for key in [
        "nodes",
        "links",
        "areas",
        "tags",
        "statuses",
        "types",
        "gkxAssessments",
        "gkxDiagnostics",
    ] {
        if !graph[key].is_array() {
            return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
        }
    }
    Ok(graph)
}

fn graph_set_key(key: &str) -> bool {
    matches!(
        key,
        "aliases"
            | "areas"
            | "diagnostic_codes"
            | "labels"
            | "lineageWarnings"
            | "statuses"
            | "supersededByIds"
            | "supersedes"
            | "supersedesIds"
            | "tags"
            | "types"
    )
}

fn canonicalize_graph_value(value: &Value, key: Option<&str>) -> WatcherResult<Value> {
    match value {
        Value::Array(values) => {
            let mut output = values
                .iter()
                .map(|item| canonicalize_graph_value(item, None))
                .collect::<WatcherResult<Vec<_>>>()?;
            if key.is_some_and(graph_set_key) && output.iter().all(Value::is_string) {
                output.sort_by(|left, right| {
                    utf16_cmp(left.as_str().unwrap(), right.as_str().unwrap())
                });
                output.dedup();
            }
            Ok(Value::Array(output))
        }
        Value::Object(values) => {
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_by(|left, right| utf16_cmp(left, right));
            let mut output = Map::new();
            for child_key in keys {
                output.insert(
                    child_key.clone(),
                    canonicalize_graph_value(&values[child_key], Some(child_key))?,
                );
            }
            Ok(Value::Object(output))
        }
        _ => Ok(value.clone()),
    }
}

fn canonical_json_cmp(left: &Value, right: &Value) -> Ordering {
    let left = canonical_json(left).unwrap_or_default();
    let right = canonical_json(right).unwrap_or_default();
    utf16_cmp(&left, &right)
}

fn sort_graph_rows(rows: &mut [Value], fields: &[&str]) {
    rows.sort_by(|left, right| {
        let key = |value: &Value| {
            let record = value.as_object();
            fields
                .iter()
                .map(|field| {
                    record
                        .and_then(|record| record.get(*field))
                        .map(|value| match value {
                            Value::String(value) => value.clone(),
                            Value::Null => "null".to_owned(),
                            _ => value.to_string(),
                        })
                        .unwrap_or_else(|| "undefined".to_owned())
                })
                .collect::<Vec<_>>()
                .join("\0")
        };
        utf16_cmp(&key(left), &key(right))
    });
}

fn normalize_raw_graph(graph: &Value) -> WatcherResult<Value> {
    let source = assert_raw_graph_shape(graph, true)?;
    let stats = object(&source["stats"])?;
    let diagnostics = object(&source["diagnostics"])?;
    let mut canonical_stats = Map::new();
    for key in GRAPH_STATS_KEYS {
        canonical_stats.insert(key.to_owned(), stats[key].clone());
    }
    let mut canonical_diagnostics = Map::new();
    for key in GRAPH_DIAGNOSTIC_KEYS {
        canonical_diagnostics.insert(key.to_owned(), diagnostics[key].clone());
    }
    let mut nodes = array(&source["nodes"])?
        .iter()
        .map(|row| canonicalize_graph_value(row, None))
        .collect::<WatcherResult<Vec<_>>>()?;
    sort_graph_rows(&mut nodes, &["id", "path"]);
    let mut links = array(&source["links"])?
        .iter()
        .map(|row| canonicalize_graph_value(row, None))
        .collect::<WatcherResult<Vec<_>>>()?;
    sort_graph_rows(&mut links, &["id", "source", "target", "kind"]);
    let mut assessments = array(&source["gkxAssessments"])?
        .iter()
        .map(|row| canonicalize_graph_value(row, None))
        .collect::<WatcherResult<Vec<_>>>()?;
    assessments.sort_by(canonical_json_cmp);
    let mut gkx_diagnostics = array(&source["gkxDiagnostics"])?
        .iter()
        .map(|row| canonicalize_graph_value(row, None))
        .collect::<WatcherResult<Vec<_>>>()?;
    gkx_diagnostics.sort_by(canonical_json_cmp);
    let normalized_graph = json!({
        "nodes": nodes,
        "links": links,
        "stats": canonicalize_graph_value(&Value::Object(canonical_stats), Some("stats"))?,
        "areas": canonicalize_graph_value(&source["areas"], Some("areas"))?,
        "tags": canonicalize_graph_value(&source["tags"], Some("tags"))?,
        "statuses": canonicalize_graph_value(&source["statuses"], Some("statuses"))?,
        "types": canonicalize_graph_value(&source["types"], Some("types"))?,
        "diagnostics": canonicalize_graph_value(&Value::Object(canonical_diagnostics), Some("diagnostics"))?,
        "__timeSpan": canonicalize_graph_value(&source["__timeSpan"], None)?,
        "gkxProfile": canonicalize_graph_value(&source["gkxProfile"], None)?,
        "gkxUidIndex": canonicalize_graph_value(&source["gkxUidIndex"], None)?,
        "gkxAssessments": assessments,
        "gkxDiagnostics": gkx_diagnostics,
    });
    Ok(json!({
        "contract_version":"gkos-watcher-canonical-gkx-graph/1.0.0-draft.1",
        "normalized_graph":normalized_graph,
    }))
}

fn normalize_already_canonical_graph(graph: &Value) -> WatcherResult<Value> {
    let source = assert_canonical_graph_shape(graph)?;
    let mut nodes = array(&source["nodes"])?
        .iter()
        .map(|row| canonicalize_graph_value(row, None))
        .collect::<WatcherResult<Vec<_>>>()?;
    sort_graph_rows(&mut nodes, &["id", "path"]);
    let mut links = array(&source["links"])?
        .iter()
        .map(|row| canonicalize_graph_value(row, None))
        .collect::<WatcherResult<Vec<_>>>()?;
    sort_graph_rows(&mut links, &["id", "source", "target", "kind"]);
    let mut assessments = array(&source["gkxAssessments"])?
        .iter()
        .map(|row| canonicalize_graph_value(row, None))
        .collect::<WatcherResult<Vec<_>>>()?;
    assessments.sort_by(canonical_json_cmp);
    let mut diagnostics = array(&source["gkxDiagnostics"])?
        .iter()
        .map(|row| canonicalize_graph_value(row, None))
        .collect::<WatcherResult<Vec<_>>>()?;
    diagnostics.sort_by(canonical_json_cmp);
    Ok(json!({
        "contract_version":"gkos-watcher-canonical-gkx-graph/1.0.0-draft.1",
        "normalized_graph":{
            "nodes":nodes,
            "links":links,
            "stats":canonicalize_graph_value(&source["stats"], Some("stats"))?,
            "areas":canonicalize_graph_value(&source["areas"], Some("areas"))?,
            "tags":canonicalize_graph_value(&source["tags"], Some("tags"))?,
            "statuses":canonicalize_graph_value(&source["statuses"], Some("statuses"))?,
            "types":canonicalize_graph_value(&source["types"], Some("types"))?,
            "diagnostics":canonicalize_graph_value(&source["diagnostics"], Some("diagnostics"))?,
            "__timeSpan":canonicalize_graph_value(&source["__timeSpan"], None)?,
            "gkxProfile":canonicalize_graph_value(&source["gkxProfile"], None)?,
            "gkxUidIndex":canonicalize_graph_value(&source["gkxUidIndex"], None)?,
            "gkxAssessments":assessments,
            "gkxDiagnostics":diagnostics,
        }
    }))
}

fn normalize_graph_delta(delta: &Value) -> WatcherResult<Value> {
    let delta = object(delta)?;
    exact_keys(
        delta,
        &[
            "addedNodes",
            "removedNodes",
            "changedNodes",
            "topologyChanged",
            "reparsed",
            "fullRebuild",
        ],
    )?;
    let sorted = |field: &str| -> WatcherResult<Value> {
        let mut rows = array(&delta[field])?.clone();
        if rows.iter().any(|row| !row.is_string()) {
            return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
        }
        rows.sort_by(|left, right| utf16_cmp(left.as_str().unwrap(), right.as_str().unwrap()));
        Ok(Value::Array(rows))
    };
    if !delta["topologyChanged"].is_boolean()
        || !is_integer(&delta["reparsed"], 0, 9_007_199_254_740_991)
        || !delta["fullRebuild"].is_boolean()
    {
        return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
    }
    Ok(json!({
        "contract_version":"gkos-watcher-normalized-graph-delta/1.0.0-draft.1",
        "delta":{
            "addedNodes":sorted("addedNodes")?,
            "removedNodes":sorted("removedNodes")?,
            "changedNodes":sorted("changedNodes")?,
            "topologyChanged":delta["topologyChanged"],
            "reparsed":delta["reparsed"],
            "fullRebuild":delta["fullRebuild"],
        }
    }))
}

fn seal_normalized_graph_delta(item: &Map<String, Value>) -> WatcherResult<()> {
    let normalized = normalize_graph_delta(&item["delta"])?;
    if normalized["delta"] != item["delta"] {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    for field in ["addedNodes", "removedNodes", "changedNodes"] {
        if !is_sorted_unique_strings(&object(&item["delta"])?[field], usize::MAX) {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
    }
    Ok(())
}

fn seal_graphiti_projection(item: &Map<String, Value>) -> WatcherResult<()> {
    let Some(episodes) = item["episodes"].as_array() else {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    };
    if text(item, "processing_time") != Some("1970-01-01T00:00:00.000Z")
        || episodes.len() > 1_000_000
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    for episode in episodes {
        let episode = object(episode)?;
        let metadata = episode
            .get("episode_metadata")
            .ok_or(WatcherError("GKX_WATCHER_CONTRACT_RELATION_INVALID"))
            .and_then(object)?;
        let Some(body_text) = text(episode, "episode_body") else {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        };
        let body: Value = serde_json::from_str(body_text)
            .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_RELATION_INVALID"))?;
        if text(metadata, "processing_time") != Some("1970-01-01T00:00:00.000Z")
            || text(object(&body)?, "processing_time") != Some("1970-01-01T00:00:00.000Z")
            || canonical_json(&body).ok().as_deref() != Some(body_text)
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
    }
    Ok(())
}

fn seal_manifest(item: &Map<String, Value>) -> WatcherResult<()> {
    let batch = text(item, "completed_batch_id").unwrap_or_default();
    let topology = text(item, "topology_snapshot_digest").unwrap_or_default();
    let count = unsigned(item, "source_removal_event_count");
    if !is_watcher_generation(&item["service_generation_id"])
        || !is_uuid7(&item["completed_batch_id"])
        || text(item, "service_generation_id") != Some(format!("watcher:{batch}").as_str())
        || !valid_label(&item["vault_id"])
        || !is_digest(&item["completed_transition_digest"])
        || !is_digest(&item["topology_snapshot_digest"])
        || text(item, "topology_artifact_file")
            != Some(format!("watcher-topology-{}.json", &topology[7..]).as_str())
        || !is_digest(&item["topology_artifact_raw_sha256"])
        || !is_digest(&item["source_observation_snapshot_digest"])
        || !is_digest(&item["effective_profile_digest"])
        || !is_digest(&item["validation_result_digest"])
        || !is_digest(&item["rejection_journal_digest"])
        || !is_digest(&item["configuration_digest"])
        || !is_digest(&item["policy_digest"])
        || !is_digest(&item["gkx_snapshot_digest"])
        || count.is_none_or(|count| count > 1_000_000)
        || count == Some(0) && !item["source_removal_event_set_digest"].is_null()
        || count.is_some_and(|count| count > 0)
            && !is_digest(&item["source_removal_event_set_digest"])
        || !is_iso(&item["created_at"])
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    seal_retrieval_state(&item["retrieval_projection_state"])?;
    seal_graph_state(&item["graph_projection_state"])?;
    Ok(())
}

fn seal_intent(item: &Map<String, Value>) -> WatcherResult<()> {
    let pointer = seal_record(&item["target_pointer"])?;
    let complete = seal_record(&item["target_complete_transition"])?;
    if complete["state"] != "complete"
        || complete["terminal_state"] != "complete"
        || complete["prior_transition_digest"] != item["prepared_transition_digest"]
        || pointer["coherent_manifest_digest"] != item["coherent_manifest_digest"]
        || pointer["prior_pointer_digest"] != item["prior_pointer_digest"]
        || !is_iso(&item["prepared_at"])
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(())
}

fn seal_outcome(item: &Map<String, Value>) -> WatcherResult<()> {
    let Some(reasons) = item["reason_codes"].as_array() else {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    };
    if !is_digest(&item["intent_digest"])
        || !is_digest(&item["coherent_manifest_digest"])
        || !matches!(text(item, "outcome"), Some("published" | "superseded"))
        || !(item["pointer_digest"].is_null() || is_digest(&item["pointer_digest"]))
        || !is_sorted_unique_strings(&item["reason_codes"], usize::MAX)
        || !is_iso(&item["recorded_at"])
        || text(item, "outcome") == Some("published")
            && (!is_digest(&item["pointer_digest"]) || !reasons.is_empty())
        || text(item, "outcome") == Some("superseded") && reasons.is_empty()
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(())
}

fn seal_journal_file(item: &Map<String, Value>) -> WatcherResult<()> {
    let role = text(item, "role").unwrap_or_default();
    let leaf = match role {
        "database" => "watcher-journal.sqlite",
        "wal" => "watcher-journal.sqlite-wal",
        "shm" => "watcher-journal.sqlite-shm",
        _ => return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID"),
    };
    let minimum = if role == "database" { 1 } else { 0 };
    let maximum = if role == "database" {
        2_048_000_000
    } else {
        9_007_199_254_740_991
    };
    if text(item, "leaf") != Some(leaf)
        || !decimal_identity(&item["device"])
        || !decimal_identity(&item["inode"])
        || unsigned(item, "mode") != Some(384)
        || !is_integer(&item["byte_size"], minimum, maximum)
        || !is_digest(&item["raw_sha256"])
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(())
}

fn seal_archive(item: &Map<String, Value>) -> WatcherResult<()> {
    let database = seal_record(&item["database_identity"])?;
    let wal = if item["wal_identity"].is_null() {
        None
    } else {
        Some(seal_record(&item["wal_identity"])?)
    };
    let shm = if item["shm_identity"].is_null() {
        None
    } else {
        Some(seal_record(&item["shm_identity"])?)
    };
    let instance = text(item, "journal_instance_id").unwrap_or_default();
    if !is_uuid7(&item["journal_instance_id"])
        || text(item, "directory_leaf") != Some(format!("journal-{instance}").as_str())
        || !decimal_identity(&item["directory_device"])
        || !decimal_identity(&item["directory_inode"])
        || unsigned(item, "directory_mode") != Some(448)
        || database["role"] != "database"
        || wal.as_ref().is_some_and(|wal| wal["role"] != "wal")
        || shm.as_ref().is_some_and(|shm| shm["role"] != "shm")
        || !is_digest(&item["outer_coherent_manifest_digest"])
        || !is_iso(&item["archived_at"])
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(())
}

fn carry_coordinates_valid(item: &Map<String, Value>) -> bool {
    let count = unsigned(item, "ready_event_count");
    count.is_some_and(|count| count <= 1_000_000)
        && ((count == Some(0)
            && item["reset_carry_event_set_digest"].is_null()
            && item["reset_carry_activation_digest"].is_null())
            || (count.is_some_and(|count| count > 0)
                && is_digest(&item["reset_carry_event_set_digest"])
                && is_digest(&item["reset_carry_activation_digest"])))
}

fn seal_reset(item: &Map<String, Value>) -> WatcherResult<()> {
    if !is_uuid7(&item["reset_id"])
        || [
            "prior_journal_generation_digest",
            "archive_manifest_digest",
            "new_journal_meta_digest",
            "new_journal_generation_digest",
            "target_journal_pointer_digest",
            "outer_coherent_manifest_digest",
        ]
        .iter()
        .any(|field| !is_digest(&item[*field]))
        || !carry_coordinates_valid(item)
        || !is_iso(&item["reset_at"])
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(())
}

fn nonce32(value: &Value) -> bool {
    value.as_str().is_some_and(|value| {
        value.len() == 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

fn seal_reset_guard(item: &Map<String, Value>) -> WatcherResult<()> {
    let instance = text(item, "new_journal_instance_id").unwrap_or_default();
    if text(item, "operation") != Some("watcher_journal_reset")
        || !nonce32(&item["owner_nonce"])
        || !decimal_identity(&item["parent_device"])
        || !decimal_identity(&item["parent_inode"])
        || unsigned(item, "parent_mode") != Some(448)
        || text(item, "guard_basename") != Some(".gkos-watcher-journal-reset.guard")
        || text(item, "guard_stage_basename") != Some(".gkos-watcher-journal-reset.guard-stage")
        || [
            "old_journal_pointer_digest",
            "old_journal_generation_digest",
            "outer_coherent_manifest_digest",
            "archive_manifest_digest",
            "new_journal_meta_digest",
            "new_journal_generation_digest",
            "reset_digest",
            "target_journal_pointer_digest",
        ]
        .iter()
        .any(|field| !is_digest(&item[*field]))
        || !is_uuid7(&item["new_journal_instance_id"])
        || text(item, "new_journal_directory_leaf") != Some(format!("journal-{instance}").as_str())
        || !carry_coordinates_valid(item)
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(())
}

fn pointer_file(operation: &str, digest: &str) -> String {
    let prefix = if operation == "replace_watcher_active_pointer" {
        "watcher-pointer"
    } else {
        "watcher-journal-pointer"
    };
    format!("{prefix}-{}.json", &digest[7..])
}

fn seal_pointer_guard(item: &Map<String, Value>) -> WatcherResult<()> {
    let operation = text(item, "operation").unwrap_or_default();
    let expected_names: &[&str] = match operation {
        "replace_watcher_active_pointer" => &[
            "watcher-active.json",
            ".watcher-active.json.gkos-watcher.guard",
            ".watcher-active.json.gkos-watcher.guard-stage",
            ".watcher-active.json.gkos-watcher.tmp",
        ],
        "replace_watcher_journal_pointer" => &[
            "watcher-journal-active.json",
            ".watcher-journal-active.json.gkos-watcher.guard",
            ".watcher-journal-active.json.gkos-watcher.guard-stage",
            ".watcher-journal-active.json.gkos-watcher.tmp",
        ],
        _ => return fail("GKX_WATCHER_CONTRACT_POINTER_INVALID"),
    };
    let name_fields = [
        "final_basename",
        "guard_basename",
        "guard_stage_basename",
        "temp_basename",
    ];
    let names = name_fields
        .iter()
        .filter_map(|field| text(item, field))
        .collect::<Vec<_>>();
    let old_fields = [
        "old_pointer_file",
        "old_pointer_digest",
        "old_pointer_raw_sha256",
        "old_pointer_byte_size",
        "old_final_device",
        "old_final_inode",
    ];
    let old_absent = old_fields.iter().all(|field| item[*field].is_null());
    let old_present = item["old_pointer_file"].is_string()
        && is_digest(&item["old_pointer_digest"])
        && is_digest(&item["old_pointer_raw_sha256"])
        && is_integer(&item["old_pointer_byte_size"], 1, 1_048_576)
        && decimal_identity(&item["old_final_device"])
        && decimal_identity(&item["old_final_inode"]);
    let old_digest = text(item, "old_pointer_digest").unwrap_or_default();
    let new_digest = text(item, "new_pointer_digest").unwrap_or_default();
    if !nonce32(&item["owner_nonce"])
        || !decimal_identity(&item["parent_device"])
        || !decimal_identity(&item["parent_inode"])
        || unsigned(item, "parent_mode") != Some(448)
        || names != expected_names
        || names.iter().any(|name| name.is_empty() || name.len() > 255)
        || names.iter().collect::<BTreeSet<_>>().len() != names.len()
        || !(old_absent || old_present)
        || operation == "replace_watcher_journal_pointer" && !old_present
        || old_present
            && text(item, "old_pointer_file") != Some(pointer_file(operation, old_digest).as_str())
        || !is_digest(&item["new_pointer_digest"])
        || text(item, "new_pointer_file") != Some(pointer_file(operation, new_digest).as_str())
        || !is_digest(&item["new_pointer_raw_sha256"])
        || !is_integer(&item["new_pointer_byte_size"], 1, 1_048_576)
        || !is_digest(&item["operation_intent_digest"])
        || !is_digest(&item["target_commit_digest"])
    {
        return fail("GKX_WATCHER_CONTRACT_POINTER_INVALID");
    }
    Ok(())
}

fn seal_pointer_decision(item: &Map<String, Value>) -> WatcherResult<()> {
    let action = text(item, "selected_action").unwrap_or_default();
    let authority = text(item, "reader_authority").unwrap_or_default();
    let disposition = text(item, "evidence_disposition").unwrap_or_default();
    let authorities: &[&str] = match action {
        "link_stage_to_guard" | "discard_incomplete_stage" => &["fixed_old", "genesis_none"],
        "unlink_stage_after_link"
        | "create_temp"
        | "replace_temp_to_fixed"
        | "discard_incomplete_temp"
        | "finalize_committed_target" => &["guard_bound_old", "genesis_none"],
        "serve_guard_bound_old" => &["fixed_old", "guard_bound_old", "genesis_none"],
        "serve_fixed_new" => &["fixed_new"],
        "retain_and_fail" => &["fail_closed"],
        _ => return fail("GKX_WATCHER_CONTRACT_POINTER_INVALID"),
    };
    let expected_disposition = match action {
        "link_stage_to_guard"
        | "discard_incomplete_stage"
        | "unlink_stage_after_link"
        | "create_temp"
        | "replace_temp_to_fixed"
        | "discard_incomplete_temp"
        | "finalize_committed_target" => "continue",
        "retain_and_fail" => "retain_and_fail",
        _ => "serve",
    };
    let null_digest = matches!(authority, "genesis_none" | "fail_closed");
    if !authorities.contains(&authority)
        || disposition != expected_disposition
        || null_digest != item["reader_pointer_digest"].is_null()
        || !null_digest && !is_digest(&item["reader_pointer_digest"])
    {
        return fail("GKX_WATCHER_CONTRACT_POINTER_INVALID");
    }
    Ok(())
}

fn seal_scope(item: &Map<String, Value>) -> WatcherResult<()> {
    if !matches!(
        text(item, "adapter_kind"),
        Some("governance_store" | "durable_ledger")
    ) || [
        "adapter_id",
        "adapter_contract_version",
        "vault_id",
        "authority_namespace",
    ]
    .iter()
    .any(|field| !valid_label(&item[*field]))
        || text(item, "authorized_operation") != Some("retrieval.source_removed/projection")
        || !is_digest(&item["configuration_digest"])
        || !is_digest(&item["policy_digest"])
    {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    }
    Ok(())
}

fn seal_binding(item: &Map<String, Value>) -> WatcherResult<()> {
    if !matches!(
        text(item, "adapter_kind"),
        Some("governance_store" | "durable_ledger")
    ) || [
        "adapter_id",
        "adapter_contract_version",
        "vault_id",
        "authority_namespace",
    ]
    .iter()
    .any(|field| !valid_label(&item[*field]))
        || !is_digest(&item["authorization_binding_digest"])
        || !is_digest(&item["configuration_digest"])
        || !is_digest(&item["policy_digest"])
        || item["capabilities"] != json!(ADAPTER_CAPABILITIES)
    {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    }
    Ok(())
}

const STATUS_REASONS: [&str; 18] = [
    "WATCHER_EVENT_OVERFLOW",
    "WATCHER_GRAPH_ARTIFACT_LIMIT_EXCEEDED",
    "WATCHER_GRAPH_DEGRADED",
    "WATCHER_JOURNAL_CAP_EXCEEDED",
    "WATCHER_JOURNAL_RECOVERY_REQUIRED",
    "WATCHER_LEDGER_ADAPTER_FAILED",
    "WATCHER_LEDGER_ADAPTER_UNAVAILABLE",
    "WATCHER_NO_COHERENT_GENERATION",
    "WATCHER_OBSERVATION_ARTIFACT_LIMIT_EXCEEDED",
    "WATCHER_PLAN_ARTIFACT_LIMIT_EXCEEDED",
    "WATCHER_POINTER_RECOVERY_REQUIRED",
    "WATCHER_REBUILD_IN_PROGRESS",
    "WATCHER_RETRIEVAL_DEGRADED",
    "WATCHER_SHUTDOWN_DRAINING",
    "WATCHER_SOURCE_CAPABILITY_UNSTABLE",
    "WATCHER_SOURCE_REJECTED",
    "WATCHER_STARTUP_RECONCILIATION",
    "WATCHER_TOPOLOGY_ARTIFACT_LIMIT_EXCEEDED",
];

const TRANSITION_REASONS: [&str; 22] = [
    "WATCHER_ACTIVATION_FAILED",
    "WATCHER_CONFIGURATION_CHANGED",
    "WATCHER_EVENT_OVERFLOW",
    "WATCHER_GRAPH_ARTIFACT_LIMIT_EXCEEDED",
    "WATCHER_GRAPH_BUILD_FAILED",
    "WATCHER_GKX_APPLY_FAILED",
    "WATCHER_JOURNAL_CAP_EXCEEDED",
    "WATCHER_JOURNAL_INVALID",
    "WATCHER_LAST_COHERENT_STALE",
    "WATCHER_PLAN_ARTIFACT_LIMIT_EXCEEDED",
    "WATCHER_POLICY_CHANGED",
    "WATCHER_RECOVERY_SUPERSEDED",
    "WATCHER_REMOVAL_ADAPTER_FAILED",
    "WATCHER_REMOVAL_ADAPTER_UNAVAILABLE",
    "WATCHER_RETRIEVAL_BUILD_FAILED",
    "WATCHER_SHUTDOWN_CHECKPOINTED",
    "WATCHER_SHUTDOWN_TIMEOUT",
    "WATCHER_SOURCE_SNAPSHOT_CHANGED",
    "WATCHER_SOURCE_UNSTABLE",
    "WATCHER_STARTUP_RECONCILIATION",
    "WATCHER_TOPOLOGY_ARTIFACT_LIMIT_EXCEEDED",
    "WATCHER_VALIDATION_REJECTED",
];

fn seal_status(item: &Map<String, Value>) -> WatcherResult<()> {
    let state = text(item, "watcher_state").unwrap_or_default();
    let freshness = text(item, "freshness").unwrap_or_default();
    let admitted = match state {
        "starting" | "reconciling" => freshness == "stale",
        "serving" => matches!(freshness, "fresh" | "degraded"),
        "stopping" => matches!(freshness, "fresh" | "stale" | "degraded"),
        "error" => matches!(freshness, "stale" | "degraded"),
        _ => false,
    };
    let Some(reasons) = item["reason_codes"].as_array() else {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    };
    if !admitted
        || !is_sorted_unique_strings(&item["reason_codes"], usize::MAX)
        || reasons.iter().any(|reason| {
            !reason
                .as_str()
                .is_some_and(|reason| STATUS_REASONS.contains(&reason))
        })
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    let pre_genesis = [
        &item["source_snapshot_digest"],
        &item["coherent_manifest_digest"],
        &item["last_sync"],
    ]
    .iter()
    .all(|value| value.is_null());
    let authority_null = item["configuration_digest"].is_null() && item["policy_digest"].is_null();
    let authority_ready = is_digest(&item["configuration_digest"])
        && is_digest(&item["policy_digest"])
        && is_digest(&item["source_snapshot_digest"])
        && is_digest(&item["coherent_manifest_digest"])
        && is_iso(&item["last_sync"]);
    if !is_uuid7(&item["service_instance_id"])
        || !is_integer(&item["pid"], 1, 9_007_199_254_740_991)
        || !is_integer(&item["document_count"], 0, 9_007_199_254_740_991)
        || !is_integer(&item["chunk_count"], 0, 9_007_199_254_740_991)
        || !is_integer(&item["uptime_ms"], 0, 9_007_199_254_740_991)
        || !(item["embedding_model"].is_null() || valid_opaque(&item["embedding_model"]))
        || !(pre_genesis && authority_null || !pre_genesis && authority_ready)
        || pre_genesis
            && (item["document_count"] != 0
                || item["chunk_count"] != 0
                || !item["embedding_model"].is_null()
                || freshness != "stale"
                || !matches!(state, "starting" | "reconciling" | "error")
                || !reasons
                    .iter()
                    .any(|reason| reason == "WATCHER_NO_COHERENT_GENERATION"))
        || freshness == "fresh" && (pre_genesis || !reasons.is_empty())
        || freshness != "fresh" && reasons.is_empty()
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(())
}

fn seal_fts(item: &Map<String, Value>) -> WatcherResult<()> {
    let qualified = item["physical_fts5_available"] == true
        && item["status"] == "qualified"
        && item["index_generation_count"] == 23
        && item["query_count"] == 22
        && item["provider_call_count"] == 0;
    let unavailable = item["physical_fts5_available"] == false
        && item["status"] == "unavailable"
        && item["index_generation_count"] == 0
        && item["query_count"] == 0
        && item["provider_call_count"] == 0;
    if !valid_opaque(&item["runtime_version"])
        || !matches!(text(item, "os"), Some("linux" | "windows"))
        || text(item, "arch") != Some("x64")
        || !matches!(text(item, "lane_kind"), Some("reference" | "matrix"))
        || !(qualified || unavailable)
    {
        return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
    }
    Ok(())
}

fn seal_environment(item: &Map<String, Value>) -> WatcherResult<()> {
    if text(item, "runtime") != Some("node")
        || !valid_opaque(&item["runtime_version"])
        || !matches!(text(item, "os"), Some("linux" | "windows"))
        || text(item, "arch") != Some("x64")
        || !valid_opaque(&item["sqlite_version"])
        || !item["physical_fts5_available"].is_boolean()
        || !matches!(text(item, "runner_class"), Some("local" | "github_hosted"))
    {
        return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
    }
    Ok(())
}

fn seal_convergence(item: &Map<String, Value>) -> WatcherResult<()> {
    let fields = [
        "incremental_canonical_gkx_digest",
        "clean_canonical_gkx_digest",
        "incremental_retrieval_manifest_digest",
        "clean_retrieval_manifest_digest",
        "incremental_canonical_graph_digest",
        "clean_canonical_graph_digest",
        "incremental_graphiti_digest",
        "clean_graphiti_digest",
    ];
    let equal = item["incremental_canonical_gkx_digest"] == item["clean_canonical_gkx_digest"]
        && item["incremental_retrieval_manifest_digest"] == item["clean_retrieval_manifest_digest"]
        && item["incremental_canonical_graph_digest"] == item["clean_canonical_graph_digest"]
        && item["incremental_graphiti_digest"] == item["clean_graphiti_digest"];
    if fields.iter().any(|field| !is_digest(&item[*field]))
        || !item["all_equal"].is_boolean()
        || item["all_equal"] != equal
    {
        return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
    }
    Ok(())
}

fn seal_measurement(item: &Map<String, Value>) -> WatcherResult<()> {
    const FAILURES: [&str; 8] = [
        "MEASURE_CONVERGENCE_INVALID",
        "MEASURE_ENVIRONMENT_INVALID",
        "MEASURE_FTS_UNAVAILABLE",
        "MEASURE_GENERATION_INVALID",
        "MEASURE_LATENCY_EXCEEDED",
        "MEASURE_PLAN_INVALID",
        "MEASURE_PROVIDER_LEDGER_INVALID",
        "MEASURE_QUERY_INVALID",
    ];
    let status = text(item, "status").unwrap_or_default();
    let Some(failures) = item["failure_codes"].as_array() else {
        return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
    };
    if text(item, "sample_plan_digest") != Some(SAMPLE_PLAN_DIGEST)
        || !matches!(status, "qualified" | "unavailable" | "failed")
        || !is_sorted_unique_strings(&item["failure_codes"], usize::MAX)
        || failures
            .iter()
            .any(|code| !code.as_str().is_some_and(|code| FAILURES.contains(&code)))
    {
        return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
    }
    let environment = seal_record(&item["environment"])?;
    let fts = seal_record(&item["fts_qualification"])?;
    if ["runtime_version", "os", "arch", "physical_fts5_available"]
        .iter()
        .any(|field| environment[*field] != fts[*field])
    {
        return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
    }
    if status == "qualified" {
        let Some(samples) = item["edit_latency_micros"].as_array() else {
            return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
        };
        if !failures.is_empty()
            || samples.len() != 20
            || samples
                .iter()
                .any(|sample| !is_integer(sample, 0, 5_000_000))
        {
            return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
        }
        let source = object(&item["source_work"])?;
        let embedding = object(&item["embedding_work"])?;
        exact_keys(
            source,
            &[
                "initial_generation_count",
                "mutation_generation_count",
                "total_generation_count",
                "query_count",
                "reparsed_source_count",
            ],
        )?;
        exact_keys(
            embedding,
            &[
                "provider_call_count",
                "provider_item_count",
                "unchanged_chunk_reembedded_count",
            ],
        )?;
        let convergence = seal_record(&item["convergence"])?;
        let mut sorted = samples
            .iter()
            .map(Value::as_u64)
            .collect::<Option<Vec<_>>>()
            .ok_or(WatcherError("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID"))?;
        sorted.sort_unstable();
        let percentiles = json!({
            "p50":sorted[9],
            "p95":sorted[18],
            "p99":sorted[19],
            "max":sorted[19],
        });
        if item["percentiles_micros"] != percentiles
            || fts["status"] != "qualified"
            || fts["index_generation_count"] != 23
            || fts["query_count"] != 22
            || fts["provider_call_count"] != 0
            || convergence["all_equal"] != true
            || item["source_work"]
                != json!({"initial_generation_count":1,"mutation_generation_count":22,"total_generation_count":23,"query_count":22,"reparsed_source_count":22})
            || item["embedding_work"]
                != json!({"provider_call_count":0,"provider_item_count":0,"unchanged_chunk_reembedded_count":0})
        {
            return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
        }
    } else {
        if [
            "edit_latency_micros",
            "percentiles_micros",
            "source_work",
            "embedding_work",
            "convergence",
        ]
        .iter()
        .any(|field| !item[*field].is_null())
        {
            return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
        }
        let only_fts = failures == &vec![Value::String("MEASURE_FTS_UNAVAILABLE".to_owned())];
        if status == "unavailable" {
            if !only_fts || fts["status"] != "unavailable" || fts["lane_kind"] != "matrix" {
                return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
            }
        } else if failures.is_empty() || only_fts {
            return fail("GKX_WATCHER_CONTRACT_MEASUREMENT_INVALID");
        }
    }
    Ok(())
}

fn seal_pack_manifest(item: &Map<String, Value>) -> WatcherResult<()> {
    let Some(files) = item["files"].as_array() else {
        return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
    };
    if text(item, "pack_contract_version") != Some(PACK_VERSION)
        || unsigned(item, "file_count") != Some(files.len() as u64)
        || !is_integer(&item["total_bytes"], 0, 9_007_199_254_740_991)
    {
        return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
    }
    let mut names = Vec::new();
    let mut total = 0_u64;
    for file in files {
        let file = object(file)?;
        exact_keys(file, &["file", "byte_size", "raw_sha256"])?;
        let Some(name) = text(file, "file") else {
            return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
        };
        if name == "pack-manifest.json"
            || !is_integer(&file["byte_size"], 1, 9_007_199_254_740_991)
            || !is_digest(&file["raw_sha256"])
        {
            return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
        }
        names.push(name);
        total += unsigned(file, "byte_size").unwrap();
    }
    if names != PACK_FILES
        || unsigned(item, "file_count") != Some(17)
        || unsigned(item, "total_bytes") != Some(total)
    {
        return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
    }
    Ok(())
}

fn seal_pointer_leaf(value: &Value, basename: &str) -> WatcherResult<Option<Value>> {
    if value.is_null() {
        return Ok(None);
    }
    let leaf = object(value)?;
    exact_keys(
        leaf,
        &[
            "basename",
            "device",
            "inode",
            "mode",
            "nlink",
            "capability_state",
            "body_class",
            "semantic_digest",
            "raw_sha256",
            "byte_size",
        ],
    )?;
    let body_class = text(leaf, "body_class").unwrap_or_default();
    let incomplete = body_class == "incomplete_noncanonical";
    let null_body = leaf["semantic_digest"].is_null()
        && leaf["raw_sha256"].is_null()
        && leaf["byte_size"].is_null();
    if text(leaf, "basename") != Some(basename)
        || !decimal_identity(&leaf["device"])
        || !decimal_identity(&leaf["inode"])
        || !is_integer(&leaf["mode"], 0, 9_007_199_254_740_991)
        || !is_integer(&leaf["nlink"], 1, 9_007_199_254_740_991)
        || !matches!(
            text(leaf, "capability_state"),
            Some(
                "exact_owned_regular_direct_nonalias_stable"
                    | "wrong_owner"
                    | "non_regular"
                    | "symlink_or_reparse"
                    | "aliased"
                    | "outside_parent"
                    | "windows_identity_unstable"
            )
        )
        || !matches!(
            body_class,
            "canonical_exact" | "canonical_mismatch" | "incomplete_noncanonical"
        )
        || incomplete != null_body
        || !incomplete
            && (!is_digest(&leaf["semantic_digest"])
                || !is_digest(&leaf["raw_sha256"])
                || !is_integer(&leaf["byte_size"], 1, 1_048_576))
    {
        return fail("GKX_WATCHER_CONTRACT_POINTER_INVALID");
    }
    Ok(Some(value.clone()))
}

fn pointer_decision(
    action: &str,
    authority: &str,
    digest: Option<&str>,
    disposition: &str,
) -> WatcherResult<Value> {
    let mut decision = json!({
        "contract_version":"gkos-watcher-pointer-recovery-decision/1.0.0-draft.1",
        "selected_action":action,
        "reader_authority":authority,
        "reader_pointer_digest":digest,
        "evidence_disposition":disposition,
        "decision_digest":"",
    });
    let digest = digest_without(&decision, "decision_digest")?;
    decision["decision_digest"] = Value::String(digest);
    seal_record(&decision)
}

fn classify_pointer_recovery(recipe_value: &Value, guard_value: &Value) -> WatcherResult<Value> {
    let recipe = object(recipe_value)?;
    exact_keys(
        recipe,
        &[
            "namespace_kind",
            "parent",
            "stage",
            "guard",
            "temp",
            "fixed",
            "old_artifact",
            "new_artifact",
            "committed_target_state",
        ],
    )?;
    let guard_value = seal_record(guard_value)?;
    let guard = object(&guard_value)?;
    let namespace = text(recipe, "namespace_kind").unwrap_or_default();
    let target = text(recipe, "committed_target_state").unwrap_or_default();
    if !matches!(namespace, "outer" | "journal")
        || namespace == "outer"
            && text(guard, "operation") != Some("replace_watcher_active_pointer")
        || namespace == "journal"
            && text(guard, "operation") != Some("replace_watcher_journal_pointer")
        || !matches!(target, "old" | "prepared" | "committed" | "ambiguous")
    {
        return fail("GKX_WATCHER_CONTRACT_POINTER_INVALID");
    }
    let parent = object(&recipe["parent"])?;
    exact_keys(parent, &["device", "inode", "mode", "capability_state"])?;
    if !decimal_identity(&parent["device"])
        || !decimal_identity(&parent["inode"])
        || !is_integer(&parent["mode"], 0, 9_007_199_254_740_991)
        || !matches!(
            text(parent, "capability_state"),
            Some(
                "exact_owned_directory_nonalias_stable"
                    | "wrong_owner"
                    | "non_directory"
                    | "symlink_or_reparse"
                    | "aliased"
                    | "windows_identity_unstable"
            )
        )
    {
        return fail("GKX_WATCHER_CONTRACT_POINTER_INVALID");
    }
    let stage = seal_pointer_leaf(
        &recipe["stage"],
        text(guard, "guard_stage_basename").unwrap(),
    )?;
    let guard_leaf = seal_pointer_leaf(&recipe["guard"], text(guard, "guard_basename").unwrap())?;
    let temp = seal_pointer_leaf(&recipe["temp"], text(guard, "temp_basename").unwrap())?;
    let fixed = seal_pointer_leaf(&recipe["fixed"], text(guard, "final_basename").unwrap())?;
    let old_artifact = if guard["old_pointer_file"].is_null() {
        if recipe["old_artifact"].is_null() {
            None
        } else {
            return fail("GKX_WATCHER_CONTRACT_POINTER_INVALID");
        }
    } else {
        seal_pointer_leaf(
            &recipe["old_artifact"],
            text(guard, "old_pointer_file").unwrap(),
        )?
    };
    let new_artifact = seal_pointer_leaf(
        &recipe["new_artifact"],
        text(guard, "new_pointer_file").unwrap(),
    )?;
    if new_artifact.is_none() {
        return fail("GKX_WATCHER_CONTRACT_POINTER_INVALID");
    }
    let exact_parent = text(parent, "capability_state")
        == Some("exact_owned_directory_nonalias_stable")
        && unsigned(parent, "mode") == Some(448)
        && parent["device"] == guard["parent_device"]
        && parent["inode"] == guard["parent_inode"]
        && parent["mode"] == guard["parent_mode"];
    let leaf_secure = |leaf: &Option<Value>| {
        leaf.as_ref().is_none_or(|leaf| {
            text(leaf.as_object().unwrap(), "capability_state")
                == Some("exact_owned_regular_direct_nonalias_stable")
        })
    };
    let all_secure = [
        &stage,
        &guard_leaf,
        &temp,
        &fixed,
        &old_artifact,
        &new_artifact,
    ]
    .iter()
    .all(|leaf| leaf_secure(leaf));
    let fixed_links_secure = [&temp, &fixed, &old_artifact, &new_artifact]
        .iter()
        .all(|leaf| {
            leaf.as_ref()
                .is_none_or(|leaf| unsigned(leaf.as_object().unwrap(), "nlink") == Some(1))
        });
    let exact_body = |leaf: &Option<Value>, digest: &Value, raw: &Value, size: &Value| {
        leaf.as_ref().is_some_and(|leaf| {
            let leaf = leaf.as_object().unwrap();
            text(leaf, "body_class") == Some("canonical_exact")
                && leaf["semantic_digest"] == *digest
                && leaf["raw_sha256"] == *raw
                && leaf["byte_size"] == *size
        })
    };
    let old_exact = if guard["old_pointer_digest"].is_null() {
        fixed.is_none()
    } else {
        exact_body(
            &fixed,
            &guard["old_pointer_digest"],
            &guard["old_pointer_raw_sha256"],
            &guard["old_pointer_byte_size"],
        )
    };
    let old_fixed_identity = guard["old_pointer_digest"].is_null()
        || !old_exact
        || fixed.as_ref().is_some_and(|fixed| {
            let fixed = fixed.as_object().unwrap();
            fixed["device"] == guard["old_final_device"]
                && fixed["inode"] == guard["old_final_inode"]
        });
    let new_exact = exact_body(
        &fixed,
        &guard["new_pointer_digest"],
        &guard["new_pointer_raw_sha256"],
        &guard["new_pointer_byte_size"],
    );
    let guard_bytes = canonical_bytes(&guard_value)?;
    let guard_raw = Value::String(sha256(&guard_bytes));
    let guard_size = Value::from(guard_bytes.len() as u64);
    let guard_exact = exact_body(&guard_leaf, &guard["guard_digest"], &guard_raw, &guard_size);
    let new_artifact_exact = exact_body(
        &new_artifact,
        &guard["new_pointer_digest"],
        &guard["new_pointer_raw_sha256"],
        &guard["new_pointer_byte_size"],
    );
    let old_artifact_exact = if guard["old_pointer_digest"].is_null() {
        old_artifact.is_none()
    } else {
        exact_body(
            &old_artifact,
            &guard["old_pointer_digest"],
            &guard["old_pointer_raw_sha256"],
            &guard["old_pointer_byte_size"],
        )
    };
    let mut reader_authority = "fail_closed";
    let mut reader_digest = None;
    if exact_parent
        && all_secure
        && fixed_links_secure
        && old_artifact_exact
        && new_artifact_exact
        && old_fixed_identity
    {
        if guard_leaf.is_some() && guard_exact {
            if guard["old_pointer_digest"].is_null() {
                reader_authority = "genesis_none";
            } else {
                reader_authority = "guard_bound_old";
                reader_digest = text(guard, "old_pointer_digest");
            }
        } else if guard_leaf.is_none() && old_exact {
            if guard["old_pointer_digest"].is_null() {
                reader_authority = "genesis_none";
            } else {
                reader_authority = "fixed_old";
                reader_digest = text(guard, "old_pointer_digest");
            }
        } else if guard_leaf.is_none() && new_exact && target == "committed" {
            reader_authority = "fixed_new";
            reader_digest = text(guard, "new_pointer_digest");
        }
    }
    let retain = || pointer_decision("retain_and_fail", "fail_closed", None, "retain_and_fail");
    if reader_authority == "fail_closed" || target == "ambiguous" {
        return retain();
    }
    if guard["old_pointer_digest"].is_null() {
        let target_allowed = if guard_leaf.is_none() && fixed.is_none() && temp.is_none() {
            if stage.is_none() {
                matches!(target, "old" | "prepared")
            } else {
                target == "prepared"
            }
        } else if guard_leaf.is_none() && stage.is_none() && temp.is_none() && new_exact {
            target == "committed"
        } else if guard_leaf.is_some() && fixed.is_none() {
            target == "prepared"
        } else if guard_leaf.is_some() && stage.is_none() && temp.is_none() && new_exact {
            matches!(target, "prepared" | "committed")
        } else {
            false
        };
        if !target_allowed {
            return retain();
        }
    }
    if let (None, Some(stage)) = (&guard_leaf, &stage) {
        let stage_record = stage.as_object().unwrap();
        if unsigned(stage_record, "nlink") != Some(1) {
            return retain();
        }
        if text(stage_record, "body_class") == Some("incomplete_noncanonical") {
            return pointer_decision(
                "discard_incomplete_stage",
                reader_authority,
                reader_digest,
                "continue",
            );
        }
        if text(stage_record, "body_class") != Some("canonical_exact")
            || stage_record["semantic_digest"] != guard["guard_digest"]
        {
            return retain();
        }
        return pointer_decision(
            "link_stage_to_guard",
            reader_authority,
            reader_digest,
            "continue",
        );
    }
    if let (Some(stage), Some(guard_leaf)) = (&stage, &guard_leaf) {
        let stage = stage.as_object().unwrap();
        let guard_leaf = guard_leaf.as_object().unwrap();
        if !guard_exact
            || text(stage, "body_class") != Some("canonical_exact")
            || stage["semantic_digest"] != guard["guard_digest"]
            || stage["device"] != guard_leaf["device"]
            || stage["inode"] != guard_leaf["inode"]
            || unsigned(stage, "nlink") != Some(2)
            || unsigned(guard_leaf, "nlink") != Some(2)
        {
            return retain();
        }
        return pointer_decision(
            "unlink_stage_after_link",
            reader_authority,
            reader_digest,
            "continue",
        );
    }
    if guard_leaf.is_some() {
        if !guard_exact || stage.is_some() {
            return retain();
        }
        if old_exact && temp.is_none() {
            return pointer_decision("create_temp", reader_authority, reader_digest, "continue");
        }
        if old_exact {
            if let Some(temp) = &temp {
                let temp_record = temp.as_object().unwrap();
                if unsigned(temp_record, "nlink") != Some(1) {
                    return retain();
                }
                if text(temp_record, "body_class") == Some("incomplete_noncanonical") {
                    return pointer_decision(
                        "discard_incomplete_temp",
                        reader_authority,
                        reader_digest,
                        "continue",
                    );
                }
                if exact_body(
                    &Some(temp.clone()),
                    &guard["new_pointer_digest"],
                    &guard["new_pointer_raw_sha256"],
                    &guard["new_pointer_byte_size"],
                ) {
                    return pointer_decision(
                        "replace_temp_to_fixed",
                        reader_authority,
                        reader_digest,
                        "continue",
                    );
                }
            }
            return retain();
        }
        if new_exact && temp.is_none() && matches!(target, "prepared" | "committed") {
            return pointer_decision(
                "finalize_committed_target",
                reader_authority,
                reader_digest,
                "continue",
            );
        }
        return retain();
    }
    if stage.is_some() || temp.is_some() {
        return retain();
    }
    if reader_authority == "fixed_new" {
        pointer_decision("serve_fixed_new", reader_authority, reader_digest, "serve")
    } else {
        pointer_decision(
            "serve_guard_bound_old",
            reader_authority,
            reader_digest,
            "serve",
        )
    }
}

fn seal_transition_sequence(value: &Value, terminal_required: bool) -> WatcherResult<Value> {
    let input = array(value)?;
    if !(1..=7).contains(&input.len()) {
        return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
    }
    let transitions = input
        .iter()
        .map(seal_record)
        .collect::<WatcherResult<Vec<_>>>()?;
    for (index, current) in transitions.iter().enumerate() {
        let current = object(current)?;
        if unsigned(current, "transition_ordinal") != Some(index as u64)
            || current["prior_transition_digest"]
                != if index == 0 {
                    Value::Null
                } else {
                    transitions[index - 1]["transition_digest"].clone()
                }
            || current["batch_id"] != transitions[0]["batch_id"]
            || current["observation_digest"] != transitions[0]["observation_digest"]
        {
            return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
        }
        if index > 0 && !matches!(text(current, "state"), Some("failed" | "superseded")) {
            let prior = object(&transitions[index - 1])?;
            for field in [
                "observation_digest",
                "plan_digest",
                "gkx_delta_digest",
                "gkx_snapshot_digest",
            ] {
                if !prior[field].is_null() && current[field] != prior[field] {
                    return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
                }
            }
            let prior_retrieval = object(&prior["retrieval_projection_state"])?;
            let prior_graph = object(&prior["graph_projection_state"])?;
            if text(prior_retrieval, "state") == Some("ready")
                && current["retrieval_projection_state"] != prior["retrieval_projection_state"]
                || text(prior_graph, "state") == Some("ready")
                    && current["graph_projection_state"] != prior["graph_projection_state"]
            {
                return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
            }
        }
        if index > 0 && matches!(text(current, "state"), Some("failed" | "superseded")) {
            let prior = object(&transitions[index - 1])?;
            for field in [
                "observation_digest",
                "plan_digest",
                "gkx_delta_digest",
                "gkx_snapshot_digest",
                "retrieval_projection_state",
                "graph_projection_state",
            ] {
                if current[field] != prior[field] {
                    return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
                }
            }
        }
    }
    if terminal_required {
        if text(object(transitions.last().unwrap())?, "terminal_state") == Some("open")
            || transitions[..transitions.len() - 1]
                .iter()
                .any(|row| text(row.as_object().unwrap(), "terminal_state") != Some("open"))
        {
            return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
        }
    } else if transitions.iter().any(|row| {
        let row = row.as_object().unwrap();
        text(row, "terminal_state") != Some("open")
            || matches!(text(row, "state"), Some("failed" | "superseded"))
    }) {
        return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
    }
    Ok(Value::Array(transitions))
}

fn artifact_coordinate(kind: &str, value: &Value) -> WatcherResult<Value> {
    let digest_field = match kind {
        "observation" => "observation_digest",
        "plan" => "plan_digest",
        "topology" => "topology_snapshot_digest",
        "graph" => "graph_artifact_digest",
        _ => return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID"),
    };
    let Some(digest) = value.get(digest_field).and_then(Value::as_str) else {
        return fail("GKX_WATCHER_CONTRACT_DIGEST_INVALID");
    };
    if !is_digest_text(digest) {
        return fail("GKX_WATCHER_CONTRACT_DIGEST_INVALID");
    }
    let bytes = canonical_bytes(value)?;
    let cap = if kind == "observation" {
        4 * 1024 * 1024
    } else {
        512 * 1024 * 1024
    };
    if bytes.len() > cap {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(json!({
        "file":format!("watcher-{kind}-{}.json", &digest[7..]),
        "byte_size":bytes.len(),
        "raw_sha256":sha256(&bytes),
        "bytes":String::from_utf8(bytes).unwrap(),
    }))
}

fn js_fnv1a(value: &str) -> u32 {
    value.encode_utf16().fold(0x811c9dc5_u32, |hash, unit| {
        (hash ^ u32::from(unit)).wrapping_mul(0x01000193)
    })
}

fn base36(mut value: u64) -> String {
    if value == 0 {
        return "0".to_owned();
    }
    let mut digits = Vec::new();
    while value > 0 {
        let digit = (value % 36) as u8;
        digits.push(if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        });
        value /= 36;
    }
    digits.reverse();
    String::from_utf8(digits).unwrap()
}

fn graphiti_content_hash(value: &str) -> String {
    format!(
        "{}:{}",
        base36(u64::from(js_fnv1a(value))),
        base36(value.encode_utf16().count() as u64)
    )
}

fn graphiti_slug(value: &str) -> String {
    let mut output = String::new();
    let mut separator = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if separator && !output.is_empty() {
                output.push('-');
            }
            separator = false;
            output.push(character);
        } else {
            separator = true;
        }
    }
    if output.is_empty() {
        "vault".to_owned()
    } else {
        output
    }
}

fn graphiti_hash32(value: &str, seed: u32) -> u32 {
    let mut hash = 0x811c9dc5_u32 ^ seed;
    for unit in value.encode_utf16() {
        hash = (hash ^ u32::from(unit)).wrapping_mul(0x01000193);
    }
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x85ebca6b);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2ae35);
    hash ^ (hash >> 16)
}

fn graphiti_uuid(value: &str) -> String {
    let mut bytes = [0_u8; 16];
    for block in 0..4_u32 {
        let seed = (block + 1).wrapping_mul(0x9e3779b1);
        let hash = graphiti_hash32(value, seed);
        bytes[block as usize * 4] = (hash >> 24) as u8;
        bytes[block as usize * 4 + 1] = (hash >> 16) as u8;
        bytes[block as usize * 4 + 2] = (hash >> 8) as u8;
        bytes[block as usize * 4 + 3] = hash as u8;
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(32);
    for byte in bytes {
        hex.push(HEX[(byte >> 4) as usize] as char);
        hex.push(HEX[(byte & 0x0f) as usize] as char);
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn graphiti_authored_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
        && matches!(bytes[14].to_ascii_lowercase(), b'1'..=b'8')
        && matches!(bytes[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b')
}

fn graphiti_slice_units(value: &str, max: usize) -> WatcherResult<Vec<u16>> {
    let units = value.encode_utf16().take(max).collect::<Vec<_>>();
    if units
        .last()
        .is_some_and(|unit| matches!(*unit, 0xd800..=0xdbff))
    {
        // Full's bounded() can split a pair, but the watcher wrapper reparses
        // the episode body through stableJson, whose canonical UTF-16 seam
        // rejects the resulting unpaired surrogate.
        return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
    }
    Ok(units)
}

fn graphiti_write_json_units(output: &mut String, units: &[u16]) {
    output.push('"');
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        match unit {
            0x0008 => output.push_str("\\b"),
            0x0009 => output.push_str("\\t"),
            0x000a => output.push_str("\\n"),
            0x000c => output.push_str("\\f"),
            0x000d => output.push_str("\\r"),
            0x0022 => output.push_str("\\\""),
            0x005c => output.push_str("\\\\"),
            0x0000..=0x001f => output.push_str(format!("\\u{unit:04x}").as_str()),
            0xd800..=0xdbff
                if units
                    .get(index + 1)
                    .is_some_and(|next| matches!(*next, 0xdc00..=0xdfff)) =>
            {
                let next = units[index + 1];
                let scalar =
                    0x1_0000 + ((u32::from(unit) - 0xd800) << 10) + (u32::from(next) - 0xdc00);
                output.push(char::from_u32(scalar).expect("valid surrogate pair"));
                index += 1;
            }
            0xd800..=0xdfff => output.push_str(format!("\\u{unit:04x}").as_str()),
            _ => output.push(char::from_u32(u32::from(unit)).expect("valid BMP scalar")),
        }
        index += 1;
    }
    output.push('"');
}

fn graphiti_object_key_order(left: &str, right: &str) -> Ordering {
    let index = |value: &str| {
        if value.is_empty()
            || value.len() > 10
            || value.len() > 1 && value.starts_with('0')
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        value
            .parse::<u64>()
            .ok()
            .filter(|value| *value < u64::from(u32::MAX))
    };
    match (index(left), index(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => utf16_cmp(left, right),
    }
}

fn graphiti_write_bounded_json(
    output: &mut String,
    value: &Value,
    max: usize,
    depth: usize,
) -> WatcherResult<()> {
    if depth > 8 {
        graphiti_write_json_units(
            output,
            &graphiti_slice_units("[depth-limited]", usize::MAX)?,
        );
        return Ok(());
    }
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(_) => output.push_str(
            canonical_json(value)
                .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))?
                .as_str(),
        ),
        Value::String(value) => {
            graphiti_write_json_units(output, &graphiti_slice_units(value, max)?);
        }
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().take(200).enumerate() {
                if index > 0 {
                    output.push(',');
                }
                graphiti_write_bounded_json(output, value, max, depth + 1)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            // Raw artifacts are canonical UTF-16-key-ordered JSON. JSON.parse
            // then exposes integer-index keys first, followed by that retained
            // insertion order. Reproduce Object.entries(...).slice(0, 200),
            // Object.fromEntries overwrite semantics, and the final stableJson
            // UTF-16 sort without converting through a Rust scalar-key map.
            let mut source = values.iter().collect::<Vec<_>>();
            source.sort_by(|(left, _), (right, _)| graphiti_object_key_order(left, right));
            let mut bounded: Vec<(Vec<u16>, &Value)> = Vec::new();
            for (key, value) in source.into_iter().take(200) {
                let key = graphiti_slice_units(key, 80)?;
                if let Some(existing) = bounded.iter_mut().find(|(candidate, _)| *candidate == key)
                {
                    existing.1 = value;
                } else {
                    bounded.push((key, value));
                }
            }
            bounded.sort_by(|(left, _), (right, _)| left.cmp(right));
            output.push('{');
            for (index, (key, value)) in bounded.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                graphiti_write_json_units(output, &key);
                output.push(':');
                graphiti_write_bounded_json(output, value, max, depth + 1)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

fn graphiti_bounded_json(value: &Value, max: usize) -> WatcherResult<String> {
    let mut output = String::new();
    graphiti_write_bounded_json(&mut output, value, max, 0)?;
    Ok(output)
}

fn graphiti_nonnull<'a>(record: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    record.get(key).filter(|value| !value.is_null())
}

fn graphiti_optional_object<'a>(
    record: &'a Map<String, Value>,
    key: &str,
) -> WatcherResult<Option<&'a Map<String, Value>>> {
    graphiti_nonnull(record, key)
        .map(|value| {
            value
                .as_object()
                .ok_or(WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))
        })
        .transpose()
}

fn graphiti_required_object<'a>(
    record: &'a Map<String, Value>,
    key: &str,
) -> WatcherResult<&'a Map<String, Value>> {
    record
        .get(key)
        .and_then(Value::as_object)
        .ok_or(WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))
}

fn graphiti_required_array<'a>(
    record: &'a Map<String, Value>,
    key: &str,
) -> WatcherResult<&'a Vec<Value>> {
    record
        .get(key)
        .and_then(Value::as_array)
        .ok_or(WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))
}

fn graphiti_text_or<'a>(
    first: Option<&'a Value>,
    second: Option<&'a Value>,
    fallback: &'a str,
) -> WatcherResult<&'a str> {
    for value in [first, second].into_iter().flatten() {
        if value.is_null() {
            continue;
        }
        let Some(value) = value.as_str() else {
            return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
        };
        if !value.is_empty() {
            return Ok(value);
        }
    }
    Ok(fallback)
}

fn graphiti_date_parseable(value: &str) -> bool {
    let bytes = value.as_bytes();
    let number = |start: usize, end: usize| {
        value
            .get(start..end)
            .filter(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|part| part.parse::<u32>().ok())
    };
    if bytes.len() < 10 || bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return false;
    }
    let (Some(year), Some(month), Some(day)) = (number(0, 4), number(5, 7), number(8, 10)) else {
        return false;
    };
    // Phase-3 first applies its exact ISO-shaped timestamp grammar and then
    // delegates calendar interpretation to JavaScript Date.parse. V8 admits
    // day 29..31 overflow (for example February 30) and the ISO end-of-day
    // spelling 24:00, normalizing both into the next calendar coordinate.
    // Graphiti deliberately preserves the authored spelling but uses that
    // Date.parse result to select its reference-time provenance, so do not
    // impose a stricter Gregorian-calendar authority here.
    let _ = year;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return false;
    }
    if bytes.len() == 10 {
        return true;
    }
    if bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.len() < 16
        || number(11, 13).is_none_or(|hour| hour > 24)
        || number(14, 16).is_none_or(|minute| minute >= 60)
    {
        return false;
    }
    let hour = number(11, 13).expect("validated hour");
    let minute = number(14, 16).expect("validated minute");
    let mut cursor = 16;
    let mut second = 0;
    let mut seconds_present = false;
    if bytes.get(cursor) == Some(&b':') {
        seconds_present = true;
        if bytes.len() < cursor + 3 {
            return false;
        }
        let Some(parsed_second) = number(cursor + 1, cursor + 3) else {
            return false;
        };
        if parsed_second >= 60 {
            return false;
        }
        second = parsed_second;
        cursor += 3;
    }
    let mut fraction_nonzero = false;
    if bytes.get(cursor) == Some(&b'.') {
        if !seconds_present {
            return false;
        }
        cursor += 1;
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            fraction_nonzero |= bytes[cursor] != b'0';
            cursor += 1;
        }
        if cursor == start {
            return false;
        }
    }
    if hour == 24 && (minute != 0 || second != 0 || fraction_nonzero) {
        return false;
    }
    match bytes.get(cursor) {
        None => true,
        Some(b'Z') => cursor + 1 == bytes.len(),
        Some(b'+' | b'-') => {
            cursor + 6 == bytes.len()
                && bytes.get(cursor + 3) == Some(&b':')
                && number(cursor + 1, cursor + 3).is_some_and(|hour| hour <= 23)
                && number(cursor + 4, cursor + 6).is_some_and(|minute| minute < 60)
        }
        _ => false,
    }
}

fn derive_graphiti(graph: &Value, vault_id: &Value) -> WatcherResult<Value> {
    let graph = assert_raw_graph_shape(graph, true)?;
    let Some(vault_id) = vault_id.as_str().filter(|_| valid_label(vault_id)) else {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    };
    let processing_time = "1970-01-01T00:00:00.000Z";
    let group_id = format!(
        "gkx-{}-{:08x}-assertions",
        graphiti_slug(vault_id),
        graphiti_hash32(vault_id, 0)
    );
    let nodes = array(&graph["nodes"])?;
    let links = array(&graph["links"])?;
    let mut labels = BTreeMap::<String, String>::new();
    for node in nodes {
        let node = object(node)?;
        let (Some(id), Some(label)) = (text(node, "id"), text(node, "label")) else {
            return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
        };
        labels.insert(id.to_owned(), label.to_owned());
    }
    let mut episodes = Vec::new();
    for node in nodes {
        let node = node
            .as_object()
            .ok_or(WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))?;
        if text(node, "kind") != Some("file") {
            continue;
        }
        let gkx = graphiti_optional_object(node, "gkx")?;
        let projection = gkx
            .map(|gkx| graphiti_optional_object(gkx, "projection"))
            .transpose()?
            .flatten();
        let (Some(id), Some(path), Some(label)) =
            (text(node, "id"), text(node, "path"), text(node, "label"))
        else {
            return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
        };
        let title = gkx
            .and_then(|gkx| text(gkx, "title"))
            .filter(|title| !title.is_empty())
            .unwrap_or(label);
        let note_type = graphiti_text_or(
            gkx.and_then(|gkx| gkx.get("type")),
            node.get("type"),
            "note",
        )?;
        let event_time = text(node, "validAt")
            .or_else(|| text(node, "createdAt"))
            .unwrap_or(processing_time);
        if event_time != processing_time && !is_iso(&Value::String(event_time.to_owned())) {
            return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
        }
        let projected_created = projection
            .and_then(|projection| projection.get("authored"))
            .and_then(Value::as_object)
            .and_then(|authored| text(authored, "createdAt"));
        let gkx_timestamp = gkx.and_then(|gkx| text(gkx, "timestamp"));
        let reference_time_source = if projected_created.is_some_and(graphiti_date_parseable) {
            "gkx.created_at"
        } else if gkx_timestamp.is_some_and(graphiti_date_parseable) {
            "gkx.timestamp"
        } else if text(node, "createdAt").is_some_and(|value| !value.is_empty()) {
            "file.created_at"
        } else if text(node, "updatedAt").is_some_and(|value| !value.is_empty()) {
            "file.updated_at"
        } else {
            "index_time_fallback"
        };
        let mut semantic = Vec::new();
        for link in links {
            let link = object(link)?;
            if text(link, "kind") == Some("semantic") && text(link, "source") == Some(id) {
                let Some(target) = text(link, "target") else {
                    return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
                };
                let label = labels
                    .get(target)
                    .cloned()
                    .unwrap_or_else(|| target.to_owned());
                if !semantic.contains(&label) {
                    semantic.push(label);
                }
            }
        }
        let tags = node.get("tags").cloned().unwrap_or_else(|| json!([]));
        let gkx_version = gkx
            .and_then(|gkx| graphiti_nonnull(gkx, "gkxVersion"))
            .cloned()
            .unwrap_or(Value::Null);
        let uid = gkx
            .and_then(|gkx| graphiti_nonnull(gkx, "uid"))
            .cloned()
            .unwrap_or(Value::Null);
        if !uid.is_null() && !uid.is_string() {
            return fail("GKX_WATCHER_CONTRACT_GRAPH_INVALID");
        }
        let (source_origin, sensitivity, policy_version) = if let Some(projection) = projection {
            let authored = graphiti_required_object(projection, "authored")?;
            let effective = graphiti_required_object(projection, "effective")?;
            let assessment = graphiti_required_object(projection, "assessment")?;
            let policy = graphiti_required_object(assessment, "policy")?;
            let source_origin = graphiti_optional_object(authored, "authorship")?
                .and_then(|authorship| text(authorship, "origin"))
                .unwrap_or("unknown")
                .to_owned();
            let sensitivity = graphiti_text_or(
                effective.get("sensitivity"),
                gkx.and_then(|gkx| gkx.get("sensitivity")),
                "internal",
            )?
            .to_owned();
            let policy_version = graphiti_nonnull(policy, "version")
                .and_then(Value::as_str)
                .unwrap_or("legacy")
                .to_owned();
            (source_origin, sensitivity, policy_version)
        } else {
            (
                "unknown".to_owned(),
                graphiti_text_or(gkx.and_then(|gkx| gkx.get("sensitivity")), None, "internal")?
                    .to_owned(),
                "legacy".to_owned(),
            )
        };
        let metadata = json!({
            "vault_identity":graphiti_content_hash(vault_id),
            "source_path_hash":graphiti_content_hash(path),
            "gkx_version":gkx_version,
            "uid":uid,
            "note_type":note_type,
            "sensitivity":sensitivity,
            "policy_version":policy_version,
            "corpus_id":group_id,
            "workspace_id":group_id,
            "event_time":event_time,
            "processing_time":processing_time,
        });
        let labels_body = if let Some(projection) = projection {
            json!({
                "authored":graphiti_required_object(projection,"authored")?.get("labels").cloned().unwrap_or(Value::Null),
                "derived":graphiti_required_object(projection,"derived")?.get("labels").cloned().unwrap_or(Value::Null),
                "proposed":graphiti_required_object(projection,"proposed")?.get("labels").cloned().unwrap_or(Value::Null),
                "approved":graphiti_required_object(projection,"approved")?.get("labels").cloned().unwrap_or(Value::Null),
                "effective":graphiti_required_object(projection,"effective")?.get("labels").cloned().unwrap_or(Value::Null),
            })
        } else {
            Value::Null
        };
        let (governance, diagnostic_codes, evidence, integrity_policy) = if let Some(projection) =
            projection
        {
            let authored = graphiti_required_object(projection, "authored")?;
            let derived = graphiti_required_object(projection, "derived")?;
            let proposed = graphiti_required_object(projection, "proposed")?;
            let approved = graphiti_required_object(projection, "approved")?;
            let effective = graphiti_required_object(projection, "effective")?;
            let assessment = graphiti_required_object(projection, "assessment")?;
            let scores = graphiti_required_object(assessment, "scores")?;
            let assessment_labels = graphiti_required_object(assessment, "labels")?;
            let policy = graphiti_required_object(assessment, "policy")?;
            let diagnostics = graphiti_required_array(projection, "diagnostics")?;
            let codes = diagnostics
                .iter()
                .map(|diagnostic| {
                    diagnostic
                        .as_object()
                        .and_then(|diagnostic| diagnostic.get("code"))
                        .cloned()
                        .ok_or(WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))
                })
                .collect::<WatcherResult<Vec<_>>>()?;
            let derived_assessment_labels = assessment_labels
                .get("derived")
                .and_then(Value::as_array)
                .ok_or(WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))?;
            let assessment_label = derived_assessment_labels
                .first()
                .and_then(Value::as_str)
                .unwrap_or("assessment:not-assessable");
            let evidence_or_empty = |origin: &Map<String, Value>| {
                graphiti_nonnull(origin, "evidence")
                    .cloned()
                    .unwrap_or_else(|| json!({}))
            };
            (
                json!({
                    "effective":{
                        "sensitivity":effective.get("sensitivity").cloned().unwrap_or(Value::Null),
                        "labels":effective.get("labels").cloned().unwrap_or(Value::Null),
                        "relationships":effective.get("relationships").cloned().unwrap_or(Value::Null),
                    },
                    "relationships":{
                        "authored":authored.get("relationships").cloned().unwrap_or(Value::Null),
                        "derived":derived.get("relationships").cloned().unwrap_or(Value::Null),
                        "proposed":proposed.get("relationships").cloned().unwrap_or(Value::Null),
                        "approved":approved.get("relationships").cloned().unwrap_or(Value::Null),
                        "effective":effective.get("relationships").cloned().unwrap_or(Value::Null),
                    },
                    "effective_sensitivity":effective.get("sensitivity").cloned().unwrap_or(Value::Null),
                    "assessment":{
                        "overall":scores.get("overall").cloned().unwrap_or(Value::Null),
                        "label":assessment_label,
                        "interpretation":assessment.get("interpretation").cloned().unwrap_or(Value::Null),
                        "policy_id":policy.get("id").cloned().unwrap_or(Value::Null),
                        "policy_version":policy.get("version").cloned().unwrap_or(Value::Null),
                        "policy_hash":policy.get("hash").cloned().unwrap_or(Value::Null),
                    },
                    "diagnostics_count":diagnostics.len(),
                }),
                Value::Array(codes),
                json!({
                    "authored":evidence_or_empty(authored),
                    "derived":evidence_or_empty(derived),
                    "proposed":evidence_or_empty(proposed),
                    "approved":evidence_or_empty(approved),
                    "effective":evidence_or_empty(effective),
                }),
                Some((
                    policy.get("id").cloned().unwrap_or(Value::Null),
                    policy.get("version").cloned().unwrap_or(Value::Null),
                    policy.get("hash").cloned().unwrap_or(Value::Null),
                )),
            )
        } else {
            (Value::Null, json!([]), Value::Null, None)
        };
        let projection_content_hash = projection
            .and_then(|projection| graphiti_nonnull(projection, "contentHash"))
            .or_else(|| graphiti_nonnull(node, "contentHash"))
            .cloned()
            .unwrap_or(Value::Null);
        let (policy_id, integrity_policy_version, policy_hash) =
            integrity_policy.unwrap_or((Value::Null, Value::Null, Value::Null));
        let resolved_supersedes = gkx
            .and_then(|gkx| graphiti_nonnull(gkx, "supersedesIds"))
            .map(|value| {
                value
                    .as_array()
                    .ok_or(WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))?
                    .iter()
                    .map(|id| {
                        let id = id
                            .as_str()
                            .ok_or(WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))?;
                        Ok(Value::String(
                            labels.get(id).cloned().unwrap_or_else(|| id.to_owned()),
                        ))
                    })
                    .collect::<WatcherResult<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        let declared_supersedes = gkx
            .and_then(|gkx| graphiti_nonnull(gkx, "supersedes"))
            .cloned()
            .unwrap_or_else(|| json!([]));
        let body = json!({
            "schema":"gkx-graphiti/2.3.0",
            "profile":"gkx-2.3-validating-projection",
            "adapter":"Gkx Governed Context Projection",
            "title":title,
            "path":path,
            "uid":uid,
            "type":note_type,
            "description":gkx.and_then(|gkx| graphiti_nonnull(gkx,"description")).cloned().unwrap_or(Value::Null),
            "tags":tags,
            "labels":labels_body,
            "event_time":event_time,
            "processing_time":processing_time,
            "reference_time_source":reference_time_source,
            "episode_metadata":metadata,
            "authority":{
                "class":source_origin,
                "governance_status":"unadjudicated",
                "projection_status":"non_authoritative",
                "accepted_semantics":false,
                "origin_separation_preserved":true,
            },
            "governance":governance,
            "diagnostic_codes":diagnostic_codes,
            "evidence":evidence,
            "integrity":{
                "content_hash":projection_content_hash,
                "hash_algorithm":"fnv1a32-with-length",
                "policy_id":policy_id,
                "policy_version":integrity_policy_version,
                "policy_hash":policy_hash,
                "schema_id":"gkx-graphiti",
                "schema_version":"2.3.0",
                "schema_hash":format!("fnv1a32-with-length:{}",graphiti_content_hash("gkx-graphiti/2.3.0")),
            },
            "lineage":{"resolved_supersedes":resolved_supersedes,"declared_supersedes":declared_supersedes},
            "related_to":semantic,
            "saga":null,
        });
        let body = graphiti_bounded_json(&body, 250)
            .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))?;
        let episode_uid = uid.as_str().filter(|uid| graphiti_authored_uuid(uid));
        let episode_uuid = episode_uid
            .map(str::to_owned)
            .unwrap_or_else(|| graphiti_uuid(format!("{vault_id}\0{path}").as_str()));
        episodes.push(json!({
            "uuid":episode_uuid,
            "name":title,
            "episode_body":body,
            "source":"json",
            "source_description":format!("GKX origin-separated source projection ({source_origin}) · KGCP non-authoritative Graphiti adapter · workspace \"{vault_id}\" · {path}"),
            "reference_time":event_time,
            "group_id":group_id,
            "episode_metadata":metadata,
        }));
        let relationships = projection
            .and_then(|projection| {
                graphiti_required_object(projection, "effective")
                    .ok()
                    .and_then(|effective| graphiti_nonnull(effective, "relationships"))
            })
            .or_else(|| gkx.and_then(|gkx| graphiti_nonnull(gkx, "relations")));
        let mut effective_relationships = Vec::<(String, String)>::new();
        if let Some(relationships) = relationships {
            let relationships = relationships
                .as_object()
                .ok_or(WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))?;
            let mut entries = relationships.iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| graphiti_object_key_order(left, right));
            for (relation, targets) in entries {
                let targets = targets
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_else(|| std::slice::from_ref(targets));
                for target in targets {
                    let target = target
                        .as_str()
                        .or_else(|| target.as_object().and_then(|target| text(target, "target")));
                    if let Some(target) = target {
                        let item = (relation.clone(), target.to_owned());
                        if !effective_relationships.contains(&item) {
                            effective_relationships.push(item);
                        }
                    }
                }
            }
        }
        let subject_uid = uid
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| episode_uuid.clone());
        for (relation, target) in effective_relationships {
            let triple_metadata = {
                let mut metadata = metadata.as_object().expect("metadata object").clone();
                metadata.insert("episode_kind".to_owned(), json!("fact_triple"));
                metadata.insert("relationship".to_owned(), json!(relation));
                Value::Object(metadata)
            };
            let triple_body = canonical_json(&json!({
                "schema":"gkx-graphiti/2.3.0",
                "subject_uid":subject_uid,
                "subject":title,
                "predicate":relation,
                "object_ref":target,
                "origin":"effective-non-proposed-projection",
                "event_time":event_time,
                "processing_time":processing_time,
            }))
            .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))?;
            episodes.push(json!({
                "uuid":graphiti_uuid(format!("{subject_uid}\0{relation}\0{target}").as_str()),
                "name":format!("{title} {relation} {target}"),
                "episode_body":triple_body,
                "source":"fact_triple",
                "source_description":format!("Effective non-proposed GKX relationship projection from {path}"),
                "reference_time":event_time,
                "group_id":group_id,
                "episode_metadata":triple_metadata,
            }));
        }
    }
    episodes.sort_by(|left, right| {
        utf16_cmp(
            left["reference_time"].as_str().unwrap_or_default(),
            right["reference_time"].as_str().unwrap_or_default(),
        )
        .then_with(|| {
            utf16_cmp(
                left["uuid"].as_str().unwrap_or_default(),
                right["uuid"].as_str().unwrap_or_default(),
            )
        })
    });
    Ok(json!({
        "contract_version":"gkos-watcher-graphiti-projection/1.0.0-draft.1",
        "processing_time":processing_time,
        "episodes":episodes,
    }))
}

fn graphiti_matches_raw(
    graphiti: &Value,
    raw_graph: &Value,
    vault_id: &Value,
) -> WatcherResult<bool> {
    Ok(*graphiti == derive_graphiti(raw_graph, vault_id)?)
}

fn seal_event_set_bundle(value: &Value) -> WatcherResult<Value> {
    let bundle = object(value)?;
    exact_keys(
        bundle,
        &[
            "event_set",
            "memberships",
            "prior_memberships",
            "events",
            "prior_events",
            "occurrences",
            "prior_occurrences",
        ],
    )?;
    let event_set = seal_record(&bundle["event_set"])?;
    let reset_carry = event_set["set_kind"] == "reset_carry";
    let n = event_set["event_count"].as_u64().unwrap_or_default() as usize;
    let memberships_raw = array(&bundle["memberships"])?;
    let events_raw = array(&bundle["events"])?;
    let occurrences_raw = array(&bundle["occurrences"])?;
    let prior_memberships_raw = array(&bundle["prior_memberships"])?;
    let prior_events_raw = array(&bundle["prior_events"])?;
    let prior_occurrences_raw = array(&bundle["prior_occurrences"])?;
    if [
        memberships_raw.len(),
        events_raw.len(),
        occurrences_raw.len(),
        prior_memberships_raw.len(),
        prior_events_raw.len(),
        prior_occurrences_raw.len(),
    ]
    .iter()
    .any(|length| *length != n)
        || memberships_raw
            .iter()
            .chain(events_raw)
            .chain(occurrences_raw)
            .any(Value::is_null)
    {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    }
    let memberships = memberships_raw
        .iter()
        .map(seal_record)
        .collect::<WatcherResult<Vec<_>>>()?;
    let events = events_raw
        .iter()
        .map(seal_record)
        .collect::<WatcherResult<Vec<_>>>()?;
    let occurrences = occurrences_raw
        .iter()
        .map(seal_record)
        .collect::<WatcherResult<Vec<_>>>()?;
    let prior_memberships = if reset_carry {
        prior_memberships_raw
            .iter()
            .map(seal_record)
            .collect::<WatcherResult<Vec<_>>>()?
    } else {
        if prior_memberships_raw
            .iter()
            .chain(prior_events_raw)
            .chain(prior_occurrences_raw)
            .any(|item| !item.is_null())
        {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
        prior_memberships_raw.clone()
    };
    let prior_events = if reset_carry {
        prior_events_raw
            .iter()
            .map(seal_record)
            .collect::<WatcherResult<Vec<_>>>()?
    } else {
        prior_events_raw.clone()
    };
    let prior_occurrences = if reset_carry {
        prior_occurrences_raw
            .iter()
            .map(seal_record)
            .collect::<WatcherResult<Vec<_>>>()?
    } else {
        prior_occurrences_raw.clone()
    };
    let membership_digests = memberships
        .iter()
        .map(|membership| membership["membership_digest"].clone())
        .collect::<Vec<_>>();
    let expected_sequence = canonical_digest(&json!({
        "contract_version":"gkos-watcher-source-removal-membership-sequence/1.0.0-draft.1",
        "membership_digests":membership_digests,
    }))
    .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID"))?;
    if event_set["membership_digest_sequence_digest"] != expected_sequence
        || n == 0
        || memberships
            .iter()
            .map(|row| row["membership_digest"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len()
            != n
        || events
            .iter()
            .map(|row| row["event_digest"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len()
            != n
        || occurrences
            .iter()
            .map(|row| row["occurrence_digest"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len()
            != n
    {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    }
    let mut prior_order: Option<String> = None;
    for index in 0..n {
        let membership = &memberships[index];
        let event = &events[index];
        let occurrence = &occurrences[index];
        if membership["event_ordinal"] != (index + 1)
            || membership["event_digest"] != event["event_digest"]
            || event["occurrence_digest"] != occurrence["occurrence_digest"]
            || !reset_carry && membership["prepared_at"] != event_set["prepared_at"]
        {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
        if !reset_carry {
            if !membership["original_membership_digest"].is_null()
                || membership["causal_batch_id"] != event_set["origin_id"]
                || membership["target_topology_snapshot_digest"]
                    != event_set["target_topology_snapshot_digest"]
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
        } else if !is_digest(&membership["original_membership_digest"])
            || membership["original_membership_digest"]
                != prior_memberships[index]["membership_digest"]
            || membership["event_digest"] != prior_memberships[index]["event_digest"]
            || membership["causal_batch_id"] != prior_memberships[index]["causal_batch_id"]
            || membership["target_topology_snapshot_digest"]
                != prior_memberships[index]["target_topology_snapshot_digest"]
            || membership["prepared_at"] != prior_memberships[index]["prepared_at"]
            || events[index] != prior_events[index]
            || occurrences[index] != prior_occurrences[index]
        {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
        let key = format!(
            "{}\0{}\0{}",
            occurrence["source_path"].as_str().unwrap_or_default(),
            occurrence["occurrence_digest"].as_str().unwrap_or_default(),
            membership["original_membership_digest"]
                .as_str()
                .unwrap_or_default()
        );
        if prior_order
            .as_deref()
            .is_some_and(|prior| utf16_cmp(prior, &key) != Ordering::Less)
        {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
        prior_order = Some(key);
    }
    Ok(value.clone())
}

fn seal_adapter_verification_bundle(value: &Value) -> WatcherResult<Value> {
    let bundle = object(value)?;
    exact_keys(
        bundle,
        &["scope", "binding", "challenge", "proof", "verification"],
    )?;
    let scope = seal_record(&bundle["scope"])?;
    let binding = seal_record(&bundle["binding"])?;
    let challenge = seal_record(&bundle["challenge"])?;
    let proof = seal_record(&bundle["proof"])?;
    let verification = seal_record(&bundle["verification"])?;
    for field in [
        "authorization_binding_digest",
        "adapter_kind",
        "adapter_id",
        "adapter_contract_version",
        "vault_id",
        "authority_namespace",
        "configuration_digest",
        "policy_digest",
    ] {
        if binding[field] != scope[field] {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
    }
    if challenge["vault_id"] != binding["vault_id"]
        || challenge["configuration_digest"] != binding["configuration_digest"]
        || challenge["policy_digest"] != binding["policy_digest"]
        || challenge["required_capabilities"] != binding["capabilities"]
        || proof["challenge_digest"] != challenge["challenge_digest"]
        || proof["binding_digest"] != binding["binding_digest"]
        || proof["adapter_kind"] != binding["adapter_kind"]
        || proof["adapter_id"] != binding["adapter_id"]
        || proof["adapter_contract_version"] != binding["adapter_contract_version"]
        || proof["authority_namespace"] != binding["authority_namespace"]
        || proof["authorization_binding_digest"] != binding["authorization_binding_digest"]
        || proof["capabilities"] != binding["capabilities"]
        || verification["binding_digest"] != binding["binding_digest"]
        || verification["challenge_digest"] != challenge["challenge_digest"]
        || verification["proof_digest"] != proof["proof_digest"]
    {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    }
    Ok(value.clone())
}

fn seal_receipt_bundle(value: &Value) -> WatcherResult<Value> {
    let bundle = object(value)?;
    exact_keys(
        bundle,
        &[
            "binding",
            "event_set_bundle",
            "activation",
            "selected_event_ordinal",
            "request",
            "response",
            "receipt",
        ],
    )?;
    if [
        "binding",
        "event_set_bundle",
        "activation",
        "request",
        "response",
        "receipt",
    ]
    .iter()
    .any(|field| bundle[*field].is_null())
    {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    }
    let binding = seal_record(&bundle["binding"])?;
    let event_bundle = seal_event_set_bundle(&bundle["event_set_bundle"])?;
    let event_bundle = object(&event_bundle)?;
    let events = array(&event_bundle["events"])?;
    let Some(ordinal) = unsigned(bundle, "selected_event_ordinal") else {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    };
    if ordinal == 0 || ordinal as usize > events.len() {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    }
    let index = ordinal as usize - 1;
    let occurrence = &array(&event_bundle["occurrences"])?[index];
    let event = &events[index];
    let membership = &array(&event_bundle["memberships"])?[index];
    let event_set = &event_bundle["event_set"];
    let activation = seal_record(&bundle["activation"])?;
    let request = seal_record(&bundle["request"])?;
    let response = seal_record(&bundle["response"])?;
    let receipt = seal_record(&bundle["receipt"])?;
    if event["occurrence_digest"] != occurrence["occurrence_digest"]
        || event["adapter_binding_digest"] != binding["binding_digest"]
        || membership["event_digest"] != event["event_digest"]
        || membership["event_ordinal"] != ordinal
        || activation["event_set_digest"] != event_set["event_set_digest"]
        || request["binding_digest"] != binding["binding_digest"]
        || request["occurrence_digest"] != occurrence["occurrence_digest"]
        || request["idempotency_key"] != occurrence["occurrence_digest"]
        || request["source_id"] != occurrence["source_id"]
        || request["source_path"] != occurrence["source_path"]
        || request["source_digest"] != occurrence["source_digest"]
        || request["prior_coherent_manifest_digest"] != occurrence["prior_coherent_manifest_digest"]
        || request["target_topology_snapshot_digest"]
            != membership["target_topology_snapshot_digest"]
        || request["observed_at"] != membership["prepared_at"]
        || response["binding_digest"] != binding["binding_digest"]
        || response["occurrence_digest"] != occurrence["occurrence_digest"]
        || receipt["event_digest"] != event["event_digest"]
        || receipt["occurrence_digest"] != occurrence["occurrence_digest"]
        || receipt["adapter_binding_digest"] != binding["binding_digest"]
        || receipt["adapter_response_digest"] != response["response_digest"]
        || receipt["adapter_result_digest"] != response["adapter_result_digest"]
        || receipt["adapter_event_id"] != response["adapter_event_id"]
        || receipt["status"] != response["status"]
    {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    }
    Ok(value.clone())
}

fn seal_status_bundle(value: &Value) -> WatcherResult<Value> {
    let bundle = object(value)?;
    exact_keys(bundle, &["locator", "status", "active", "manifest"])?;
    let locator = seal_record(&bundle["locator"])?;
    let status = seal_record(&bundle["status"])?;
    if status["service_instance_id"] != locator["service_instance_id"]
        || status["pid"] != locator["pid"]
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    if bundle["active"].is_null() || bundle["manifest"].is_null() {
        if !bundle["active"].is_null()
            || !bundle["manifest"].is_null()
            || [
                "source_snapshot_digest",
                "coherent_manifest_digest",
                "configuration_digest",
                "policy_digest",
                "last_sync",
            ]
            .iter()
            .any(|field| !status[*field].is_null())
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
        return Ok(value.clone());
    }
    let active = seal_record(&bundle["active"])?;
    let manifest = seal_record(&bundle["manifest"])?;
    if status["source_snapshot_digest"] != manifest["source_observation_snapshot_digest"]
        || status["coherent_manifest_digest"] != manifest["coherent_manifest_digest"]
        || status["configuration_digest"] != manifest["configuration_digest"]
        || status["policy_digest"] != manifest["policy_digest"]
        || status["last_sync"] != active["activated_at"]
        || active["coherent_manifest_digest"] != manifest["coherent_manifest_digest"]
        || active["service_generation_id"] != manifest["service_generation_id"]
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(value.clone())
}

fn seal_failure_retry_bundle(value: &Value) -> WatcherResult<Value> {
    let inner = || -> WatcherResult<Value> {
        let bundle = object(value)?;
        exact_keys(
            bundle,
            &[
                "failed_batch",
                "failed_observation",
                "failed_observation_authority",
                "failed_pre_scan_state",
                "failed_transitions",
                "retry_batch",
                "retry_observation",
                "retry_observation_authority",
                "retry_pre_scan_state",
            ],
        )?;
        let failed_batch = seal_record(&bundle["failed_batch"])?;
        let failed_observation = seal_record(&bundle["failed_observation"])?;
        let failed_authority = seal_record(&bundle["failed_observation_authority"])?;
        let failed_pre_scan = seal_record(&bundle["failed_pre_scan_state"])?;
        let failed_transitions = seal_transition_sequence(&bundle["failed_transitions"], true)?;
        let failed_transition = array(&failed_transitions)?.last().unwrap();
        let retry_batch = seal_record(&bundle["retry_batch"])?;
        let retry_observation = seal_record(&bundle["retry_observation"])?;
        let retry_authority = seal_record(&bundle["retry_observation_authority"])?;
        let retry_pre_scan = seal_record(&bundle["retry_pre_scan_state"])?;
        let failed_coord = artifact_coordinate("observation", &failed_observation)?;
        let retry_coord = artifact_coordinate("observation", &retry_observation)?;
        if failed_transition["batch_id"] != failed_batch["batch_id"]
            || failed_transition["state"] != "failed"
            || failed_transition["terminal_state"] != "failed"
            || failed_batch["batch_id"] != failed_observation["batch_id"]
            || failed_batch["observation_authority_digest"] != failed_authority["authority_digest"]
            || failed_transition["observation_digest"] != failed_observation["observation_digest"]
            || failed_authority["observation_digest"] != failed_observation["observation_digest"]
            || failed_authority["pre_scan_state_digest"]
                != canonical_digest(&failed_pre_scan).unwrap()
            || failed_authority["observation_artifact_file"] != failed_coord["file"]
            || failed_authority["observation_raw_sha256"] != failed_coord["raw_sha256"]
            || failed_authority["observation_byte_size"] != failed_coord["byte_size"]
            || failed_authority["started_at"] != failed_observation["started_at"]
            || retry_batch["batch_kind"] != "failure_reconciliation"
            || retry_batch["execution_kind"] != "set_files"
            || retry_batch["retry_of_batch_id"] != failed_batch["batch_id"]
            || retry_batch["batch_id"] != retry_observation["batch_id"]
            || retry_observation["batch_kind"] != "failure_reconciliation"
            || retry_observation["unscoped"] != true
            || retry_batch["observation_authority_digest"] != retry_authority["authority_digest"]
            || retry_authority["observation_digest"] != retry_observation["observation_digest"]
            || retry_authority["pre_scan_state_digest"]
                != canonical_digest(&retry_pre_scan).unwrap()
            || retry_authority["observation_artifact_file"] != retry_coord["file"]
            || retry_authority["observation_raw_sha256"] != retry_coord["raw_sha256"]
            || retry_authority["observation_byte_size"] != retry_coord["byte_size"]
            || retry_authority["started_at"] != retry_observation["started_at"]
            || retry_pre_scan != failed_pre_scan
        {
            return fail("GKX_WATCHER_CONTRACT_RETRY_INVALID");
        }
        Ok(value.clone())
    };
    inner().map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_RETRY_INVALID"))
}

fn seal_coherent_activation_bundle(value: &Value, guard_value: &Value) -> WatcherResult<Value> {
    let bundle = object(value)?;
    exact_keys(
        bundle,
        &[
            "batch",
            "observation",
            "observation_authority",
            "pre_scan_state",
            "plan",
            "plan_authority",
            "topology",
            "transitions",
            "normalized_graph_delta",
            "canonical_graph",
            "raw_graph",
            "graphiti_projection",
            "manifest",
            "pointer",
            "intent",
            "outcome",
            "active",
            "source_removal_event_set_bundle",
            "source_removal_activation",
        ],
    )?;
    let batch = seal_record(&bundle["batch"])?;
    let observation = seal_record(&bundle["observation"])?;
    let observation_authority = seal_record(&bundle["observation_authority"])?;
    let pre_scan = seal_record(&bundle["pre_scan_state"])?;
    let plan = seal_record(&bundle["plan"])?;
    let plan_authority = seal_record(&bundle["plan_authority"])?;
    let topology = seal_record(&bundle["topology"])?;
    let transition_input = array(&bundle["transitions"])?;
    let prepared_only = transition_input
        .last()
        .and_then(Value::as_object)
        .and_then(|row| text(row, "state"))
        == Some("activation_prepared");
    let transitions = seal_transition_sequence(&bundle["transitions"], !prepared_only)?;
    let transitions_array = array(&transitions)?;
    if transitions_array.len() != if prepared_only { 6 } else { 7 } {
        return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
    }
    let delta = seal_record(&bundle["normalized_graph_delta"])?;
    let canonical_graph = seal_record(&bundle["canonical_graph"])?;
    let raw_graph = seal_record(&bundle["raw_graph"])?;
    let graphiti = seal_record(&bundle["graphiti_projection"])?;
    let manifest = seal_record(&bundle["manifest"])?;
    let pointer = seal_record(&bundle["pointer"])?;
    let intent = seal_record(&bundle["intent"])?;
    let outcome = if prepared_only {
        if !bundle["outcome"].is_null() {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
        None
    } else {
        Some(seal_record(&bundle["outcome"])?)
    };
    let active = if prepared_only {
        if !bundle["active"].is_null() {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
        None
    } else {
        Some(seal_record(&bundle["active"])?)
    };
    let removal_count = unsigned(object(&plan_authority)?, "source_removal_event_count")
        .unwrap_or_default() as usize;
    let removal_bundle = if removal_count == 0 {
        if !bundle["source_removal_event_set_bundle"].is_null() {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
        None
    } else {
        Some(seal_event_set_bundle(
            &bundle["source_removal_event_set_bundle"],
        )?)
    };
    let removal_activation = if prepared_only || removal_count == 0 {
        if !bundle["source_removal_activation"].is_null() {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
        None
    } else {
        Some(seal_record(&bundle["source_removal_activation"])?)
    };
    let guard = seal_record(guard_value)?;
    let observation_coordinate = artifact_coordinate("observation", &observation)?;
    let plan_coordinate = artifact_coordinate("plan", &plan)?;
    let topology_coordinate = artifact_coordinate("topology", &topology)?;
    let raw_graph_coordinate = artifact_coordinate("graph", &raw_graph)?;
    let pre_scan_digest = canonical_digest(&pre_scan)
        .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_RELATION_INVALID"))?;
    let complete = if prepared_only {
        seal_record(&intent["target_complete_transition"])?
    } else {
        transitions_array.last().unwrap().clone()
    };
    let retrieval_state = &complete["retrieval_projection_state"];
    let graph_state = &complete["graph_projection_state"];
    let expected_execution = if matches!(
        observation["batch_kind"].as_str(),
        Some("startup_reconciliation" | "failure_reconciliation")
    ) || observation["unscoped"] == true
        || observation["overflow"] == true
    {
        "set_files"
    } else {
        "apply_changes"
    };
    let mutation_set_digest = canonical_digest(&json!({
        "contract_version":"gkos-watcher-mutation-set/1.0.0-draft.1",
        "pre_scan_state_digest":pre_scan_digest,
        "topology_snapshot_digest":plan["topology_snapshot_digest"],
        "intended_source_mutations":plan["intended_source_mutations"],
        "folder_set_changed":plan["folder_set_changed"],
        "attachment_set_changed":plan["attachment_set_changed"],
    }))
    .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_RELATION_INVALID"))?;
    if batch["batch_id"] != observation["batch_id"]
        || batch["batch_kind"] != observation["batch_kind"]
        || batch["started_at"] != observation["started_at"]
        || batch["execution_kind"] != expected_execution
        || batch["observation_authority_digest"] != observation_authority["authority_digest"]
        || observation_authority["batch_id"] != observation["batch_id"]
        || observation_authority["observation_digest"] != observation["observation_digest"]
        || observation_authority["started_at"] != observation["started_at"]
        || observation_authority["observation_artifact_file"] != observation_coordinate["file"]
        || observation_authority["observation_raw_sha256"] != observation_coordinate["raw_sha256"]
        || observation_authority["observation_byte_size"] != observation_coordinate["byte_size"]
        || observation_authority["pre_scan_state_digest"] != pre_scan_digest
        || plan["batch_id"] != batch["batch_id"]
        || plan["observation_digest"] != observation["observation_digest"]
        || plan["mutation_set_digest"] != mutation_set_digest
        || plan["effective_profile_digest"] != pre_scan["effective_profile_digest"]
        || plan["validation_result_digest"] != topology["validation_result_digest"]
        || plan["rejection_journal_digest"] != topology["rejection_journal_digest"]
        || plan_authority["batch_id"] != batch["batch_id"]
        || plan_authority["observation_digest"] != observation["observation_digest"]
        || plan_authority["plan_digest"] != plan["plan_digest"]
        || plan_authority["plan_artifact_file"] != plan_coordinate["file"]
        || plan_authority["plan_raw_sha256"] != plan_coordinate["raw_sha256"]
        || plan_authority["plan_byte_size"] != plan_coordinate["byte_size"]
        || plan_authority["target_topology_snapshot_digest"] != plan["topology_snapshot_digest"]
        || plan["topology_snapshot_digest"] != topology["topology_snapshot_digest"]
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    if let Some(removal_bundle) = &removal_bundle {
        let event_set = &removal_bundle["event_set"];
        if event_set["event_set_digest"] != plan_authority["source_removal_event_set_digest"]
            || event_set["event_count"] != removal_count
            || event_set["set_kind"] != "batch"
            || event_set["origin_id"] != batch["batch_id"]
            || event_set["target_topology_snapshot_digest"] != topology["topology_snapshot_digest"]
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
    }
    let delta_digest = canonical_digest(&delta).unwrap();
    for (index, transition) in transitions_array.iter().enumerate() {
        if transition["batch_id"] != batch["batch_id"]
            || transition["observation_digest"] != observation["observation_digest"]
            || index >= 1 && transition["plan_digest"] != plan["plan_digest"]
            || index >= 2 && transition["gkx_delta_digest"] != delta_digest
        {
            return fail("GKX_WATCHER_CONTRACT_TRANSITION_INVALID");
        }
    }
    if array(&plan["intended_source_mutations"])?.is_empty()
        && plan["folder_set_changed"] == false
        && plan["attachment_set_changed"] == false
        && topology["topology_snapshot_digest"] == pre_scan["topology_snapshot_digest"]
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    let expected_canonical = normalize_raw_graph(&raw_graph["graph"])?;
    let raw = object(&raw_graph["graph"])?;
    let raw_paths = array(&raw["nodes"])?
        .iter()
        .filter_map(|node| {
            let node = node.as_object()?;
            if node.get("kind")?.as_str()? == "file" {
                node.get("path").cloned()
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let accepted_paths = array(&topology["accepted_sources"])?
        .iter()
        .map(|source| source["source_path"].clone())
        .collect::<Vec<_>>();
    let mut sorted_raw = raw_paths.clone();
    let mut sorted_accepted = accepted_paths.clone();
    sorted_raw.sort_by(|a, b| utf16_cmp(a.as_str().unwrap(), b.as_str().unwrap()));
    sorted_accepted.sort_by(|a, b| utf16_cmp(a.as_str().unwrap(), b.as_str().unwrap()));
    let stats = object(&raw["stats"])?;
    let diagnostics = object(&raw["diagnostics"])?;
    if manifest["completed_batch_id"] != batch["batch_id"]
        || manifest["vault_id"] != pre_scan["vault_id"]
        || manifest["configuration_digest"] != pre_scan["configuration_digest"]
        || manifest["policy_digest"] != pre_scan["policy_digest"]
        || manifest["effective_profile_digest"] != pre_scan["effective_profile_digest"]
        || manifest["validation_result_digest"] != topology["validation_result_digest"]
        || manifest["rejection_journal_digest"] != topology["rejection_journal_digest"]
        || manifest["source_observation_snapshot_digest"]
            != topology["source_observation_snapshot_digest"]
        || manifest["topology_snapshot_digest"] != topology["topology_snapshot_digest"]
        || manifest["topology_artifact_file"] != topology_coordinate["file"]
        || manifest["topology_artifact_raw_sha256"] != topology_coordinate["raw_sha256"]
        || manifest["completed_transition_digest"] != complete["transition_digest"]
        || raw_graph["service_generation_id"] != manifest["service_generation_id"]
        || raw_graph["topology_snapshot_digest"] != topology["topology_snapshot_digest"]
        || sorted_raw != sorted_accepted
        || stats["files"] != accepted_paths.len()
        || diagnostics["notes"] != accepted_paths.len()
        || diagnostics["attachments"] != array(&topology["attachment_paths"])?.len()
        || canonical_graph != expected_canonical
        || !graphiti_matches_raw(&graphiti, &raw_graph["graph"], &pre_scan["vault_id"])?
        || graph_state["graph_artifact_file"] != raw_graph_coordinate["file"]
        || graph_state["graph_artifact_digest"] != raw_graph["graph_artifact_digest"]
        || graph_state["canonical_graph_digest"] != canonical_digest(&canonical_graph).unwrap()
        || graph_state["gkx_delta_digest"] != delta_digest
        || graph_state["graphiti_projection_digest"] != canonical_digest(&graphiti).unwrap()
        || manifest["retrieval_projection_state"] != *retrieval_state
        || manifest["graph_projection_state"] != *graph_state
        || manifest["gkx_snapshot_digest"] != complete["gkx_snapshot_digest"]
        || manifest["source_removal_event_count"] != removal_count
        || manifest["source_removal_event_set_digest"]
            != plan_authority["source_removal_event_set_digest"]
        || pointer["service_generation_id"] != manifest["service_generation_id"]
        || pointer["coherent_manifest_digest"] != manifest["coherent_manifest_digest"]
        || pointer["prior_pointer_digest"] != pre_scan["active_pointer_digest"]
        || intent["prepared_transition_digest"] != transitions_array[5]["transition_digest"]
        || intent["coherent_manifest_digest"] != manifest["coherent_manifest_digest"]
        || intent["prior_pointer_digest"] != pre_scan["active_pointer_digest"]
        || intent["target_pointer"] != pointer
        || intent["target_complete_transition"] != complete
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    if !prepared_only {
        let outcome = outcome.as_ref().unwrap();
        let active = active.as_ref().unwrap();
        if outcome["intent_digest"] != intent["intent_digest"]
            || outcome["coherent_manifest_digest"] != manifest["coherent_manifest_digest"]
            || outcome["outcome"] != "published"
            || outcome["pointer_digest"] != pointer["pointer_digest"]
            || active["service_generation_id"] != manifest["service_generation_id"]
            || active["coherent_manifest_digest"] != manifest["coherent_manifest_digest"]
            || active["pointer_digest"] != pointer["pointer_digest"]
            || active["intent_digest"] != intent["intent_digest"]
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
    }
    if let Some(removal_activation) = &removal_activation {
        let active = active.as_ref().unwrap();
        if removal_activation["event_set_digest"]
            != plan_authority["source_removal_event_set_digest"]
            || removal_activation["coherent_manifest_digest"]
                != manifest["coherent_manifest_digest"]
            || removal_activation["activated_at"] != active["activated_at"]
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
    }
    let pointer_bytes = canonical_bytes(&pointer)?;
    if guard["operation"] != "replace_watcher_active_pointer"
        || guard["operation_intent_digest"] != intent["intent_digest"]
        || guard["target_commit_digest"] != complete["transition_digest"]
        || guard["new_pointer_file"]
            != format!(
                "watcher-pointer-{}.json",
                &pointer["pointer_digest"].as_str().unwrap()[7..]
            )
        || guard["new_pointer_digest"] != pointer["pointer_digest"]
        || guard["new_pointer_raw_sha256"] != sha256(&pointer_bytes)
        || guard["new_pointer_byte_size"] != pointer_bytes.len()
        || if pre_scan["active_pointer_digest"].is_null() {
            !guard["old_pointer_digest"].is_null()
        } else {
            guard["old_pointer_digest"] != pre_scan["active_pointer_digest"]
        }
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    let accepted_by_path = array(&topology["accepted_sources"])?
        .iter()
        .map(|row| (row["source_path"].as_str().unwrap(), row))
        .collect::<BTreeMap<_, _>>();
    let rejected_by_path = array(&topology["rejected_sources"])?
        .iter()
        .map(|row| (row["source_path"].as_str().unwrap(), row))
        .collect::<BTreeMap<_, _>>();
    let mutations = array(&plan["intended_source_mutations"])?;
    for mutation in mutations {
        let kind = mutation["kind"].as_str().unwrap();
        if matches!(kind, "add" | "change" | "rename") {
            let target = accepted_by_path.get(mutation["to_path"].as_str().unwrap());
            if target.is_none_or(|target| {
                target["source_id"] != mutation["source_id_after"]
                    || target["source_digest"] != mutation["source_digest_after"]
                    || target["parser_descriptor_digest"]
                        != mutation["parser_descriptor_digest_after"]
            }) {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
        if matches!(kind, "delete" | "rename")
            && accepted_by_path
                .get(mutation["from_path"].as_str().unwrap())
                .is_some_and(|target| target["source_id"] == mutation["source_id_before"])
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
        if mutation["cause"] == "validation_rejection" {
            let rejected = rejected_by_path.get(mutation["from_path"].as_str().unwrap());
            if rejected.is_none_or(|rejected| {
                !rejected["source_id"].is_null()
                    && rejected["source_id"] != mutation["source_id_before"]
                    || !rejected["source_digest"].is_null()
                        && rejected["source_digest"] != mutation["source_digest_before"]
                    || !rejected["parser_descriptor_digest"].is_null()
                        && rejected["parser_descriptor_digest"]
                            != mutation["parser_descriptor_digest_before"]
            }) {
                return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
            }
        }
    }
    let physical = mutations
        .iter()
        .filter(|mutation| {
            mutation["kind"] == "delete" && mutation["cause"] == "physical_disappearance"
        })
        .collect::<Vec<_>>();
    let occurrences = removal_bundle
        .as_ref()
        .map(|bundle| array(&bundle["occurrences"]).unwrap().clone())
        .unwrap_or_default();
    if physical.len() != occurrences.len()
        || physical
            .iter()
            .zip(&occurrences)
            .any(|(mutation, occurrence)| {
                occurrence["source_id"] != mutation["source_id_before"]
                    || occurrence["source_path"] != mutation["from_path"]
                    || occurrence["source_digest"] != mutation["source_digest_before"]
                    || occurrence["prior_coherent_manifest_digest"]
                        != pre_scan["active_coherent_manifest_digest"]
                    || occurrence["prior_topology_snapshot_digest"]
                        != pre_scan["topology_snapshot_digest"]
            })
    {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    }
    Ok(value.clone())
}

#[derive(Clone)]
struct ReadyRemoval {
    membership: Value,
    event: Value,
    occurrence: Value,
}

fn old_journal_ready(value: &Value, outer: &str) -> WatcherResult<Vec<ReadyRemoval>> {
    let authority = object(value)?;
    exact_keys(
        authority,
        &["activated_event_set_bundles", "responses", "receipts"],
    )?;
    let activated = array(&authority["activated_event_set_bundles"])?;
    let response_values = array(&authority["responses"])?;
    let receipt_values = array(&authority["receipts"])?;
    let mut memberships = Vec::new();
    let mut prior_claims = Vec::new();
    let mut activated_sets = BTreeSet::new();
    let mut membership_outer = BTreeMap::<String, String>::new();
    let mut event_by_digest = BTreeMap::<String, Value>::new();
    let mut event_by_occurrence = BTreeMap::<String, Value>::new();
    let mut occurrence_by_digest = BTreeMap::<String, Value>::new();
    for candidate in activated {
        let wrapper = object(candidate)?;
        exact_keys(wrapper, &["event_set_bundle", "activation"])?;
        let event_bundle = seal_event_set_bundle(&wrapper["event_set_bundle"])?;
        let activation = seal_record(&wrapper["activation"])?;
        let event_set = &event_bundle["event_set"];
        let set_digest = event_set["event_set_digest"].as_str().unwrap().to_owned();
        if activation["event_set_digest"] != event_set["event_set_digest"]
            || !activated_sets.insert(set_digest)
        {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
        let rows = array(&event_bundle["memberships"])?;
        let events = array(&event_bundle["events"])?;
        let occurrences = array(&event_bundle["occurrences"])?;
        for index in 0..rows.len() {
            let membership = rows[index].clone();
            let event = events[index].clone();
            let occurrence = occurrences[index].clone();
            let event_digest = event["event_digest"].as_str().unwrap().to_owned();
            let occurrence_digest = event["occurrence_digest"].as_str().unwrap().to_owned();
            if event_by_digest
                .get(&event_digest)
                .is_some_and(|prior| prior != &event)
                || event_by_occurrence
                    .get(&occurrence_digest)
                    .is_some_and(|prior| prior != &event)
                || occurrence_by_digest
                    .get(&occurrence_digest)
                    .is_some_and(|prior| prior != &occurrence)
            {
                return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
            }
            event_by_digest.insert(event_digest, event.clone());
            event_by_occurrence.insert(occurrence_digest.clone(), event.clone());
            occurrence_by_digest.insert(occurrence_digest, occurrence.clone());
            membership_outer.insert(
                membership["membership_digest"].as_str().unwrap().to_owned(),
                activation["coherent_manifest_digest"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
            memberships.push(membership);
        }
        if event_set["set_kind"] == "reset_carry" {
            let prior_memberships = array(&event_bundle["prior_memberships"])?;
            let prior_events = array(&event_bundle["prior_events"])?;
            let prior_occurrences = array(&event_bundle["prior_occurrences"])?;
            for index in 0..prior_memberships.len() {
                prior_claims.push(ReadyRemoval {
                    membership: prior_memberships[index].clone(),
                    event: prior_events[index].clone(),
                    occurrence: prior_occurrences[index].clone(),
                });
            }
        }
    }
    let mut membership_by_digest = BTreeMap::<String, Value>::new();
    for membership in &memberships {
        let digest = membership["membership_digest"].as_str().unwrap().to_owned();
        if membership_by_digest
            .insert(digest, membership.clone())
            .is_some()
        {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
    }
    for claim in &prior_claims {
        if membership_by_digest.get(claim.membership["membership_digest"].as_str().unwrap())
            != Some(&claim.membership)
            || event_by_digest.get(claim.event["event_digest"].as_str().unwrap())
                != Some(&claim.event)
            || occurrence_by_digest.get(claim.occurrence["occurrence_digest"].as_str().unwrap())
                != Some(&claim.occurrence)
        {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
    }
    let responses = response_values
        .iter()
        .map(seal_record)
        .collect::<WatcherResult<Vec<_>>>()?;
    let receipts = receipt_values
        .iter()
        .map(seal_record)
        .collect::<WatcherResult<Vec<_>>>()?;
    let response_by_digest = responses
        .iter()
        .map(|row| {
            (
                row["response_digest"].as_str().unwrap().to_owned(),
                row.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut receipted_responses = BTreeSet::new();
    let mut delivered_events = BTreeSet::new();
    for receipt in &receipts {
        let response_digest = receipt["adapter_response_digest"].as_str().unwrap();
        let event_digest = receipt["event_digest"].as_str().unwrap();
        let Some(response) = response_by_digest.get(response_digest) else {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        };
        let Some(event) = event_by_digest.get(event_digest) else {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        };
        if receipt["occurrence_digest"] != event["occurrence_digest"]
            || response["occurrence_digest"] != event["occurrence_digest"]
            || response["binding_digest"] != event["adapter_binding_digest"]
            || receipt["adapter_binding_digest"] != event["adapter_binding_digest"]
            || receipt["adapter_result_digest"] != response["adapter_result_digest"]
            || receipt["adapter_event_id"] != response["adapter_event_id"]
            || receipt["status"] != response["status"]
            || !receipted_responses.insert(response_digest.to_owned())
            || !delivered_events.insert(event_digest.to_owned())
        {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
    }
    if receipted_responses.len() != responses.len() {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    }
    let referenced = memberships
        .iter()
        .filter_map(|membership| membership["original_membership_digest"].as_str())
        .collect::<BTreeSet<_>>();
    let mut terminal_by_event = BTreeMap::<String, Value>::new();
    for membership in &memberships {
        if referenced.contains(membership["membership_digest"].as_str().unwrap()) {
            continue;
        }
        let event = membership["event_digest"].as_str().unwrap().to_owned();
        if terminal_by_event
            .insert(event, membership.clone())
            .is_some()
        {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
    }
    let mut ready = Vec::new();
    for (event_digest, membership) in terminal_by_event {
        let event = event_by_digest.get(&event_digest).unwrap();
        if event["delivery_mode"] != "adapter"
            || delivered_events.contains(&event_digest)
            || membership_outer.get(membership["membership_digest"].as_str().unwrap())
                != Some(&outer.to_owned())
        {
            continue;
        }
        let Some(occurrence) =
            occurrence_by_digest.get(event["occurrence_digest"].as_str().unwrap())
        else {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        };
        ready.push(ReadyRemoval {
            membership,
            event: event.clone(),
            occurrence: occurrence.clone(),
        });
    }
    ready.sort_by(|left, right| {
        let key = |row: &ReadyRemoval| {
            format!(
                "{}\0{}\0{}",
                row.occurrence["source_path"].as_str().unwrap(),
                row.occurrence["occurrence_digest"].as_str().unwrap(),
                row.membership["membership_digest"].as_str().unwrap()
            )
        };
        utf16_cmp(&key(left), &key(right))
    });
    Ok(ready)
}

fn seal_journal_reset_bundle(
    value: &Value,
    old_authority: &Value,
    pointer_guard_value: &Value,
) -> WatcherResult<Value> {
    let bundle = object(value)?;
    exact_keys(
        bundle,
        &[
            "old_meta",
            "old_generation",
            "old_pointer",
            "archive",
            "reset",
            "guard",
            "new_meta",
            "new_generation",
            "target_pointer",
            "reset_carry_bundle",
        ],
    )?;
    let old_meta = seal_record(&bundle["old_meta"])?;
    let old_generation = seal_record(&bundle["old_generation"])?;
    let old_pointer = seal_record(&bundle["old_pointer"])?;
    let archive = seal_record(&bundle["archive"])?;
    let reset = seal_record(&bundle["reset"])?;
    let guard = seal_record(&bundle["guard"])?;
    let new_meta = seal_record(&bundle["new_meta"])?;
    let new_generation = seal_record(&bundle["new_generation"])?;
    let target_pointer = seal_record(&bundle["target_pointer"])?;
    let pointer_guard = seal_record(pointer_guard_value)?;
    let outer = reset["outer_coherent_manifest_digest"].as_str().unwrap();
    let ready = old_journal_ready(old_authority, outer)?;
    let mut carry_set = None;
    let mut carry_activation = None;
    if !bundle["reset_carry_bundle"].is_null() {
        let carry = object(&bundle["reset_carry_bundle"])?;
        exact_keys(carry, &["event_set_bundle", "activation"])?;
        let event_bundle = seal_event_set_bundle(&carry["event_set_bundle"])?;
        let activation = seal_record(&carry["activation"])?;
        let prior_memberships = array(&event_bundle["prior_memberships"])?;
        let events = array(&event_bundle["events"])?;
        let occurrences = array(&event_bundle["occurrences"])?;
        if ready.len() != prior_memberships.len()
            || ready.iter().enumerate().any(|(index, row)| {
                row.membership != prior_memberships[index]
                    || row.event != events[index]
                    || row.occurrence != occurrences[index]
            })
        {
            return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
        }
        carry_set = Some(event_bundle["event_set"].clone());
        carry_activation = Some(activation);
    } else if !ready.is_empty() {
        return fail("GKX_WATCHER_CONTRACT_SOURCE_REMOVAL_INVALID");
    }
    let old_bytes = canonical_bytes(&old_pointer)?;
    let target_bytes = canonical_bytes(&target_pointer)?;
    let carry_set_digest = carry_set
        .as_ref()
        .map(|set| set["event_set_digest"].clone());
    let carry_activation_digest = carry_activation
        .as_ref()
        .map(|activation| activation["activation_digest"].clone());
    let count = ready.len();
    if old_generation["journal_instance_id"] != old_meta["journal_instance_id"]
        || old_generation["meta_digest"] != old_meta["meta_digest"]
        || old_generation["anchor_coherent_manifest_digest"] != outer
        || old_meta["anchor_coherent_manifest_digest"] != outer
        || old_pointer["journal_generation_digest"] != old_generation["journal_generation_digest"]
        || archive["journal_instance_id"] != old_generation["journal_instance_id"]
        || archive["directory_leaf"] != old_generation["directory_leaf"]
        || archive["outer_coherent_manifest_digest"] != outer
        || new_generation["journal_instance_id"] != new_meta["journal_instance_id"]
        || new_generation["meta_digest"] != new_meta["meta_digest"]
        || new_generation["anchor_coherent_manifest_digest"] != outer
        || new_meta["anchor_coherent_manifest_digest"] != outer
        || [
            "vault_id",
            "configuration_digest",
            "policy_digest",
            "effective_profile_digest",
        ]
        .iter()
        .any(|field| new_meta[*field] != old_meta[*field])
        || target_pointer["journal_generation_digest"]
            != new_generation["journal_generation_digest"]
        || target_pointer["prior_pointer_digest"] != old_pointer["pointer_digest"]
        || reset["prior_journal_generation_digest"] != old_generation["journal_generation_digest"]
        || reset["archive_manifest_digest"] != archive["archive_manifest_digest"]
        || reset["new_journal_meta_digest"] != new_meta["meta_digest"]
        || reset["new_journal_generation_digest"] != new_generation["journal_generation_digest"]
        || reset["target_journal_pointer_digest"] != target_pointer["pointer_digest"]
        || guard["old_journal_pointer_digest"] != old_pointer["pointer_digest"]
        || guard["old_journal_generation_digest"] != old_generation["journal_generation_digest"]
        || guard["outer_coherent_manifest_digest"] != outer
        || guard["archive_manifest_digest"] != archive["archive_manifest_digest"]
        || guard["new_journal_instance_id"] != new_generation["journal_instance_id"]
        || guard["new_journal_directory_leaf"] != new_generation["directory_leaf"]
        || guard["new_journal_meta_digest"] != new_meta["meta_digest"]
        || guard["new_journal_generation_digest"] != new_generation["journal_generation_digest"]
        || guard["reset_digest"] != reset["reset_digest"]
        || guard["target_journal_pointer_digest"] != target_pointer["pointer_digest"]
        || reset["ready_event_count"] != count
        || guard["ready_event_count"] != count
        || reset["reset_carry_event_set_digest"] != carry_set_digest.clone().unwrap_or(Value::Null)
        || guard["reset_carry_event_set_digest"] != carry_set_digest.clone().unwrap_or(Value::Null)
        || reset["reset_carry_activation_digest"]
            != carry_activation_digest.clone().unwrap_or(Value::Null)
        || guard["reset_carry_activation_digest"]
            != carry_activation_digest.clone().unwrap_or(Value::Null)
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    if let (Some(set), Some(activation)) = (&carry_set, &carry_activation) {
        if set["set_kind"] != "reset_carry"
            || set["origin_id"] != reset["reset_id"]
            || set["event_count"] != count
            || !set["target_topology_snapshot_digest"].is_null()
            || activation["event_set_digest"] != set["event_set_digest"]
            || activation["coherent_manifest_digest"] != outer
        {
            return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
        }
    }
    if pointer_guard["operation"] != "replace_watcher_journal_pointer"
        || pointer_guard["parent_device"] != guard["parent_device"]
        || pointer_guard["parent_inode"] != guard["parent_inode"]
        || pointer_guard["parent_mode"] != guard["parent_mode"]
        || pointer_guard["old_pointer_file"]
            != format!(
                "watcher-journal-pointer-{}.json",
                &old_pointer["pointer_digest"].as_str().unwrap()[7..]
            )
        || pointer_guard["old_pointer_digest"] != old_pointer["pointer_digest"]
        || pointer_guard["old_pointer_raw_sha256"] != sha256(&old_bytes)
        || pointer_guard["old_pointer_byte_size"] != old_bytes.len()
        || pointer_guard["new_pointer_file"]
            != format!(
                "watcher-journal-pointer-{}.json",
                &target_pointer["pointer_digest"].as_str().unwrap()[7..]
            )
        || pointer_guard["new_pointer_digest"] != target_pointer["pointer_digest"]
        || pointer_guard["new_pointer_raw_sha256"] != sha256(&target_bytes)
        || pointer_guard["new_pointer_byte_size"] != target_bytes.len()
        || pointer_guard["operation_intent_digest"] != guard["guard_digest"]
        || pointer_guard["target_commit_digest"] != reset["reset_digest"]
    {
        return fail("GKX_WATCHER_CONTRACT_RELATION_INVALID");
    }
    Ok(value.clone())
}

fn validate_sql(value: &Value) -> WatcherResult<Value> {
    let recipe = object(value)?;
    let kind = text(recipe, "recipe_kind").unwrap_or_default();
    if matches!(kind, "pre_transaction" | "post_reopen") {
        exact_keys(
            recipe,
            &[
                "recipe_kind",
                "current_database_bytes",
                "blob_bytes",
                "mutated_rows",
                "wal_bytes",
                "shm_bytes",
            ],
        )?;
        for field in [
            "current_database_bytes",
            "blob_bytes",
            "mutated_rows",
            "wal_bytes",
            "shm_bytes",
        ] {
            if !is_integer(&recipe[field], 0, 9_007_199_254_740_991) {
                return fail("GKX_WATCHER_CONTRACT_SQL_INVALID");
            }
        }
        if kind == "pre_transaction" {
            let blob = unsigned(recipe, "blob_bytes").unwrap();
            let rows = unsigned(recipe, "mutated_rows").unwrap();
            if recipe["wal_bytes"] != 0
                || recipe["shm_bytes"] != 0
                || blob > 33_554_432
                || rows > 10_000
            {
                return fail("GKX_WATCHER_CONTRACT_SQL_INVALID");
            }
            let dirty = blob.div_ceil(4_096) + 4 * rows + 4_096;
            let projected = unsigned(recipe, "current_database_bytes").unwrap() + dirty * 4_096;
            let wal = 32 + dirty * 4_120;
            if projected > 2_048_000_000 || projected + wal + 67_108_864 > 4_294_967_296 {
                return fail("GKX_WATCHER_CONTRACT_SQL_INVALID");
            }
            return Ok(Value::Null);
        }
        if recipe["blob_bytes"] != 0
            || recipe["mutated_rows"] != 0
            || unsigned(recipe, "current_database_bytes").unwrap()
                + unsigned(recipe, "wal_bytes").unwrap()
                + unsigned(recipe, "shm_bytes").unwrap()
                > 4_294_967_296
        {
            return fail("GKX_WATCHER_CONTRACT_SQL_INVALID");
        }
        return Ok(Value::Null);
    }
    exact_keys(recipe, &["recipe_kind", "target", "mutation"])?;
    const KINDS: [&str; 10] = [
        "body_scalar",
        "column",
        "foreign_key",
        "identity",
        "index",
        "integrity",
        "outbox",
        "pragma",
        "reset",
        "sqlite_master",
    ];
    const MUTATIONS: [&str; 22] = [
        "affinity_drift",
        "alias_swap",
        "body_digest_mismatch",
        "column_order_drift",
        "corrupt_database",
        "extra_object",
        "foreign_key_drift",
        "hardlink",
        "integrity_failure",
        "missing_object",
        "mode_widened",
        "noncanonical_body",
        "notnull_drift",
        "parent_swap",
        "pragma_drift",
        "primary_key_drift",
        "reparse",
        "sqlite_replacement",
        "trigger_added",
        "unknown_reserved_leaf",
        "view_added",
        "virtual_table_added",
    ];
    if !KINDS.contains(&kind)
        || !valid_label(&recipe["target"])
        || !text(recipe, "mutation").is_some_and(|mutation| MUTATIONS.contains(&mutation))
    {
        return fail("GKX_WATCHER_CONTRACT_SQL_INVALID");
    }
    match kind {
        "outbox" => fail("GKX_WATCHER_CONTRACT_RESET_INVALID"),
        "identity" | "reset" => fail("GKX_WATCHER_CONTRACT_POINTER_INVALID"),
        _ => fail("GKX_WATCHER_CONTRACT_SQL_INVALID"),
    }
}

fn decode_base64(value: &str) -> WatcherResult<Vec<u8>> {
    if value.is_empty() || value.len() % 4 != 0 {
        return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
    }
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    let bytes = value.as_bytes();
    for (index, block) in bytes.chunks_exact(4).enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let decode = |byte: u8| -> Option<u8> {
            match byte {
                b'A'..=b'Z' => Some(byte - b'A'),
                b'a'..=b'z' => Some(byte - b'a' + 26),
                b'0'..=b'9' => Some(byte - b'0' + 52),
                b'+' => Some(62),
                b'/' => Some(63),
                _ => None,
            }
        };
        let a = decode(block[0]).ok_or(WatcherError("GKX_WATCHER_CONTRACT_PACK_INVALID"))?;
        let b = decode(block[1]).ok_or(WatcherError("GKX_WATCHER_CONTRACT_PACK_INVALID"))?;
        if block[2] == b'=' {
            if !last || block[3] != b'=' || b & 0x0f != 0 {
                return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
            }
            output.push((a << 2) | (b >> 4));
            continue;
        }
        let c = decode(block[2]).ok_or(WatcherError("GKX_WATCHER_CONTRACT_PACK_INVALID"))?;
        output.push((a << 2) | (b >> 4));
        if block[3] == b'=' {
            if !last || c & 0x03 != 0 {
                return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
            }
            output.push((b << 4) | (c >> 2));
            continue;
        }
        let d = decode(block[3]).ok_or(WatcherError("GKX_WATCHER_CONTRACT_PACK_INVALID"))?;
        output.push((b << 4) | (c >> 2));
        output.push((c << 6) | d);
    }
    Ok(output)
}

const SCHEMA_ROOT: &str =
    "https://gkos.example/contracts/watcher/gkos-watcher-recovery-1.0.0-draft.1/";

fn validate_schema_structure(value: &Value, partial_composition_arm: bool) -> WatcherResult<()> {
    if let Some(items) = value.as_array() {
        for item in items {
            validate_schema_structure(item, partial_composition_arm)?;
        }
        return Ok(());
    }
    let Some(record) = value.as_object() else {
        return Ok(());
    };
    if let Some(reference) = record.get("$ref") {
        if !reference
            .as_str()
            .is_some_and(|reference| reference.starts_with(SCHEMA_ROOT))
        {
            return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
        }
    }
    if !partial_composition_arm
        && text(record, "type") == Some("object")
        && record.contains_key("properties")
        && record.get("required").is_some_and(Value::is_array)
        && (record.get("additionalProperties") != Some(&Value::Bool(false))
            || record.get("unevaluatedProperties") != Some(&Value::Bool(false)))
    {
        return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
    }
    for (key, child) in record {
        validate_schema_structure(
            child,
            partial_composition_arm || matches!(key.as_str(), "if" | "then"),
        )?;
    }
    Ok(())
}

fn validate_pack(value: &Value) -> WatcherResult<Value> {
    let bundle = object(value)?;
    exact_keys(bundle, &["pack_root_manifest", "files"])?;
    let manifest = seal_record(&bundle["pack_root_manifest"])?;
    let files = array(&bundle["files"])?;
    let manifest_files = array(&manifest["files"])?;
    if files.len() != 17 || manifest_files.len() != 17 {
        return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
    }
    let mut total = 0_u64;
    let mut definition_owners = BTreeMap::<String, String>::new();
    for index in 0..17 {
        let row = object(&files[index])?;
        let authority = object(&manifest_files[index])?;
        exact_keys(row, &["file", "bytes_base64"])?;
        if row["file"] != authority["file"]
            || !text(row, "file").is_some_and(|file| PACK_FILES.contains(&file))
        {
            return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
        }
        let bytes = decode_base64(text(row, "bytes_base64").unwrap_or_default())?;
        let file = text(row, "file").unwrap();
        if bytes.len() as u64 != unsigned(authority, "byte_size").unwrap_or_default()
            || sha256(&bytes) != text(authority, "raw_sha256").unwrap_or_default()
            || bytes.contains(&0)
            || bytes.contains(&b'\r')
            || bytes.starts_with(&[0xef, 0xbb, 0xbf])
            || if file == "watcher-sample-plan.json" {
                bytes.last() == Some(&b'\n')
            } else {
                bytes.last() != Some(&b'\n')
            }
            || std::str::from_utf8(&bytes).is_err()
        {
            return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
        }
        if file.ends_with(".json") {
            let parsed: Value = serde_json::from_slice(&bytes)
                .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_PACK_INVALID"))?;
            let expected = if file == "watcher-sample-plan.json" {
                canonical_json(&parsed)
                    .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_PACK_INVALID"))?
                    .into_bytes()
            } else {
                canonical_bytes(&parsed)
                    .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_PACK_INVALID"))?
            };
            if expected != bytes {
                return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
            }
            if file.ends_with(".schema.json") {
                let schema = object(&parsed)?;
                if text(schema, "$schema") != Some("https://json-schema.org/draft/2020-12/schema")
                    || text(schema, "$id") != Some(format!("{SCHEMA_ROOT}{file}").as_str())
                {
                    return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
                }
                validate_schema_structure(&parsed, false)?;
                let definitions = object(&schema["$defs"])?;
                for name in definitions.keys() {
                    if definition_owners
                        .insert(name.clone(), file.to_owned())
                        .is_some()
                    {
                        return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
                    }
                }
            }
        }
        total += bytes.len() as u64;
    }
    if total != unsigned(object(&manifest)?, "total_bytes").unwrap_or_default() {
        return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
    }
    if definition_owners.get("acceptedSource").map(String::as_str) != Some("topology.schema.json")
        || definition_owners.get("rejectedSource").map(String::as_str)
            != Some("topology.schema.json")
    {
        return fail("GKX_WATCHER_CONTRACT_PACK_INVALID");
    }
    Ok(Value::Null)
}

fn validate_cli(value: &Value) -> WatcherResult<Value> {
    let fixture = object(value)?;
    exact_keys(
        fixture,
        &[
            "contract_version",
            "state_fixtures",
            "commands",
            "fixture_digest",
        ],
    )?;
    if text(fixture, "contract_version") != Some("gkos-watcher-cli-fixture/1.0.0-draft.1")
        || text(fixture, "fixture_digest")
            != Some("sha256:0e05988ff481b58c9f9ec8262b75a8d642012aa3325121e1f3ff259ee593f96d")
        || digest_without(value, "fixture_digest")?
            != "sha256:0e05988ff481b58c9f9ec8262b75a8d642012aa3325121e1f3ff259ee593f96d"
        || array(&fixture["state_fixtures"])?.len() != 7
        || array(&fixture["commands"])?.len() != 35
    {
        return fail("GKX_WATCHER_CONTRACT_CLI_INVALID");
    }
    // The exact fixture digest freezes the derived stdout/stderr/argv catalog;
    // still independently seal every embedded governed record.
    for state in array(&fixture["state_fixtures"])? {
        let state = object(state)?;
        exact_keys(
            state,
            &[
                "fixture_id",
                "capability_state",
                "locator",
                "status",
                "active_coherent",
                "coherent_manifest",
                "journal_generation",
                "journal_pointer",
                "reset_result",
            ],
        )?;
        for field in [
            "locator",
            "status",
            "active_coherent",
            "coherent_manifest",
            "journal_generation",
            "journal_pointer",
            "reset_result",
        ] {
            if !state[field].is_null() {
                seal_record(&state[field])
                    .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_CLI_INVALID"))?;
            }
        }
    }
    Ok(Value::Null)
}

fn seal_sample_plan(value: &Value) -> WatcherResult<Value> {
    let plan = object(value)?;
    exact_keys(
        plan,
        &[
            "contract_version",
            "execution",
            "fixture",
            "percentile",
            "thresholds",
            "timing",
            "watcher",
        ],
    )?;
    if text(plan, "contract_version") != Some(SAMPLE_PLAN_VERSION)
        || canonical_digest(value).ok().as_deref() != Some(SAMPLE_PLAN_DIGEST)
    {
        return fail("GKX_WATCHER_CONTRACT_SAMPLE_PLAN_INVALID");
    }
    let fixture = object(&plan["fixture"])?;
    for field in ["alpha", "omega"] {
        let source = object(&fixture[field])?;
        let bytes = decode_base64(text(source, "source_bytes_base64").unwrap_or_default())
            .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_SAMPLE_PLAN_INVALID"))?;
        if unsigned(source, "byte_size") != Some(bytes.len() as u64)
            || text(source, "source_digest") != Some(sha256(&bytes).as_str())
        {
            return fail("GKX_WATCHER_CONTRACT_SAMPLE_PLAN_INVALID");
        }
    }
    Ok(value.clone())
}

fn operation_result(operation: &str, kind: &str, result: Value) -> WatcherResult<String> {
    canonical_digest(&json!({
        "contract_version":"gkos-watcher-conformance-operation-result/1.0.0-draft.1",
        "operation":operation,
        "result_kind":kind,
        "result":result,
    }))
    .map_err(|_| WatcherError("GKX_WATCHER_CONTRACT_DIGEST_INVALID"))
}

fn invoke(operation: &str, input: &Value) -> WatcherResult<String> {
    let input = object(input)?;
    exact_keys(input, &["arguments"])?;
    let arguments = array(&input["arguments"])?;
    let (result, kind) = match operation {
        "derive_graphiti_projection" if arguments.len() == 2 => {
            (derive_graphiti(&arguments[0], &arguments[1])?, "record")
        }
        "normalize_canonical_graph" if arguments.len() == 1 => {
            (normalize_raw_graph(&arguments[0])?, "record")
        }
        "normalize_graph_delta" if arguments.len() == 1 => {
            (normalize_graph_delta(&arguments[0])?, "record")
        }
        "seal_coherent_activation_bundle" if arguments.len() == 2 => (
            seal_coherent_activation_bundle(&arguments[0], &arguments[1])?,
            "record",
        ),
        "seal_failure_retry_bundle" if arguments.len() == 1 => {
            (seal_failure_retry_bundle(&arguments[0])?, "record")
        }
        "seal_journal_reset_bundle" if arguments.len() == 3 => (
            seal_journal_reset_bundle(&arguments[0], &arguments[1], &arguments[2])?,
            "record",
        ),
        "seal_measurement" if arguments.len() == 1 => (seal_record(&arguments[0])?, "record"),
        "seal_pointer_recovery" if arguments.len() == 2 => (
            classify_pointer_recovery(&arguments[0], &arguments[1])?,
            "record",
        ),
        "seal_record" if arguments.len() == 1 => (seal_record(&arguments[0])?, "record"),
        "seal_source_removal_adapter_verification_bundle" if arguments.len() == 1 => {
            (seal_adapter_verification_bundle(&arguments[0])?, "record")
        }
        "seal_source_removal_event_set_bundle" if arguments.len() == 1 => {
            (seal_event_set_bundle(&arguments[0])?, "record")
        }
        "seal_source_removal_receipt_bundle" if arguments.len() == 1 => {
            (seal_receipt_bundle(&arguments[0])?, "record")
        }
        "seal_status_bundle" if arguments.len() == 1 => {
            (seal_status_bundle(&arguments[0])?, "record")
        }
        "seal_transition_chain" if arguments.len() == 1 => (
            seal_transition_sequence(&arguments[0], true)?,
            "record_array",
        ),
        "validate_cli_fixture" if arguments.len() == 1 => (validate_cli(&arguments[0])?, "null"),
        "validate_pack" if arguments.len() == 1 => (validate_pack(&arguments[0])?, "null"),
        "validate_path" if arguments.len() == 1 => {
            if !valid_source_path(&arguments[0]) {
                return fail("GKX_WATCHER_CONTRACT_PATH_INVALID");
            }
            (Value::Null, "null")
        }
        "validate_sql_authority" if arguments.len() == 1 => (validate_sql(&arguments[0])?, "null"),
        _ => return fail("GKX_WATCHER_CONTRACT_RECORD_INVALID"),
    };
    operation_result(operation, kind, result)
}

#[cfg(test)]
fn schema_pattern_matches(pattern: &str, value: &str) -> bool {
    let ascii_hex = |value: &str, length: usize, lowercase: bool| {
        value.len() == length
            && value.bytes().all(|byte| {
                byte.is_ascii_digit()
                    || matches!(byte, b'a'..=b'f')
                    || !lowercase && matches!(byte, b'A'..=b'F')
            })
    };
    let uuid = |value: &str, version7_only: bool, lowercase: bool| {
        let bytes = value.as_bytes();
        bytes.len() == 36
            && bytes[8] == b'-'
            && bytes[13] == b'-'
            && bytes[18] == b'-'
            && bytes[23] == b'-'
            && matches!(bytes[14], b'1'..=b'8')
            && (!version7_only || bytes[14] == b'7')
            && matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B')
            && bytes.iter().enumerate().all(|(index, byte)| {
                matches!(index, 8 | 13 | 18 | 23)
                    || byte.is_ascii_digit()
                    || matches!(byte, b'a'..=b'f')
                    || !lowercase && matches!(byte, b'A'..=b'F')
            })
    };
    let lowercase_hex_tail = |prefix: &str, suffix: &str, digits: usize| {
        value
            .strip_prefix(prefix)
            .and_then(|tail| tail.strip_suffix(suffix))
            .is_some_and(|hex| ascii_hex(hex, digits, true))
    };
    match pattern {
        "^(?!/)(?![A-Za-z]:)(?![A-Za-z][A-Za-z0-9+.-]*:)(?!.*:)(?!.*\\\\)(?!.*(?:^|/)\\.\\.?(?:/|$))(?!.*//)[^\\u0000-\\u001f<>:\"|?*\\u007f]+$" => {
            let bytes = value.as_bytes();
            let scheme = value.find(':').is_some_and(|index| {
                index > 0
                    && value[..index].bytes().enumerate().all(|(position, byte)| {
                        if position == 0 {
                            byte.is_ascii_alphabetic()
                        } else {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'-')
                        }
                    })
            });
            !value.is_empty()
                && !value.starts_with('/')
                && !(bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
                && !scheme
                && !value.contains(':')
                && !value.contains('\\')
                && !value.contains("//")
                && !value.split('/').any(|segment| matches!(segment, "." | ".."))
                && !value.chars().any(|character| {
                    character <= '\u{1f}'
                        || character == '\u{7f}'
                        || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
                })
        }
        "^(?:0|[1-9][0-9]*)$" => decimal_identity(&Value::String(value.to_owned())),
        "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$" => uuid(value, false, false),
        "^[0-9a-f]{32}$" => ascii_hex(value, 32, true),
        "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$" => uuid(value, true, true),
        "^[^\\u0000-\\u001f\\u007f]+$" => !value.is_empty()
            && !value
                .chars()
                .any(|character| character <= '\u{1f}' || character == '\u{7f}'),
        "^[a-z0-9](?:[a-z0-9._:-]{0,127})$" => valid_label(&Value::String(value.to_owned())),
        "^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}\\.\\d{3}Z$" => {
            let bytes = value.as_bytes();
            bytes.len() == 24
                && bytes.iter().enumerate().all(|(index, byte)| match index {
                    4 | 7 => *byte == b'-',
                    10 => *byte == b'T',
                    13 | 16 => *byte == b':',
                    19 => *byte == b'.',
                    23 => *byte == b'Z',
                    _ => byte.is_ascii_digit(),
                })
        }
        "^ingest:[0-9a-f]{24}$" => lowercase_hex_tail("ingest:", "", 24),
        "^journal-[0-9a-f-]{36}$" => value.strip_prefix("journal-").is_some_and(|tail| {
            tail.len() == 36
                && tail
                    .bytes()
                    .all(|byte| byte == b'-' || byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        }),
        "^retrieval-[0-9a-f]{64}\\.sqlite$" => lowercase_hex_tail("retrieval-", ".sqlite", 64),
        "^retrieval:[0-9a-f]{24}$" => lowercase_hex_tail("retrieval:", "", 24),
        "^sha256:[0-9a-f]{64}$" => is_digest_text(value),
        "^watcher-coherent-[0-9a-f]{64}\\.json$" => lowercase_hex_tail("watcher-coherent-", ".json", 64),
        "^watcher-graph-[0-9a-f]{64}\\.json$" => lowercase_hex_tail("watcher-graph-", ".json", 64),
        "^watcher-journal-generation-[0-9a-f]{64}\\.json$" => lowercase_hex_tail("watcher-journal-generation-", ".json", 64),
        "^watcher-observation-[0-9a-f]{64}\\.json$" => lowercase_hex_tail("watcher-observation-", ".json", 64),
        "^watcher-plan-[0-9a-f]{64}\\.json$" => lowercase_hex_tail("watcher-plan-", ".json", 64),
        "^watcher-topology-[0-9a-f]{64}\\.json$" => lowercase_hex_tail("watcher-topology-", ".json", 64),
        "^watcher:[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$" => value
            .strip_prefix("watcher:")
            .is_some_and(|tail| uuid(tail, true, true)),
        _ => false,
    }
}

#[cfg(test)]
fn schema_type_matches(kind: &str, instance: &Value) -> bool {
    match kind {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "integer" => {
            instance.as_i64().is_some()
                || instance.as_u64().is_some()
                || instance
                    .as_f64()
                    .is_some_and(|value| value.is_finite() && value.fract() == 0.0)
        }
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        _ => false,
    }
}

#[cfg(test)]
fn resolve_schema_reference<'a>(
    reference: &str,
    schemas: &'a BTreeMap<String, Value>,
) -> Option<&'a Value> {
    let owned = reference.strip_prefix(SCHEMA_ROOT)?;
    let (file, fragment) = owned.split_once('#').unwrap_or((owned, ""));
    let mut target = schemas.get(file)?;
    if !fragment.is_empty() {
        target = target.pointer(fragment)?;
    }
    Some(target)
}

#[cfg(test)]
fn schema_valid(schema: &Value, instance: &Value, schemas: &BTreeMap<String, Value>) -> bool {
    if let Some(allowed) = schema.as_bool() {
        return allowed;
    }
    let Some(schema) = schema.as_object() else {
        return false;
    };
    if let Some(reference) = text(schema, "$ref") {
        let Some(referenced) = resolve_schema_reference(reference, schemas) else {
            return false;
        };
        if !schema_valid(referenced, instance, schemas) {
            return false;
        }
    }
    if let Some(kind) = schema.get("type") {
        let type_valid = kind
            .as_str()
            .is_some_and(|kind| schema_type_matches(kind, instance))
            || kind.as_array().is_some_and(|kinds| {
                kinds.iter().any(|kind| {
                    kind.as_str()
                        .is_some_and(|kind| schema_type_matches(kind, instance))
                })
            });
        if !type_valid {
            return false;
        }
    }
    if schema
        .get("const")
        .is_some_and(|constant| constant != instance)
    {
        return false;
    }
    if schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|values| !values.contains(instance))
    {
        return false;
    }
    if schema
        .get("allOf")
        .and_then(Value::as_array)
        .is_some_and(|branches| {
            !branches
                .iter()
                .all(|branch| schema_valid(branch, instance, schemas))
        })
        || schema
            .get("anyOf")
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                !branches
                    .iter()
                    .any(|branch| schema_valid(branch, instance, schemas))
            })
        || schema
            .get("oneOf")
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                branches
                    .iter()
                    .filter(|branch| schema_valid(branch, instance, schemas))
                    .count()
                    != 1
            })
    {
        return false;
    }
    if let Some(condition) = schema.get("if") {
        let branch = if schema_valid(condition, instance, schemas) {
            schema.get("then")
        } else {
            schema.get("else")
        };
        if branch.is_some_and(|branch| !schema_valid(branch, instance, schemas)) {
            return false;
        }
    }
    if let Some(string) = instance.as_str() {
        let length = string.chars().count() as u64;
        if unsigned(schema, "minLength").is_some_and(|minimum| length < minimum)
            || unsigned(schema, "maxLength").is_some_and(|maximum| length > maximum)
            || text(schema, "pattern")
                .is_some_and(|pattern| !schema_pattern_matches(pattern, string))
            || text(schema, "format") == Some("date-time")
                && !is_iso(&Value::String(string.to_owned()))
        {
            return false;
        }
    }
    if let Some(number) = instance.as_f64() {
        if schema
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|minimum| number < minimum)
            || schema
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|maximum| number > maximum)
        {
            return false;
        }
    }
    if let Some(items) = instance.as_array() {
        if unsigned(schema, "minItems").is_some_and(|minimum| items.len() < minimum as usize)
            || unsigned(schema, "maxItems").is_some_and(|maximum| items.len() > maximum as usize)
            || schema.get("uniqueItems") == Some(&Value::Bool(true))
                && items
                    .iter()
                    .enumerate()
                    .any(|(index, item)| items[..index].contains(item))
        {
            return false;
        }
        let prefix_count = schema
            .get("prefixItems")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        if let Some(prefix) = schema.get("prefixItems").and_then(Value::as_array) {
            for (item, item_schema) in items.iter().zip(prefix) {
                if !schema_valid(item_schema, item, schemas) {
                    return false;
                }
            }
        }
        if let Some(item_schema) = schema.get("items") {
            for item in items.iter().skip(prefix_count) {
                if !schema_valid(item_schema, item, schemas) {
                    return false;
                }
            }
        }
    }
    if let Some(record) = instance.as_object() {
        if schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|required| {
                required
                    .iter()
                    .any(|key| key.as_str().is_none_or(|key| !record.contains_key(key)))
            })
        {
            return false;
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(properties) = properties {
            for (key, property_schema) in properties {
                if record
                    .get(key)
                    .is_some_and(|value| !schema_valid(property_schema, value, schemas))
                {
                    return false;
                }
            }
            if (schema.get("additionalProperties") == Some(&Value::Bool(false))
                || schema.get("unevaluatedProperties") == Some(&Value::Bool(false)))
                && record.keys().any(|key| !properties.contains_key(key))
            {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    const PACK: &str = "../../contracts/gkos-watcher-recovery-1.0.0-draft.1";

    fn pack_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(PACK)
            .join(name)
    }

    fn fixture(name: &str) -> Value {
        serde_json::from_slice(&fs::read(pack_path(name)).expect("read watcher fixture"))
            .expect("parse watcher fixture")
    }

    fn semantic_case(case_id: &str) -> Value {
        fixture("watcher-conformance-fixture.json")["semantic_cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["case_id"] == case_id)
            .unwrap_or_else(|| panic!("missing semantic case {case_id}"))
            .clone()
    }

    fn reseal_for_test(value: &mut Value, digest_key: &str) {
        value[digest_key] = Value::Null;
        value[digest_key] = Value::String(digest_without(value, digest_key).unwrap());
    }

    fn encode_base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut output = String::new();
        for chunk in bytes.chunks(3) {
            output.push(ALPHABET[(chunk[0] >> 2) as usize] as char);
            output.push(
                ALPHABET[(((chunk[0] & 0x03) << 4)
                    | (chunk.get(1).copied().unwrap_or_default() >> 4))
                    as usize] as char,
            );
            if let Some(second) = chunk.get(1) {
                output.push(
                    ALPHABET[(((second & 0x0f) << 2)
                        | (chunk.get(2).copied().unwrap_or_default() >> 6))
                        as usize] as char,
                );
            } else {
                output.push('=');
            }
            if let Some(third) = chunk.get(2) {
                output.push(ALPHABET[(third & 0x3f) as usize] as char);
            } else {
                output.push('=');
            }
        }
        output
    }

    fn mutate_pack_schema(bundle: &mut Value, schema_file: &str, mutate: impl FnOnce(&mut Value)) {
        let index = bundle["files"]
            .as_array()
            .unwrap()
            .iter()
            .position(|row| row["file"] == schema_file)
            .unwrap();
        let encoded = bundle["files"][index]["bytes_base64"].as_str().unwrap();
        let mut schema: Value = serde_json::from_slice(&decode_base64(encoded).unwrap()).unwrap();
        mutate(&mut schema);
        let bytes = canonical_bytes(&schema).unwrap();
        bundle["files"][index]["bytes_base64"] = Value::String(encode_base64(&bytes));
        bundle["pack_root_manifest"]["files"][index]["byte_size"] = Value::from(bytes.len() as u64);
        bundle["pack_root_manifest"]["files"][index]["raw_sha256"] = Value::String(sha256(&bytes));
        let total = bundle["pack_root_manifest"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["byte_size"].as_u64().unwrap())
            .sum::<u64>();
        bundle["pack_root_manifest"]["total_bytes"] = Value::from(total);
        reseal_for_test(&mut bundle["pack_root_manifest"], "pack_digest");
    }

    #[test]
    fn frozen_full_pack_pin_and_private_boundary_are_exact() {
        let pin = fixture("FULL-PIN.json");
        let pin_record = object(&pin).unwrap();
        exact_keys(
            pin_record,
            &[
                "reference_repository",
                "reference_commit",
                "reference_package_version",
                "reference_state",
                "contract_version",
                "pin_kind",
                "publication_qualified",
                "pack_file_count",
                "pack_manifest_file_count",
                "pack_byte_count",
                "pack_digest",
                "note",
                "files",
            ],
        )
        .unwrap();
        assert_eq!(pin["reference_repository"], "Odenknight/GKOS-Engine");
        assert_eq!(
            pin["reference_commit"],
            "420a9d704f1fd12a6a61e4dd60abeb70757a9b2d"
        );
        assert_eq!(pin["reference_package_version"], "2.1.2");
        assert_eq!(
            pin["reference_state"],
            "full_phase5_slice_a_published_hosted_green"
        );
        assert_eq!(pin["contract_version"], PACK_VERSION);
        assert_eq!(
            pin["pin_kind"],
            "exact_full_commit_pack_manifest_and_slice_file_sha256"
        );
        assert_eq!(pin["publication_qualified"], true);
        assert_eq!(pin["pack_file_count"], 18);
        assert_eq!(pin["pack_manifest_file_count"], 17);
        assert_eq!(pin["pack_byte_count"], 5_860_943);
        assert_eq!(
            pin["pack_digest"],
            "sha256:c08520c1392d6be04c71159050c0d60f5bf03afeeb915ae44920e758e35cb49a"
        );
        let files = object(&pin["files"]).unwrap();
        assert_eq!(files.len(), 18);
        let mut expected_names = PACK_FILES.iter().copied().collect::<BTreeSet<_>>();
        assert!(expected_names.insert("pack-manifest.json"));
        assert_eq!(
            files.keys().map(String::as_str).collect::<BTreeSet<_>>(),
            expected_names
        );
        let mut actual_sizes = BTreeMap::new();
        for (name, digest) in files {
            let bytes = fs::read(pack_path(name)).unwrap_or_else(|_| panic!("read {name}"));
            assert_eq!(sha256(&bytes), digest.as_str().unwrap(), "{name}");
            actual_sizes.insert(name.as_str(), bytes.len() as u64);
            assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]), "{name}: BOM");
            assert!(!bytes.contains(&b'\r'), "{name}: CR");
            assert!(std::str::from_utf8(&bytes).is_ok(), "{name}: UTF-8");
            if name == "watcher-sample-plan.json" {
                assert_ne!(bytes.last(), Some(&b'\n'));
            } else {
                assert_eq!(bytes.last(), Some(&b'\n'));
                assert!(!bytes.ends_with(b"\n\n"));
            }
        }
        let manifest = fixture("pack-manifest.json");
        let manifest = seal_record(&manifest).unwrap();
        assert_eq!(
            manifest["contract_version"],
            "gkos-watcher-recovery-pack-manifest/1.0.0-draft.1"
        );
        assert_eq!(manifest["pack_contract_version"], PACK_VERSION);
        assert_eq!(manifest["file_count"], 17);
        assert_eq!(manifest["total_bytes"], 5_860_943);
        assert_eq!(manifest["pack_digest"], pin["pack_digest"]);
        let rows = array(&manifest["files"]).unwrap();
        assert_eq!(rows.len(), 17);
        let mut governed_total = 0_u64;
        for (index, row) in rows.iter().enumerate() {
            let row = object(row).unwrap();
            let name = text(row, "file").unwrap();
            assert_eq!(name, PACK_FILES[index]);
            assert_eq!(unsigned(row, "byte_size"), actual_sizes.get(name).copied());
            assert_eq!(text(row, "raw_sha256"), files[name].as_str());
            governed_total += actual_sizes[name];
        }
        assert_eq!(governed_total, 5_860_943);
        assert_eq!(fs::read_dir(pack_path("")).unwrap().count(), 19);
        let source = include_str!("watcher.rs");
        let pure = source.split("#[cfg(test)]").next().unwrap();
        assert!(!pure.contains("pub fn "));
        assert!(!pure.contains("pub(crate) fn "));
        assert!(!pure.contains("use std::fs"));
        assert!(!pure.contains("rusqlite"));
        assert!(!pure.contains("std::net"));
        assert!(!pure.contains("std::process"));
    }

    #[test]
    fn sample_plan_transport_is_exact() {
        let bytes = fs::read(pack_path("watcher-sample-plan.json")).unwrap();
        assert_eq!(bytes.len(), 3_978);
        assert_eq!(sha256(&bytes), SAMPLE_PLAN_DIGEST);
        assert_ne!(bytes.last(), Some(&b'\n'));
        let plan: Value = serde_json::from_slice(&bytes).unwrap();
        seal_sample_plan(&plan).unwrap();
    }

    #[test]
    fn watcher_paths_reuse_the_phase_three_portable_grammar() {
        for path in ["policy/agent-writing.md", "con.md", "archive/ordinary.md"] {
            assert!(is_valid_retrieval_source_path(path));
            assert!(valid_source_path(&Value::String(path.to_owned())));
        }
        for path in [
            "C:/vault/note.md",
            "note.md:stream",
            "../note.md",
            "note.md\u{7f}",
        ] {
            assert!(!valid_source_path(&Value::String(path.to_owned())));
        }
    }

    #[test]
    fn utf16_pretty_bytes_and_year_zero_match_full() {
        let astral = "\u{10000}";
        let bmp = "\u{e000}";
        let value = json!({bmp:3,astral:{bmp:1,astral:2}});
        let expected = format!(
            "{{\n  \"{astral}\": {{\n    \"{astral}\": 2,\n    \"{bmp}\": 1\n  }},\n  \"{bmp}\": 3\n}}\n"
        );
        assert_eq!(canonical_bytes(&value).unwrap(), expected.as_bytes());
        let coordinate = artifact_coordinate(
            "graph",
            &json!({
                "contract_version":"test-only",
                "graph":value,
                "graph_artifact_digest":format!("sha256:{}","4".repeat(64)),
            }),
        )
        .unwrap();
        assert_eq!(
            coordinate["raw_sha256"],
            sha256(coordinate["bytes"].as_str().unwrap().as_bytes())
        );

        let mut observation = semantic_case("coherent-activation-complete")["input"]["arguments"]
            [0]["observation"]
            .clone();
        observation["started_at"] = json!("0000-01-01T00:00:00.000Z");
        observation["observation_digest"] = Value::Null;
        let digest = digest_without(&observation, "observation_digest").unwrap();
        observation["observation_digest"] = Value::String(digest);
        seal_record(&observation).unwrap();
    }

    #[test]
    fn nonempty_and_rich_graphiti_match_pinned_full() {
        let case = semantic_case("graphiti-body-noncanonical");
        let mut bundle = case["input"]["arguments"][0].clone();
        let guard = case["input"]["arguments"][1].clone();
        let raw_graph = bundle["raw_graph"]["graph"].clone();
        let vault_id = bundle["pre_scan_state"]["vault_id"].clone();
        let mut expected = bundle["graphiti_projection"].clone();
        let body_text = expected["episodes"][0]["episode_body"].as_str().unwrap();
        let body: Value = serde_json::from_str(body_text).unwrap();
        expected["episodes"][0]["episode_body"] = Value::String(canonical_json(&body).unwrap());
        let derived = derive_graphiti(&raw_graph, &vault_id).unwrap();
        assert_eq!(derived, expected);
        assert!(graphiti_matches_raw(&expected, &raw_graph, &vault_id).unwrap());
        bundle["graphiti_projection"] = expected;
        seal_coherent_activation_bundle(&bundle, &guard).unwrap();

        for case_id in [
            "graphiti-vault-substitution",
            "graphiti-episode-order-substitution",
        ] {
            let case = semantic_case(case_id);
            let bundle = &case["input"]["arguments"][0];
            assert!(!graphiti_matches_raw(
                &bundle["graphiti_projection"],
                &bundle["raw_graph"]["graph"],
                &bundle["pre_scan_state"]["vault_id"],
            )
            .unwrap());
        }

        // Cascade a wrong-vault projection through every downstream digest so
        // rejection cannot be credited to an unrelated stale transition or
        // pointer coordinate.
        let mut cascaded = bundle.clone();
        cascaded["graphiti_projection"] = semantic_case("graphiti-vault-substitution")["input"]
            ["arguments"][0]["graphiti_projection"]
            .clone();
        let graphiti_digest = canonical_digest(&cascaded["graphiti_projection"]).unwrap();
        {
            let transitions = cascaded["transitions"].as_array_mut().unwrap();
            for index in 4..=6 {
                transitions[index]["graph_projection_state"]["graphiti_projection_digest"] =
                    Value::String(graphiti_digest.clone());
                if index > 4 {
                    transitions[index]["prior_transition_digest"] =
                        transitions[index - 1]["transition_digest"].clone();
                }
                reseal_for_test(&mut transitions[index], "transition_digest");
            }
        }
        let complete = cascaded["transitions"][6].clone();
        let prepared = cascaded["transitions"][5].clone();
        cascaded["manifest"]["completed_transition_digest"] = complete["transition_digest"].clone();
        cascaded["manifest"]["graph_projection_state"] = complete["graph_projection_state"].clone();
        reseal_for_test(&mut cascaded["manifest"], "coherent_manifest_digest");
        let manifest_digest = cascaded["manifest"]["coherent_manifest_digest"].clone();
        cascaded["pointer"]["coherent_manifest_digest"] = manifest_digest.clone();
        cascaded["pointer"]["coherent_manifest_file"] = Value::String(format!(
            "watcher-coherent-{}.json",
            &manifest_digest.as_str().unwrap()[7..]
        ));
        reseal_for_test(&mut cascaded["pointer"], "pointer_digest");
        let pointer = cascaded["pointer"].clone();
        cascaded["intent"]["prepared_transition_digest"] = prepared["transition_digest"].clone();
        cascaded["intent"]["coherent_manifest_digest"] = manifest_digest.clone();
        cascaded["intent"]["target_pointer"] = pointer.clone();
        cascaded["intent"]["target_complete_transition"] = complete.clone();
        reseal_for_test(&mut cascaded["intent"], "intent_digest");
        let intent_digest = cascaded["intent"]["intent_digest"].clone();
        cascaded["outcome"]["intent_digest"] = intent_digest.clone();
        cascaded["outcome"]["coherent_manifest_digest"] = manifest_digest.clone();
        cascaded["outcome"]["pointer_digest"] = pointer["pointer_digest"].clone();
        reseal_for_test(&mut cascaded["outcome"], "outcome_digest");
        cascaded["active"]["intent_digest"] = intent_digest.clone();
        cascaded["active"]["coherent_manifest_digest"] = manifest_digest;
        cascaded["active"]["pointer_digest"] = pointer["pointer_digest"].clone();
        reseal_for_test(&mut cascaded["active"], "active_digest");
        let mut cascaded_guard = guard.clone();
        let pointer_bytes = canonical_bytes(&pointer).unwrap();
        cascaded_guard["new_pointer_file"] = Value::String(format!(
            "watcher-pointer-{}.json",
            &pointer["pointer_digest"].as_str().unwrap()[7..]
        ));
        cascaded_guard["new_pointer_digest"] = pointer["pointer_digest"].clone();
        cascaded_guard["new_pointer_raw_sha256"] = Value::String(sha256(&pointer_bytes));
        cascaded_guard["new_pointer_byte_size"] = Value::from(pointer_bytes.len() as u64);
        cascaded_guard["operation_intent_digest"] = intent_digest;
        cascaded_guard["target_commit_digest"] = complete["transition_digest"].clone();
        reseal_for_test(&mut cascaded_guard, "guard_digest");
        assert_eq!(
            seal_coherent_activation_bundle(&cascaded, &cascaded_guard),
            Err(WatcherError("GKX_WATCHER_CONTRACT_RELATION_INVALID"))
        );

        let mut authored_graph = raw_graph.clone();
        let file_node = authored_graph["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|node| node["kind"] == "file")
            .unwrap();
        file_node["gkx"] = json!({"uid":"123e4567-e89b-42d3-a456-426614174000"});
        let authored = derive_graphiti(&authored_graph, &vault_id).unwrap();
        assert_eq!(
            canonical_digest(&authored).unwrap(),
            "sha256:4f2bba4b8688c6c5ce31758ca4eca330eb5b6f4868f93222fadacc542c551cfe"
        );
        assert_eq!(
            authored["episodes"][0]["uuid"],
            "123e4567-e89b-42d3-a456-426614174000"
        );

        let mut offset_graph = raw_graph.clone();
        offset_graph["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|node| node["kind"] == "file")
            .unwrap()["gkx"] = json!({"timestamp":"2025-01-02T03:04:05.123-04:00"});
        let offset = derive_graphiti(&offset_graph, &vault_id).unwrap();
        assert_eq!(
            canonical_digest(&offset).unwrap(),
            "sha256:16eadb8bd692b49acb0297b6d8da090d852e4544734c49f132f28bff616009a3"
        );
        assert!(offset["episodes"][0]["episode_body"]
            .as_str()
            .unwrap()
            .contains("\"reference_time_source\":\"gkx.timestamp\""));

        let mut hour24_graph = raw_graph.clone();
        let hour24_node = hour24_graph["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|node| node["kind"] == "file")
            .unwrap();
        hour24_node["gkx"] = json!({"timestamp":"2025-01-01T24:00Z"});
        hour24_node["validAt"] = json!("2025-01-02T00:00:00.000Z");
        let hour24 = derive_graphiti(&hour24_graph, &vault_id).unwrap();
        assert_eq!(
            canonical_digest(&hour24).unwrap(),
            "sha256:8af2a73d49741d5c61e27a250258e9e848d53fed7e0e75c1114375f23c75eb0e"
        );
        assert!(hour24["episodes"][0]["episode_body"]
            .as_str()
            .unwrap()
            .contains("\"reference_time_source\":\"gkx.timestamp\""));

        let mut normalized_calendar_graph = raw_graph.clone();
        let normalized_calendar_node = normalized_calendar_graph["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|node| node["kind"] == "file")
            .unwrap();
        normalized_calendar_node["validAt"] = json!("2025-03-02T00:00:00.000Z");
        normalized_calendar_node["gkx"] = json!({
            "projection":{
                "authored":{
                    "createdAt":"2025-02-30T00:00Z",
                    "authorship":{"origin":"human"},
                    "labels":[],
                    "relationships":{},
                    "evidence":{},
                },
                "derived":{"labels":[],"relationships":{},"evidence":{}},
                "proposed":{"labels":[],"relationships":{},"evidence":{}},
                "approved":{"labels":[],"relationships":{},"evidence":{}},
                "effective":{
                    "sensitivity":"internal",
                    "labels":[],
                    "relationships":{},
                    "evidence":{},
                },
                "diagnostics":[],
                "assessment":{
                    "scores":{"overall":0},
                    "labels":{"derived":[]},
                    "policy":{"id":"policy:test","version":"1","hash":"hash"},
                    "interpretation":"test",
                },
            },
        });
        let normalized_calendar = derive_graphiti(&normalized_calendar_graph, &vault_id).unwrap();
        assert_eq!(
            canonical_digest(&normalized_calendar).unwrap(),
            "sha256:e0104a94bc958c1bd43ff5bff430e6c0e0b51294e4205606fd7118800b371943"
        );
        assert!(normalized_calendar["episodes"][0]["episode_body"]
            .as_str()
            .unwrap()
            .contains("\"reference_time_source\":\"gkx.created_at\""));

        // Carry the normalized-calendar Full differential through every
        // graph/transition/manifest/pointer/intent/publication coordinate.
        // This proves the reference-source match is not masked by a stale
        // downstream digest that would reject for an unrelated reason.
        let mut calendar_bundle = bundle.clone();
        calendar_bundle["raw_graph"]["graph"] = normalized_calendar_graph.clone();
        reseal_for_test(&mut calendar_bundle["raw_graph"], "graph_artifact_digest");
        calendar_bundle["canonical_graph"] =
            normalize_raw_graph(&calendar_bundle["raw_graph"]["graph"]).unwrap();
        calendar_bundle["graphiti_projection"] = normalized_calendar.clone();
        let raw_coordinate = artifact_coordinate("graph", &calendar_bundle["raw_graph"]).unwrap();
        let canonical_graph_digest = canonical_digest(&calendar_bundle["canonical_graph"]).unwrap();
        let graphiti_projection_digest =
            canonical_digest(&calendar_bundle["graphiti_projection"]).unwrap();
        let mut graph_state = calendar_bundle["transitions"][4]["graph_projection_state"].clone();
        graph_state["graph_artifact_file"] = raw_coordinate["file"].clone();
        graph_state["graph_artifact_digest"] =
            calendar_bundle["raw_graph"]["graph_artifact_digest"].clone();
        graph_state["canonical_graph_digest"] = Value::String(canonical_graph_digest);
        graph_state["graphiti_projection_digest"] = Value::String(graphiti_projection_digest);
        {
            let transitions = calendar_bundle["transitions"].as_array_mut().unwrap();
            for index in 4..=6 {
                transitions[index]["graph_projection_state"] = graph_state.clone();
                if index > 4 {
                    transitions[index]["prior_transition_digest"] =
                        transitions[index - 1]["transition_digest"].clone();
                }
                reseal_for_test(&mut transitions[index], "transition_digest");
            }
        }
        let calendar_complete = calendar_bundle["transitions"][6].clone();
        let calendar_prepared = calendar_bundle["transitions"][5].clone();
        calendar_bundle["manifest"]["completed_transition_digest"] =
            calendar_complete["transition_digest"].clone();
        calendar_bundle["manifest"]["graph_projection_state"] =
            calendar_complete["graph_projection_state"].clone();
        reseal_for_test(&mut calendar_bundle["manifest"], "coherent_manifest_digest");
        let calendar_manifest_digest =
            calendar_bundle["manifest"]["coherent_manifest_digest"].clone();
        calendar_bundle["pointer"]["coherent_manifest_digest"] = calendar_manifest_digest.clone();
        calendar_bundle["pointer"]["coherent_manifest_file"] = Value::String(format!(
            "watcher-coherent-{}.json",
            &calendar_manifest_digest.as_str().unwrap()[7..]
        ));
        reseal_for_test(&mut calendar_bundle["pointer"], "pointer_digest");
        let calendar_pointer = calendar_bundle["pointer"].clone();
        calendar_bundle["intent"]["prepared_transition_digest"] =
            calendar_prepared["transition_digest"].clone();
        calendar_bundle["intent"]["coherent_manifest_digest"] = calendar_manifest_digest.clone();
        calendar_bundle["intent"]["target_pointer"] = calendar_pointer.clone();
        calendar_bundle["intent"]["target_complete_transition"] = calendar_complete.clone();
        reseal_for_test(&mut calendar_bundle["intent"], "intent_digest");
        let calendar_intent_digest = calendar_bundle["intent"]["intent_digest"].clone();
        calendar_bundle["outcome"]["intent_digest"] = calendar_intent_digest.clone();
        calendar_bundle["outcome"]["coherent_manifest_digest"] = calendar_manifest_digest.clone();
        calendar_bundle["outcome"]["pointer_digest"] = calendar_pointer["pointer_digest"].clone();
        reseal_for_test(&mut calendar_bundle["outcome"], "outcome_digest");
        calendar_bundle["active"]["intent_digest"] = calendar_intent_digest.clone();
        calendar_bundle["active"]["coherent_manifest_digest"] = calendar_manifest_digest;
        calendar_bundle["active"]["pointer_digest"] = calendar_pointer["pointer_digest"].clone();
        reseal_for_test(&mut calendar_bundle["active"], "active_digest");
        let mut calendar_guard = guard.clone();
        let calendar_pointer_bytes = canonical_bytes(&calendar_pointer).unwrap();
        calendar_guard["new_pointer_file"] = Value::String(format!(
            "watcher-pointer-{}.json",
            &calendar_pointer["pointer_digest"].as_str().unwrap()[7..]
        ));
        calendar_guard["new_pointer_digest"] = calendar_pointer["pointer_digest"].clone();
        calendar_guard["new_pointer_raw_sha256"] = Value::String(sha256(&calendar_pointer_bytes));
        calendar_guard["new_pointer_byte_size"] = Value::from(calendar_pointer_bytes.len() as u64);
        calendar_guard["operation_intent_digest"] = calendar_intent_digest;
        calendar_guard["target_commit_digest"] = calendar_complete["transition_digest"].clone();
        reseal_for_test(&mut calendar_guard, "guard_digest");
        seal_coherent_activation_bundle(&calendar_bundle, &calendar_guard).unwrap();

        let mut fallback_graph = raw_graph.clone();
        let fallback_node = fallback_graph["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|node| node["kind"] == "file")
            .unwrap();
        fallback_node["contentHash"] = json!("node-content-fallback");
        let fallback = derive_graphiti(&fallback_graph, &vault_id).unwrap();
        assert_eq!(
            canonical_digest(&fallback).unwrap(),
            "sha256:34a5dd51c7bee1dbd1143f21b119d895baf9a36c41d39528c02ea89ff77391be"
        );
        let fallback_body: Value =
            serde_json::from_str(fallback["episodes"][0]["episode_body"].as_str().unwrap())
                .unwrap();
        assert_eq!(
            fallback_body["integrity"]["content_hash"],
            "node-content-fallback"
        );

        let mut rich_graph = raw_graph;
        let file_node = rich_graph["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|node| node["kind"] == "file")
            .unwrap();
        file_node["contentHash"] = json!("node-content-fallback");
        file_node["gkx"] = json!({
            "gkxVersion":"2.3",
            "uid":"123e4567-e89b-42d3-a456-426614174000",
            "type":"specification",
            "title":"Rich governed title",
            "description":"Rich governed description",
            "timestamp":"2025-01-02T03:04:05.000Z",
            "sensitivity":"restricted",
            "supersedes":["Old authored title"],
            "supersedesIds":["missing-old-node"],
            "relations":{"supports":["legacy-unused"]},
            "projection":{
                "profile":"gkx-2.3-validating-projection",
                "conformanceClaim":"reader-and-deterministic-assessor",
                "mode":"compatible",
                "sourceVersion":"2.3",
                "sourcePath":file_node["path"],
                "contentHash":"projection-content-hash",
                "rawFrontmatter":{},
                "extensions":{},
                "authored":{
                    "labels":["label:authored"],
                    "relationships":{"supports":["authored-unused"]},
                    "evidence":{"source":"frontmatter"},
                    "authorship":{"origin":"human"},
                    "createdAt":"2024-12-31T23:59:58.000Z",
                },
                "derived":{
                    "labels":["label:derived"],
                    "relationships":{},
                    "evidence":{"derived":true},
                },
                "proposed":{
                    "labels":["label:proposed"],
                    "relationships":{"supports":["never-fact"]},
                    "evidence":null,
                },
                "approved":{
                    "labels":["label:approved"],
                    "relationships":{},
                    "evidence":{"approved":true},
                },
                "effective":{
                    "labels":["label:effective"],
                    "relationships":{
                        "depends_on":"target-gamma",
                        "supports":["target-alpha",{"target":"target-alpha"},{"target":"target-beta"},42],
                    },
                    "sensitivity":"restricted",
                    "evidence":{"effective":"yes"},
                },
                "diagnostics":[{"code":"GKX_TEST"}],
                "assessment":{
                    "assessmentId":"assessment:test",
                    "targetUid":"123e4567-e89b-42d3-a456-426614174000",
                    "profile":"gkx-2.3-validating-projection",
                    "policy":{
                        "id":"policy:test",
                        "version":"1.2.3",
                        "hash":format!("sha256:{}","a".repeat(64)),
                        "weights":{},
                        "missingValueBehavior":"exclude",
                    },
                    "assessor":{"id":"tool:gkos-engine","engineVersion":"2.1.2"},
                    "inputHash":"hash",
                    "calculatedAt":"2025-01-01T00:00:00.000Z",
                    "scores":{"overall":0.75},
                    "exclusions":[],
                    "labels":{"derived":["assessment:well-documented"]},
                    "diagnostics":[],
                    "interpretation":"documentation-and-support-quality-not-truth",
                },
            },
        });
        let rich = derive_graphiti(&rich_graph, &vault_id).unwrap();
        assert_eq!(
            canonical_digest(&rich).unwrap(),
            "sha256:9233a249a81a55b0ccdfd2c4a0f207ec9937d3f64b15e0751ff7a626364766e6"
        );
        assert_eq!(rich["episodes"].as_array().unwrap().len(), 4);
        let body: Value =
            serde_json::from_str(rich["episodes"][0]["episode_body"].as_str().unwrap()).unwrap();
        assert_eq!(body["integrity"]["content_hash"], "projection-content-hash");
        assert_eq!(body["authority"]["class"], "human");
        assert_eq!(body["reference_time_source"], "gkx.created_at");
        let triples = rich["episodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|episode| episode["source"] == "fact_triple")
            .collect::<Vec<_>>();
        assert_eq!(triples.len(), 3);
        assert!(triples.iter().all(|episode| episode["episode_body"]
            .as_str()
            .unwrap()
            .contains("target-")));

        let mut bounded_graph = authored_graph.clone();
        bounded_graph["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|node| node["kind"] == "file")
            .unwrap()["gkx"] = json!({"description":"a".repeat(251)});
        let bounded = derive_graphiti(&bounded_graph, &vault_id).unwrap();
        assert_eq!(
            canonical_digest(&bounded).unwrap(),
            "sha256:53810e39a6f848ba5462521cede599a7f38f0a513995f1a123aba0e9b25c747f"
        );
        let bounded_body: Value =
            serde_json::from_str(bounded["episodes"][0]["episode_body"].as_str().unwrap()).unwrap();
        assert_eq!(
            bounded_body["description"]
                .as_str()
                .unwrap()
                .encode_utf16()
                .count(),
            250
        );

        let mut split_graph = authored_graph;
        split_graph["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|node| node["kind"] == "file")
            .unwrap()["gkx"] = json!({"description":format!("{}😀tail","a".repeat(249))});
        assert_eq!(
            derive_graphiti(&split_graph, &vault_id),
            Err(WatcherError("GKX_WATCHER_CONTRACT_GRAPH_INVALID"))
        );
    }

    #[test]
    fn self_resealed_schema_reopening_and_local_refs_are_rejected() {
        let accepted =
            semantic_case("pack-all-seventeen-canonical-bytes")["input"]["arguments"][0].clone();
        validate_pack(&accepted).unwrap();

        let mut local_ref = accepted.clone();
        mutate_pack_schema(&mut local_ref, "batch.schema.json", |schema| {
            schema["$defs"]["plan"]["properties"]["intended_source_mutations"]["items"]["$ref"] =
                Value::String("#/$defs/sourceMutation".to_owned());
        });
        assert_eq!(
            validate_pack(&local_ref),
            Err(WatcherError("GKX_WATCHER_CONTRACT_PACK_INVALID"))
        );

        let mut reopened = accepted;
        mutate_pack_schema(&mut reopened, "batch.schema.json", |schema| {
            schema["$defs"]["observation"] = json!({
                "type":"object",
                "properties":{},
                "required":[],
                "additionalProperties":true,
                "unevaluatedProperties":false,
            });
        });
        let reopened_row = reopened["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["file"] == "batch.schema.json")
            .unwrap();
        let reopened_schema: Value = serde_json::from_slice(
            &decode_base64(reopened_row["bytes_base64"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(reopened_schema["$defs"]["observation"]["type"], "object");
        assert_eq!(
            validate_schema_structure(&reopened_schema["$defs"]["observation"], false),
            Err(WatcherError("GKX_WATCHER_CONTRACT_PACK_INVALID"))
        );
        assert_eq!(
            validate_schema_structure(&reopened_schema, false),
            Err(WatcherError("GKX_WATCHER_CONTRACT_PACK_INVALID"))
        );
        assert_eq!(
            validate_pack(&reopened),
            Err(WatcherError("GKX_WATCHER_CONTRACT_PACK_INVALID"))
        );
    }

    #[test]
    fn all_frozen_semantic_cases_match_exact_results_and_errors() {
        let conformance = fixture("watcher-conformance-fixture.json");
        assert_eq!(conformance["status"], "frozen");
        assert_eq!(conformance["frozen"], true);
        let cases = array(&conformance["semantic_cases"]).unwrap();
        assert_eq!(cases.len(), 360);
        let mut ids = BTreeSet::new();
        let mut mismatches = Vec::new();
        for case in cases {
            let case = object(case).unwrap();
            let id = text(case, "case_id").unwrap();
            assert!(ids.insert(id), "duplicate {id}");
            let operation = text(case, "operation").unwrap();
            let expectation = object(&case["expectation"]).unwrap();
            let result = invoke(operation, &case["input"]);
            if expectation["accepted"] == true {
                match result {
                    Ok(actual) if Some(actual.as_str()) == text(expectation, "output_digest") => {}
                    Ok(actual) => mismatches.push(format!(
                        "{id}: digest {actual}, expected {}",
                        text(expectation, "output_digest").unwrap_or("null")
                    )),
                    Err(error) => mismatches.push(format!("{id}: unexpected {}", error.0)),
                }
            } else {
                match result {
                    Err(error) if Some(error.0) == text(expectation, "error_code") => {}
                    Err(error) => mismatches.push(format!(
                        "{id}: error {}, expected {}",
                        error.0,
                        text(expectation, "error_code").unwrap_or("null")
                    )),
                    Ok(digest) => mismatches.push(format!("{id}: unexpectedly accepted {digest}")),
                }
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    }

    #[test]
    fn schema_and_recovery_case_catalogs_are_all_and_only() {
        let conformance = fixture("watcher-conformance-fixture.json");
        let schema_cases = array(&conformance["schema_cases"]).unwrap();
        assert_eq!(schema_cases.len(), 85);
        let schemas = [
            "authority.schema.json",
            "batch.schema.json",
            "coherent-manifest.schema.json",
            "conformance.schema.json",
            "journal.schema.json",
            "sample-plan.schema.json",
            "source-removal.schema.json",
            "status.schema.json",
            "topology.schema.json",
            "transition.schema.json",
        ]
        .into_iter()
        .map(|name| (name.to_owned(), fixture(name)))
        .collect::<BTreeMap<_, _>>();
        let mut schema_ids = BTreeSet::new();
        for case in schema_cases {
            let case = object(case).unwrap();
            let id = text(case, "case_id").unwrap();
            assert!(schema_ids.insert(id), "duplicate schema case {id}");
            let schema_file = text(case, "schema_file").unwrap();
            let actual = schema_valid(&schemas[schema_file], &case["value"], &schemas);
            assert_eq!(
                actual,
                case["expected_valid"].as_bool().unwrap(),
                "schema case {id}"
            );
        }
        assert_eq!(schema_ids.len(), 85);
        let semantic_ids = array(&conformance["semantic_cases"])
            .unwrap()
            .iter()
            .map(|row| row["case_id"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        let recovery = fixture("watcher-recovery-fixture.json");
        let recovery_record = object(&recovery).unwrap();
        let mut consumed = BTreeSet::new();
        for key in [
            "event_cases",
            "transition_cases",
            "topology_cases",
            "pointer_cases",
            "crash_cases",
            "source_removal_cases",
            "status_control_cases",
            "provider_cases",
            "path_identity_cases",
            "shutdown_cases",
        ] {
            for id in array(&recovery_record[key]).unwrap() {
                let id = id.as_str().unwrap();
                assert!(consumed.insert(id), "duplicate recovery case {id}");
            }
        }
        assert_eq!(consumed, semantic_ids);
    }
}

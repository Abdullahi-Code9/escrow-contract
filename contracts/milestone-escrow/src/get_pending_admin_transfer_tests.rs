//! Tests for `get_pending_admin_transfer`.
//!
//! # Purpose
//!
//! `get_pending_admin_transfer` is a **pure read** endpoint: it may only
//! call `env.storage().persistent().get(…)` and must never write to any
//! storage tier (instance, persistent, or temporary).
//!
//! # Test matrix
//!
//! | # | Scenario                                   | Expected return   | Storage effect |
//! |---|-------------------------------------------|-------------------|----------------|
//! | 1 | No proposal has ever been created          | `None`            | zero writes    |
//! | 2 | After a successful `propose_admin_transfer`| `Some(…)`         | zero writes    |
//! | 3 | After `cancel_admin_transfer_proposal`     | `None`            | zero writes    |
//! | 4 | After `execute_admin_transfer`             | `None`            | zero writes    |
//! | 5 | Contract not yet initialized               | `None`            | zero writes    |
//! | 6 | Called multiple times in a row             | same value        | zero writes    |
//! | 7 | Snapshot before == snapshot after          | —                 | **byte-identical** |
//!
//! Every scenario includes a full ledger snapshot taken immediately before
//! and immediately after the call; the two snapshots are compared field by
//! field to prove that `get_pending_admin_transfer` is side-effect free.

use crate::test::setup_funded_escrow;
use crate::{DataKey, MilestoneEscrow, MilestoneEscrowClient, PendingAdminTransfer};
use soroban_sdk::{testutils::Address as _, vec, Address, Env};

// ── ledger snapshot helpers ───────────────────────────────────────────────────

/// A snapshot of every ledger entry that `get_pending_admin_transfer` could
/// conceivably touch: the pending-transfer key itself, the instance-storage
/// admin key, and the pause flag.
///
/// Adding a field here ensures new keys are automatically covered in the
/// byte-identity assertions below.
#[derive(Debug, PartialEq)]
struct LedgerSnapshot {
    /// Current value of `DataKey::PendingAdminTransfer` in persistent storage.
    pending: Option<PendingAdminTransfer>,
    /// Current value of `DataKey::Admin` in instance storage.
    admin: Option<Address>,
    /// Current value of `DataKey::Paused` in instance storage.
    paused: bool,
    /// `true` when `DataKey::PendingAdminTransfer` exists at all (vs `None`
    /// because the key is absent).  Lets us distinguish "absent" from a
    /// hypothetical serialised `None` value.
    pending_key_exists: bool,
}

fn capture_ledger(env: &Env, contract_id: &Address) -> LedgerSnapshot {
    env.as_contract(contract_id, || {
        let pending_key_exists = env
            .storage()
            .persistent()
            .has(&DataKey::PendingAdminTransfer);
        let pending: Option<PendingAdminTransfer> = env
            .storage()
            .persistent()
            .get(&DataKey::PendingAdminTransfer);
        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        // The emergency-pause flag lives under DataKey::Ep (not DataKey::Paused).
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Ep)
            .unwrap_or(false);
        LedgerSnapshot {
            pending,
            admin,
            paused,
            pending_key_exists,
        }
    })
}

/// Assert that `before` and `after` snapshots are identical – i.e. the call
/// under test performed zero writes to any observed storage entry.
fn assert_storage_unchanged(before: &LedgerSnapshot, after: &LedgerSnapshot) {
    assert_eq!(
        after.pending, before.pending,
        "DataKey::PendingAdminTransfer must not be mutated by get_pending_admin_transfer"
    );
    assert_eq!(
        after.pending_key_exists, before.pending_key_exists,
        "presence of DataKey::PendingAdminTransfer must not change"
    );
    assert_eq!(
        after.admin, before.admin,
        "DataKey::Admin must not be mutated by get_pending_admin_transfer"
    );
    assert_eq!(
        after.paused, before.paused,
        "DataKey::Paused must not be mutated by get_pending_admin_transfer"
    );
}

// ── scenario 1: no proposal ever created ─────────────────────────────────────

/// When no `propose_admin_transfer` has ever been called the function must
/// return `None` and the snapshot taken before must be byte-identical to the
/// snapshot taken after.
#[test]
fn test_get_pending_admin_transfer_returns_none_when_no_proposal_exists() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, _, _, contract_id, client) =
        setup_funded_escrow(&env, vec![&env, 1_000_i128]);

    let before = capture_ledger(&env, &contract_id);
    assert!(!before.pending_key_exists, "no proposal should exist yet");

    let result = client.get_pending_admin_transfer();

    let after = capture_ledger(&env, &contract_id);
    assert_eq!(result, None, "must return None when no proposal exists");
    assert_storage_unchanged(&before, &after);
}

// ── scenario 2: after a successful proposal ───────────────────────────────────

/// After `propose_admin_transfer` succeeds the function must return the
/// proposal that was stored and the snapshot must be byte-identical before and
/// after the read.
#[test]
fn test_get_pending_admin_transfer_returns_some_after_proposal() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, admin_addr, _, contract_id, client) =
        setup_funded_escrow(&env, vec![&env, 1_000_i128]);
    let new_admin = Address::generate(&env);

    client.propose_admin_transfer(&admin_addr, &new_admin, &42u32);

    let before = capture_ledger(&env, &contract_id);
    assert!(
        before.pending_key_exists,
        "pending key must exist after propose_admin_transfer"
    );

    let result = client.get_pending_admin_transfer();

    let after = capture_ledger(&env, &contract_id);

    let proposal = result.expect("must return Some after a pending proposal");
    assert_eq!(proposal.new_admin, new_admin);
    assert_eq!(proposal.proposal_id, 42u32);
    assert_storage_unchanged(&before, &after);
}

// ── scenario 3: after cancel_admin_transfer_proposal ─────────────────────────

/// After the proposal is cancelled the function must return `None` and leave
/// storage unchanged.
#[test]
fn test_get_pending_admin_transfer_returns_none_after_cancel() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, admin_addr, _, contract_id, client) =
        setup_funded_escrow(&env, vec![&env, 1_000_i128]);
    let new_admin = Address::generate(&env);

    client.propose_admin_transfer(&admin_addr, &new_admin, &7u32);
    client.cancel_admin_transfer_proposal(&admin_addr);

    let before = capture_ledger(&env, &contract_id);
    assert!(
        !before.pending_key_exists,
        "pending key must be absent after cancellation"
    );

    let result = client.get_pending_admin_transfer();

    let after = capture_ledger(&env, &contract_id);
    assert_eq!(result, None, "must return None after cancellation");
    assert_storage_unchanged(&before, &after);
}

// ── scenario 4: after execute_admin_transfer ─────────────────────────────────

/// After the proposal is executed (the admin key is rotated) the function must
/// return `None` and leave storage unchanged.
#[test]
fn test_get_pending_admin_transfer_returns_none_after_execute() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, admin_addr, _, contract_id, client) =
        setup_funded_escrow(&env, vec![&env, 1_000_i128]);
    let new_admin = Address::generate(&env);

    // Set up a 1-of-1 multisig approval so execute_admin_transfer can proceed.
    let signers = vec![&env, new_admin.clone()];
    client.multisig_approval_init(&admin_addr, &signers, &1u32);

    client.propose_admin_transfer(&admin_addr, &new_admin, &1u32);
    client.multisig_approve(&new_admin, &1u32);
    // execute_admin_transfer reads caller identity from storage; no arg needed.
    client.execute_admin_transfer();

    let before = capture_ledger(&env, &contract_id);
    assert!(
        !before.pending_key_exists,
        "pending key must be cleared after execute_admin_transfer"
    );

    let result = client.get_pending_admin_transfer();

    let after = capture_ledger(&env, &contract_id);
    assert_eq!(
        result, None,
        "must return None after the transfer is executed"
    );
    assert_storage_unchanged(&before, &after);
}

// ── scenario 5: contract not yet initialized ──────────────────────────────────

/// On a freshly registered but uninitialized contract the function must return
/// `None` and write nothing.
#[test]
fn test_get_pending_admin_transfer_returns_none_on_uninitialized_contract() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MilestoneEscrow, ());
    let client = MilestoneEscrowClient::new(&env, &contract_id);

    let before = capture_ledger(&env, &contract_id);

    let result = client.get_pending_admin_transfer();

    let after = capture_ledger(&env, &contract_id);
    assert_eq!(result, None, "must return None on uninitialized contract");
    assert_storage_unchanged(&before, &after);
}

// ── scenario 6: idempotent – multiple successive reads ────────────────────────

/// Calling `get_pending_admin_transfer` multiple times in a row must return
/// the same value each time and each call must leave storage unchanged.
#[test]
fn test_get_pending_admin_transfer_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, admin_addr, _, contract_id, client) =
        setup_funded_escrow(&env, vec![&env, 1_000_i128]);
    let new_admin = Address::generate(&env);

    client.propose_admin_transfer(&admin_addr, &new_admin, &99u32);

    // Three successive reads – each one must leave the ledger untouched.
    for _ in 0..3 {
        let before = capture_ledger(&env, &contract_id);
        let result = client.get_pending_admin_transfer();
        let after = capture_ledger(&env, &contract_id);

        let proposal = result.expect("must return Some on every successive read");
        assert_eq!(proposal.new_admin, new_admin);
        assert_eq!(proposal.proposal_id, 99u32);
        assert_storage_unchanged(&before, &after);
    }
}

// ── scenario 7: snapshot byte-identity (core invariant) ──────────────────────

/// Full ledger snapshot taken immediately before and immediately after
/// `get_pending_admin_transfer` must be byte-identical.
///
/// This test is intentionally redundant with the checks in every other
/// scenario above; its purpose is to be the single, clearly named proof that
/// the **read-only contract** holds in the general case.
#[test]
fn test_get_pending_admin_transfer_snapshot_is_byte_identical_before_and_after() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, admin_addr, _, contract_id, client) =
        setup_funded_escrow(&env, vec![&env, 1_000_i128]);
    let new_admin = Address::generate(&env);

    // Arrange: a proposal is pending so the key is populated.
    client.propose_admin_transfer(&admin_addr, &new_admin, &5u32);

    let before = capture_ledger(&env, &contract_id);

    // Act: the read-only call under scrutiny.
    let _ = client.get_pending_admin_transfer();

    // Assert: every field observed is unchanged.
    let after = capture_ledger(&env, &contract_id);
    assert_storage_unchanged(&before, &after);
}

/// Snapshot-identity holds even when no proposal exists (all fields are their
/// zero/absent defaults and must remain so after the read).
#[test]
fn test_get_pending_admin_transfer_snapshot_byte_identical_when_no_proposal() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, _, _, contract_id, client) =
        setup_funded_escrow(&env, vec![&env, 1_000_i128]);

    // No proposal – the key is absent.
    let before = capture_ledger(&env, &contract_id);

    let _ = client.get_pending_admin_transfer();

    let after = capture_ledger(&env, &contract_id);
    assert_storage_unchanged(&before, &after);
}

// ── proposal payload integrity ────────────────────────────────────────────────

/// The returned `PendingAdminTransfer` must mirror exactly what was stored by
/// `propose_admin_transfer` – no field may be altered in transit.
#[test]
fn test_get_pending_admin_transfer_returns_exact_proposal_payload() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, _, admin_addr, _, _, client) =
        setup_funded_escrow(&env, vec![&env, 1_000_i128]);
    let new_admin = Address::generate(&env);
    let proposal_id: u32 = 12_345;

    client.propose_admin_transfer(&admin_addr, &new_admin, &proposal_id);

    let proposal = client
        .get_pending_admin_transfer()
        .expect("proposal must be present");

    assert_eq!(
        proposal.new_admin, new_admin,
        "new_admin must be returned verbatim"
    );
    assert_eq!(
        proposal.proposal_id, proposal_id,
        "proposal_id must be returned verbatim"
    );
}

/// Proposal fields are returned unchanged even after the pause flag is toggled
/// (to ensure the read path does not branch on pause state in a way that
/// silently swallows the proposal).
#[test]
fn test_get_pending_admin_transfer_unaffected_by_pause_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (client_addr, freelancer_addr, _, admin_addr, _, contract_id, client) =
        setup_funded_escrow(&env, vec![&env, 1_000_i128]);
    let new_admin = Address::generate(&env);

    client.propose_admin_transfer(&admin_addr, &new_admin, &3u32);

    // Pause the contract.
    client.emergency_pause(&client_addr, &freelancer_addr);

    let before = capture_ledger(&env, &contract_id);
    assert!(before.paused, "contract must be paused for this test");

    let result = client.get_pending_admin_transfer();

    let after = capture_ledger(&env, &contract_id);
    let proposal = result.expect("proposal must still be readable while paused");
    assert_eq!(proposal.new_admin, new_admin);
    assert_eq!(proposal.proposal_id, 3u32);
    assert_storage_unchanged(&before, &after);
}

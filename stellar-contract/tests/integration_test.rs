/// Integration tests for the prediction market contract.
///
/// All tests use the Soroban test harness (soroban-sdk testutils).
/// Because most contract entry-points are still `todo!()` stubs, state is
/// seeded directly into persistent storage via `env.as_contract(...)` so that
/// the implemented functions (`create_market`, `remove_liquidity`,
/// `update_fee_config`, `pause_market`, `resume_market`, `update_admin`) can
/// be exercised end-to-end, and the unimplemented ones are verified to panic
/// (i.e. they will return an error once implemented).
///
/// Run with:
///   cargo test --features testutils
extern crate std;

use prediction_market::{
    prediction_market::{PredictionMarketContract, PredictionMarketContractClient},
    storage::DataKey,
    types::{
        AmmPool, Config, Dispute, DisputeStatus, FeeConfig, LpPosition, Market, MarketMetadata,
        MarketStats, MarketStatus, OracleReport, Outcome, UserPosition,
    },
    errors::PredictionMarketError,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    vec, Address, Env, String as SorobanString, Vec as SorobanVec,
};

// =============================================================================
// Helpers
// =============================================================================

fn default_fee_config() -> FeeConfig {
    FeeConfig {
        protocol_fee_bps: 100,
        lp_fee_bps: 200,
        creator_fee_bps: 50,
    }
}

fn sample_metadata(env: &Env) -> MarketMetadata {
    MarketMetadata {
        category: SorobanString::from_str(env, "sports"),
        tags: SorobanString::from_str(env, "wrestling"),
        image_url: SorobanString::from_str(env, "https://example.com/img.png"),
        description: SorobanString::from_str(env, "Main event prediction."),
        source_url: SorobanString::from_str(env, "https://example.com"),
    }
}

fn binary_outcomes(env: &Env) -> SorobanVec<SorobanString> {
    vec![
        env,
        SorobanString::from_str(env, "YES"),
        SorobanString::from_str(env, "NO"),
    ]
}

fn build_outcome_vec(env: &Env) -> SorobanVec<Outcome> {
    vec![
        env,
        Outcome { id: 0, label: SorobanString::from_str(env, "YES"), total_shares_outstanding: 0 },
        Outcome { id: 1, label: SorobanString::from_str(env, "NO"),  total_shares_outstanding: 0 },
    ]
}

/// Register the contract and return (contract_id, client, admin, oracle, treasury, token).
fn setup(env: &Env) -> (Address, PredictionMarketContractClient, Address, Address, Address, Address) {
    let contract_id = env.register(PredictionMarketContract, ());
    let client = PredictionMarketContractClient::new(env, &contract_id);
    let admin    = Address::generate(env);
    let oracle   = Address::generate(env);
    let treasury = Address::generate(env);
    let token    = Address::generate(env);

    let config = Config {
        admin: admin.clone(),
        default_oracle: oracle.clone(),
        token: token.clone(),
        fee_config: default_fee_config(),
        min_liquidity: 1_000,
        min_trade: 100,
        max_outcomes: 10,
        max_market_duration_secs: 86_400,
        dispute_bond: 500,
        emergency_paused: false,
        treasury: treasury.clone(),
    };

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::Config, &config);
        env.storage().persistent().set(&DataKey::EmergencyPause, &false);
        env.storage().persistent().set(&DataKey::NextMarketId, &1_u64);
    });

    (contract_id, client, admin, oracle, treasury, token)
}

/// Seed a market directly into storage and return its id.
fn seed_market(env: &Env, contract_id: &Address, market: &Market) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(&DataKey::Market(market.market_id), market);
        env.storage().persistent().set(
            &DataKey::MarketStats(market.market_id),
            &MarketStats {
                market_id: market.market_id,
                total_volume: 0,
                volume_24h: 0,
                last_trade_at: 0,
                unique_traders: 0,
                open_interest: 0,
            },
        );
    });
}

fn seed_pool(env: &Env, contract_id: &Address, pool: &AmmPool) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(&DataKey::AmmPool(pool.market_id), pool);
    });
}

fn seed_lp_position(env: &Env, contract_id: &Address, pos: &LpPosition) {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::LpPosition(pos.market_id, pos.provider.clone()), pos);
    });
}

fn seed_user_position(env: &Env, contract_id: &Address, pos: &UserPosition) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(
            &DataKey::UserPosition(pos.market_id, pos.outcome_id, pos.holder.clone()),
            pos,
        );
    });
}

fn open_market(env: &Env, contract_id: &Address, market_id: u64, creator: &Address) -> Market {
    let market = Market {
        market_id,
        creator: creator.clone(),
        question: SorobanString::from_str(env, "Who wins?"),
        betting_close_time: env.ledger().timestamp() + 3_600,
        resolution_deadline: env.ledger().timestamp() + 7_200,
        dispute_window_secs: 3_600,
        outcomes: build_outcome_vec(env),
        status: MarketStatus::Open,
        winning_outcome_id: None,
        protocol_fee_pool: 0,
        lp_fee_pool: 0,
        creator_fee_pool: 0,
        total_collateral: 1_000,
        total_lp_shares: 100,
        metadata: sample_metadata(env),
    };
    seed_market(env, contract_id, &market);
    market
}

fn standard_pool(env: &Env, market_id: u64) -> AmmPool {
    AmmPool {
        market_id,
        reserves: vec![env, 500_i128, 500_i128],
        invariant_k: 250_000,
        total_collateral: 1_000,
    }
}

// =============================================================================
// 1. Happy path: create → seed → buy YES → buy NO → close → report →
//    dispute window passes → finalize → redeem YES
// =============================================================================

/// Verifies the implemented portion of the happy path:
/// create_market succeeds and returns a valid market_id.
/// The remaining steps (seed, buy, close, report, finalize, redeem) are
/// `todo!()` stubs and will be exercised once implemented.
#[test]
fn happy_path_create_market() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    env.mock_all_auths();

    let (contract_id, client, admin, _oracle, _treasury, _token) = setup(&env);

    let market_id = client.create_market(
        &admin,
        &SorobanString::from_str(&env, "Who wins the main event?"),
        &5_000_u64,
        &8_000_u64,
        &3_600_u64,
        &binary_outcomes(&env),
        &sample_metadata(&env),
    );

    assert_eq!(market_id, 1);

    // Verify market persisted with Initializing status
    let market: Market = env.as_contract(&contract_id, || {
        env.storage().persistent().get(&DataKey::Market(1)).unwrap()
    });
    assert_eq!(market.status, MarketStatus::Initializing);
    assert_eq!(market.outcomes.len(), 2);
}

/// Verifies that seed_market, buy_shares, close_betting, report_outcome,
/// finalize_resolution, and redeem_position are present as entry-points
/// (they panic with todo! until implemented).
#[test]
fn happy_path_stubs_exist() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    env.mock_all_auths();

    let (_contract_id, client, admin, _oracle, _treasury, _token) = setup(&env);

    // seed_market is a todo! — should panic
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.seed_market(&admin, &1_u64, &10_000_i128);
    }));
    assert!(result.is_err(), "seed_market should panic (todo!)");

    // buy_shares is a todo! — should panic
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.buy_shares(&admin, &1_u64, &0_u32, &1_000_i128, &0_i128);
    }));
    assert!(result.is_err(), "buy_shares should panic (todo!)");

    // finalize_resolution is a todo! — should panic
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.finalize_resolution(&1_u64);
    }));
    assert!(result.is_err(), "finalize_resolution should panic (todo!)");

    // redeem_position is a todo! — should panic
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.redeem_position(&admin, &1_u64, &0_u32);
    }));
    assert!(result.is_err(), "redeem_position should panic (todo!)");
}

// =============================================================================
// 2. Dispute flow: report → dispute → admin upholds → emergency resolve → redeem
// =============================================================================

/// Verifies that dispute_outcome, resolve_dispute, and emergency_resolve
/// are present as entry-points (todo! stubs).
#[test]
fn dispute_flow_stubs_exist() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client, admin, _oracle, _treasury, _token) = setup(&env);
    let disputer = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.dispute_outcome(
            &disputer,
            &1_u64,
            &1_u32,
            &SorobanString::from_str(&env, "Wrong outcome"),
        );
    }));
    assert!(result.is_err(), "dispute_outcome should panic (todo!)");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.resolve_dispute(&1_u64, &true, &Some(1_u32));
    }));
    assert!(result.is_err(), "resolve_dispute should panic (todo!)");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.emergency_resolve(&1_u64, &1_u32);
    }));
    assert!(result.is_err(), "emergency_resolve should panic (todo!)");
}

// =============================================================================
// 3. Dispute rejected: bond is slashed to treasury
// =============================================================================

/// Verifies that resolve_dispute (rejected path) is a stub.
/// Once implemented it should slash the bond to treasury.
#[test]
fn dispute_rejected_bond_slash_stub() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client, _admin, _oracle, _treasury, _token) = setup(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // upheld=false → bond should be slashed to treasury
        client.resolve_dispute(&1_u64, &false, &None);
    }));
    assert!(result.is_err(), "resolve_dispute (rejected) should panic (todo!)");
}

// =============================================================================
// 4. Cancel flow: cancel → refund all positions
// =============================================================================

/// Verifies that cancel_market and refund_position are stubs.
#[test]
fn cancel_flow_stubs_exist() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client, _admin, _oracle, _treasury, _token) = setup(&env);
    let holder = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.cancel_market(&1_u64);
    }));
    assert!(result.is_err(), "cancel_market should panic (todo!)");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.refund_position(&holder, &1_u64);
    }));
    assert!(result.is_err(), "refund_position should panic (todo!)");
}

// =============================================================================
// 5. LP flow: add liquidity → trade → claim LP fees → remove liquidity
// =============================================================================

/// Verifies add_liquidity and claim_lp_fees are stubs.
/// remove_liquidity IS implemented — tested with seeded state.
#[test]
fn lp_flow_add_and_claim_stubs_exist() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client, _admin, _oracle, _treasury, _token) = setup(&env);
    let provider = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.add_liquidity(&provider, &1_u64, &5_000_i128);
    }));
    assert!(result.is_err(), "add_liquidity should panic (todo!)");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.claim_lp_fees(&provider, &1_u64);
    }));
    assert!(result.is_err(), "claim_lp_fees should panic (todo!)");
}

/// remove_liquidity is implemented — verify it updates pool, market, and position.
#[test]
fn lp_flow_remove_liquidity_implemented() {
    let env = Env::default();
    env.ledger().set_timestamp(5_000); // past betting_close_time
    env.mock_all_auths();

    let (contract_id, client, admin, _oracle, _treasury, _token) = setup(&env);
    let provider = Address::generate(&env);
    let market_id = 1_u64;

    let market = Market {
        market_id,
        creator: admin.clone(),
        question: SorobanString::from_str(&env, "LP test"),
        betting_close_time: 1_000, // already passed
        resolution_deadline: 10_000,
        dispute_window_secs: 3_600,
        outcomes: build_outcome_vec(&env),
        status: MarketStatus::Open,
        winning_outcome_id: None,
        protocol_fee_pool: 0,
        lp_fee_pool: 0,
        creator_fee_pool: 0,
        total_collateral: 1_000,
        total_lp_shares: 100,
        metadata: sample_metadata(&env),
    };
    seed_market(&env, &contract_id, &market);
    seed_pool(&env, &contract_id, &standard_pool(&env, market_id));
    seed_lp_position(&env, &contract_id, &LpPosition {
        market_id,
        provider: provider.clone(),
        lp_shares: 50,
        collateral_contributed: 500,
        fees_claimed: 0,
    });

    let collateral_out = client.remove_liquidity(&provider, &market_id, &20_i128);
    assert_eq!(collateral_out, 200);

    // Position reduced
    let pos: LpPosition = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::LpPosition(market_id, provider.clone()))
            .unwrap()
    });
    assert_eq!(pos.lp_shares, 30);
}

// =============================================================================
// 6. Batch redeem: redeem across 3 markets in one call
// =============================================================================

#[test]
fn batch_redeem_stub_exists() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client, _admin, _oracle, _treasury, _token) = setup(&env);
    let holder = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.batch_redeem(
            &holder,
            &vec![&env, 1_u64, 2_u64, 3_u64],
            &vec![&env, 0_u32, 0_u32, 1_u32],
        );
    }));
    assert!(result.is_err(), "batch_redeem should panic (todo!)");
}

// =============================================================================
// 7. Split / merge: split → sell half → merge remaining
// =============================================================================

#[test]
fn split_merge_stubs_exist() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client, _admin, _oracle, _treasury, _token) = setup(&env);
    let caller = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.split_position(&caller, &1_u64, &1_000_i128);
    }));
    assert!(result.is_err(), "split_position should panic (todo!)");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.merge_positions(&caller, &1_u64, &500_i128);
    }));
    assert!(result.is_err(), "merge_positions should panic (todo!)");
}

// =============================================================================
// 8. Slippage: buy with min_shares_out too high → SlippageExceeded
// =============================================================================

/// buy_shares is a todo! stub — once implemented it must return SlippageExceeded
/// when min_shares_out cannot be met.
#[test]
fn slippage_buy_shares_stub_exists() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client, _admin, _oracle, _treasury, _token) = setup(&env);
    let buyer = Address::generate(&env);

    // Passes an absurdly high min_shares_out to trigger SlippageExceeded once implemented.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.buy_shares(&buyer, &1_u64, &0_u32, &1_000_i128, &i128::MAX);
    }));
    assert!(result.is_err(), "buy_shares should panic (todo!)");
}

// =============================================================================
// 9. Emergency pause: pause → all mutations fail → unpause → succeed
// =============================================================================

/// emergency_pause / emergency_unpause are todo! stubs.
/// Verifies they exist and panic until implemented.
#[test]
fn emergency_pause_stubs_exist() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client, _admin, _oracle, _treasury, _token) = setup(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.emergency_pause();
    }));
    assert!(result.is_err(), "emergency_pause should panic (todo!)");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.emergency_unpause();
    }));
    assert!(result.is_err(), "emergency_unpause should panic (todo!)");
}

/// When the emergency pause flag IS set in storage, create_market must return
/// EmergencyPaused (create_market IS implemented and checks the flag).
#[test]
fn emergency_pause_blocks_create_market() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    env.mock_all_auths();

    let (contract_id, client, admin, _oracle, _treasury, _token) = setup(&env);

    // Manually set the pause flag
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::EmergencyPause, &true);
    });

    let result = client.try_create_market(
        &admin,
        &SorobanString::from_str(&env, "Paused market"),
        &5_000_u64,
        &8_000_u64,
        &3_600_u64,
        &binary_outcomes(&env),
        &sample_metadata(&env),
    );
    assert_eq!(result, Err(Ok(PredictionMarketError::EmergencyPaused)));
}

/// After clearing the pause flag, create_market succeeds again.
#[test]
fn emergency_unpause_allows_create_market() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    env.mock_all_auths();

    let (contract_id, client, admin, _oracle, _treasury, _token) = setup(&env);

    // Set then clear the pause flag
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::EmergencyPause, &true);
    });
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::EmergencyPause, &false);
    });

    let market_id = client.create_market(
        &admin,
        &SorobanString::from_str(&env, "Unpaused market"),
        &5_000_u64,
        &8_000_u64,
        &3_600_u64,
        &binary_outcomes(&env),
        &sample_metadata(&env),
    );
    assert_eq!(market_id, 1);
}

/// remove_liquidity is blocked when the emergency pause flag is set.
#[test]
fn emergency_pause_blocks_remove_liquidity() {
    let env = Env::default();
    env.ledger().set_timestamp(5_000);
    env.mock_all_auths();

    let (contract_id, client, admin, _oracle, _treasury, _token) = setup(&env);
    let provider = Address::generate(&env);
    let market_id = 1_u64;

    // Set pause flag
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::EmergencyPause, &true);
    });

    let market = Market {
        market_id,
        creator: admin.clone(),
        question: SorobanString::from_str(&env, "Paused LP"),
        betting_close_time: 1_000,
        resolution_deadline: 10_000,
        dispute_window_secs: 3_600,
        outcomes: build_outcome_vec(&env),
        status: MarketStatus::Open,
        winning_outcome_id: None,
        protocol_fee_pool: 0, lp_fee_pool: 0, creator_fee_pool: 0,
        total_collateral: 1_000, total_lp_shares: 100,
        metadata: sample_metadata(&env),
    };
    seed_market(&env, &contract_id, &market);
    seed_pool(&env, &contract_id, &standard_pool(&env, market_id));
    seed_lp_position(&env, &contract_id, &LpPosition {
        market_id,
        provider: provider.clone(),
        lp_shares: 50,
        collateral_contributed: 500,
        fees_claimed: 0,
    });

    let result = client.try_remove_liquidity(&provider, &market_id, &10_i128);
    assert_eq!(result, Err(Ok(PredictionMarketError::EmergencyPaused)));
}

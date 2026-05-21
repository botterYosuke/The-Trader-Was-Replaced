"""engine.live.safety_rails 単体テスト (Phase 10 Step 4 / §2.4)。

純粋ロジック層: LiveRiskEngineConfig 組み立て（ネイティブ rail）+ 独自 pre/post-trade
評価。OrderDenied 生成や event push は呼び出し側の責務なのでここでは検証しない。
"""

from engine.live.safety_rails import (
    KIND_ALLOWED_INSTRUMENTS,
    KIND_MAX_DAILY_LOSS,
    KIND_MAX_POSITION_SIZE,
    SafetyLimits,
    SafetyRails,
)


# --- to_live_risk_engine_config (native rails) ------------------------------

def test_risk_config_maps_order_value_and_rate():
    rails = SafetyRails(
        SafetyLimits(max_order_value_jpy=500_000, max_orders_per_minute=5)
    )
    cfg = rails.to_live_risk_engine_config(["7203.TSE"])
    assert cfg.max_notional_per_order == {"7203.TSE": 500_000}
    assert cfg.max_order_submit_rate == "5/00:01:00"


def test_risk_config_zero_values_disable_native_rails():
    rails = SafetyRails(SafetyLimits())  # all zero
    cfg = rails.to_live_risk_engine_config(["7203.TSE"])
    # 0 → フィールド未設定（Nautilus default に委ねる）
    assert cfg.max_notional_per_order == {}
    assert cfg.max_order_submit_rate == "100/00:00:01"  # Nautilus default


def test_risk_config_notional_applies_to_each_instrument():
    rails = SafetyRails(SafetyLimits(max_order_value_jpy=300_000))
    cfg = rails.to_live_risk_engine_config(["7203.TSE", "9984.TSE"])
    assert cfg.max_notional_per_order == {"7203.TSE": 300_000, "9984.TSE": 300_000}


# --- pre-trade: allowed_instruments -----------------------------------------

def test_allowed_instruments_blocks_outside_whitelist():
    rails = SafetyRails(SafetyLimits(allowed_instruments=("7203.TSE",)))
    v = rails.check_pre_trade(
        instrument_id="9984.TSE", order_notional_jpy=1000, current_position_value_jpy=0
    )
    assert v is not None and v.kind == KIND_ALLOWED_INSTRUMENTS


def test_allowed_instruments_permits_listed():
    rails = SafetyRails(SafetyLimits(allowed_instruments=("7203.TSE",)))
    assert (
        rails.check_pre_trade(
            instrument_id="7203.TSE", order_notional_jpy=1000, current_position_value_jpy=0
        )
        is None
    )


def test_empty_whitelist_means_no_instrument_restriction():
    rails = SafetyRails(SafetyLimits())
    assert (
        rails.check_pre_trade(
            instrument_id="anything.TSE", order_notional_jpy=1, current_position_value_jpy=0
        )
        is None
    )


# --- pre-trade: max_position_size_jpy ---------------------------------------

def test_position_size_cap_blocks_when_projected_exceeds():
    rails = SafetyRails(SafetyLimits(max_position_size_jpy=1_000_000))
    v = rails.check_pre_trade(
        instrument_id="7203.TSE",
        order_notional_jpy=400_000,
        current_position_value_jpy=700_000,  # 700k + 400k = 1.1M > 1.0M
    )
    assert v is not None and v.kind == KIND_MAX_POSITION_SIZE


def test_position_size_cap_allows_within_limit():
    rails = SafetyRails(SafetyLimits(max_position_size_jpy=1_000_000))
    assert (
        rails.check_pre_trade(
            instrument_id="7203.TSE",
            order_notional_jpy=200_000,
            current_position_value_jpy=700_000,  # 900k <= 1.0M
        )
        is None
    )


def test_position_size_uses_absolute_values_for_short():
    """Short ポジション（負の評価額）も絶対値で評価する。"""
    rails = SafetyRails(SafetyLimits(max_position_size_jpy=1_000_000))
    v = rails.check_pre_trade(
        instrument_id="7203.TSE",
        order_notional_jpy=400_000,
        current_position_value_jpy=-700_000,
    )
    assert v is not None and v.kind == KIND_MAX_POSITION_SIZE


def test_zero_position_cap_disables_check():
    rails = SafetyRails(SafetyLimits(max_position_size_jpy=0))
    assert (
        rails.check_pre_trade(
            instrument_id="7203.TSE",
            order_notional_jpy=10_000_000,
            current_position_value_jpy=10_000_000,
        )
        is None
    )


# --- post-trade: max_daily_loss_jpy -----------------------------------------

def test_daily_loss_breach_returns_violation():
    rails = SafetyRails(SafetyLimits(max_daily_loss_jpy=100_000))
    v = rails.check_post_trade(daily_pnl_jpy=-100_001)
    assert v is not None and v.kind == KIND_MAX_DAILY_LOSS


def test_daily_loss_within_limit_ok():
    rails = SafetyRails(SafetyLimits(max_daily_loss_jpy=100_000))
    assert rails.check_post_trade(daily_pnl_jpy=-99_999) is None
    assert rails.check_post_trade(daily_pnl_jpy=50_000) is None  # profit


def test_zero_daily_loss_disables_check():
    rails = SafetyRails(SafetyLimits(max_daily_loss_jpy=0))
    assert rails.check_post_trade(daily_pnl_jpy=-9_999_999) is None

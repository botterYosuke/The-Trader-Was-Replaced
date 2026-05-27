//! K10 order_form_controls_and_validation — Manual 注文フォームの BUY/SELL、MARKET/LIMIT、数量/価格 +/-、
//! TIF ボタンが `OrderForm` を更新し、不正入力では確認モーダルを開かずエラーを出すことを保証する（kind:ui）。
//!
//! テストでは各 form button interaction と invalid state を注入し、`OrderForm` / `OrderConfirm.last_error` / modal 非表示を観測する。
//!
//! 検証フロー:
//! 1. SideSell → form.side == Sell。
//! 2. TypeLimit → form.order_type == Limit。
//! 3. QtyInc / QtyDec → qty が LOT_SIZE 単位で増減する。
//! 4. PriceInc / PriceDec → price が TICK_SIZE 単位で増減する。
//! 5. Tif(Opening) / Tif(Closing) → form.tif が更新される。
//! 6. フォーム編集後は last_error がクリアされる。
//! 7. validate_order の純粋関数: 各エラー条件を直接テストする。

use bevy::prelude::*;
use tokio::sync::mpsc;

use backcast::trading::{
    OrderFeedback, SecretPrompt, SelectedSymbol, TransportCommand, TransportCommandSender,
    VenueState, VenueStatusRes,
};
use backcast::ui::order_panel::{
    order_form_button_system, order_submit_button_system, validate_order,
    OrderButton, OrderButtonPressed, OrderConfirm, OrderForm, OrderType, Side, TimeInForce,
};

const DEFAULT_LOT_SIZE: f64 = 100.0;

fn build_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    let mut app = App::new();
    app.insert_resource(VenueStatusRes {
        state: VenueState::Connected,
        venue_id: Some("MOCK".to_string()),
        ..Default::default()
    });
    app.insert_resource(SelectedSymbol {
        id: Some("7203.TSE".to_string()),
    });
    app.insert_resource(OrderForm::default()); // side=Buy, type=Market, qty=100, price=0, tif=Day
    app.insert_resource(OrderConfirm::default());
    app.insert_resource(OrderFeedback::default());
    app.insert_resource(SecretPrompt::default());
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(TransportCommandSender { tx });

    app.add_message::<OrderButtonPressed>();
    app.add_systems(Update, (order_form_button_system, order_submit_button_system));

    (app, rx)
}

#[test]
fn k10_order_form_controls_and_validation() {
    // ── Case 1: SideSell ボタン → form.side = Sell ──────────────────────────────
    {
        let (mut app, _) = build_app();
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::SideSell));
        app.update();
        assert_eq!(
            app.world().resource::<OrderForm>().side,
            Side::Sell,
            "SideSell must set form.side = Sell"
        );
    }

    // ── Case 2: SideBuy ボタン → form.side = Buy ──────────────────────────────
    {
        let (mut app, _) = build_app();
        // まず Sell に変えてから Buy に戻す。
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::SideSell));
        app.update();
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::SideBuy));
        app.update();
        assert_eq!(
            app.world().resource::<OrderForm>().side,
            Side::Buy,
            "SideBuy must reset form.side = Buy"
        );
    }

    // ── Case 3: TypeLimit → form.order_type = Limit ─────────────────────────────
    {
        let (mut app, _) = build_app();
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::TypeLimit));
        app.update();
        assert_eq!(
            app.world().resource::<OrderForm>().order_type,
            OrderType::Limit,
            "TypeLimit must set form.order_type = Limit"
        );
    }

    // ── Case 4: TypeMarket → form.order_type = Market ───────────────────────────
    {
        let (mut app, _) = build_app();
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::TypeLimit));
        app.update();
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::TypeMarket));
        app.update();
        assert_eq!(
            app.world().resource::<OrderForm>().order_type,
            OrderType::Market,
            "TypeMarket must reset form.order_type = Market"
        );
    }

    // ── Case 5: QtyInc → qty += DEFAULT_LOT_SIZE ────────────────────────────────
    {
        let (mut app, _) = build_app();
        // デフォルト qty = 100
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::QtyInc));
        app.update();
        assert_eq!(
            app.world().resource::<OrderForm>().qty,
            200.0,
            "QtyInc must add one lot (100 → 200)"
        );
    }

    // ── Case 6: QtyDec → qty -= DEFAULT_LOT_SIZE (下限 0 でクランプ) ────────────
    {
        let (mut app, _) = build_app();
        // qty = 100 → 0 へ（負にならない）。
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::QtyDec));
        app.update();
        assert_eq!(
            app.world().resource::<OrderForm>().qty,
            0.0,
            "QtyDec at minimum must clamp to 0 not go negative"
        );
    }

    // ── Case 7: PriceInc → price += 1.0 (TICK_SIZE) ────────────────────────────
    {
        let (mut app, _) = build_app();
        // デフォルト price = 0
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::PriceInc));
        app.update();
        assert_eq!(
            app.world().resource::<OrderForm>().price,
            1.0,
            "PriceInc must add one tick"
        );
    }

    // ── Case 8: PriceDec は price 0 のとき 0 にクランプ ─────────────────────────
    {
        let (mut app, _) = build_app();
        // price = 0 → 0 (負にならない)。
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::PriceDec));
        app.update();
        assert_eq!(
            app.world().resource::<OrderForm>().price,
            0.0,
            "PriceDec at zero must clamp to 0"
        );
    }

    // ── Case 9: TIF ボタン → form.tif が更新される ──────────────────────────────
    {
        let (mut app, _) = build_app();
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::Tif(TimeInForce::Opening)));
        app.update();
        assert_eq!(
            app.world().resource::<OrderForm>().tif,
            TimeInForce::Opening,
            "Tif(Opening) button must set form.tif = Opening"
        );
    }

    {
        let (mut app, _) = build_app();
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::Tif(TimeInForce::Closing)));
        app.update();
        assert_eq!(
            app.world().resource::<OrderForm>().tif,
            TimeInForce::Closing,
            "Tif(Closing) button must set form.tif = Closing"
        );
    }

    // ── Case 10: フォーム編集時に last_error がクリアされる ─────────────────────
    {
        let (mut app, _) = build_app();
        // 事前に last_error をセット。
        app.world_mut().resource_mut::<OrderConfirm>().last_error =
            Some("銘柄が未選択です".to_string());

        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::SideSell));
        app.update();

        assert!(
            app.world().resource::<OrderConfirm>().last_error.is_none(),
            "editing the form must clear any prior last_error"
        );
    }

    // ── Case 11: validate_order 純粋関数テスト — SymbolNotSelected ───────────────
    // これらは order_panel.rs の mod tests と重複するが、E2E レイヤーでの可観測性確認として価値がある。
    {
        use backcast::ui::order_panel::OrderValidationError;

        let form = OrderForm {
            side: Side::Buy,
            order_type: OrderType::Market,
            qty: 100.0,
            price: 0.0,
            tif: TimeInForce::Day,
        };
        // symbol None → SymbolNotSelected
        assert_eq!(
            validate_order(&form, None, DEFAULT_LOT_SIZE, 1.0),
            Err(OrderValidationError::SymbolNotSelected),
            "no symbol must return SymbolNotSelected"
        );
        // symbol empty → SymbolNotSelected
        assert_eq!(
            validate_order(&form, Some(""), DEFAULT_LOT_SIZE, 1.0),
            Err(OrderValidationError::SymbolNotSelected),
            "empty symbol must return SymbolNotSelected"
        );
    }

    // ── Case 12: validate_order — QtyNotLotMultiple ──────────────────────────────
    {
        use backcast::ui::order_panel::OrderValidationError;

        let form = OrderForm {
            side: Side::Buy,
            order_type: OrderType::Market,
            qty: 150.0, // 100 の倍数でない
            price: 0.0,
            tif: TimeInForce::Day,
        };
        assert_eq!(
            validate_order(&form, Some("7203.TSE"), DEFAULT_LOT_SIZE, 1.0),
            Err(OrderValidationError::QtyNotLotMultiple),
            "qty not a multiple of lot size must return QtyNotLotMultiple"
        );
    }

    // ── Case 13: validate_order — Limit 注文で price = 0 → PriceRequiredForLimit ─
    {
        use backcast::ui::order_panel::OrderValidationError;

        let form = OrderForm {
            side: Side::Buy,
            order_type: OrderType::Limit,
            qty: 100.0,
            price: 0.0,
            tif: TimeInForce::Day,
        };
        assert_eq!(
            validate_order(&form, Some("7203.TSE"), DEFAULT_LOT_SIZE, 1.0),
            Err(OrderValidationError::PriceRequiredForLimit),
            "Limit with price=0 must return PriceRequiredForLimit"
        );
    }

    // ── Case 14: validate_order — Limit 注文で tick 非倍数 → PriceNotTickMultiple ─
    {
        use backcast::ui::order_panel::OrderValidationError;

        let form = OrderForm {
            side: Side::Buy,
            order_type: OrderType::Limit,
            qty: 100.0,
            price: 2500.5, // tick = 1.0 の倍数でない
            tif: TimeInForce::Day,
        };
        assert_eq!(
            validate_order(&form, Some("7203.TSE"), DEFAULT_LOT_SIZE, 1.0),
            Err(OrderValidationError::PriceNotTickMultiple),
            "Limit with non-tick price must return PriceNotTickMultiple"
        );
    }

    // ── Case 15: Market 注文で valid → Ok(()) ────────────────────────────────────
    {
        let form = OrderForm {
            side: Side::Buy,
            order_type: OrderType::Market,
            qty: 100.0,
            price: 0.0, // 成行は price 不問
            tif: TimeInForce::Day,
        };
        assert_eq!(
            validate_order(&form, Some("7203.TSE"), DEFAULT_LOT_SIZE, 1.0),
            Ok(()),
            "valid market order must pass validation"
        );
    }

    // ── Case 16: Submit が validate_order の NG で pending を立てない ──────────────
    // order_submit_button_system が validation gate を通じて OrderConfirm を保護していることを E2E で確認。
    {
        let (mut app, mut rx) = build_app();
        // qty を非 lot 倍数にする。
        app.world_mut().resource_mut::<OrderForm>().qty = 150.0;

        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::Submit));
        app.update();

        let confirm = app.world().resource::<OrderConfirm>();
        assert!(
            confirm.pending.is_none(),
            "invalid qty must not open confirm modal"
        );
        assert!(
            confirm.last_error.is_some(),
            "invalid qty must set last_error"
        );
        assert!(
            rx.try_recv().is_err(),
            "invalid submit must not fire any command"
        );
    }
}
